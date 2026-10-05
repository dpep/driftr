# Changelog

## Unreleased

- Initial spike: `snapshot`, `log`, `diff`, `restore` and `watch`. Snapshots live on `refs/driftr/snapshots`, built through a private index, so your branch, HEAD and staged changes are never touched.
- Scope: Markdown by default, configurable with globs and `re:` regexes; `driftr track` adds a path, `driftr files` lists what's in scope.
- `driftr hook` for Claude Code: snapshots at turn boundaries tagged with the session, in repos opted in with `driftr init`.
- `driftr status`, `-j`/`-J` on every command, and `restore --dry-run`.
- Linked worktrees keep their own snapshot chain.
