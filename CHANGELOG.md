# Changelog

## Unreleased

- Initial spike: `snapshot`, `log`, `diff`, `restore` and `watch`. Snapshots live on `refs/driftr/snapshots`, built through a private index, so your branch, HEAD and staged changes are never touched.
- Scope: Markdown by default, configurable with globs and `re:` regexes; `driftr track` adds a path, `driftr files` lists what's in scope.
- `driftr hook` for Claude Code: snapshots at turn boundaries tagged with the session, in repos opted in with `driftr init`.
- `driftr status`, `-j`/`-J` on every command, and `restore --dry-run`.
- Linked worktrees keep their own snapshot chain.
- Directories that aren't git repos: `driftr init` tracks one through a store under `~/.local/share/driftr` (or `$DRIFTR_HOME`), adding no `.git` to the directory.
- `driftr untrack` narrows the scope; `track` and `untrack` take several paths.
- `@` names the latest snapshot and `@~N` older ones, wherever an id is taken.
- The hook also snapshots the tracked repo of the file a tool is editing, so `PreToolUse`/`PostToolUse` on `Write|Edit` catch edits outside the session's cwd.
