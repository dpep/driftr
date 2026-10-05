//! Which git repos are opted in, so the end-of-turn sweep can find them.
//!
//! `<data>/repos/<escaped path>` is a symlink to a repo's common git dir: readable names,
//! `readlink` gives the location, a dangling link means the repo is gone. One atomic
//! symlink per repo needs no lock. Directories tracked through a driftr store are already
//! listed by `dirs/`, so they aren't registered here.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::external;

fn dir() -> Result<PathBuf> {
    Ok(external::data_dir()?.join("repos"))
}

/// Registers the repo whose common git dir is `common_dir` (idempotent).
pub fn register(common_dir: &Path) -> Result<()> {
    let common_dir = fs::canonicalize(common_dir)?;
    if common_dir.starts_with(external::data_dir()?.join("dirs")) {
        return Ok(()); // a driftr store: `dirs/` already lists it
    }
    let link = dir()?.join(external::escape(&common_dir));
    if link.symlink_metadata().is_ok() {
        return Ok(());
    }
    fs::create_dir_all(dir()?)?;
    match symlink(&common_dir, &link) {
        // Another session registered it first.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        other => Ok(other?),
    }
}

/// The working trees of every opted-in repo (each worktree separately) and tracked
/// directory. Entries whose repo is gone or no longer opted in are pruned.
pub fn worktrees() -> Result<Vec<PathBuf>> {
    let mut tops = vec![];
    if let Ok(entries) = fs::read_dir(dir()?) {
        for entry in entries.flatten() {
            let link = entry.path();
            let common = fs::read_link(&link).unwrap_or_default();
            if !common.join("driftr/config.toml").exists() {
                let _ = fs::remove_file(&link);
                continue;
            }
            tops.extend(repo_worktrees(&common));
        }
    }
    for (dir, store) in external::all()? {
        if dir.is_dir() && store.join("driftr/config.toml").exists() {
            tops.push(dir);
        }
    }
    Ok(tops)
}

/// A repo's main worktree plus its linked ones, read from the git dir without git.
fn repo_worktrees(common: &Path) -> Vec<PathBuf> {
    let mut tops = vec![];
    if common.file_name().is_some_and(|n| n == ".git")
        && let Some(main) = common.parent()
    {
        tops.push(main.to_path_buf());
    }
    // Each `worktrees/<name>/gitdir` holds the path of that worktree's `.git` file.
    if let Ok(linked) = fs::read_dir(common.join("worktrees")) {
        for wt in linked.flatten() {
            let Ok(gitdir) = fs::read_to_string(wt.path().join("gitdir")) else {
                continue;
            };
            if let Some(top) = Path::new(gitdir.trim()).parent()
                && top.is_dir()
            {
                tops.push(top.to_path_buf());
            }
        }
    }
    tops
}
