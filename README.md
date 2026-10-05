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
driftr log [path] [--session id]          # snapshots, newest first
driftr diff <id> [path]                   # snapshot vs. the current in-scope files
driftr restore <path> --at <id> [--dry-run]   # snapshots what it overwrites first
driftr watch [--idle 30] [--max 300]      # poll; snapshot when the tree settles
driftr gc [--dry-run]                     # rewrite history to the current scope
driftr report [--hours N]                 # what each hook costs and catches
```

Wherever a snapshot id is taken, `@` means the latest and `@~N` the Nth before it.

Every command takes `-j` (pretty JSON) or `-J` (one JSON object per line), and `--profile` to print where the time went, per git command and phase, to stderr. Exit codes: `0` success, `1` empty result (`log`, `files`) or not enabled (`status`), `2` error, `64` usage error.

Snapshots are ordinary git commits on `refs/driftr/snapshots` (linked worktrees: `refs/driftr/worktrees/<name>/snapshots`), so plain git works on them too:

```
git log refs/driftr/snapshots
git show <id>:path/to/file.md
```

## Scope

By default driftr snapshots Markdown (`**/*.md`) and skips `node_modules`. Gitignored files are never included. The config lives at `.git/driftr/config.toml`, local to your clone and shared by its worktrees:

```toml
[scope]
include = ["**/*.md", "notes.txt", "re:^skills/.*\\.toml$"]
exclude = ["**/node_modules/**"]
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

`UserPromptSubmit` captures edits made between turns and `Stop` the turn itself. The tool events bracket each file edit, including edits to tracked directories outside the session's cwd, such as `~/.claude/skills` edited from a project session. `Bash` on `PostToolUse` attributes edits made through shell commands to the command that made them.

Hooks are cheap when nothing changed, which is most of the time. driftr finds the repo without running git and compares a stat fingerprint of the in-scope files with the one saved at the last snapshot, so a no-op hook spawns no git at all. Only a real change pays for a snapshot. Concurrent sessions are serialized by a per-repo lock; a hook waits at most 3s, then skips (the next hook catches up).

Each event is appended to `<git dir>/driftr/events.jsonl`, and every hook run, with its timings and outcome, to `~/.local/share/driftr/hooks.jsonl` (capped). `driftr report` summarizes it per event and tool: runs, how many caught a change, how many the fast path settled, busy skips, errors, and p50/p95 latency.

## Build

```
cargo build --release
```
