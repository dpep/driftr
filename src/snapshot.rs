use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use serde::Serialize;

use crate::config::{Config, Matcher};
use crate::external;
use crate::fast;
use crate::git::Git;
use crate::lock::{Guard, RepoLock};

/// The main worktree's snapshot chain; linked worktrees get `refs/driftr/worktrees/<name>/snapshots`.
/// Not `refs/worktree/`: gc run from another worktree doesn't see those and prunes the snapshots.
pub const REF: &str = "refs/driftr/snapshots";

#[derive(Debug, Default)]
pub struct SnapshotOpts {
    pub message: Option<String>,
    /// Extra `Key: value` trailers, e.g. `Driftr-Session`.
    pub trailers: Vec<(String, String)>,
}

impl SnapshotOpts {
    pub fn session(id: &str) -> SnapshotOpts {
        SnapshotOpts {
            message: None,
            trailers: vec![("Driftr-Session".into(), id.into())],
        }
    }
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "action", content = "pattern", rename_all = "lowercase")]
pub enum Untrack {
    /// An include pattern was dropped.
    Removed(String),
    /// The path was added to the excludes.
    Excluded(String),
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Entry {
    pub id: String,
    pub time: String,
    pub subject: String,
}

pub struct Repo {
    /// Runs from the caller's directory, so user-supplied paths resolve as typed.
    pub(crate) git: Git,
    /// Runs from the worktree root, where scope patterns are anchored.
    root: Git,
    top: PathBuf,
    git_dir: PathBuf,
    /// Backed by a driftr-owned store rather than the user's repo.
    external: bool,
    /// Per-worktree.
    pub(crate) index: PathBuf,
    /// Shared by all worktrees of a repo.
    shared: PathBuf,
    snapshot_ref: String,
    lock: RepoLock,
}

impl Repo {
    /// Opens the git repo containing `dir`, else the external store of a tracked ancestor.
    pub fn open(dir: &Path) -> Result<Repo> {
        let git = Git::new(dir);
        if let Some(repo) = Repo::locate(&git)? {
            return Ok(repo);
        }
        if let Some((top, store)) = external::find(dir)? {
            let git = git.with_repo(&store, &top);
            let mut repo = Repo::locate(&git)?.context("external store is unreadable")?;
            repo.external = true;
            return Ok(repo);
        }
        bail!("not in a git repository or a directory driftr tracks (`driftr init` tracks one)")
    }

    /// Opts `dir` in. Outside any git repo, first creates an external store for it.
    /// Returns the repo and whether a config was created.
    pub fn init_dir(dir: &Path) -> Result<(Repo, bool)> {
        let git = Git::new(dir);
        if Repo::locate(&git)?.is_none() && external::find(dir)?.is_none() {
            external::create(dir)?;
        }
        let repo = Repo::open(dir)?;
        let created = repo.init()?;
        Ok((repo, created))
    }

    fn locate(git: &Git) -> Result<Option<Repo>> {
        let Some(out) = git.try_run(&[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ])?
        else {
            return Ok(None);
        };
        let [top, git_dir, common] = out.lines().collect::<Vec<_>>()[..] else {
            bail!("unexpected rev-parse output: {out}");
        };
        let snapshot_ref = match Path::new(git_dir).file_name() {
            Some(name) if git_dir != common => {
                format!("refs/driftr/worktrees/{}/snapshots", name.to_string_lossy())
            }
            _ => REF.to_string(),
        };
        Ok(Some(Repo {
            snapshot_ref,
            root: git.at(Path::new(top)),
            top: top.into(),
            git_dir: git_dir.into(),
            external: false,
            index: Path::new(git_dir).join("driftr").join("index"),
            shared: Path::new(common).join("driftr"),
            lock: RepoLock::new(Path::new(common).join("driftr").join("lock")),
            git: git.clone(),
        }))
    }

    /// Serializes writers across processes; hold it across any stage + commit pair.
    pub fn lock(&self) -> Result<Guard<'_>> {
        self.lock.acquire()
    }

