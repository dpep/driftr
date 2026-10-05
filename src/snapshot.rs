use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use serde::Serialize;

use crate::config::{Config, Matcher};
use crate::git::Git;

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
pub struct Entry {
    pub id: String,
    pub time: String,
    pub subject: String,
}

pub struct Repo {
    /// Runs from the caller's directory, so user-supplied paths resolve as typed.
    git: Git,
    /// Runs from the worktree root, where scope patterns are anchored.
    root: Git,
    top: PathBuf,
    /// Per-worktree.
    index: PathBuf,
    /// Shared by all worktrees of a repo.
    shared: PathBuf,
    snapshot_ref: String,
}

impl Repo {
    pub fn open(dir: &Path) -> Result<Repo> {
        let git = Git::new(dir);
        let out = git
            .run(&[
                "rev-parse",
                "--path-format=absolute",
                "--show-toplevel",
                "--git-dir",
                "--git-common-dir",
            ])
            .context("not in a git worktree")?;
        let [top, git_dir, common] = out.lines().collect::<Vec<_>>()[..] else {
            bail!("unexpected rev-parse output: {out}");
        };
        let snapshot_ref = match Path::new(git_dir).file_name() {
            Some(name) if git_dir != common => {
                format!("refs/driftr/worktrees/{}/snapshots", name.to_string_lossy())
            }
            _ => REF.to_string(),
        };
        Ok(Repo {
            snapshot_ref,
            root: Git::new(Path::new(top)),
            top: top.into(),
            index: Path::new(git_dir).join("driftr").join("index"),
            shared: Path::new(common).join("driftr"),
            git,
        })
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
        let prefix = self.git.run(&["rev-parse", "--show-prefix"])?;
        let rel = format!("{prefix}{}", path.trim_start_matches("./"));
        let rel = rel.trim_end_matches('/');
        let mut pattern = globset::escape(rel);
        if self.top.join(rel).is_dir() {
            pattern.push_str("/**");
        }

        let mut config = self.config()?;
        if !config.scope.include.contains(&pattern) {
            config.scope.include.push(pattern.clone());
        }
        config.save(&self.config_path())?;
        Ok(pattern)
    }

    /// In-scope files present on disk, repo-relative and sorted.
    pub fn files(&self) -> Result<Vec<String>> {
        let matcher = Matcher::new(&self.config()?.scope)?;
        let out = self.root.run(&[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])?;
        // A set: conflicted files appear once per stage.
        let files: BTreeSet<&str> = out
            .split('\0')
            .filter(|p| !p.is_empty() && matcher.matches(p) && self.top.join(p).is_file())
            .collect();
        Ok(files.into_iter().map(String::from).collect())
    }

    /// Writes the in-scope files to a tree object and returns its id.
    pub fn stage(&self) -> Result<String> {
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
            git.run_input(&pathspec_cmd(&["rm", "--cached", "-q"]), &nul_join(&stale))?;
        }
        if !files.is_empty() {
            git.run_input(&pathspec_cmd(&["add"]), &nul_join(&files))?;
        }
        git.run(&["write-tree"])
    }

    /// Snapshots the in-scope files; `None` when nothing changed since the last snapshot.
    pub fn snapshot(&self, opts: &SnapshotOpts) -> Result<Option<String>> {
        let tree = self.stage()?;
        self.commit(&tree, opts)
    }

    /// Records `tree` as the next snapshot unless it matches the last one.
    pub fn commit(&self, tree: &str, opts: &SnapshotOpts) -> Result<Option<String>> {
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
        Ok(Some(id))
    }

    pub fn last(&self) -> Result<Option<String>> {
        self.git
            .try_run(&["rev-parse", "--verify", "-q", &self.snapshot_ref])
    }

    /// Snapshots newest first, optionally limited to a path and/or session.
    pub fn log(&self, path: Option<&str>, session: Option<&str>) -> Result<Vec<Entry>> {
        if self.last()?.is_none() {
            return Ok(vec![]);
        }
        let grep = session.map(|s| format!("--grep=^Driftr-Session: {s}$"));
        let mut args = vec![
            "log",
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
        let tree = self.stage()?;
        let mut args = vec!["diff", &commit, &tree];
        if let Some(path) = path {
            args.extend(["--", path]);
        }
        self.git.run(&args)
    }

    /// Restores `path` from snapshot `id`, first snapshotting what it overwrites.
    /// Returns that safety snapshot, if the working tree had unsaved changes.
    pub fn restore(&self, id: &str, path: &str) -> Result<Option<String>> {
        let commit = self.resolve_path(id, path)?;

        let safety = self.snapshot(&SnapshotOpts {
            message: Some(format!("Before restoring {path} from {id}")),
            trailers: vec![],
        })?;
        // --worktree only: the user's index stays as it was.
        self.git.run(&[
            "restore",
            &format!("--source={commit}"),
            "--worktree",
            "--",
            path,
        ])?;
        Ok(safety)
    }

    /// The diff `restore` would apply, without applying it.
    pub fn preview_restore(&self, id: &str, path: &str) -> Result<String> {
        let commit = self.resolve_path(id, path)?;
        let tree = self.stage()?;
        self.git.run(&["diff", &tree, &commit, "--", path])
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
        fs::create_dir_all(&self.shared)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.shared.join("events.jsonl"))?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    fn resolve(&self, id: &str) -> Result<String> {
        self.git
            .try_run(&["rev-parse", "--verify", "-q", &format!("{id}^{{commit}}")])?
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

/// Paths go on stdin, NUL-separated and literal, so odd filenames can't be read as globs.
fn pathspec_cmd<'a>(cmd: &[&'a str]) -> Vec<&'a str> {
    let mut args = vec!["--literal-pathspecs"];
    args.extend(cmd);
    args.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
    args
}

fn nul_join<S: AsRef<str>>(paths: &[S]) -> Vec<u8> {
    let mut out = vec![];
    for p in paths {
        out.extend(p.as_ref().as_bytes());
        out.push(0);
    }
    out
}
