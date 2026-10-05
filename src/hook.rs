//! Claude Code hook entry point: snapshot at agent turn boundaries, tagged with the session.

use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::fast::{self, Located};
use crate::snapshot::{Repo, SnapshotOpts, append_event};

/// A sweep skips a repo any session swept this recently: N sessions ending turns together
/// shouldn't fan out N x M checks, and the sweep is only a backstop.
const SWEEP_THROTTLE: Duration = Duration::from_secs(30);
/// How far back a sweep looks for sessions with strong evidence in a repo.
const CANDIDATE_WINDOW: Duration = Duration::from_secs(15 * 60);

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

/// How a repo came to be targeted: the evidence that this session touched it, strongest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    /// The session's working directory.
    Cwd,
    /// The file a tool is editing.
    File,
    /// A path named in a shell command.
    Command,
    /// The end-of-turn sweep of every opted-in repo: changed during the turn, by someone.
    Sweep,
}

impl Via {
    fn as_str(self) -> &'static str {
        match self {
            Via::Cwd => "cwd",
            Via::File => "file",
            Via::Command => "command",
            Via::Sweep => "sweep",
        }
    }
}

/// What the caller knows beyond the payload. The CLI fills it from the registry; the
/// library default is empty, so nothing outside the payload's own paths is touched.
#[derive(Debug, Default)]
pub struct Context {
    /// Working trees of every opted-in repo.
    pub registered: Vec<PathBuf>,
}

/// One opted-in repo the hook acted on.
#[derive(Debug, Serialize)]
pub struct Visit {
    pub repo: String,
    pub via: Via,
    pub snapshot: Option<String>,
    /// Another driftr held the repo past the hook's wait; the next hook catches up.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub busy: bool,
    /// Settled by the stat fingerprint alone: nothing changed, and git never ran.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub fast: bool,
    /// Where the repo's shared git dir is (for the registry); not logged.
    #[serde(skip)]
    pub common_dir: PathBuf,
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

/// Handles one hook invocation. Snapshots the session's cwd repo, the repo of a file a
/// tool is editing, repos a shell command names, and on Stop sweeps every registered repo.
/// Repos that haven't opted in (no driftr config) are left untouched.
pub fn run(input: &str, ctx: &Context) -> Result<Outcome> {
    let payload: Payload = serde_json::from_str(input)?;

    let mut seen = vec![];
    let mut visits = vec![];
    for (dir, via) in targets(&payload, ctx) {
        // Found without git; no repo, or one not opted in, costs no spawns at all.
        let Some(loc) = fast::discover(&dir) else {
            continue;
        };
        if !loc.config_path().exists() || seen.contains(&loc.git_dir) {
            continue;
        }
        seen.push(loc.git_dir.clone());
        if via == Via::Sweep && swept_recently(&loc) {
            continue;
        }
        if let Some(visit) = visit(&payload, &dir, &loc, via)? {
            visits.push(visit);
        }
    }
    Ok(Outcome {
        event: payload.hook_event_name,
        tool: payload.tool_name,
        session: payload.session_id,
        visits,
    })
}

fn visit(payload: &Payload, dir: &Path, loc: &Located, via: Via) -> Result<Option<Visit>> {
    let driftr_dir = loc.common_dir.join("driftr");
    let mut visit = Visit {
        repo: loc.top.display().to_string(),
        via,
        snapshot: None,
        busy: false,
        fast: false,
        common_dir: loc.common_dir.clone(),
    };
    if fast::unchanged(loc) {
        append_event(&driftr_dir, &event_line(payload, via, None).to_string())?;
        visit.fast = true;
        return Ok(Some(visit));
    }
    let Ok(repo) = Repo::open(dir) else {
        return Ok(None);
    };

    // No author claim: a UserPromptSubmit snapshot holds edits made *between* turns, and a
    // sweep's holds whatever changed during the turn. Event and via say how it was taken;
    // for a sweep, the sessions with strong evidence in the repo are named as candidates.
    let mut opts = SnapshotOpts::default();
    if let Some(session) = &payload.session_id {
        opts.trailers
            .push(("Driftr-Session".into(), session.clone()));
    }
    if let Some(event) = &payload.hook_event_name {
        opts.trailers.push(("Driftr-Event".into(), event.clone()));
    }
    opts.trailers
        .push(("Driftr-Via".into(), via.as_str().into()));
    if via == Via::Sweep {
        let candidates = recent_sessions(&driftr_dir);
        if !candidates.is_empty() {
            let list = candidates.into_iter().collect::<Vec<_>>().join(", ");
            opts.trailers.push(("Driftr-Candidates".into(), list));
        }
    }

    // Never stall the agent: a busy repo is skipped, and the next hook catches up.
    repo.set_lock_wait(Duration::from_secs(3));
    match repo.snapshot(&opts) {
        Ok(snapshot) => visit.snapshot = snapshot,
        Err(e) if format!("{e:#}").starts_with("busy:") => visit.busy = true,
        Err(e) => return Err(e),
    }
    if !visit.busy {
        repo.record_event(&event_line(payload, via, visit.snapshot.as_deref()).to_string())?;
    }
    Ok(Some(visit))
}

/// Every place this hook should look, strongest evidence first (dedupe keeps the first).
fn targets(p: &Payload, ctx: &Context) -> Vec<(PathBuf, Via)> {
    let mut targets = vec![(p.cwd.clone().unwrap_or_else(|| ".".into()), Via::Cwd)];
    let input = p.tool_input.as_ref();
    // The nearest existing directory: a PreToolUse Write may target a path not yet created.
    let file = input
        .and_then(|t| t.get("file_path"))
        .and_then(Value::as_str);
    if let Some(dir) = file.and_then(|f| Path::new(f).ancestors().skip(1).find(|a| a.is_dir())) {
        targets.push((dir.to_path_buf(), Via::File));
    }
    if let Some(command) = input.and_then(|t| t.get("command")).and_then(Value::as_str) {
        for path in named_paths(command) {
            if let Some(top) = ctx.registered.iter().find(|t| path.starts_with(t)) {
                targets.push((top.clone(), Via::Command));
            }
        }
    }
    if p.hook_event_name.as_deref() == Some("Stop") {
        targets.extend(ctx.registered.iter().map(|t| (t.clone(), Via::Sweep)));
    }
    targets
}

/// Absolute and `~/` paths in a shell command: usually the paths it touches. Only used to
/// pick registered repos to check, so a false match costs a fingerprint, never a wrong snapshot.
fn named_paths(command: &str) -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    command
        .split(|c: char| c.is_whitespace() || "\"'=;|&()<>`".contains(c))
        .filter_map(|token| {
            if let Some(rest) = token.strip_prefix("~/") {
                Some(home.as_ref()?.join(rest))
            } else if token.starts_with('/') {
                Some(PathBuf::from(token))
            } else {
                None
            }
        })
        .collect()
}

