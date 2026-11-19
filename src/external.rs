//! Tracking directories that aren't git repos, through a store kept outside them.
//!
//! Each tracked directory gets a bare repo at `<data>/dirs/<escaped path>.git` with
//! `core.worktree` pointing back at it, so the directory itself gains no `.git`.
//! The store's name encodes the path, so lookup needs no registry: one stat per ancestor.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::git::Git;

/// Loose objects before a snapshot asks git to pack the store.
pub const GC_AUTO: &str = "1000";

/// `$DRIFTR_HOME`, else `$XDG_DATA_HOME/driftr`, else `~/.local/share/driftr`.
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("DRIFTR_HOME").filter(|v| !v.is_empty()) {
        let dir = PathBuf::from(dir);
        // Relative would resolve per-process cwd, silently giving each directory its own store.
        if dir.is_relative() {
            bail!("DRIFTR_HOME must be absolute, got {}", dir.display());
        }
        return Ok(dir);
    }
    if let Some(xdg) = env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
        let xdg = PathBuf::from(xdg);
        if xdg.is_absolute() {
            return Ok(xdg.join("driftr"));
        }
    }
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local/share/driftr"))
}

/// The store for `dir` (canonical), whether or not it exists.
pub fn store_for(dir: &Path) -> Result<PathBuf> {
    Ok(data_dir()?
        .join("dirs")
        .join(format!("{}.git", escape(dir))))
}

/// A path as one file name: reversible and still readable in a directory listing.
pub fn escape(path: &Path) -> String {
    path.to_string_lossy()
        .replace('%', "%25")
        .replace('/', "%2F")
}

pub fn unescape(name: &str) -> PathBuf {
    PathBuf::from(name.replace("%2F", "/").replace("%25", "%"))
}

/// Every directory tracked through a store, with the store.
pub fn all() -> Result<Vec<(PathBuf, PathBuf)>> {
    let Ok(entries) = fs::read_dir(data_dir()?.join("dirs")) else {
        return Ok(vec![]);
    };
    Ok(entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let dir = unescape(name.strip_suffix(".git")?);
            Some((dir, e.path()))
        })
        .collect())
}

/// The nearest tracked ancestor of `dir` (inclusive) and its store.
pub fn find(dir: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
    let Ok(dir) = fs::canonicalize(dir) else {
        return Ok(None);
    };
    for ancestor in dir.ancestors() {
        let store = store_for(ancestor)?;
        if store.is_dir() {
            return Ok(Some((ancestor.to_path_buf(), store)));
        }
    }
    Ok(None)
}

/// Creates the store for `dir`. Refuses `/` and `$HOME`: everything beneath would be opted in.
pub fn create(dir: &Path) -> Result<PathBuf> {
    let dir = fs::canonicalize(dir)?;
    let home = env::var_os("HOME").and_then(|h| fs::canonicalize(h).ok());
    if dir.parent().is_none() || Some(&dir) == home.as_ref() {
        bail!(
            "refusing to track {}: it would opt in every directory beneath it",
            dir.display()
        );
    }

    let store = store_for(&dir)?;
    fs::create_dir_all(store.parent().unwrap())?;
    let git = Git::new(&dir);
    git.run(&["init", "-q", "--bare", &store.to_string_lossy()])?;
    let config = Git::new(&dir).with_repo(&store, &dir);
    config.run(&["config", "core.bare", "false"])?;
    config.run(&["config", "core.worktree", &dir.to_string_lossy()])?;
    // Snapshots add a handful of objects each, so git's default (6700 loose) would leave a
    // store unpacked for months; this packs it every few hundred snapshots.
    config.run(&["config", "gc.auto", GC_AUTO])?;
    Ok(store)
}
