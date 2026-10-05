# driftr

Automatic, recoverable working history for AI-assisted development — stored in plain git, without touching your branch, HEAD, or staging area.

Status: spike. See [docs/DESIGN.md](docs/DESIGN.md) for the plan and [docs/decisions.md](docs/decisions.md) for what the spike settled.

## Usage

```
driftr init                               # opt this repo in (writes the default scope config)
driftr track <path>                       # add a file or directory to the scope
driftr files                              # what's in scope right now
driftr status                             # scope, snapshot count, whether the hook is wired up

driftr snapshot [-m msg] [--session id]   # prints the snapshot id, or nothing if unchanged
driftr log [path] [--session id]          # snapshots, newest first
driftr diff <id> [path]                   # snapshot vs. the current in-scope files
driftr restore <path> --at <id> [--dry-run]   # snapshots what it overwrites first
driftr watch [--idle 30] [--max 300]      # poll; snapshot when the tree settles
```

Every command takes `-j` (pretty JSON) or `-J` (one JSON object per line). Exit codes: `0` success, `1` empty result (`log`, `files`) or not enabled (`status`), `2` error, `64` usage error.

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

Patterns match repo-relative paths. Globs are gitignore-like (`*` stays within a directory, `**` crosses them); a `re:` prefix makes a regex. `driftr track` appends to `include`.

## Claude Code

`driftr hook` reads Claude Code's hook JSON on stdin and snapshots at turn boundaries, tagged `Driftr-Session` and `Driftr-Event`. It acts only in repos where you've run `driftr init`, never writes stdout, and always exits 0. In `~/.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "driftr hook" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "driftr hook" }] }]
  }
}
```

The `UserPromptSubmit` snapshot captures edits made between turns, and `Stop` captures the turn itself. Each event is also appended to `.git/driftr/events.jsonl`, including the `file_path` of tool calls when wired to `PostToolUse`.

## Build

```
cargo build --release
```
