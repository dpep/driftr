//! Claude Code hook entry point: snapshot at agent turn boundaries, tagged with the session.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::fast;
use crate::snapshot::{Repo, SnapshotOpts, append_event};

/// The subset of Claude Code's hook payload driftr reads; every field is optional.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Payload {
    pub session_id: Option<String>,
    pub hook_event_name: Option<String>,
    pub cwd: Option<PathBuf>,
    pub tool_name: Option<String>,
    pub tool_input: Option<Value>,
}

/// One opted-in repo the hook acted on.
#[derive(Debug, Serialize)]
pub struct Visit {
    pub repo: String,
    pub snapshot: Option<String>,
    /// Another driftr held the repo past the hook's wait; the next hook catches up.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub busy: bool,
    /// Settled by the stat fingerprint alone: nothing changed, and git never ran.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fast: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct Outcome {
    pub event: Option<String>,
    pub tool: Option<String>,
    pub session: Option<String>,
    pub visits: Vec<Visit>,
}

impl Outcome {
    pub fn snapshots(&self) -> Vec<String> {
        self.visits
            .iter()
            .filter_map(|v| v.snapshot.clone())
            .collect()
    }
}

/// Handles one hook invocation, snapshotting the session's cwd repo and the repo of the
/// file a tool is touching, so edits outside the cwd (e.g. to ~/.claude/skills) are caught.
/// Repos that haven't opted in (no driftr config) are left untouched.
pub fn run(input: &str) -> Result<Outcome> {
    let payload: Payload = serde_json::from_str(input)?;

    // No author claim: a UserPromptSubmit snapshot holds edits made *between* turns.
    // The event says which boundary took it; attribution is inferred from that later.
    let mut opts = SnapshotOpts::default();
    if let Some(session) = &payload.session_id {
        opts.trailers
            .push(("Driftr-Session".into(), session.clone()));
    }
    if let Some(event) = &payload.hook_event_name {
        opts.trailers.push(("Driftr-Event".into(), event.clone()));
    }

    let mut seen = vec![];
    let mut visits = vec![];
    for dir in target_dirs(&payload) {
        // Found without git; no repo, or one not opted in, costs no spawns at all.
        let Some(loc) = fast::discover(&dir) else {
            continue;
        };
        if !loc.config_path().exists() || seen.contains(&loc.git_dir) {
            continue;
        }
        seen.push(loc.git_dir.clone());
        if fast::unchanged(&loc) {
            append_event(
                &loc.common_dir.join("driftr"),
                &event_line(&payload, None).to_string(),
            )?;
            visits.push(Visit {
                repo: loc.top.display().to_string(),
                snapshot: None,
                busy: false,
                fast: true,
            });
            continue;
        }
        let Ok(repo) = Repo::open(&dir) else {
            continue;
        };
        // Never stall the agent: a busy repo is skipped, and the next hook catches up.
        repo.set_lock_wait(Duration::from_secs(3));
        let (snapshot, busy) = match repo.snapshot(&opts) {
            Ok(snapshot) => (snapshot, false),
            Err(e) if format!("{e:#}").starts_with("busy:") => (None, true),
            Err(e) => return Err(e),
        };
        if !busy {
            repo.record_event(&event_line(&payload, snapshot.as_deref()).to_string())?;
        }
        visits.push(Visit {
            repo: repo.top().display().to_string(),
            snapshot,
            busy,
            fast: false,
        });
    }
    Ok(Outcome {
        event: payload.hook_event_name,
        tool: payload.tool_name,
        session: payload.session_id,
        visits,
    })
}

/// The cwd, plus the nearest existing directory of the tool's `file_path` (which a
/// PreToolUse Write may not have created yet).
fn target_dirs(p: &Payload) -> Vec<PathBuf> {
    let mut dirs = vec![p.cwd.clone().unwrap_or_else(|| ".".into())];
    let file = p
        .tool_input
        .as_ref()
        .and_then(|t| t.get("file_path"))
        .and_then(Value::as_str);
    if let Some(dir) = file.and_then(|f| Path::new(f).ancestors().skip(1).find(|a| a.is_dir())) {
        dirs.push(dir.to_path_buf());
    }
    dirs
}

/// Raw material for attribution experiments: what the agent said it touched vs. what changed.
fn event_line(p: &Payload, snapshot: Option<&str>) -> Value {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = p
        .tool_input
        .as_ref()
        .and_then(|t| t.get("file_path"))
        .cloned();
    json!({
        "time": time,
        "event": p.hook_event_name,
        "session": p.session_id,
        "tool": p.tool_name,
        "file": file,
        "snapshot": snapshot,
    })
}
