# qs-tests

The single integration test binary for tests that exercise more than one crate
(`docs/conventions.md`, Tests). A test of one crate lives in that crate.

- **Layer:** `tests` in `layers.toml` ([`docs/layers.md`](../../docs/layers.md)); it is a test harness and may depend on any crate.
- **Spec sections:** whichever the tests in each module cite.
- **Shipped:** no.

| Module | What it checks |
| --- | --- |
| `builtins` | A minimal progs that calls a builtin, run through `qc-vm` with `qc-builtins`' mock builtins: the dispatcher reads parameters and writes the result |
| `deep_nesting` | Input nested far past the limit, and a long chain of macros, go through the whole front end under the FTEQCC profile without exhausting the stack |
| `diagnostics_unified` | The diagnostics of as-you-type editing and of a full build are the same set (COMP-2) |
| `map_entities` | The entities of a shipped map with no `.map` source are read from the BSP by `qs-bsp`, edited by `qs-map` without losing a byte, and written back into the BSP or a `.ent` file (MAP-22) |
| `paths` | `qs-paths` resolves absolute application directories in the real environment |
| `scenario_numbers` | The number vectors of a differential scenario (`docs/spec/vectors/scenario/numbers.txt`) checked from the Rust side |
| `spec_coverage` | The spec-citation ratchet: every statement in `docs/spec/` is cited by a `// spec` line in test code or listed in `docs/spec/coverage-baseline.tsv`, and `docs/spec/requirements.tsv` matches the specs (`docs/spec/README.md`) |
| `spec_vectors` | The VM test vectors in `docs/spec/vectors/vm/` are well formed, cite active statements, and cover every vanilla opcode |

Add a module to `tests/it/` and declare it in `tests/it/main.rs`; never add a file directly
under `tests/`, which would link another binary. Keep `[lints] workspace = true`; a test module
opts out of the no-panic lints it uses with a module-level
`#![expect(clippy::unwrap_used, ..., reason = "test code: ...")]`.

```bash
cargo test -p qs-tests
```
