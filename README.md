# driftr

Automatic, recoverable working history for AI-assisted development — stored in plain git, without touching your branch, HEAD, or staging area.

Status: spike. See [docs/DESIGN.md](docs/DESIGN.md) for the plan and [docs/decisions.md](docs/decisions.md) for what the spike settled.

## Usage

```
driftr init                               # opt this repo (or plain directory) in
driftr track <path>...                    # add files or directories to the scope
driftr untrack <path|pattern>...          # drop a matching include, else exclude the path
driftr files                              # what's in scope right now
driftr status                             # scope, snapshot count, whether the hook is wired up

driftr snapshot [-m msg] [--session id]   # prints the snapshot id, or nothing if unchanged
driftr log [path] [--session id]          # snapshots, newest last
driftr diff <id> [path]                   # snapshot vs. the current in-scope files
driftr restore <path> --at <id> [--dry-run]   # undoable: snapshots before and after
driftr watch [--idle 30] [--max 300]      # poll; snapshot when the tree settles
driftr gc [--dry-run]                     # rewrite history to the current scope
driftr report [--hours N]                 # what each hook costs and catches
```

Wherever a snapshot id is taken, `@` means the latest and `@~N` the Nth before it. Given a path, they count that path's own versions: `driftr diff @~1 notes/plan.md` compares with the previous version of `plan.md`, however many other snapshots came between.

Restoring a directory makes it match the snapshot: in-scope files added since are removed (out-of-scope files are never touched), exactly as `--dry-run` shows. The state it replaces is snapshotted first and the result after, so `@` is the restored state and the command prints the id to undo with.

Every command takes `-j` (pretty JSON) or `-J` (one JSON object per line), and `--profile` to print where the time went, per git command and phase, to stderr. Exit codes: `0` success, `1` empty result (`log`, `files`) or not enabled (`status`), `2` error, `64` usage error.

Snapshots are ordinary git commits on `refs/driftr/snapshots` (linked worktrees: `refs/driftr/worktrees/<name>/snapshots`), so plain git works on them too:

```
git log refs/driftr/snapshots
git show <id>:path/to/file.md
```

## Scope

By default driftr snapshots Markdown (`**/*.md`) and skips `node_modules` and `.trash`. Gitignored files are never included. The config lives at `.git/driftr/config.toml`, local to your clone and shared by its worktrees:

```toml
[scope]
include = ["**/*.md", "notes.txt", "re:^skills/.*\\.toml$"]
exclude = ["**/node_modules/**", "**/.trash/**"]
```

Patterns match repo-relative paths. Globs are gitignore-like (`*` stays within a directory, `**` crosses them); a `re:` prefix makes a regex. `driftr track` appends to `include`; `driftr untrack` removes a matching include or adds an exclude. To replace the default with an allowlist:

```
driftr untrack '**/*.md'
driftr track CLAUDE.md skills docs
```

Narrowing the scope doesn't touch history already taken. `driftr gc` rewrites the snapshot chain to the current scope, keeping messages, dates and session trailers and dropping snapshots left with nothing to record. In a driftr-owned store (below) the purged content is deleted immediately; in your own repo, git's normal two-week expiry applies, so your own unreachable objects are never pruned early.

## Directories that aren't git repos

`driftr init` outside any git repo tracks the directory through a store kept elsewhere, `~/.local/share/driftr/dirs/<escaped path>.git` (or under `$DRIFTR_HOME` / `$XDG_DATA_HOME`), so the directory gains no `.git`. Everything else works the same, from any subdirectory. A git repo nested inside wins for its own files. `init` refuses `/` and `$HOME`, which would opt in everything beneath them.

## Claude Code

`driftr hook` reads Claude Code's hook JSON on stdin and snapshots the session's cwd repo plus the repo of any `file_path` the tool is touching, tagged `Driftr-Session` and `Driftr-Event`. It acts only where you've run `driftr init`, never writes stdout, and always exits 0. In `~/.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "driftr hook" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "driftr hook" }] }],
    "PreToolUse": [{ "matcher": "Write|Edit|MultiEdit|NotebookEdit", "hooks": [{ "type": "command", "command": "driftr hook" }] }],
    "PostToolUse": [{ "matcher": "Write|Edit|MultiEdit|NotebookEdit|Bash", "hooks": [{ "type": "command", "command": "driftr hook" }] }]
  }
}
```

`UserPromptSubmit` captures edits made between turns and `Stop` the turn itself. The tool events bracket each file edit, including edits to tracked directories outside the session's cwd, such as `~/.claude/skills` edited from a project session. `Bash` on `PostToolUse` attributes edits made through shell commands to the command that made them. A command that names a path in another opted-in repo (`sed -i ~/.claude/skills/...`) snapshots that repo too.

At `Stop`, the hook also sweeps every opted-in repo (each worktree separately) and snapshots any that changed during the turn: the backstop for shell edits the command text didn't reveal. A repo swept by any session in the last 30s is skipped. Each snapshot's `Driftr-Via:` records how its repo was found, strongest evidence first: `cwd`, `file`, `command`, `sweep`. A sweep can't know who made the change, so it also lists `Driftr-Candidates:`, the sessions with stronger evidence in that repo in the last 15 minutes.

Opted-in git repos are registered as symlinks in `~/.local/share/driftr/repos/` (named by path, pointing at the repo's git dir); any `init` or `snapshot` registers, and the sweep prunes entries whose repo is gone. Each clone is its own entry, since identity is the location on disk. Worktrees share their repo's opt-in and are found through it.

Hooks are cheap when nothing changed, which is most of the time. driftr finds the repo without running git and compares a stat fingerprint of the in-scope files with the one saved at the last snapshot, so a no-op hook spawns no git at all. Only a real change pays for a snapshot. Concurrent sessions are serialized by a per-repo lock; a hook waits at most 3s, then skips (the next hook catches up).

Each event is appended to `<git dir>/driftr/events.jsonl`, and every hook run, with its timings and outcome, to `~/.local/share/driftr/hooks.jsonl` (capped). `driftr report` summarizes it per event and tool: runs, how many caught a change, how many the fast path settled, busy skips, errors, and p50/p95 latency.

## Build

```
cargo build --release
```
