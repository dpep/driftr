use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use std::time::Instant;

use anyhow::{Context, Result, bail};

use crate::profile;

/// `git <subcommand>`, skipping leading global options like `-c k=v` and `--literal-pathspecs`.
fn span_name(args: &[&str]) -> String {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "-c" => {
                iter.next();
            }
            a if a.starts_with('-') => {}
            a => return format!("git {a}"),
        }
    }
    "git".into()
}

/// Thin wrapper over the git CLI, run from a fixed directory.
#[derive(Clone, Debug)]
pub struct Git {
    dir: PathBuf,
    index: Option<PathBuf>,
    /// Explicit (git dir, work tree), for a directory whose repo lives elsewhere.
    repo: Option<(PathBuf, PathBuf)>,
    env: Vec<(String, String)>,
}

impl Git {
    pub fn new(dir: &Path) -> Git {
        Git {
            dir: dir.to_path_buf(),
            index: None,
            repo: None,
            env: vec![],
        }
    }

    /// Same repo, with an extra environment variable on every command.
    pub fn with_env(&self, key: &str, value: &str) -> Git {
        let mut git = self.clone();
        git.env.push((key.into(), value.into()));
        git
    }

    /// Same repo, but staging into a private index so the user's is never touched.
    pub fn with_index(&self, index: &Path) -> Git {
        Git {
            index: Some(index.to_path_buf()),
            ..self.clone()
        }
    }

    /// Same repo, run from another directory.
    pub fn at(&self, dir: &Path) -> Git {
        Git {
            dir: dir.to_path_buf(),
            ..self.clone()
        }
    }

    /// Same directory, backed by a git dir outside it.
    pub fn with_repo(&self, git_dir: &Path, work_tree: &Path) -> Git {
        Git {
            repo: Some((git_dir.to_path_buf(), work_tree.to_path_buf())),
            ..self.clone()
        }
    }

    /// Runs git, returning trimmed stdout; a non-zero exit is an error carrying stderr.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        Self::check(args, self.output(args)?)
    }

    /// Runs git where failure is an expected answer (e.g. `rev-parse --verify`).
    pub fn try_run(&self, args: &[&str]) -> Result<Option<String>> {
        let out = self.output(args)?;
        Ok(out
            .status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).trim_end().to_string()))
    }

    /// Like `run`, feeding `input` on stdin.
    pub fn run_input(&self, args: &[&str], input: &[u8]) -> Result<String> {
        let mut child = self
            .command(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("failed to run git")?;
        let start = Instant::now();
        child.stdin.take().unwrap().write_all(input)?;
        let out = child.wait_with_output()?;
        profile::record(&span_name(args), start.elapsed());
        Self::check(args, out)
    }

    fn output(&self, args: &[&str]) -> Result<Output> {
        let start = Instant::now();
        let out = self.command(args).output().context("failed to run git");
        profile::record(&span_name(args), start.elapsed());
        out
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        cmd.current_dir(&self.dir).args(args);
        // The repo is chosen by `dir`; an inherited GIT_DIR (e.g. under `rebase --exec`) must not redirect it.
        for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
            cmd.env_remove(var);
        }
        if let Some((git_dir, work_tree)) = &self.repo {
            cmd.env("GIT_DIR", git_dir).env("GIT_WORK_TREE", work_tree);
        }
        if let Some(index) = &self.index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        cmd.envs(self.env.iter().map(|(k, v)| (k, v)));
        cmd
    }

    fn check(args: &[&str], out: Output) -> Result<String> {
        if !out.status.success() {
            bail!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    }
}
