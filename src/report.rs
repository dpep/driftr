//! What hooks cost and catch, from the usage log: the evidence for keeping, tuning or
//! dropping each hook (e.g. whether `Bash` on PostToolUse earns its latency).

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct Row {
    pub event: String,
    pub tool: String,
    pub runs: usize,
    /// Runs that took at least one snapshot: changes the hook actually caught.
    pub caught: usize,
    /// Runs where the end-of-turn sweep caught a change the targeted hooks hadn't.
    pub swept: usize,
    /// Runs settled by the fingerprint alone (no git).
    pub fast: usize,
    pub busy: usize,
    pub errors: usize,
    pub p50_ms: f64,
    pub p95_ms: f64,
}

/// One row per (event, tool), busiest first. `since` is a unix time lower bound.
pub fn summarize(records: &[Value], since: u64) -> Vec<Row> {
    let mut groups: BTreeMap<(String, String), Vec<&Value>> = BTreeMap::new();
    for r in records {
        if r["time"].as_u64().unwrap_or(0) < since {
            continue;
        }
        let event = r["event"].as_str().unwrap_or("?").to_string();
        let tool = r["tool"].as_str().unwrap_or("-").to_string();
        groups.entry((event, tool)).or_default().push(r);
    }
    let mut rows: Vec<Row> = groups
        .into_iter()
        .map(|((event, tool), rs)| {
            let visits = |r: &Value| r["visits"].as_array().cloned().unwrap_or_default();
            let any = |r: &Value, f: &dyn Fn(&Value) -> bool| visits(r).iter().any(f);
            let mut ms: Vec<f64> = rs
                .iter()
                .filter_map(|r| r["profile"]["total_ms"].as_f64())
                .collect();
            ms.sort_by(f64::total_cmp);
            Row {
                runs: rs.len(),
                caught: rs
                    .iter()
                    .filter(|r| any(r, &|v| v["snapshot"].is_string()))
                    .count(),
                swept: rs
                    .iter()
                    .filter(|r| any(r, &|v| v["via"] == "sweep" && v["snapshot"].is_string()))
                    .count(),
                fast: rs.iter().filter(|r| any(r, &|v| v["fast"] == true)).count(),
                busy: rs.iter().filter(|r| any(r, &|v| v["busy"] == true)).count(),
                errors: rs.iter().filter(|r| r["error"].is_string()).count(),
                p50_ms: percentile(&ms, 0.5),
                p95_ms: percentile(&ms, 0.95),
                event,
                tool,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.runs.cmp(&a.runs).then(a.event.cmp(&b.event)));
    rows
}

/// Nearest-rank percentile, to 0.1 ms.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let i = ((sorted.len() as f64 * p).ceil() as usize).clamp(1, sorted.len()) - 1;
    (sorted[i] * 10.0).round() / 10.0
}

pub fn render(rows: &[Row]) -> String {
    let mut out = format!(
        "{:<18} {:<14} {:>6} {:>7} {:>6} {:>6} {:>5} {:>6} {:>8} {:>8}\n",
        "event", "tool", "runs", "caught", "swept", "fast", "busy", "errors", "p50 ms", "p95 ms"
    );
    for r in rows {
        out.push_str(&format!(
            "{:<18} {:<14} {:>6} {:>7} {:>6} {:>6} {:>5} {:>6} {:>8.1} {:>8.1}\n",
            r.event,
            r.tool,
            r.runs,
            r.caught,
            r.swept,
            r.fast,
            r.busy,
            r.errors,
            r.p50_ms,
            r.p95_ms
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn groups_by_event_and_tool_and_counts_outcomes() {
        let records = vec![
            json!({"time": 10, "event": "PostToolUse", "tool": "Bash", "profile": {"total_ms": 10.0}, "visits": [{"fast": true}]}),
            json!({"time": 11, "event": "PostToolUse", "tool": "Bash", "profile": {"total_ms": 90.0}, "visits": [{"snapshot": "abc"}]}),
            json!({"time": 12, "event": "PostToolUse", "tool": "Bash", "profile": {"total_ms": 20.0}, "visits": [{"busy": true}]}),
            json!({"time": 13, "event": "Stop", "profile": {"total_ms": 5.0}, "error": "boom"}),
            json!({"time": 14, "event": "Stop", "profile": {"total_ms": 7.0}, "visits": [{"via": "cwd", "fast": true}, {"via": "sweep", "snapshot": "def"}]}),
            json!({"time": 1, "event": "Stop", "profile": {"total_ms": 5.0}}),
        ];
        let rows = summarize(&records, 10);
        assert_eq!(rows.len(), 2);
        let bash = &rows[0];
        assert_eq!(
            (bash.event.as_str(), bash.tool.as_str()),
            ("PostToolUse", "Bash")
        );
        assert_eq!(
            (bash.runs, bash.caught, bash.fast, bash.busy, bash.errors),
            (3, 1, 1, 1, 0)
        );
        assert_eq!((bash.p50_ms, bash.p95_ms), (20.0, 90.0));
        let stop = &rows[1];
        assert_eq!((stop.tool.as_str(), stop.runs, stop.errors), ("-", 2, 1));
        assert_eq!((stop.caught, stop.swept, stop.fast), (1, 1, 1));
    }
}
