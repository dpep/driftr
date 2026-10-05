use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::snapshot::Repo;

#[derive(Debug, Serialize)]
pub struct Status {
    pub repo: String,
    /// Opted in: a config exists, so hooks act here.
    pub enabled: bool,
    pub config: String,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub files: usize,
    pub snapshot_ref: String,
    pub snapshots: usize,
    pub last: Option<String>,
    /// Claude Code settings files that wire up `driftr hook`.
    pub hooks: Vec<String>,
}

pub fn status(repo: &Repo) -> Result<Status> {
    let config = repo.config()?;
    // Canonical, to match the resolved paths git reports (e.g. macOS /var -> /private/var).
    let home = std::env::var_os("HOME").map(|h| fs::canonicalize(&h).unwrap_or(h.into()));
    Ok(Status {
        repo: tilde(repo.top(), home.as_deref()),
        enabled: repo.enabled(),
        config: tilde(&repo.config_path(), home.as_deref()),
        include: config.scope.include,
        exclude: config.scope.exclude,
        files: repo.files()?.len(),
        snapshot_ref: repo.snapshot_ref().into(),
        snapshots: repo.count()?,
        last: repo.last()?,
        hooks: hook_settings(repo.top(), home.as_deref())
            .iter()
            .map(|p| tilde(p, home.as_deref()))
            .collect(),
    })
}

/// Settings files that mention `driftr hook`. A substring check: good enough to answer "did it install?".
fn hook_settings(top: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut candidates = vec![
        top.join(".claude/settings.json"),
        top.join(".claude/settings.local.json"),
    ];
    if let Some(home) = home {
        candidates.insert(0, home.join(".claude/settings.json"));
    }
    let mut found: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|p| fs::read_to_string(p).is_ok_and(|s| s.contains("driftr hook")))
        .filter_map(|p| fs::canonicalize(p).ok())
        .collect();
    // A repo at $HOME would otherwise list its settings twice.
    found.dedup();
    found
}

fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
