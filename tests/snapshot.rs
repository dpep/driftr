mod common;

use std::fs;
use std::path::Path;

use common::{git, init_repo, read, write};
use driftr::{Repo, SnapshotOpts, Untrack};
use tempfile::TempDir;

/// A repo with one commit containing README.md.
fn repo() -> (TempDir, Repo) {
    let tmp = TempDir::new().unwrap();
    init_repo(tmp.path());
    let repo = Repo::open(tmp.path()).unwrap();
    (tmp, repo)
}

fn snap(repo: &Repo) -> Option<String> {
    repo.snapshot(&SnapshotOpts::default()).unwrap()
}

fn tree_files(dir: &Path, rev: &str) -> Vec<String> {
    git(dir, &["ls-tree", "-r", "--name-only", rev])
        .lines()
        .map(String::from)
        .collect()
}

#[test]
fn first_snapshot_records_the_starting_state() {
    let (tmp, repo) = repo();
    let id = snap(&repo).unwrap();
    let subject = git(tmp.path(), &["log", "-1", "--format=%s", &id]);
    assert_eq!(subject, "Start tracking 1 file");
    assert_eq!(snap(&repo), None);
}

#[test]
fn captures_modified_and_untracked_files() {
    let (tmp, repo) = repo();
    write(tmp.path(), "README.md", "v2\n");
    write(tmp.path(), "notes/new.md", "fresh\n");

    let id = snap(&repo).unwrap();
    assert_eq!(git(tmp.path(), &["show", &format!("{id}:README.md")]), "v2");
    assert_eq!(
        git(tmp.path(), &["show", &format!("{id}:notes/new.md")]),
        "fresh"
    );
}

#[test]
fn unchanged_tree_is_a_noop() {
    let (tmp, repo) = repo();
    write(tmp.path(), "README.md", "v2\n");
    let first = snap(&repo).unwrap();
    assert_eq!(snap(&repo), None);
    assert_eq!(repo.last().unwrap(), Some(first));
}

#[test]
fn snapshots_chain_and_carry_trailers() {
    let (tmp, repo) = repo();
    write(tmp.path(), "README.md", "v2\n");
    let first = snap(&repo).unwrap();
    write(tmp.path(), "README.md", "v3\n");
    let second = repo
        .snapshot(&SnapshotOpts::session("abc-123"))
        .unwrap()
        .unwrap();

    assert_eq!(
        git(tmp.path(), &["rev-parse", &format!("{second}^")]),
        first
    );
    let head = git(tmp.path(), &["rev-parse", "HEAD"]);
    let body = git(tmp.path(), &["log", "-1", "--format=%B", &second]);
    assert!(body.contains(&format!("Driftr-Head: {head}")), "{body}");
    assert!(body.contains("Driftr-Session: abc-123"), "{body}");
}

#[test]
fn leaves_head_branch_and_user_index_alone() {
    let (tmp, repo) = repo();
    write(tmp.path(), "README.md", "staged\n");
    git(tmp.path(), &["add", "README.md"]);
    write(tmp.path(), "README.md", "unstaged\n");
    write(tmp.path(), "untracked.md", "u\n");

    let before = (
        git(tmp.path(), &["rev-parse", "HEAD"]),
        git(tmp.path(), &["symbolic-ref", "HEAD"]),
        git(tmp.path(), &["status", "--porcelain"]),
        git(tmp.path(), &["diff", "--cached"]),
    );
    let id = snap(&repo).unwrap();
    let after = (
        git(tmp.path(), &["rev-parse", "HEAD"]),
        git(tmp.path(), &["symbolic-ref", "HEAD"]),
        git(tmp.path(), &["status", "--porcelain"]),
        git(tmp.path(), &["diff", "--cached"]),
    );
    assert_eq!(before, after);
    // Snapshots stay off every branch.
    assert_eq!(git(tmp.path(), &["branch", "--contains", &id]), "");
    assert_eq!(git(tmp.path(), &["rev-parse", repo.snapshot_ref()]), id);
}

#[test]
fn respects_gitignore() {
    let (tmp, repo) = repo();
    write(tmp.path(), ".gitignore", "secret.md\n");
    write(tmp.path(), "secret.md", "shh\n");
    let id = snap(&repo).unwrap();
    assert_eq!(tree_files(tmp.path(), &id), ["README.md"]);
}

