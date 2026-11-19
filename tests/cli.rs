mod common;

use common::{driftr, driftr_with, git, init_repo, read, write};
use serde_json::Value;
use tempfile::TempDir;

fn repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path());
    tmp
}

#[test]
fn help_lists_every_subcommand_and_the_exit_codes() {
    let tmp = TempDir::new().unwrap();
    let help = driftr(tmp.path(), &["--help"]);
    assert_eq!(help.code, 0);
    for cmd in [
        "init", "track", "untrack", "files", "status", "snapshot", "log", "diff", "restore",
        "watch", "hook",
    ] {
        assert!(help.stdout.contains(&format!("  {cmd} ")), "{cmd} missing");
    }
    assert!(help.stdout.contains("Exit codes:"));
}

#[test]
fn usage_errors_exit_64() {
    let tmp = TempDir::new().unwrap();
    assert_eq!(driftr(tmp.path(), &["nope"]).code, 64);
}

#[test]
fn queries_exit_1_when_empty() {
    let tmp = repo();
    assert_eq!(driftr(tmp.path(), &["log"]).code, 1);
    assert_eq!(driftr(tmp.path(), &["log", "-j"]).stdout.trim(), "[]");
}

#[test]
fn log_renders_json_and_ndjson_rows() {
    let tmp = repo();
    driftr(tmp.path(), &["snapshot"]);
    write(tmp.path(), "a.md", "1\n");
    driftr(tmp.path(), &["snapshot", "--session", "s1"]);

    let rows: Vec<Value> =
        serde_json::from_str(&driftr(tmp.path(), &["log", "-j"]).stdout).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["subject"], "Add a.md", "newest last");

    let ndjson = driftr(tmp.path(), &["log", "-J", "--session", "s1"]).stdout;
    let lines: Vec<Value> = ndjson
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["id"], rows[1]["id"]);
}

#[test]
fn snapshot_json_reports_null_when_unchanged() {
    let tmp = repo();
    let first: Value =
        serde_json::from_str(&driftr(tmp.path(), &["snapshot", "-J"]).stdout).unwrap();
    assert!(first["snapshot"].is_string());
    let again = driftr(tmp.path(), &["snapshot", "-J"]);
    assert_eq!(again.code, 0);
    assert_eq!(again.stdout.trim(), r#"{"snapshot":null}"#);
}

#[test]
fn errors_exit_2_with_json_on_stderr() {
    let tmp = repo();
    let run = driftr(tmp.path(), &["diff", "nope", "-j"]);
    assert_eq!(run.code, 2);
    assert_eq!(run.stdout, "");
    let err: Value = serde_json::from_str(&run.stderr).unwrap();
    assert!(err["error"].as_str().unwrap().contains("unknown snapshot"));
}

#[test]
fn restore_dry_run_changes_nothing() {
    let tmp = repo();
    let id = driftr(tmp.path(), &["snapshot"]).stdout.trim().to_string();
    write(tmp.path(), "README.md", "changed\n");

    let run = driftr(
        tmp.path(),
        &["restore", "README.md", "--at", &id, "--dry-run"],
    );
    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("-changed\n+v1"), "{}", run.stdout);
    assert_eq!(read(tmp.path(), "README.md"), "changed\n");
    assert_eq!(
        driftr(tmp.path(), &["log", "-j"])
            .stdout
            .matches("\"id\"")
            .count(),
        1
    );
}

#[test]
fn status_reports_enablement_and_hook_wiring() {
    let tmp = repo();
    let before = driftr(tmp.path(), &["status", "-j"]);
    assert_eq!(before.code, 1);
    let s: Value = serde_json::from_str(&before.stdout).unwrap();
    assert_eq!(s["enabled"], false);
    assert_eq!(s["hooks"], serde_json::json!([]));

    driftr(tmp.path(), &["init"]);
    write(
        tmp.path(),
        ".claude/settings.json",
        r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"driftr hook"}]}]}}"#,
    );
    let after = driftr(tmp.path(), &["status", "-j"]);
    assert_eq!(after.code, 0);
    let s: Value = serde_json::from_str(&after.stdout).unwrap();
    assert_eq!(s["enabled"], true);
    assert_eq!(s["hooks"], serde_json::json!(["~/.claude/settings.json"]));

    // Wired in the repo rather than globally.
    let home = TempDir::new().unwrap();
    let home = home.path().to_str().unwrap();
    let elsewhere = driftr_with(tmp.path(), &["status", "-j"], None, &[("HOME", home)]);
    let s: Value = serde_json::from_str(&elsewhere.stdout).unwrap();
    let hooks = s["hooks"].as_array().unwrap();
    assert_eq!(hooks.len(), 1);
    assert!(
        hooks[0]
            .as_str()
            .unwrap()
            .ends_with("/.claude/settings.json")
    );
}

#[test]
fn hook_is_silent_and_never_fails() {
    let tmp = repo();
    driftr(tmp.path(), &["init"]);
    write(tmp.path(), "a.md", "1\n");
    let payload = serde_json::json!({
        "session_id": "sess-9",
        "hook_event_name": "Stop",
        "cwd": tmp.path(),
    })
    .to_string();

    // An inherited GIT_DIR must not redirect where the snapshot goes.
    let run = driftr_with(
        tmp.path(),
        &["hook"],
        Some(&payload),
        &[("GIT_DIR", "/nonexistent")],
    );
    assert_eq!((run.code, run.stdout.as_str()), (0, ""), "{}", run.stderr);
    let body = git(
        tmp.path(),
        &["log", "-1", "--format=%B", "refs/driftr/snapshots"],
    );
    assert!(body.contains("Driftr-Session: sess-9"), "{body}");

    let bad = driftr_with(tmp.path(), &["hook"], Some("not json"), &[]);
    assert_eq!((bad.code, bad.stdout.as_str()), (0, ""));
    assert!(bad.stderr.contains("driftr hook:"));
}