    /// How long writers wait for another process's lock (hooks keep this short).
    pub fn set_lock_wait(&self, wait: std::time::Duration) {
        self.lock.set_wait(wait);
    }

    /// The git dir shared by all worktrees (`.git`, or the external store).
    pub fn common_dir(&self) -> &Path {
        self.shared.parent().unwrap()
    }

    /// True for a plain directory tracked through a driftr-owned store.
    pub fn is_external(&self) -> bool {
        self.external
    }

    /// Where the snapshots are stored: `.git`, or an external store.
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// This worktree's snapshot chain, outside any branch.
    pub fn snapshot_ref(&self) -> &str {
        &self.snapshot_ref
    }

    pub fn top(&self) -> &Path {
        &self.top
    }

    pub fn count(&self) -> Result<usize> {
        if self.last()?.is_none() {
            return Ok(0);
        }
        Ok(self
            .git
            .run(&["rev-list", "--count", &self.snapshot_ref])?
            .parse()?)
    }

    pub fn config_path(&self) -> PathBuf {
        self.shared.join("config.toml")
    }

    /// Driftr acts on its own (e.g. from hooks) only in repos that opted in by having a config.
    pub fn enabled(&self) -> bool {
        self.config_path().exists()
    }

    /// Writes the default config; false if one already exists.
    pub fn init(&self) -> Result<bool> {
        if self.enabled() {
            return Ok(false);
        }
        Config::default().save(&self.config_path())?;
        Ok(true)
    }

    pub fn config(&self) -> Result<Config> {
        Config::load(&self.config_path())
    }

    /// Adds `path` (relative to the caller's directory) to the scope; returns the pattern added.
    pub fn track(&self, path: &str) -> Result<String> {
        let pattern = self.pattern_for(path)?;
        let mut config = self.config()?;
        if !config.scope.include.contains(&pattern) {
            config.scope.include.push(pattern.clone());
        }
        config.save(&self.config_path())?;
        Ok(pattern)
    }

    /// Narrows the scope. An include matching `arg` (as typed, or as the pattern `track` would
    /// make of it) is removed; otherwise the path is excluded.
    pub fn untrack(&self, arg: &str) -> Result<Untrack> {
        let pattern = self.pattern_for(arg)?;
        let mut config = self.config()?;
        let include = &mut config.scope.include;
        let result = match include.iter().position(|p| p == arg || *p == pattern) {
            Some(i) => Untrack::Removed(include.remove(i)),
            None => {
                if !config.scope.exclude.contains(&pattern) {
                    config.scope.exclude.push(pattern.clone());
                }
                Untrack::Excluded(pattern)
            }
        };
        config.save(&self.config_path())?;
        Ok(result)
    }

    /// A path relative to the caller's directory, as a literal repo-relative scope pattern.
    fn pattern_for(&self, path: &str) -> Result<String> {
        let prefix = self.git.run(&["rev-parse", "--show-prefix"])?;
        let rel = format!("{prefix}{}", path.trim_start_matches("./"));
        // A trailing slash says "directory" even before it exists.
        let dir_hint = rel.ends_with('/');
        let rel = rel.trim_end_matches('/');
        let mut pattern = globset::escape(rel);
        if dir_hint || self.top.join(rel).is_dir() {
            pattern.push_str("/**");
        }
        Ok(pattern)
    }

    /// In-scope files present on disk, repo-relative and sorted.
    pub fn files(&self) -> Result<Vec<String>> {
        let scope = self.config()?.scope;
        let matcher = Matcher::new(&scope)?;
        let mut args = vec![
            "--literal-pathspecs",
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ];
        // Untracked-file discovery walks the tree; limiting it to the scope's roots is the win.
        let roots = scope.roots();
        if let Some(roots) = &roots {
            args.push("--");
            args.extend(roots.iter().map(String::as_str));
        }
        let out = self.root.run(&args)?;
        // A set: conflicted files appear once per stage.
        let files: BTreeSet<&str> = out
            .split('\0')
            .filter(|p| !p.is_empty() && matcher.matches(p) && self.top.join(p).is_file())
            .collect();
        Ok(files.into_iter().map(String::from).collect())
    }

