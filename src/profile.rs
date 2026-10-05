//! Lightweight timing: every git spawn is recorded automatically, plus named phases.
//! Read by `--profile` (printed to stderr) and by the hook (written to its usage log).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

static SPANS: Mutex<Vec<(String, Duration)>> = Mutex::new(Vec::new());

pub fn record(name: &str, elapsed: Duration) {
    if let Ok(mut spans) = SPANS.lock() {
        spans.push((name.to_string(), elapsed));
    }
}

/// Times `f` as the phase `name`.
pub fn time<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let out = f();
    record(name, start.elapsed());
    out
}

#[derive(Debug, Serialize)]
pub struct Line {
    pub name: String,
    pub count: usize,
    pub ms: f64,
}

#[derive(Debug, Serialize)]
pub struct Profile {
    pub total_ms: f64,
    pub git_spawns: usize,
    /// Time not inside any recorded span: process start, Rust-side work, stat calls.
    pub unaccounted_ms: f64,
    pub lines: Vec<Line>,
}

/// Summarizes everything recorded since process start against `total`, slowest first.
/// Phases may contain git spans, so `unaccounted` subtracts git time only.
pub fn summary(total: Duration) -> Profile {
    let spans = SPANS.lock().map(|s| s.clone()).unwrap_or_default();
    let mut by_name: BTreeMap<String, (usize, Duration)> = BTreeMap::new();
    for (name, d) in &spans {
        let entry = by_name.entry(name.clone()).or_default();
        entry.0 += 1;
        entry.1 += *d;
    }
    let git: Duration = spans
        .iter()
        .filter(|(n, _)| n.starts_with("git "))
        .map(|(_, d)| *d)
        .sum();
    let mut lines: Vec<Line> = by_name
        .into_iter()
        .map(|(name, (count, d))| Line {
            name,
            count,
            ms: ms(d),
        })
        .collect();
    lines.sort_by(|a, b| b.ms.total_cmp(&a.ms));
    Profile {
        total_ms: ms(total),
        git_spawns: spans.iter().filter(|(n, _)| n.starts_with("git ")).count(),
        unaccounted_ms: ms(total.saturating_sub(git)),
        lines,
    }
}

/// Milliseconds to 0.1 ms: finer than that is timer noise for a process-spawning tool.
fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 10_000.0).round() / 10.0
}

impl Profile {
    pub fn render(&self) -> String {
        let mut out = format!(
            "total {:.1} ms, {} git spawns, {:.1} ms outside git\n",
            self.total_ms, self.git_spawns, self.unaccounted_ms
        );
        for l in &self.lines {
            out.push_str(&format!("{:>8.1} ms  {:>3}x  {}\n", l.ms, l.count, l.name));
        }
        out
    }
}
