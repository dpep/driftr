//! Claude Code hook entry point: snapshot at agent turn boundaries, tagged with the session.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::snapshot::{Repo, SnapshotOpts};

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

/// Handles one hook invocation. Returns the snapshot id, if one was taken.
/// Repos that haven't opted in (no driftr config) are left untouched.
pub fn run(input: &str) -> Result<Option<String>> {
    let payload: Payload = serde_json::from_str(input)?;
    let dir = payload.cwd.clone().unwrap_or_else(|| ".".into());
    let Ok(repo) = Repo::open(Path::new(&dir)) else {
        return Ok(None);
    };
    if !repo.enabled() {
        return Ok(None);
    }

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
    let snapshot = repo.snapshot(&opts)?;
    repo.record_event(&event_line(&payload, snapshot.as_deref()).to_string())?;
    Ok(snapshot)
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