    /// Writes the in-scope files to a tree object and returns its id.
    pub fn stage(&self) -> Result<String> {
        self.healing(|| self.stage_once())
    }

    /// Runs `f`, and if it trips over an object a prune deleted, rebuilds the private index
    /// and runs it once more. The index caches blobs by stat and trees by directory, so after
    /// a prune it can name objects that no longer exist: `write-tree` then returns a cached
    /// tree id without writing it, and the failure only surfaces at the next command.
    fn healing<T>(&self, f: impl Fn() -> Result<T>) -> Result<T> {
        let _lock = self.lock()?;
        match f() {
            Err(e) if is_missing_object(&e) => {
                let _ = fs::remove_file(&self.index);
                f()
            }
            result => result,
        }
    }

    fn stage_once(&self) -> Result<String> {
        let _lock = self.lock()?;
        fs::create_dir_all(self.index.parent().unwrap())?;
        let git = self.root.with_index(&self.index);
        let files = self.files()?;

        let staged = git.run(&["ls-files", "-z"])?;
        let wanted: BTreeSet<&str> = files.iter().map(String::as_str).collect();
        let stale: Vec<&str> = staged
            .split('\0')
            .filter(|p| !p.is_empty() && !wanted.contains(p))
            .collect();

        // The index persists between runs, so unchanged files aren't rehashed.
        if !stale.is_empty() {
            // -f: only the private index changes, and git's "staged content differs" guard
            // protects a user's staging, which this isn't.
            git.run_input(
                &pathspec_cmd(&["rm", "--cached", "-f", "-q"]),
                &nul_join(&stale),
            )?;
        }
        if !files.is_empty() {
            git.run_input(&pathspec_cmd(&["add"]), &nul_join(&files))?;
        }
        git.run(&["write-tree"])
    }

    /// Snapshots the in-scope files; `None` when nothing changed since the last snapshot.
    pub fn snapshot(&self, opts: &SnapshotOpts) -> Result<Option<String>> {
        let _lock = self.lock()?;
        // Taken before staging: a file changing mid-snapshot leaves it stale, which only
        // costs the next fast-path check a full run, never a missed change.
        let fp = fast::fingerprint(&self.top, &self.config_path()).unwrap_or(None);
        let result = self.healing(|| {
            let tree = self.stage_once()?;
            self.commit(&tree, opts)
        });
        fast::save(&self.git_dir, result.as_ref().ok().and(fp));
        result
    }

    /// Records `tree` as the next snapshot unless it matches the last one.
    pub fn commit(&self, tree: &str, opts: &SnapshotOpts) -> Result<Option<String>> {
        let _lock = self.lock()?;
        let last = self.last()?;
        let base = match &last {
            Some(rev) => self.git.run(&["rev-parse", &format!("{rev}^{{tree}}")])?,
            None => self.git.run(&["hash-object", "-t", "tree", "/dev/null"])?,
        };
        if tree == base {
            return Ok(None);
        }

        let subject = match &opts.message {
            Some(m) => m.clone(),
            None => self.describe(&base, tree, last.is_none())?,
        };
        let head = self.head()?.unwrap_or_else(|| "none".into());
        let trailers: Vec<String> = [("Driftr-Head".to_string(), head)]
            .iter()
            .chain(&opts.trailers)
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        let trailers = trailers.join("\n");

        let mut args = vec![
            "-c",
            "user.name=driftr",
            "-c",
            "user.email=driftr@localhost",
            "commit-tree",
            // Background snapshots must never trigger a signing prompt.
            "--no-gpg-sign",
            tree,
            "-m",
            &subject,
            "-m",
            &trailers,
        ];
        if let Some(parent) = &last {
            args.extend(["-p", parent]);
        }
        let id = self.git.run(&args)?;

        // Compare-and-swap so a concurrent snapshot can't be silently dropped.
        let old = last.unwrap_or_else(|| "0".repeat(id.len()));
        self.git.run(&[
            "update-ref",
            "--create-reflog",
            "-m",
            "driftr: snapshot",
            &self.snapshot_ref,
            &id,
            &old,
        ])?;
        if self.external {
            self.compact();
        }
        Ok(Some(id))
    }

