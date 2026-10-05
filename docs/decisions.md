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

## Plain directories: an external store named by its path

Outside git, `init` creates a bare repo at `<data>/dirs/<escaped path>.git` with `core.worktree` pointing back, and driftr runs git with `GIT_DIR`/`GIT_WORK_TREE` set. Everything above `Repo::open` is unchanged. The store's name is the path (`%` and `/` escaped), so finding a tracked ancestor is one `stat` per ancestor: no registry to keep in sync. The directory gains no `.git`, so editors, other tools and enclosing repos never notice.

Precedence: a real git repo wins, so a repo nested in a tracked directory owns its files, and the outer listing skips it as git does. `/` and `$HOME` are refused: everything beneath would be opted in, and the hook would start snapshotting it.

A renamed directory orphans its store (a future `relink`). A directory that later becomes a git repo can absorb its history with `git fetch <store> 'refs/driftr/*:refs/driftr/*'`.

## Listing is limited to the scope's literal roots

Measured on `~/.claude` (342 files in scope, plus large transcript and plugin-cache trees out of it): `ls-files --others` walked the whole tree and was the slowest step (84 ms). Includes with a literal prefix (`skills/**` → `skills`) now become `ls-files` pathspecs. A no-op snapshot went from ~250 ms (one run) to a 137 ms median over 15. Any include without a literal root (`**/*.md`, a regex) falls back to the full walk, so the result never changes, only the cost.

## The hook follows the file, not just the cwd

Sessions rarely run inside `~/.claude`, but they edit it constantly. So the hook snapshots the cwd repo *and* the opted-in repo containing the tool's `file_path` (its nearest existing directory, since a `PreToolUse` Write may target a path not yet created), deduplicated. Wired on `PreToolUse`/`PostToolUse` for file-writing tools, each edit is bracketed: before (including anything else that changed since) and after.

## gc rewrites to the current scope; it prunes only what driftr owns

Narrowing the scope left binaries and synced files in old snapshots (7.0 MB for `~/.claude`). `gc` replays the chain through a scratch index with out-of-scope paths removed. It keeps messages, dates and trailers, drops snapshots that end up empty, and swaps the ref with compare-and-swap, so a hook snapshot landing mid-rewrite aborts the gc rather than being lost. On `~/.claude`: 16 snapshots became 14, 246 paths were removed, and the store went from 7.0 MB to 372 KB.

Only driftr's ref and reflog are expired. `git gc --prune=now` runs only in external stores: in the user's repo it would also delete their own unreachable objects (dropped stashes, abandoned commits), so there git's normal expiry decides.

Two bugs it surfaced: `rm --cached` refuses entries that differ from both file and HEAD, which only guards a *user's* staging, so driftr's private-index removals use `-f`. And a prune can delete blobs only the private index references, after which the stat cache skips rehashing them, so staging rebuilds the index once on "invalid object".

## Open

- A removed worktree leaves its ref behind, keeping its objects alive. That needs a `driftr gc` eventually.
- Gitignored files can't be tracked, even explicitly. Fine for now; `track --force` if a real need shows up.
- Snapshot ref growth and pruning policy are unmeasured.
- Edits made through shell commands aren't bracketed: they land in the next snapshot, tagged with whatever event took it. `Bash` on `PostToolUse` would fix attribution at ~40–140 ms per command.
- Multi-file subjects always say "Update", even when most files were added (e.g. newly in scope).
- `gc` keeps subjects verbatim, so generated ones that count files go stale ("Start tracking 342 files" now holds 96). Regenerating generated subjects needs a marker for which ones were explicit.
- Concurrent sessions share one private index per worktree; a lock collision fails one hook's snapshot (swallowed). Unmeasured so far.
- Finding why a file is in scope (or what's bloating it) took manual surveying; `files --why` and a size/churn view would have surfaced `skills/synced` and `bin/` immediately.
