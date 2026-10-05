mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{Run, driftr_with, git, init_repo, read, write};
use serde_json::Value;
use tempfile::TempDir;

/// A plain (non-git) directory to track, with HOME and the data dir as siblings.
struct Env {
    _root: TempDir,
    home: PathBuf,
    data: PathBuf,
    work: PathBuf,
}

impl Env {
    fn new() -> Env {
        let root = TempDir::new().unwrap();
        let base = fs::canonicalize(root.path()).unwrap();
        let (home, data, work) = (
            base.join("home"),
            base.join("data"),
            base.join("home/notes"),
        );
        fs::create_dir_all(&work).unwrap();
        write(&work, "plan.md", "v1\n");
        Env {
            _root: root,
            home,
            data,
            work,
        }
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Run {
        self.run_with(dir, args, None)
    }

    fn run_with(&self, dir: &Path, args: &[&str], stdin: Option<&str>) -> Run {
        let (home, data) = (self.home.to_str().unwrap(), self.data.to_str().unwrap());
        driftr_with(dir, args, stdin, &[("HOME", home), ("DRIFTR_HOME", data)])
    }

    fn json(&self, dir: &Path, args: &[&str]) -> Value {
        let run = self.run(dir, args);
        assert_eq!(run.code, 0, "{args:?}: {}", run.stderr);
        serde_json::from_str(&run.stdout).unwrap()
    }
}

#[test]
fn untracked_plain_directory_points_at_init() {
    let env = Env::new();
    let run = env.run(&env.work, &["snapshot"]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("driftr init"), "{}", run.stderr);
}

#[test]
fn init_tracks_a_plain_directory_without_adding_dot_git() {
    let env = Env::new();
    let init = env.json(&env.work, &["init", "-j"]);
    assert_eq!(init["created"], true);
    let store = PathBuf::from(init["store"].as_str().unwrap());
    assert!(
        store.starts_with(env.data.join("dirs")),
        "{}",
        store.display()
    );
    assert!(!env.work.join(".git").exists());

    let snap = env.json(&env.work, &["snapshot", "-J"]);
    assert!(snap["snapshot"].is_string());
    let log: Vec<Value> = serde_json::from_value(env.json(&env.work, &["log", "-j"])).unwrap();
    assert_eq!(log[0]["subject"], "Start tracking 1 file");

    let status = env.json(&env.work, &["status", "-j"]);
    assert_eq!(status["enabled"], true);
    assert_eq!(status["repo"], "~/notes");
}

#[test]
fn subdirectories_resolve_to_the_tracked_ancestor() {
    let env = Env::new();
    env.run(&env.work, &["init"]);
    write(&env.work, "deep/dir/note.md", "n\n");
    write(&env.work, "deep/dir/data.txt", "d\n");
    let sub = env.work.join("deep/dir");

    let track = env.json(&sub, &["track", "data.txt", "-j"]);
    assert_eq!(track, serde_json::json!(["deep/dir/data.txt"]));
    let files = env.run(&sub, &["files"]).stdout;
    assert_eq!(files, "deep/dir/data.txt\ndeep/dir/note.md\nplan.md\n");
}

#[test]
fn restore_works_in_a_plain_directory() {
    let env = Env::new();
    env.run(&env.work, &["init"]);
    let id = env.run(&env.work, &["snapshot"]).stdout.trim().to_string();
    write(&env.work, "plan.md", "clobbered\n");

    let run = env.run(&env.work, &["restore", "plan.md", "--at", &id]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(read(&env.work, "plan.md"), "v1\n");
}

#[test]
fn honors_gitignore_in_a_plain_directory() {
    let env = Env::new();
    write(&env.work, ".gitignore", "private.md\n");
    write(&env.work, "private.md", "shh\n");
    env.run(&env.work, &["init"]);
    assert_eq!(env.run(&env.work, &["files"]).stdout, "plan.md\n");
}

#[test]
fn a_nested_git_repo_owns_its_own_files() {
    let env = Env::new();
    env.run(&env.work, &["init"]);
    let nested = env.work.join("project");
    fs::create_dir_all(&nested).unwrap();
    init_repo(&nested);

    // The outer store skips the nested repo's files...
    assert_eq!(env.run(&env.work, &["files"]).stdout, "plan.md\n");
    // ...and inside it, the real repo wins (not opted in, so hooks would leave it alone).
    let run = env.run(&nested, &["status", "-j"]);
    assert_eq!(run.code, 1, "{}", run.stderr);
    let status: Value = serde_json::from_str(&run.stdout).unwrap();
    assert_eq!(status["enabled"], false);
    assert!(status["store"].as_str().unwrap().ends_with("project/.git"));
    git(&nested, &["status"]);
}

#[test]
fn hook_snapshots_a_tracked_plain_directory() {
    let env = Env::new();
    env.run(&env.work, &["init"]);
    let payload = serde_json::json!({
        "session_id": "s-ext",
        "hook_event_name": "Stop",
        "cwd": env.work.join("."),
    })
    .to_string();
    let run = env.run_with(&env.home, &["hook"], Some(&payload));
    assert_eq!((run.code, run.stdout.as_str()), (0, ""), "{}", run.stderr);

    let log = env.run(&env.work, &["log", "--session", "s-ext"]);
    assert_eq!(log.code, 0, "{}", log.stderr);
}

#[test]
fn refuses_to_track_home() {
    let env = Env::new();
    let run = env.run(&env.home, &["init"]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("refusing to track"), "{}", run.stderr);
    assert!(!env.data.join("dirs").exists());
}

#[test]
fn rejects_a_relative_data_dir() {
    let env = Env::new();
    let home = env.home.to_str().unwrap();
    let run = driftr_with(
        &env.work,
        &["init"],
        None,
        &[("HOME", home), ("DRIFTR_HOME", "relative/data")],
    );
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("must be absolute"), "{}", run.stderr);
}

#[test]
fn gc_purges_untracked_content_from_an_external_store() {
    let env = Env::new();
    write(&env.work, "bin/blob", "big binary\n");
    env.run(&env.work, &["init"]);
    env.run(&env.work, &["track", "bin/"]);
    env.run(&env.work, &["snapshot"]);
    let store = env.json(&env.work, &["status", "-j"])["store"]
        .as_str()
        .unwrap()
        .replace('~', env.home.to_str().unwrap());
    let blob = git(&env.work, &["hash-object", "bin/blob"]);

    env.run(&env.work, &["untrack", "bin/"]);
    let report = env.json(&env.work, &["gc", "-j"]);
    assert_eq!(report["pruned"], true);
    assert_eq!(report["removed_paths"], serde_json::json!(["bin/blob"]));

    let gone = std::process::Command::new("git")
        .args(["--git-dir", &store, "cat-file", "-e", &blob])
        .status()
        .unwrap();
    assert!(!gone.success(), "blob still in the store");
    assert_eq!(env.run(&env.work, &["log"]).stdout.lines().count(), 1);
}
