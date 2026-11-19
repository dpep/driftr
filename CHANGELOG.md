# Changelog

## 0.2.0 — 2026-11-18

First public release.

- Snapshots: `driftr snapshot` commits the in-scope files to `refs/driftr/snapshots` through a private index, so your branch, HEAD and staged changes are never touched. Linked worktrees keep their own chain. Subjects say what happened ("Add 5 files in notes/old/").
- Recovery: `log` (newest last), `diff`, `show` and `restore`. `@` is the latest snapshot and `@~N` an older one; given a path, they count that path's own versions. `restore` makes a file or directory match the snapshot, exactly as `--dry-run` shows, and snapshots before and after so it can be undone.
- Scope: Markdown by default, skipping `node_modules` and `.trash`; `track` and `untrack` adjust it with paths, globs or `re:` regexes, and `files` lists it. `gc` rewrites history to the current scope.
- Directories that aren't git repos: `driftr init` tracks one through a store under `~/.local/share/driftr`, adding no `.git`. These stores pack themselves in the background.
- Claude Code: `driftr hook` snapshots at turn boundaries and around each edit, in the session's repo and in any opted-in repo a tool or shell command touches, tagged with the session. An end-of-turn sweep catches what the targeted hooks missed. A no-op hook spawns no git, and concurrent sessions are serialized by a per-repo lock.
- `watch` polls and snapshots when the tree settles.
- Observability: `-j`/`-J` on every command, `--profile` for where the time went, and `report` for what each hook costs and catches.
