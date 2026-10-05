# driftr

Automatic working history for AI-assisted development, kept in plain git. See README.md for usage, docs/DESIGN.md for the plan, docs/decisions.md for settled choices and why.

## First principles

- Never touch the user's branch, HEAD, index, or staged changes. Snapshots go through a private index (`GIT_INDEX_FILE`) onto `refs/driftr/...`.
- `snapshot` is idempotent; every trigger (watch, hook, explicit) just calls it.
- Git is the store. No database or custom object format without a measured reason.
- Attribution is evidence, not fact: record what's known (session, event), don't claim an author.
- The hook never fails, blocks, or writes stdout, and a no-op hook spawns no git.
- Measure before optimizing: `--profile`, `driftr report`, and the benchmark numbers in docs/decisions.md.

## Layout

- `src/snapshot.rs`: `Repo`, the core (scope, stage, commit, log, diff, restore)
- `src/config.rs`: scope config and pattern matching
- `src/hook.rs`: Claude Code hook payload handling
- `src/fast.rs`: git-free discovery and the stat fingerprint (the no-op fast path)
- `src/lock.rs`: the per-repo lock serializing writers across sessions
- `src/external.rs`: stores for directories that aren't git repos
- `src/gc.rs`: rewriting history to the current scope
- `src/profile.rs`, `src/usage.rs`, `src/report.rs`: timings, the hook usage log, and its summary
- `src/git.rs`: git CLI wrapper; clears inherited `GIT_DIR`/`GIT_WORK_TREE`/`GIT_INDEX_FILE`
- `src/output.rs`: the one renderer behind `-j`/`-J`
- `src/status.rs`, `src/watch.rs`, `src/main.rs`
- `tests/snapshot.rs` (library), `tests/cli.rs`, `tests/external.rs`, `tests/fast.rs`, `tests/concurrency.rs` (binary end to end), `tests/common/`

## Gate

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Tests must keep passing with a hostile environment: `GIT_DIR=/nonexistent cargo test`.
