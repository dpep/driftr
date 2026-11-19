# Changelog

## Unreleased

- `driftr log` lists oldest first, so the newest snapshot is last, next to your prompt. Scripts reading `log -j` should take the last row for the latest.
- `.trash` directories are excluded by default. Existing configs keep their own `exclude` list: add `"**/.trash/**"` to it to match.
- Driftr-owned stores pack themselves: after a snapshot, git packs the store in the background once 1000 loose objects pile up. Stores created earlier use git's default of 6700; run `git --git-dir <store> config gc.auto 1000` to match, where `driftr status` shows the store.
- `driftr report --help` explains each column.
- Initial spike: `snapshot`, `log`, `diff`, `restore` and `watch`. Snapshots live on `refs/driftr/snapshots`, built through a private index, so your branch, HEAD and staged changes are never touched.
- Scope: Markdown by default, configurable with globs and `re:` regexes; `driftr track` adds a path, `driftr files` lists what's in scope.
- `driftr hook` for Claude Code: snapshots at turn boundaries tagged with the session, in repos opted in with `driftr init`.
- `driftr status`, `-j`/`-J` on every command, and `restore --dry-run`.
- Linked worktrees keep their own snapshot chain.
- Directories that aren't git repos: `driftr init` tracks one through a store under `~/.local/share/driftr` (or `$DRIFTR_HOME`), adding no `.git` to the directory.
- `driftr untrack` narrows the scope; `track` and `untrack` take several paths.
- `@` names the latest snapshot and `@~N` older ones, wherever an id is taken.
- The hook also snapshots the tracked repo of the file a tool is editing, so `PreToolUse`/`PostToolUse` on `Write|Edit` catch edits outside the session's cwd.
- `driftr gc` rewrites snapshot history to the current scope, e.g. after `untrack`, and prunes the purged content from driftr-owned stores.
- `track dir/` with a trailing slash means a directory even before it exists.
- Safe under concurrent sessions: a per-repo lock spans stage and commit; hooks wait at most 3s, then skip.
- No-op hooks spawn no git: the repo is found directly and a stat fingerprint settles "nothing changed" (an opted-in no-op went from ~100 ms to ~13 ms).
- `--profile` on every command; every hook run is logged with its timings, and `driftr report` summarizes what each hook costs and catches.
- End-of-turn sweep: on `Stop`, every opted-in repo (and worktree) that changed during the turn is snapshotted, throttled to once per 30s per repo. Shell commands naming a path in another opted-in repo snapshot it immediately. Snapshots record `Driftr-Via:` (how the repo was found) and, for sweeps, `Driftr-Candidates:`. Repos opted in earlier join the sweep on their next `snapshot`.
