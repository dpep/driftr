mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{Run, driftr_with, git, init_repo, settle, write};
use serde_json::{Value, json};
use tempfile::TempDir;

/// A home with two opted-in git repos (a, b) and a tracked plain directory (notes).
struct World {
    _root: TempDir,
    home: PathBuf,
    data: PathBuf,
    a: PathBuf,
    b: PathBuf,
    notes: PathBuf,
}

impl World {
    fn new() -> World {
        let root = TempDir::new().unwrap();
        let base = fs::canonicalize(root.path()).unwrap();
        let (home, data) = (base.join("home"), base.join("data"));
        let (a, b, notes) = (home.join("a"), home.join("b"), home.join("notes"));
        for repo in [&a, &b] {
            fs::create_dir_all(repo).unwrap();
            init_repo(repo);
        }
        fs::create_dir_all(&notes).unwrap();
        write(&notes, "n.md", "n\n");
        let w = World {
            _root: root,
            home,
            data,
            a,
            b,
            notes,
        };
        for dir in [&w.a, &w.b, &w.notes] {
            assert_eq!(w.run(dir, &["init"], None).code, 0);
        }
        w
    }

    fn run(&self, dir: &Path, args: &[&str], stdin: Option<&str>) -> Run {
        let env = [
            ("HOME", self.home.to_str().unwrap()),
            ("DRIFTR_HOME", self.data.to_str().unwrap()),
        ];
        driftr_with(dir, args, stdin, &env)
    }

    /// Runs a hook and returns its usage-log record.
    fn hook(&self, payload: Value) -> Value {
        let run = self.run(&self.home, &["hook"], Some(&payload.to_string()));
        assert_eq!(run.stderr, "");
        let log = fs::read_to_string(self.data.join("hooks.jsonl")).unwrap();
        serde_json::from_str(log.lines().last().unwrap()).unwrap()
    }

    fn stop(&self, session: &str, cwd: &Path) -> Value {
        self.hook(json!({"hook_event_name": "Stop", "session_id": session, "cwd": cwd}))
    }

    fn visit<'a>(record: &'a Value, repo: &Path) -> Option<&'a Value> {
        record["visits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["repo"] == repo.to_str().unwrap())
    }

    fn last_body(&self, repo: &Path) -> String {
        git(repo, &["log", "-1", "--format=%B", "refs/driftr/snapshots"])
    }
}

#[test]
fn init_registers_git_repos_but_not_driftr_stores() {
    let w = World::new();
    let names: Vec<String> = fs::read_dir(w.data.join("repos"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.iter().all(|n| n.ends_with("%2F.git")), "{names:?}");
}

#[test]
fn stop_sweeps_changed_repos_outside_the_cwd() {
    let w = World::new();
    write(&w.b, "b.md", "changed by a shell command\n");
    write(&w.notes, "n.md", "also changed\n");

    let record = w.stop("s2", &w.a);
    for repo in [&w.b, &w.notes] {
        let v = World::visit(&record, repo).unwrap_or_else(|| panic!("{record}"));
        assert_eq!(v["via"], "sweep");
        assert!(v["snapshot"].is_string(), "{record}");
    }
    assert!(w.last_body(&w.b).contains("Driftr-Via: sweep"));
}

#[test]
fn a_repo_swept_moments_ago_is_skipped() {
    let w = World::new();
    w.stop("s1", &w.a);
    write(&w.b, "b.md", "changed right after\n");
    let record = w.stop("s2", &w.a);
    assert!(World::visit(&record, &w.b).is_none(), "{record}");
}

#[test]
fn a_sweep_names_sessions_with_strong_evidence_as_candidates() {
    let w = World::new();
    // s1 edits a file in b from a session rooted in a: strong (file) evidence for b.
    write(&w.b, "b.md", "s1's edit\n");
    w.hook(json!({
        "hook_event_name": "PostToolUse", "session_id": "s1", "cwd": w.a,
        "tool_name": "Edit", "tool_input": {"file_path": w.b.join("b.md")},
    }));
    write(&w.b, "b.md", "edited again, by someone\n");

    w.stop("s2", &w.a);
    let body = w.last_body(&w.b);
    assert!(body.contains("Driftr-Via: sweep"), "{body}");
    assert!(body.contains("Driftr-Candidates: s1"), "{body}");
}

#[test]
fn a_shell_command_naming_a_path_snapshots_that_repo() {
    let w = World::new();
    write(&w.b, "b.md", "sed'd\n");
    let record = w.hook(json!({
        "hook_event_name": "PostToolUse", "session_id": "s1", "cwd": w.a, "tool_name": "Bash",
        "tool_input": {"command": format!("sed -i '' s/x/y/ {}", w.b.join("b.md").display())},
    }));
    let v = World::visit(&record, &w.b).unwrap_or_else(|| panic!("{record}"));
    assert_eq!(v["via"], "command");
    assert!(v["snapshot"].is_string(), "{record}");
}

#[test]
fn an_unchanged_world_is_swept_without_git() {
    let w = World::new();
    for dir in [&w.a, &w.b, &w.notes] {
        settle(dir);
        w.run(dir, &["snapshot"], None);
    }
    let record = w.stop("s1", &w.home);
    assert_eq!(record["visits"].as_array().unwrap().len(), 3, "{record}");
    assert_eq!(record["profile"]["git_spawns"], 0, "{record}");
}

#[test]
fn worktrees_of_registered_repos_are_swept() {
    let w = World::new();
    let wt = w.home.join("a-wt");
    git(
        &w.a,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "side"],
    );
    write(&wt, "wt.md", "worktree edit\n");

    let record = w.stop("s1", &w.b);
    let v = World::visit(&record, &wt).unwrap_or_else(|| panic!("{record}"));
    assert!(v["snapshot"].is_string(), "{record}");
}

#[test]
fn entries_for_vanished_repos_are_pruned() {
    let w = World::new();
    fs::remove_dir_all(&w.b).unwrap();
    w.stop("s1", &w.a);
    let left = fs::read_dir(w.data.join("repos")).unwrap().count();
    assert_eq!(left, 1);
}
