mod common;

use std::thread;

use common::{driftr, driftr_with, git, init_repo, write};
use tempfile::TempDir;

const WORKERS: usize = 8;
const ROUNDS: usize = 5;

/// Many sessions snapshotting one repo at once: every call must succeed, nothing may be
/// lost, and the chain must stay linear.
#[test]
fn concurrent_snapshots_all_succeed_and_lose_nothing() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path());
    driftr(tmp.path(), &["init"]);

    thread::scope(|s| {
        for w in 0..WORKERS {
            let dir = tmp.path();
            s.spawn(move || {
                for r in 0..ROUNDS {
                    write(dir, &format!("w{w}.md"), &format!("round {r}\n"));
                    let run = driftr(dir, &["snapshot"]);
                    assert_eq!(run.code, 0, "worker {w} round {r}: {}", run.stderr);
                }
            });
        }
    });

    // Whatever interleaving happened, the latest snapshot holds every worker's last write.
    let last = driftr(tmp.path(), &["snapshot"]); // catches anything raced past the final call
    assert_eq!(last.code, 0, "{}", last.stderr);
    for w in 0..WORKERS {
        let shown = git(
            tmp.path(),
            &["show", &format!("refs/driftr/snapshots:w{w}.md")],
        );
        assert_eq!(shown, format!("round {}", ROUNDS - 1));
    }
    let merges = git(
        tmp.path(),
        &["rev-list", "--min-parents=2", "refs/driftr/snapshots"],
    );
    assert_eq!(merges, "", "chain must stay linear");
}

/// Hooks from parallel sessions never error: a hook that can't get the lock in time skips
/// (recorded as busy in the usage log), and the next hook catches up.
#[test]
fn concurrent_hooks_never_error() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path());
    driftr(tmp.path(), &["init"]);
    let home = tmp.path().join(".driftr-data");
    let home = home.to_str().unwrap();

    thread::scope(|s| {
        for w in 0..WORKERS {
            let dir = tmp.path();
            s.spawn(move || {
                for r in 0..ROUNDS {
                    write(dir, &format!("h{w}.md"), &format!("{r}\n"));
                    let payload = serde_json::json!({
                        "session_id": format!("s{w}"),
                        "hook_event_name": "PostToolUse",
                        "cwd": dir,
                    })
                    .to_string();
                    let run = driftr_with(dir, &["hook"], Some(&payload), &[("DRIFTR_HOME", home)]);
                    assert_eq!(run.stderr, "", "worker {w} round {r}");
                }
            });
        }
    });

    // Every write is in the history once any later hook has run.
    let payload = serde_json::json!({"hook_event_name": "Stop", "cwd": tmp.path()}).to_string();
    driftr_with(
        tmp.path(),
        &["hook"],
        Some(&payload),
        &[("DRIFTR_HOME", home)],
    );
    for w in 0..WORKERS {
        let shown = git(
            tmp.path(),
            &["show", &format!("refs/driftr/snapshots:h{w}.md")],
        );
        assert_eq!(shown, format!("{}", ROUNDS - 1));
    }
    let log = std::fs::read_to_string(tmp.path().join(".driftr-data/hooks.jsonl")).unwrap();
    assert_eq!(
        log.lines().count(),
        WORKERS * ROUNDS + 1,
        "every run is logged"
    );
}
