# qs-tests

The single integration test binary for tests that exercise more than one crate
(`docs/conventions.md`, Tests). A test of one crate lives in that crate.

- **Layer:** `tests` in `layers.toml` ([`docs/layers.md`](../../docs/layers.md)); it is a test harness and may depend on any crate.
- **Spec sections:** whichever the tests in each module cite.
- **Shipped:** no.

Add a module to `tests/it/` and declare it in `tests/it/main.rs`; never add a file directly
under `tests/`, which would link another binary. Keep `[lints] workspace = true`; a test module
opts out of the no-panic lints it uses with a module-level
`#![expect(clippy::unwrap_used, ..., reason = "test code: ...")]`.

```bash
cargo test -p qs-tests
```
