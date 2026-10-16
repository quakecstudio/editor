# xtask

Build automation for the workspace, run as `cargo xtask <command>` (the alias is in
`.cargo/config.toml`).

- **Layer:** tooling; not shipped.
- **Spec sections:** none.

| Command | What it checks |
| --- | --- |
| `ci` | Everything a change must pass before it merges, in one run: `cargo fmt --check`, `conventions`, `adr`, `layers`, `licenses`, `attribution`, clippy with `-D warnings`, the tests, `clippy-fixtures`, `cargo deny check` and `deny-fixtures`. Every step runs; it exits non-zero if any failed. `make ci` and CI run this |
| `adr` | Every ADR in `docs/adr/` follows `0000-template.md` and is listed in the index in `docs/adr/README.md` (ADR-0001) |
| `conventions` | Every config value matches `docs/conventions.md` (see "Checking conformance" there) |
| `clippy-fixtures` | The workspace lints reject each fixture in `fixtures/clippy/` with the lint it names, and pass the controls |
| `deny-fixtures` | `deny.toml` rejects a GPL-incompatible license and a wildcard version, and passes a control; needs the crates.io index |
| `layers` | Every workspace crate is in `layers.toml`, and depends only on crates in its own layer or lower ([`docs/layers.md`](../docs/layers.md)) |
| `licenses` | Every file is classified; source files start with an allowed SPDX line and pass the ported-file header rule (H1 to H5 in [`docs/porting.md`](../docs/porting.md)); no file carries a disallowed SPDX tag; `LICENSE` is the GPL-3.0 text |
| `bench` | Runs the performance-budget benchmarks in `crates/qs-bench`, fails when an active budget in [`docs/perf-budgets.md`](../docs/perf-budgets.md) is over budget or more than 10% slower than the last passing run on the same host, and appends the run to the JSON history (`target/bench/history.json`, or `--history <path>`); `--accept` records an intended slowdown. `bench history` prints the history. Not part of `ci` |
| `readiness <check> --out <file> [--os <name>] <crate>...` | Measures one readiness check (`lines`, `branches`, `mutation`, `spec` or `flakes`) for each crate, named by package or directory, and writes the numbers as JSON ([`docs/testing.md`](../docs/testing.md#measuring-a-layer)). Needs cargo-llvm-cov, cargo-mutants and the pinned nightly for the checks that use them. Not part of `ci` |
| `ratchet [--history <file>] [<measurement>...]` | Judges measurements against the bar in ADR-0009 and the minimums in [`ratchets.toml`](../ratchets.toml), checks `ratchets.toml` and every commit that changed it, and appends the run to a JSON history. Not part of `ci`; the `Readiness` workflow runs it |
| `corpus verify [--online]` | `corpus/manifest.toml` follows the corpus policy in [`corpus/README.md`](../corpus/README.md) and no corpus file or game data is tracked; `--online` downloads every pinned file and compares SHA-256. Not part of `ci`; the Test Assets job runs it |
| `corpus fetch [<id>...]`, `corpus build [<id>...]` | `fetch` downloads codebases and maps by checksum into the cache outside the source tree, refusing `never` entries; `build` compiles id 1.01 and the rerelease with the oracles and checks each progs.dat round-trips through `qc-progs` byte for byte. Not part of `ci` |
| `assets fetch`, `assets precache` | The open test assets download, match their pinned checksums and hold what a headless scenario needs, and cover the rerelease's precaches ([`docs/test-assets.md`](../docs/test-assets.md)). Not part of `ci` |
| `attribution` | Every asset file (images, fonts, sounds, Quake formats) has a row in [`ATTRIBUTION.md`](../ATTRIBUTION.md) with an allowed license, and every row names a file in the repository |

`licenses` and `attribution` check the tracked files plus untracked files that are not
ignored, so a new file is caught before it is committed.

Scratch workspaces go under `target/xtask/`; the self-tests' fixture workspaces go under
`target/xtask-tests/`.