    pub fn last(&self) -> Result<Option<String>> {
        self.git
            .try_run(&["rev-parse", "--verify", "-q", &self.snapshot_ref])
    }

    /// Lets git pack loose objects once there are enough, in the background (`gc.autoDetach`),
    /// so a snapshot never waits on it. Driftr-owned stores only: the user's repo is git's to
    /// maintain. Best effort, so a failure never fails the snapshot that triggered it.
    fn compact(&self) {
        let _ = self.git.run(&["gc", "--auto", "--quiet"]);
    }

    /// Snapshots oldest first, so the newest lands next to the prompt; optionally limited to a
    /// path and/or session.
    pub fn log(&self, path: Option<&str>, session: Option<&str>) -> Result<Vec<Entry>> {
        if self.last()?.is_none() {
            return Ok(vec![]);
        }
        let grep = session.map(|s| format!("--grep=^Driftr-Session: {s}$"));
        let mut args = vec![
            "log",
            "--reverse",
            "--format=%h%x09%cd%x09%s",
            "--date=format-local:%Y-%m-%d %H:%M:%S",
        ];
        if let Some(grep) = &grep {
            args.push(grep);
        }
        args.push(&self.snapshot_ref);
        if let Some(path) = path {
            args.extend(["--", path]);
        }
        let out = self.git.run(&args)?;
        Ok(out
            .lines()
            .filter_map(|line| {
                let mut parts = line.splitn(3, '\t');
                Some(Entry {
                    id: parts.next()?.into(),
                    time: parts.next()?.into(),
                    subject: parts.next()?.into(),
                })
            })
            .collect())
    }

    /// Diff from a snapshot to the current in-scope files, including untracked ones.
    pub fn diff(&self, id: &str, path: Option<&str>) -> Result<String> {
        let commit = self.resolve(id)?;
        self.healing(|| {
            let tree = self.stage_once()?;
            let mut args = vec!["diff", &commit, &tree];
            if let Some(path) = path {
                args.extend(["--", path]);
            }
            self.git.run(&args)
        })
    }

    /// Restores `path` from snapshot `id`, first snapshotting what it overwrites.
    /// In-scope files added since the snapshot are removed, as the dry run shows, and the
    /// result is snapshotted. Returns the safety snapshot, if the working tree had unsaved
    /// changes.
    pub fn restore(&self, id: &str, path: &str) -> Result<Option<String>> {
        let _lock = self.lock()?;
        let commit = self.resolve_path(id, path)?;

        let safety = self.snapshot(&SnapshotOpts {
            message: Some(format!("Before restoring {path} from {id}")),
            trailers: vec![],
        })?;
        // `git restore` only removes files git tracks, and in-scope files usually aren't.
        let added = self.healing(|| {
            let tree = self.stage_once()?;
            self.git.run(&[
                "diff",
                "--name-only",
                "--no-renames",
                "--diff-filter=D",
                &tree,
                &commit,
                "--",
                path,
            ])
        })?;
        // --worktree only: the user's index stays as it was.
        self.git.run(&[
            "restore",
            &format!("--source={commit}"),
            "--worktree",
            "--",
            path,
        ])?;
        for p in added.lines() {
            let file = self.top.join(p);
            if file.exists() {
                fs::remove_file(&file).with_context(|| format!("removing {p}"))?;
            }
        }
        // So `@` is the restored state, not the one just left.
        self.snapshot(&SnapshotOpts {
            message: Some(format!("Restore {path} from {}", &commit[..7])),
            trailers: vec![],
        })?;
        Ok(safety)
    }

