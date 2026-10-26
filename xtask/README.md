# xtask

Build automation for the workspace, run as `cargo xtask <command>` (the alias is in
`.cargo/config.toml`).

- **Layer:** tooling; not shipped.
- **Spec sections:** none.

| Command | What it checks |
| --- | --- |
| `ci` | Everything a change must pass before it merges, in one run, in this order (`STEPS` in `src/ci.rs`): `cargo fmt --check`, `plan/check.py`, every `plan/test_*.py`, `scripts/refresh-specs.sh --verify`, `conventions`, `adr`, `layers`, `licenses`, `attribution`, `contrast`, clippy with `-D warnings`, the tests, `clippy-fixtures`, `cargo deny check`, `deny-fixtures` and `differential --fast`. Every step runs; it exits non-zero if any failed. `make ci` runs this; CI runs it as `ci --skip <step>,...`, which leaves out the named steps (`fmt`, `clippy`, `test`, `deny`, ...) because another job runs them ([`docs/ci.md`](../docs/ci.md#which-job-runs-each-step-of-cargo-xtask-ci)), and fails on a name no step has |
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
| `contrast` | Every colour pair in [`docs/design/tokens.toml`](../docs/design/tokens.toml) meets WCAG AA in both themes |
| `fuzz [<target>...] [--time <duration>]` | Runs the cargo-fuzz targets in `fuzz/` on nightly for a fixed time each, every target when none is named; `make test-fuzz` and the nightly `Fuzz` workflow call it. Not part of `ci` |
| `oracles [build \| scenarios [<name>...] \| fuzz <target> [--time <s>] \| builtins [--record] \| id101 <dir> \| rerelease <dir> \| json <trace>]` | Fetches id's and FTEQCC's sources by checksum, builds the oracles in `oracles/` (id's VM as a trace runner, id's qcc and FTEQCC) with the system C compiler and checks them; `id101` also compiles and runs id's QuakeC 1.01, `rerelease` compiles the rerelease with FTEQCC, and the other subcommands run one part ([`oracles/README.md`](../oracles/README.md)). Not part of `ci` |
| `differential [--fast \| --full]` | The differential VM suite: builds the oracles, then runs the `qs-differential` binary on the corpus progs in the VM oracle and QuakeC Studio's runner and compares the traces. `--fast` (the default) is id 1.01 with zero divergences, the `ci` step; `--full` is every oracle-built codebase, zero divergences with statement coverage reported (ADR-0013) |
| `lex-corpus <dir>` | Lexes every `.qc` file of a codebase directory and preprocesses the files its `progs.src` lists, failing on any error. Not part of `ci` |
| `parse-corpus [--vanilla] [--fteqcc] [--deletions] <dir>...` | Parses every `.qc` and `.qh` file under each codebase directory and checks the tree gives back each file; `--vanilla` fails on any error, `--fteqcc` parses under the FTEQCC profile and `--deletions` checks that recovery from each deleted token is local. Not part of `ci` |
| `project-corpus [--files] [--fteqcc] [--progs <progs.dat>] [--expect-warning <code>=<modules>] <dir>...` | Opens each codebase directory as a project with `qc-project` and checks each module's file graph; `--progs` checks it against the function order of a progs.dat id's qcc built, `--fteqcc` against the files the pinned FTEQCC reports compiling and including, `--files` prints every graph, and `--expect-warning` requires a warning in exactly that many modules. Not part of `ci` |
| `sema-corpus [--write] <dir>[=<progs.dat>]...` | Resolves each codebase through `qc-sema` and compares its index with the golden file in `docs/spec/vectors/sema/`; with a progs.dat, every global definition the progs keeps must agree with it unless the codebase's `.differences.txt` lists it (counted across all of the codebase's modules). Not part of `ci` |
| `fteqcc-coverage <dir>...` | Parses the rerelease codebases with the vanilla parser and lists every construct it rejects, grouped into the classes of [`docs/spec/dialect-fteqcc-rerelease.md`](../docs/spec/dialect-fteqcc-rerelease.md); fails on a construct no class names. Not part of `ci` |
| `fteqcc-full-coverage <dir>...` | Scans the FTEQCC corpus mods for the full dialect's constructs, grouped into the classes of [`docs/spec/dialect-fteqcc.md`](../docs/spec/dialect-fteqcc.md); fails on a class no section names. Not part of `ci` |
| `fteqcc-profile <dir>...` | Preprocesses, parses and resolves every module of each rerelease codebase under the FTEQCC profile and fails on any error, then checks the vanilla profile rejects each extension. Not part of `ci` |
| `map-corpus <dir>...` | Reads every `.map` file under each directory with `qs-map` and checks that each writes back byte for byte. Not part of `ci` |
| `engines [--check]` | Fetches the fact sources pinned in `crates/qc-engines/sources.toml` by checksum and writes `crates/qc-engines/src/tables.rs` from them; `--check` fails when the checked-in tables differ. Not part of `ci` |

`licenses` and `attribution` check the tracked files plus untracked files that are not
ignored, so a new file is caught before it is committed.

Scratch workspaces go under `target/xtask/`; the self-tests' fixture workspaces go under
`target/xtask-tests/`. The tests run in parallel, so each one writes only directories of its
own: it takes them from `test_dirs::fresh`, or `test_dirs::claim`s a fixed path, and a test
that claims a directory overlapping another test's fails.