/// True if any session swept this repo within the throttle window; otherwise marks it swept.
fn swept_recently(loc: &Located) -> bool {
    let marker = loc.git_dir.join("driftr/swept");
    let fresh = fs::metadata(&marker)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|age| age < SWEEP_THROTTLE));
    if !fresh {
        let _ = fs::create_dir_all(marker.parent().unwrap());
        let _ = fs::write(&marker, "");
    }
    fresh
}

/// Sessions with strong evidence (cwd, file, command) in this repo within the window,
/// read from the tail of its event log.
fn recent_sessions(driftr_dir: &Path) -> BTreeSet<String> {
    const TAIL: u64 = 256 * 1024;
    let mut tail = String::new();
    if let Ok(mut f) = fs::File::open(driftr_dir.join("events.jsonl")) {
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let _ = f.seek(SeekFrom::Start(len.saturating_sub(TAIL)));
        let _ = f.read_to_string(&mut tail);
    }
    let since = now().saturating_sub(CANDIDATE_WINDOW.as_secs());
    tail.lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|e| e["time"].as_u64().unwrap_or(0) >= since)
        .filter(|e| matches!(e["via"].as_str(), Some("cwd" | "file" | "command")))
        .filter_map(|e| e["session"].as_str().map(String::from))
        .collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Raw material for attribution: what the agent said it touched vs. what changed.
fn event_line(p: &Payload, via: Via, snapshot: Option<&str>) -> Value {
    let file = p
        .tool_input
        .as_ref()
        .and_then(|t| t.get("file_path"))
        .cloned();
    json!({
        "time": now(),
        "event": p.hook_event_name,
        "session": p.session_id,
        "tool": p.tool_name,
        "via": via,
        "file": file,
        "snapshot": snapshot,
    })
}
