# Decisions

## Snapshots live on a private ref, built through a private index

Each snapshot is a normal git commit on `refs/driftr/snapshots`, staged through `$GIT_DIR/driftr/index` (`GIT_INDEX_FILE`) → `write-tree` → `commit-tree` → `update-ref`. This is how `git stash` builds commits without disturbing anything.

Why not auto-commit to the branch (DESIGN §4.1): that approach causes most of the risks in §8.4 — staged changes, merges and rebases in progress, noisy history, history rewriting to clean up. A private ref sidesteps all of them while keeping snapshots as real git objects: `git log`, `git show`, `git diff` and the reflog all work, and the ref keeps the objects alive through gc.

Consequences:
- Consolidation becomes "commit this snapshot's tree onto the branch" rather than squash and rewrite.
- `update-ref` is compare-and-swap against the previous snapshot, so concurrent snapshotters fail loudly instead of dropping one.
- Snapshot commits are authored by `driftr` and never GPG-signed, so background runs can't prompt.
- The chain doesn't link to HEAD; the HEAD at snapshot time is recorded in a `Driftr-Head:` trailer.

## Snapshotting is idempotent; triggers are interchangeable

`snapshot` is a no-op when the tree matches the last snapshot (or HEAD, before the first one). Any trigger can call it freely: `watch` polling, a Claude Code `Stop` hook, an explicit agent call, and later `notify`. Polling plus "tree unchanged across one interval" gives idle coalescing without filesystem-event debouncing.

## Scope is an allowlist, Markdown by default

Snapshots contain only in-scope files: candidates come from `git ls-files --cached --others --exclude-standard` (so gitignore always wins), then the include/exclude patterns filter them. The private index persists between runs and is pruned to the scope each time, so unchanged files aren't rehashed.

The first snapshot is compared against an empty tree, not HEAD. It records the starting state ("Start tracking N files"); comparing a scoped tree to HEAD's full tree would never match.

Config lives in the common git dir (`.git/driftr/config.toml`): personal, never committed, shared across worktrees. Its existence is the opt-in hooks check, so a global hook never starts writing refs into repos you didn't choose.

## Session attribution starts as commit trailers

`--session <id>` adds `Driftr-Session:`; the hook also adds `Driftr-Event:`. No session store until a query needs one.

The hook makes no author claim. A `UserPromptSubmit` snapshot holds edits made between turns (probably not the agent's) and a `Stop` snapshot holds the turn (probably the agent's, unless someone edited alongside). The event records which boundary took the snapshot; attribution is inferred from it, with that uncertainty kept visible. `events.jsonl` keeps the raw hook stream for Experiment C.

## Worktrees: one shared ref namespace, one chain each

The main worktree uses `refs/driftr/snapshots`; a linked worktree uses `refs/driftr/worktrees/<name>/snapshots`, mirroring `.git/worktrees/<name>`. The private index is per worktree; config and the event log are shared.

Not `refs/worktree/`, the per-worktree namespace git provides: measured, `git gc --prune=now` from the main worktree pruned a linked worktree's snapshot held there (`worktrees_keep_separate_chains_and_share_config`).

## Open

- A removed worktree leaves its ref behind, keeping its objects alive. That needs a `driftr gc` eventually.
- Gitignored files can't be tracked, even explicitly. Fine for now; `track --force` if a real need shows up.
- Snapshot ref growth and pruning policy are unmeasured.
