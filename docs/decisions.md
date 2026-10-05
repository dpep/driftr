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

## Concurrency: one lock per repo, across stage and commit

Measured with 8 parallel writers: they collided on the private index's `index.lock` and lost the ref compare-and-swap. A slow writer could also commit an older tree over a newer one. An advisory lock in `<common dir>/driftr/lock`, shared across worktrees, now spans stage + commit, `gc`, and `watch`'s tick. It is re-entrant per process, so restore's safety snapshot nests. The CLI waits up to 30s; hooks wait 3s, then record `busy` and skip, which is safe because snapshots are idempotent and the next hook catches up. CAS on the ref stays as a second line of defense.

The tests also caught a prune hazard. The private index's cache-tree can name a tree that `git gc --prune=now` deleted, and `write-tree` then returns that id without writing the tree. Any missing-object failure rebuilds the index (it is only a cache) and retries once.

## Fast path: no git for a no-op

Profiled: a hook's cost was git process startup (~13 ms per spawn here, ~11 spawns), with ~6 ms of driftr's own work. Most hook runs change nothing, so the no-op path now avoids git entirely:

- Discovery walks up for `.git` (dir or `gitdir:` file, plus `commondir`) or a driftr store, in the same order of precedence as `Repo::open`.
- A fingerprint hashes the config text plus each in-scope file's path, size, mtime (ns), inode and mode. The walk uses the `ignore` crate, gitignore-aware, skipping `dir/**`-excluded directories. It is saved at each snapshot, taken *before* staging and under the lock. If it matches, the last snapshot already holds this state.
- Safety: a file modified in the last 2s makes the fingerprint untrusted (none is saved). A change during a snapshot leaves the saved fingerprint stale, which costs one full run, never a miss. The walk may see more than git does (nested repos), which only causes extra full runs. The one blind spot is a file that is both tracked and gitignored.

Measured, median of 20 runs (wall time, including process start): opted-in no-op 102 → 12.8 ms; edit into `~/.claude` from another repo 140 → 25 ms, then ~5 ms less after directory pruning; repo not opted in 21 → 14 ms; not a repo 19 → 14.5 ms. That made `Bash` on `PostToolUse` affordable.

## The sweep: target first, sweep as backstop, and grade the evidence

`Bash` has no `file_path`, so a shell edit to an opted-in repo outside the cwd went unseen until some later hook targeted it. Two mechanisms, in order:

- **Command paths:** absolute and `~/` tokens in a `Bash` command are matched against registered repos, which are then snapshotted at once. A false match costs a fingerprint, never a wrong snapshot.
- **Sweep:** at `Stop`, every registered worktree is checked: a fingerprint per repo (no git), and a snapshot only if it changed. A per-repo `swept` marker throttles it to once per 30s across sessions, so N sessions ending turns together don't fan out N×M. Cost scales with changes, not with sessions.

Every snapshot carries `Driftr-Via: cwd|file|command|sweep`, strongest evidence first (dedupe keeps the strongest). A sweep's change could be anyone's, so it names `Driftr-Candidates:`: sessions with `cwd`/`file`/`command` events in that repo's own event log in the last 15 minutes. That is a graded guess, not an author claim.

Registry: `<data>/repos/<escaped path>` symlinks to each repo's common git dir. A dangling link means the repo is gone, and there is one atomic file per repo, so no lock is needed. External stores are already listed by `dirs/`. Worktrees are read from `<common>/worktrees/*/gitdir`, without git. Identity is location: each clone is its own entry (their unsaved work differs), and branches need nothing (snapshots follow the working tree; `Driftr-Head:` records the branch tip).

Rejected: a separate session→repo registry. Each repo's `events.jsonl` already records session, time and `via`, so candidates are read from there. That is the same answer with no second source of truth to prune.

To keep tests off real repos, the CLI reads the registry and passes it to the hook. The library never reads it, so library tests can't sweep the developer's own repos.

## Open

- A removed worktree leaves its ref behind, keeping its objects alive. That needs a `driftr gc` eventually.
- Gitignored files can't be tracked, even explicitly. Fine for now; `track --force` if a real need shows up.
- Snapshot ref growth and pruning policy are unmeasured.
- Judge the sweep with `driftr report`'s `swept` column: if it rarely catches what the targeted hooks missed, pull it.
- Multi-file subjects always say "Update", even when most files were added (e.g. newly in scope).
- `gc` keeps subjects verbatim, so generated ones that count files go stale ("Start tracking 342 files" now holds 96). Regenerating generated subjects needs a marker for which ones were explicit.
- Finding why a file is in scope (or what's bloating it) took manual surveying; `files --why` and a size/churn view would have surfaced `skills/synced` and `bin/` immediately.
