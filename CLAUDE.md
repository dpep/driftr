# driftr

See README.md for usage and docs/decisions.md for settled choices and why. docs/DESIGN.md is the original plan, kept for history.

## Rules

- Never touch the user's branch, HEAD, index, or staged changes. Snapshots go through a private index (`GIT_INDEX_FILE`) onto `refs/driftr/...`.
- `snapshot` is idempotent; every trigger (watch, hook, explicit) just calls it.
- Git is the store. No database or custom object format without a measured reason.
- Attribution is evidence, not fact: record what's known (session, event), don't claim an author.
- The hook never fails, blocks, or writes stdout, and a no-op hook spawns no git.
- Measure before optimizing: `--profile`, `driftr report`, and the benchmark numbers in docs/decisions.md.
- Add a CHANGELOG entry under `## Unreleased` with the change that earns it.

## Gate

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
GIT_DIR=/nonexistent cargo test
```

The last run checks that tests survive a hostile environment.
