//! The no-op fast path: decide "nothing to snapshot" without spawning git.
//!
//! Git startup is the floor of every snapshot (~13 ms per spawn, ~10 spawns), and most
//! hook runs change nothing. So: find the repo by walking up for `.git` (or a driftr
//! store), and compare a stat fingerprint of the in-scope files with the one saved at
//! the last snapshot. Any doubt falls through to the full path, which is always correct.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::Result;
use ignore::WalkBuilder;

use crate::config::{Config, Matcher};
use crate::external;
use crate::profile;

/// Where a directory's snapshots live, found without running git.
#[derive(Debug, PartialEq)]
pub struct Located {
    pub top: PathBuf,
    /// Per-worktree: holds the private index and fingerprint.
    pub git_dir: PathBuf,
    /// Shared: holds the config.
    pub common_dir: PathBuf,
}

impl Located {
    pub fn config_path(&self) -> PathBuf {
        self.common_dir.join("driftr/config.toml")
    }

    pub fn fingerprint_path(&self) -> PathBuf {
        self.git_dir.join("driftr/fingerprint")
    }
}

/// Mirrors `Repo::open`'s precedence: the nearest `.git` wins, else a tracked ancestor's store.
/// `None` means not found, or a layout this doesn't understand (the caller asks git).
pub fn discover(dir: &Path) -> Option<Located> {
    profile::time("discover", || {
        let dir = fs::canonicalize(dir).ok()?;
        for ancestor in dir.ancestors() {
            let dot_git = ancestor.join(".git");
            let Ok(meta) = fs::metadata(&dot_git) else {
                continue;
            };
            let git_dir = if meta.is_dir() {
                dot_git
            } else {
                // A worktree or submodule: `gitdir: <path>`, relative to the file's directory.
                let text = fs::read_to_string(&dot_git).ok()?;
                ancestor.join(text.strip_prefix("gitdir:")?.trim())
            };
            let common_dir = match fs::read_to_string(git_dir.join("commondir")) {
                Ok(rel) => git_dir.join(rel.trim()),
                Err(_) => git_dir.clone(),
            };
            return Some(Located {
                top: ancestor.to_path_buf(),
                git_dir: fs::canonicalize(&git_dir).ok()?,
                common_dir: fs::canonicalize(&common_dir).ok()?,
            });
        }
        let (top, store) = external::find(&dir).ok()??;
        Some(Located {
            top,
            git_dir: store.clone(),
            common_dir: store,
        })
    })
}

/// A hash of the scope config and every in-scope file's path and stat.
/// `None` when a file changed too recently to trust its timestamp.
pub fn fingerprint(top: &Path, config_path: &Path) -> Result<Option<u64>> {
    profile::time("fingerprint", || {
        let config_text = fs::read_to_string(config_path).unwrap_or_default();
        let scope = Config::load(config_path)?.scope;
        let matcher = Matcher::new(&scope)?;
        let mut entries = vec![];
        let roots: Vec<PathBuf> = match scope.roots() {
            Some(roots) => roots.iter().map(|r| top.join(r)).collect(),
            None => vec![top.to_path_buf()],
        };
        let racy_after = SystemTime::now() - Duration::from_secs(2);

        for root in roots.iter().filter(|r| r.exists()) {
            let pruner = Matcher::new(&scope)?;
            let walk_top = top.to_path_buf();
            let walk = WalkBuilder::new(root)
                .hidden(false)
                .ignore(false) // git doesn't read `.ignore`; skipping what git sees would miss changes
                .require_git(false) // driftr stores keep the git dir elsewhere
                .filter_entry(move |e| {
                    if e.file_name() == ".git" {
                        return false;
                    }
                    let is_dir = e.file_type().is_some_and(|t| t.is_dir());
                    let rel = e.path().strip_prefix(&walk_top).unwrap_or(e.path());
                    !(is_dir && pruner.prunes_dir(&rel.to_string_lossy()))
                })
                .build();
            for entry in walk {
                let entry = entry?;
                if entry.file_type().is_some_and(|t| t.is_dir()) {
                    continue;
                }
                let Ok(rel) = entry.path().strip_prefix(top) else {
                    continue;
                };
                let rel = rel.to_string_lossy();
                if !matcher.matches(&rel) {
                    continue;
                }
                let meta = entry.path().symlink_metadata()?;
                if meta.modified()? > racy_after {
                    return Ok(None);
                }
                entries.push((
                    rel.into_owned(),
                    meta.len(),
                    meta.mtime(),
                    meta.mtime_nsec(),
                    meta.ino(),
                    meta.mode(),
                ));
            }
        }
        entries.sort();
        let mut hasher = DefaultHasher::new();
        config_text.hash(&mut hasher);
        entries.hash(&mut hasher);
        Ok(Some(hasher.finish()))
    })
}

/// True when the saved fingerprint matches now: the last snapshot already holds this state.
pub fn unchanged(loc: &Located) -> bool {
    let Ok(saved) = fs::read_to_string(loc.fingerprint_path()) else {
        return false;
    };
    matches!(fingerprint(&loc.top, &loc.config_path()), Ok(Some(now)) if saved.trim() == format!("{now:016x}"))
}

/// Records `fp` (taken before staging) as the state the latest snapshot holds, or clears it.
pub fn save(loc_git_dir: &Path, fp: Option<u64>) {
    let path = loc_git_dir.join("driftr/fingerprint");
    match fp {
        Some(fp) => {
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
            if fs::write(&tmp, format!("{fp:016x}\n")).is_ok() {
                let _ = fs::rename(tmp, path);
            }
        }
        None => {
            let _ = fs::remove_file(path);
        }
    }
}