#[test]
fn works_before_the_first_commit() {
    let tmp = TempDir::new().unwrap();
    git(tmp.path(), &["init", "-q"]);
    let repo = Repo::open(tmp.path()).unwrap();
    assert_eq!(snap(&repo), None);

    write(tmp.path(), "draft.md", "hello\n");
    let id = snap(&repo).unwrap();
    let body = git(tmp.path(), &["log", "-1", "--format=%B", &id]);
    assert!(body.starts_with("Start tracking 1 file"), "{body}");
    assert!(body.contains("Driftr-Head: none"), "{body}");
}

#[test]
fn describes_changes_deterministically() {
    let (tmp, repo) = repo();
    write(tmp.path(), "README.md", "v2\n");
    snap(&repo).unwrap();
    fs::remove_file(tmp.path().join("README.md")).unwrap();
    snap(&repo).unwrap();
    for name in ["a.md", "b.md", "c.md", "d.md"] {
        write(tmp.path(), name, "x\n");
    }
    snap(&repo).unwrap();

    let subjects: Vec<String> = repo
        .log(None, None)
        .unwrap()
        .into_iter()
        .map(|e| e.subject)
        .collect();
    assert_eq!(
        subjects,
        [
            "Update a.md, b.md, c.md (+1 more)",
            "Delete README.md",
            "Start tracking 1 file"
        ]
    );
}

#[test]
fn log_filters_by_path() {
    let (tmp, repo) = repo();
    write(tmp.path(), "a.md", "1\n");
    let a = snap(&repo).unwrap();
    write(tmp.path(), "b.md", "1\n");
    snap(&repo).unwrap();

    let ids: Vec<String> = repo
        .log(Some("a.md"), None)
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids.len(), 1);
    assert!(a.starts_with(&ids[0]));
}

#[test]
fn diff_includes_untracked_files() {
    let (tmp, repo) = repo();
    write(tmp.path(), "README.md", "v2\n");
    let id = snap(&repo).unwrap();
    write(tmp.path(), "new.md", "brand new\n");
    let diff = repo.diff(&id, None).unwrap();
    assert!(diff.contains("+brand new"), "{diff}");
}

#[test]
fn restore_recovers_a_rewritten_file_and_saves_the_rewrite() {
    let (tmp, repo) = repo();
    write(tmp.path(), "README.md", "good\n");
    let good = snap(&repo).unwrap();
    write(tmp.path(), "README.md", "clobbered\n");

    let safety = repo.restore(&good, "README.md").unwrap().unwrap();
    assert_eq!(read(tmp.path(), "README.md"), "good\n");
    assert_eq!(
        git(tmp.path(), &["show", &format!("{safety}:README.md")]),
        "clobbered"
    );
}

#[test]
fn restore_recovers_a_deleted_file() {
    let (tmp, repo) = repo();
    write(tmp.path(), "notes.md", "keep me\n");
    let id = snap(&repo).unwrap();
    fs::remove_file(tmp.path().join("notes.md")).unwrap();

    repo.restore(&id, "notes.md").unwrap();
    assert_eq!(read(tmp.path(), "notes.md"), "keep me\n");
}

#[test]
fn restore_rejects_a_path_missing_from_the_snapshot() {
    let (tmp, repo) = repo();
    write(tmp.path(), "a.md", "1\n");
    let id = snap(&repo).unwrap();
    let err = repo.restore(&id, "nope.md").unwrap_err();
    assert!(err.to_string().contains("does not exist"), "{err}");
}

#[test]
fn default_scope_is_markdown_only() {
    let (tmp, repo) = repo();
    write(tmp.path(), "src/main.rs", "fn main() {}\n");
    write(tmp.path(), "docs/plan.md", "plan\n");
    write(tmp.path(), "node_modules/pkg/README.md", "vendored\n");

    let id = snap(&repo).unwrap();
    assert_eq!(tree_files(tmp.path(), &id), ["README.md", "docs/plan.md"]);
}

#[test]
fn track_adds_a_file_outside_the_default_scope() {
    let (tmp, repo) = repo();
    write(tmp.path(), "notes.txt", "hi\n");
    assert_eq!(repo.track("notes.txt").unwrap(), "notes.txt");
    assert!(repo.enabled());
    assert_eq!(repo.files().unwrap(), ["README.md", "notes.txt"]);
}

