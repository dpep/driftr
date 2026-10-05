#![allow(dead_code)]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Runs git isolated from the user's config and any inherited repo pointers.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .trim_end()
        .to_string()
}

pub fn write(dir: &Path, path: &str, body: &str) {
    let path = dir.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

pub fn read(dir: &Path, path: &str) -> String {
    fs::read_to_string(dir.join(path)).unwrap()
}

/// A git repo with one commit containing README.md.
pub fn init_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    write(dir, "README.md", "v1\n");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "-m", "init"]);
}

pub struct Run {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn driftr(dir: &Path, args: &[&str]) -> Run {
    driftr_with(dir, args, None, &[])
}

/// Runs the binary with HOME at `dir` and DRIFTR_HOME pinned beneath it, so no test
/// can reach the real data dir; `env` overrides either.
pub fn driftr_with(dir: &Path, args: &[&str], stdin: Option<&str>, env: &[(&str, &str)]) -> Run {
    let mut child = Command::new(env!("CARGO_BIN_EXE_driftr"))
        .current_dir(dir)
        .args(args)
        .env("HOME", dir)
        .env("DRIFTR_HOME", dir.join(".driftr-data"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_INDEX_FILE")
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    Run {
        code: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
    }
}