    /// The diff `restore` would apply, without applying it; `restore` must match it.
    pub fn preview_restore(&self, id: &str, path: &str) -> Result<String> {
        let commit = self.resolve_path(id, path)?;
        self.healing(|| {
            let tree = self.stage_once()?;
            self.git.run(&["diff", &tree, &commit, "--", path])
        })
    }

    /// Drops the private index, e.g. after a prune; the next stage rebuilds it.
    pub(crate) fn forget_index(&self) {
        let _ = fs::remove_file(&self.index);
    }

    /// Resolves `id`, requiring `path` (relative to the cwd) to exist in it.
    fn resolve_path(&self, id: &str, path: &str) -> Result<String> {
        let commit = self.resolve(id)?;
        // `./` makes the path relative to the cwd rather than the repo root.
        let spec = format!("{commit}:./{path}");
        if self.git.try_run(&["cat-file", "-e", &spec])?.is_none() {
            bail!("{path} does not exist in snapshot {id}");
        }
        Ok(commit)
    }

    /// Appends one line to the shared event log.
    pub fn record_event(&self, line: &str) -> Result<()> {
        append_event(&self.shared, line)
    }

    /// A snapshot id, or `@` for the latest (`@~1` the one before, and so on).
    fn resolve(&self, id: &str) -> Result<String> {
        let rev = match id.strip_prefix('@') {
            Some(rest) => format!("{}{rest}", self.snapshot_ref),
            None => id.to_string(),
        };
        self.git
            .try_run(&["rev-parse", "--verify", "-q", &format!("{rev}^{{commit}}")])?
            .with_context(|| format!("unknown snapshot: {id}"))
    }

    fn head(&self) -> Result<Option<String>> {
        self.git.try_run(&["rev-parse", "--verify", "-q", "HEAD"])
    }

    /// Deterministic subject from the changed paths.
    fn describe(&self, from: &str, to: &str, first: bool) -> Result<String> {
        let out = self
            .git
            .run(&["diff-tree", "-r", "--name-status", from, to])?;
        let changes: Vec<(&str, &str)> = out.lines().filter_map(|l| l.split_once('\t')).collect();
        if first {
            let n = changes.len();
            return Ok(format!(
                "Start tracking {n} file{}",
                if n == 1 { "" } else { "s" }
            ));
        }
        Ok(match changes.as_slice() {
            [(status, path)] => {
                let verb = match *status {
                    "A" => "Add",
                    "D" => "Delete",
                    _ => "Update",
                };
                format!("{verb} {path}")
            }
            _ => {
                let shown: Vec<&str> = changes.iter().take(3).map(|(_, p)| *p).collect();
                let more = changes.len().saturating_sub(shown.len());
                let mut s = format!("Update {}", shown.join(", "));
                if more > 0 {
                    s.push_str(&format!(" (+{more} more)"));
                }
                s
            }
        })
    }
}

/// Appends one line to a repo's event log (`<common dir>/driftr/events.jsonl`).
pub fn append_event(driftr_dir: &Path, line: &str) -> Result<()> {
    fs::create_dir_all(driftr_dir)?;
    // One write per line: O_APPEND keeps concurrent writers' lines whole.
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(driftr_dir.join("events.jsonl"))?
        .write_all(format!("{line}\n").as_bytes())?;
    Ok(())
}

fn is_missing_object(e: &anyhow::Error) -> bool {
    let msg = format!("{e:#}");
    ["invalid object", "bad object", "not a valid 'tree' object"]
        .iter()
        .any(|m| msg.contains(m))
}

/// Paths go on stdin, NUL-separated and literal, so odd filenames can't be read as globs.
pub(crate) fn pathspec_cmd<'a>(cmd: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["--literal-pathspecs"];
    args.extend(cmd);
    args.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
    args
}

pub(crate) fn nul_join<S: AsRef<str>>(paths: &[S]) -> Vec<u8> {
    let mut out = vec![];
    for p in paths {
        out.extend(p.as_ref().as_bytes());
        out.push(0);
    }
    out
}