#[test]
fn track_resolves_paths_from_a_subdirectory() {
    let (tmp, _) = repo();
    write(tmp.path(), "app/config/settings.toml", "x = 1\n");
    let repo = Repo::open(&tmp.path().join("app")).unwrap();

    assert_eq!(repo.track("config/").unwrap(), "app/config/**");
    assert!(
        repo.files()
            .unwrap()
            .contains(&"app/config/settings.toml".into())
    );
}

#[test]
fn track_treats_glob_characters_in_names_literally() {
    let (tmp, repo) = repo();
    write(tmp.path(), "weird[1].txt", "a\n");
    write(tmp.path(), "weird1.txt", "b\n");
    repo.track("weird[1].txt").unwrap();
    assert_eq!(repo.files().unwrap(), ["README.md", "weird[1].txt"]);
    let id = snap(&repo).unwrap();
    assert_eq!(tree_files(tmp.path(), &id), ["README.md", "weird[1].txt"]);
}

#[test]
fn files_leaving_scope_drop_out_of_the_next_snapshot() {
    let (tmp, repo) = repo();
    write(tmp.path(), "draft.md", "d\n");
    snap(&repo).unwrap();

    repo.init().unwrap();
    let mut config = repo.config().unwrap();
    config.scope.exclude.push("draft.md".into());
    config.save(&repo.config_path()).unwrap();

    let id = snap(&repo).unwrap();
    assert_eq!(tree_files(tmp.path(), &id), ["README.md"]);
    assert_eq!(
        git(tmp.path(), &["log", "-1", "--format=%s", &id]),
        "Delete draft.md"
    );
}

#[test]
fn log_filters_by_session() {
    let (tmp, repo) = repo();
    snap(&repo).unwrap();
    write(tmp.path(), "a.md", "1\n");
    let mine = repo
        .snapshot(&SnapshotOpts::session("s1"))
        .unwrap()
        .unwrap();
    write(tmp.path(), "b.md", "1\n");
    repo.snapshot(&SnapshotOpts::session("s10")).unwrap();

    let ids: Vec<String> = repo
        .log(None, Some("s1"))
        .unwrap()
        .into_iter()
        .map(|e| e.id)
        .collect();
    assert_eq!(ids.len(), 1);
    assert!(mine.starts_with(&ids[0]));
}

fn hook_input(dir: &Path, event: &str) -> String {
    serde_json::json!({
        "session_id": "sess-1",
        "hook_event_name": event,
        "cwd": dir,
        "tool_name": "Write",
        "tool_input": {"file_path": dir.join("a.md")},
    })
    .to_string()
}

