mod common;

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use common::{driftr, driftr_with, git, init_repo, write};
use driftr::Repo;
use driftr::fast::discover;
use serde_json::Value;
use tempfile::TempDir;

static SETTLES: AtomicU64 = AtomicU64::new(0);

/// Backdates every file under `dir` (but not .git) so the fingerprint trusts their stat.
/// Each call lands on a distinct second: identical backdated times would fool git's own stat
/// cache on a same-size edit, which real (forward-moving) mtimes never do.
fn settle(dir: &Path) {
    let n = SETTLES.fetch_add(1, Ordering::SeqCst);
    let past = SystemTime::now() - Duration::from_secs(1000) + Duration::from_secs(5 * n);
    settle_at(dir, past);
}

fn settle_at(dir: &Path, past: SystemTime) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() == ".git" {
            continue;
        }
        if path.is_dir() {
            settle_at(&path, past);
        } else {
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(past)
                .unwrap();
        }
    }
}

/// Runs the hook as a session would and returns its usage-log record.
fn hook(dir: &Path) -> Value {
    let payload =
        serde_json::json!({"hook_event_name": "PostToolUse", "cwd": dir, "tool_name": "Bash"});
    let home = dir.join(".driftr-data");
    let run = driftr_with(
        dir,
        &["hook"],
        Some(&payload.to_string()),
        &[("DRIFTR_HOME", home.to_str().unwrap())],
    );
    assert_eq!(run.stderr, "");
    let log = fs::read_to_string(home.join("hooks.jsonl")).unwrap();
    serde_json::from_str(log.lines().last().unwrap()).unwrap()
}

fn enabled_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path());
    fs::write(tmp.path().join(".gitignore"), ".driftr-data/\n").unwrap();
    driftr(tmp.path(), &["init"]);
    tmp
}

fn canon(p: &Path) -> std::path::PathBuf {
    fs::canonicalize(p).unwrap()
}

#[test]
fn discover_agrees_with_git() {
    let tmp = TempDir::new().unwrap();
    let root = canon(tmp.path());
    let repo_dir = root.join("repo");
    fs::create_dir_all(repo_dir.join("sub/deeper")).unwrap();
    init_repo(&repo_dir);
    git(&repo_dir, &["worktree", "add", "-q", "../wt", "-b", "side"]);

    for dir in [
        repo_dir.clone(),
        repo_dir.join("sub/deeper"),
        root.join("wt"),
    ] {
        let found = discover(&dir).unwrap();
        let repo = Repo::open(&dir).unwrap();
        assert_eq!(found.top, canon(repo.top()), "{}", dir.display());
        assert_eq!(found.git_dir, canon(repo.git_dir()), "{}", dir.display());
    }
    assert_eq!(
        discover(&root.join("wt")).unwrap().common_dir,
        repo_dir.join(".git")
    );
    assert_eq!(discover(&root), None);
}

#[test]
fn unchanged_tree_settles_without_any_git() {
    let tmp = enabled_repo();
    settle(tmp.path());
    driftr(tmp.path(), &["snapshot"]);

    let record = hook(tmp.path());
    assert_eq!(record["visits"][0]["fast"], true, "{record}");
    assert_eq!(record["profile"]["git_spawns"], 0, "{record}");
}

#[test]
fn repos_not_opted_in_cost_no_git() {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path());
    let record = hook(tmp.path());
    assert_eq!(record["visits"], serde_json::json!([]));
    assert_eq!(record["profile"]["git_spawns"], 0, "{record}");
}

#[test]
fn every_kind_of_change_falls_through_to_a_snapshot() {
    let tmp = enabled_repo();
    let dir = tmp.path();
    type Change<'a> = (&'a str, Box<dyn Fn() + 'a>);
    let changes: [Change; 4] = [
        (
            "same-size edit",
            Box::new(|| write(dir, "README.md", "v2\n")),
        ),
        ("new file", Box::new(|| write(dir, "new.md", "n\n"))),
        (
            "deletion",
            Box::new(|| fs::remove_file(dir.join("new.md")).unwrap()),
        ),
        (
            "scope change",
            Box::new(|| {
                write(dir, "notes.txt", "t\n");
                settle(dir);
                driftr(dir, &["track", "notes.txt"]);
            }),
        ),
    ];
    for (what, change) in changes {
        settle(dir);
        driftr(dir, &["snapshot"]);
        assert_eq!(hook(dir)["visits"][0]["fast"], true, "{what}: baseline");

        change();
        settle(dir);
        let record = hook(dir);
        assert_eq!(record["visits"][0]["fast"], Value::Null, "{what}: {record}");
        assert!(
            record["visits"][0]["snapshot"].is_string(),
            "{what}: {record}"
        );
    }
}

#[test]
fn recently_modified_files_are_not_trusted() {
    let tmp = enabled_repo();
    driftr(tmp.path(), &["snapshot"]); // files were just written: too fresh to fingerprint
    let record = hook(tmp.path());
    assert_eq!(record["visits"][0]["fast"], Value::Null, "{record}");
}