#[test]
fn hook_ignores_repos_that_have_not_opted_in() {
    let (tmp, repo) = repo();
    write(tmp.path(), "a.md", "1\n");
    assert!(
        driftr::hook::run(&hook_input(tmp.path(), "Stop"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(repo.last().unwrap(), None);
}

#[test]
fn hook_ignores_directories_outside_git() {
    let tmp = TempDir::new().unwrap();
    assert!(
        driftr::hook::run(&hook_input(tmp.path(), "Stop"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hook_snapshots_with_session_and_records_the_event() {
    let (tmp, repo) = repo();
    repo.init().unwrap();
    write(tmp.path(), "a.md", "1\n");

    let [id] = &driftr::hook::run(&hook_input(tmp.path(), "Stop")).unwrap()[..] else {
        panic!("expected one snapshot");
    };
    let body = git(tmp.path(), &["log", "-1", "--format=%B", id]);
    assert!(!body.contains("Driftr-Agent"), "{body}");
    for trailer in ["Driftr-Session: sess-1", "Driftr-Event: Stop"] {
        assert!(body.contains(trailer), "{body}");
    }

    let events = read(tmp.path(), ".git/driftr/events.jsonl");
    let event: serde_json::Value = serde_json::from_str(events.trim()).unwrap();
    assert_eq!(event["event"], "Stop");
    assert_eq!(event["session"], "sess-1");
    assert_eq!(event["snapshot"], id.as_str());
}

#[test]
fn hook_snapshots_the_repo_of_the_file_being_edited() {
    let (tmp, repo) = repo();
    repo.init().unwrap();
    write(tmp.path(), "skills/new.md", "draft\n");
    let elsewhere = TempDir::new().unwrap();
    let payload = serde_json::json!({
        "session_id": "sess-2",
        "hook_event_name": "PostToolUse",
        "cwd": elsewhere.path(),
        "tool_name": "Write",
        "tool_input": {"file_path": tmp.path().join("skills/new.md")},
    });
    let taken = driftr::hook::run(&payload.to_string()).unwrap();
    assert_eq!(taken.len(), 1);
    assert_eq!(repo.last().unwrap().as_ref(), taken.first());

    // A file in the cwd repo is snapshotted once, not twice.
    write(tmp.path(), "skills/new.md", "edited\n");
    let same = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "cwd": tmp.path(),
        "tool_input": {"file_path": tmp.path().join("skills/new.md")},
    });
    assert_eq!(driftr::hook::run(&same.to_string()).unwrap().len(), 1);

    // Before a Write creates a file in a new directory, its nearest existing ancestor counts.
    let pre = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "cwd": elsewhere.path(),
        "tool_input": {"file_path": tmp.path().join("brand/new/dir/x.md")},
    });
    write(tmp.path(), "skills/new.md", "again\n");
    assert_eq!(driftr::hook::run(&pre.to_string()).unwrap().len(), 1);
}

#[test]
fn worktrees_keep_separate_chains_and_share_config() {
    let (tmp, main) = repo();
    let linked_dir = tmp.path().join("linked");
    git(
        tmp.path(),
        &["worktree", "add", "-q", "linked", "-b", "side"],
    );
    let linked = Repo::open(&linked_dir).unwrap();

    write(&linked_dir, "notes.txt", "side\n");
    main.track("notes.txt").unwrap();
    assert_eq!(linked.files().unwrap(), ["README.md", "notes.txt"]);

    let a = snap(&main).unwrap();
    let b = snap(&linked).unwrap();
    assert_eq!(main.last().unwrap(), Some(a));
    assert_eq!(linked.last().unwrap(), Some(b.clone()));

    // gc from the main worktree must not prune the linked worktree's snapshots.
    git(tmp.path(), &["reflog", "expire", "--expire=now", "--all"]);
    git(tmp.path(), &["gc", "-q", "--prune=now"]);
    assert_eq!(git(tmp.path(), &["cat-file", "-t", &b]), "commit");
}

#[test]
fn linked_worktree_snapshots_live_under_a_shared_namespace() {
    let (tmp, main) = repo();
    git(tmp.path(), &["worktree", "add", "-q", "wt", "-b", "side"]);
    let linked = Repo::open(&tmp.path().join("wt")).unwrap();
    assert_eq!(main.snapshot_ref(), "refs/driftr/snapshots");
    assert_eq!(linked.snapshot_ref(), "refs/driftr/worktrees/wt/snapshots");
}

#[test]
fn untrack_removes_a_matching_include_else_excludes() {
    let (tmp, repo) = repo();
    write(tmp.path(), "docs/a.md", "a\n");
    write(tmp.path(), "notes.txt", "n\n");
    repo.track("notes.txt").unwrap();

    assert_eq!(
        repo.untrack("**/*.md").unwrap(),
        Untrack::Removed("**/*.md".into())
    );
    assert_eq!(repo.files().unwrap(), ["notes.txt"]);

    repo.track("docs").unwrap();
    assert_eq!(
        repo.untrack("docs/a.md").unwrap(),
        Untrack::Excluded("docs/a.md".into())
    );
    assert_eq!(repo.files().unwrap(), ["notes.txt"]);
    assert_eq!(
        repo.untrack("notes.txt").unwrap(),
        Untrack::Removed("notes.txt".into())
    );
}

#[test]
fn at_names_the_latest_snapshot_and_its_ancestors() {
    let (tmp, repo) = repo();
    write(tmp.path(), "README.md", "v2\n");
    snap(&repo).unwrap();
    write(tmp.path(), "README.md", "v3\n");
    snap(&repo).unwrap();
    write(tmp.path(), "README.md", "v4\n");

    assert!(repo.diff("@", None).unwrap().contains("-v3\n+v4"));
    repo.restore("@~1", "README.md").unwrap();
    assert_eq!(read(tmp.path(), "README.md"), "v2\n");
}
