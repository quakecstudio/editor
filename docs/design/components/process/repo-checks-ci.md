# Repository checks and CI

**Crates:** `xtask` (`ci` and the checks), `qs-log` (L0), `qs-bench` (tests), `.github/workflows/`. **Specs:** none of its own; QA-1, CONV-1 to CONV-9.

## Context and design philosophy

This component is the merge gate: the checks every change must pass, the GitHub Actions
workflows that run them, and the root configuration they enforce. QA-1 in
`plan/requirements.yaml` names it: "cargo xtask ci runs fmt, clippy with panics denied, all
tests, layering, license and asset-attribution checks, differential VM suite; required before
merge". Its configuration intent is CONV-1 to CONV-9 and CONV-15 in `docs/conventions.md`;
the layer rule is ARCH-1 (`docs/layers.md`); the decision process it checks is ADR-0001; the
visual contrast rule is ADR-0010 (QA-9, UI-15); the license rules are ADR-0002 and ADR-0017;
performance budgets are TEST-8 (`docs/perf-budgets.md`).

Two principles show throughout the code:

- **Docs as data.** A check reads its rules from the document a person edits, so the doc and
  the check cannot drift: `docs/conventions.md` fenced TOML blocks and the clippy deny table,
  the `docs/adr/` template and index, the `ATTRIBUTION.md` table, the `docs/porting.md`
  Upstreams table and header examples, `docs/perf-budgets.md`, `docs/design/tokens.toml`.
- **One command locally and in CI.** `make ci` is `cargo xtask ci` (`Makefile:54-55`); CI runs
  the same command with `--skip` for the steps another job already runs (`docs/ci.md`).

Shipped-crate lints apply to xtask too (`xtask/Cargo.toml:29-30`); the CLI prints with a
crate-root `#![expect(clippy::print_stdout, print_stderr)]` (`xtask/src/main.rs:77-81`).

## Components

### `cargo xtask ci` (xtask/src/ci.rs)

`STEPS` (`xtask/src/ci.rs:32-88`) holds 16 steps, cheapest first: `fmt`, `plan`, `plan-tests`,
`specs` (`scripts/refresh-specs.sh --verify`), `conventions`, `adr`, `layers`, `licenses`,
`attribution`, `contrast`, `clippy -D warnings`, `test`, `clippy-fixtures`, `deny`,
`deny-fixtures`, `differential --fast`. Steps are `Cargo`, `Xtask` (run in-process) or
`Program`. Every step runs even after a failure; a summary follows and the exit code is
non-zero if any failed. `--skip a,b,c` omits steps by name; an unknown name or a repeated
`--skip` is a usage error (`xtask/src/ci.rs:148-152`).

`xtask/src/main.rs` dispatches three ways: argument-taking commands, `ci`, and zero-argument
in-process checks returning a `Report { passed, problems }` (`xtask/src/main.rs:137-211`).

### The checks

| Check | Source of rules | What fails |
| --- | --- | --- |
| `conventions` (`xtask/src/conventions.rs`) | `docs/conventions.md` fenced ` ```toml file=<path> [exact]` blocks; clippy table between `conventions:clippy-deny` markers | Config values differing from the doc; CalVer `YYYY.MM.N`; toolchain channel `x.y.z` equal to `rust-version`; workspace deps not `=x.y.z` or without `default-features = false`; external deps missing from `.clippy.toml` `absolute-paths-allowed-crates`; deny skips without reason; members not inheriting package fields, lints and `workspace = true` deps (`conventions.rs:281-477`) |
| `layers` (`xtask/src/layers.rs`) | `layers.toml` | A workspace crate missing from `layers.toml`, an unknown layer, an upward dependency, or any dependency on a `tests` or `tooling` crate; normal, dev, build and target-specific deps all count (`layers.rs:84-183`) |
| `licenses` (`xtask/src/licenses.rs`, `ported.rs`) | Kind table in code; `specs/manifest.tsv`; `docs/porting.md` Upstreams | An unclassified file type; a source file without the GPL-3.0-or-later SPDX line; a disallowed SPDX tag in a text file; a ported file breaking H1 to H5 |
| `attribution` (`xtask/src/attribution.rs`) | `ATTRIBUTION.md` table | An asset (24 extensions) with no row or a license outside the seven allowed; a row naming no file |
| `adr` (`xtask/src/adr.rs`) | `docs/adr/0000-template.md` constants, `docs/adr/README.md` index | Missing header fields or sections, bad status, numbering, an ADR missing from the index (ADR-0001) |
| `contrast` (`xtask/src/contrast.rs`) | `docs/design/tokens.toml` | A pair below its `min`, a `min` below 4.5, a token in no rule and not decorative, themes with different token sets (ADR-0010) |
| `clippy-fixtures`, `deny-fixtures` (`xtask/src/fixtures.rs`) | `xtask/fixtures/clippy/*.rs`; four deny cases | A lint fixture that does not fire exactly its lint; `deny.toml` accepting gpl2-only, openssl or a wildcard, or rejecting the control |

`licenses` and `attribution` list files with `git ls-files --cached --others --exclude-standard`
(`xtask/src/repo.rs`), so an untracked new file is caught before commit.

### Benchmarks (`cargo xtask bench`, crates/qs-bench)

Not part of `ci`; `.github/workflows/bench.yml` runs it weekly. It runs criterion's `budgets`
group in `crates/qs-bench/benches/budgets.rs`, reads the table between `perf-budgets` markers
in `docs/perf-budgets.md`, fails an active budget over its limit or more than 10% slower than
the last passing run on the same host key (`xtask/src/bench.rs:31, 199-205, 457`), and appends
to `target/bench/history.json`. `--accept` lets a regression through but never an over-budget
result. All four budgets are `pending` and the benchmarks are placeholders measuring
`black_box(name.len())` (`crates/qs-bench/benches/budgets.rs:22-45`).

### Logging (crates/qs-log)

`qs-log` (L0) is the one tracing subscriber the apps install: a file layer writing to the
per-OS log directory from `qs-paths`, unbuffered appends, rotation at 5 MiB with five files
kept, a crash record on panic, and redaction of source text (QA-7, CONV-16;
`crates/qs-log/README.md:21-43`). It has no `docs/spec/` citations; its tests pin QA-7's
acceptance criteria.

### Workflows

| Workflow | Trigger | Role |
| --- | --- | --- |
| `ci.yml` | PR, merge group, push to main, schedule | Required checks: `changes` classifier, `fmt`, `clippy`, `test` (3 OS), `build` (4 targets), `build-macos`, `xtask-ci`, `deny`, `dependency-review`, `test-assets` (3 OS) |
| `secure_workflows.yml` | PR, merge group | Every action SHA-pinned; zizmor (CONV-7) |
| `bench.yml` | weekly, dispatch | Performance budgets (TEST-8) |
| `fuzz.yml` | nightly | cargo-fuzz targets and the oracle libFuzzer targets |
| `differential.yml` | nightly | Full differential VM corpus |
| `readiness.yml` | dispatch | Coverage, mutation, flakes, spec, judged by `cargo xtask ratchet` (ADR-0009) |

`scripts/ci-changes.sh` classifies a diff as light (only `docs/**` except `docs/spec/**`,
`plan/**`, `.claude/**`, `.kiro/**`, root `*.md`) or full; any `.rs`, unknown path, empty or
uncomputable diff is full (`scripts/ci-changes.sh:19-31`). On a light change every required
check still reports its name: skipped jobs use `if:` with `!cancelled()`, matrix jobs skip their
steps instead, and `xtask-ci` always runs fully with `SKIP` chosen by ref
(`.github/workflows/ci.yml:327`). The light-path readers are listed in `docs/ci.md`, "Light
changes", and the classifier and job structure are tested in `xtask/src/ci/changes_tests.rs`.

### Root configuration

`Cargo.toml` (members, the deny-level lint set, `[workspace.dependencies]`, profiles `release`,
`ci`, `mutants`), `layers.toml`, `deny.toml` (six targets, GPL-3.0-compatible allowlist, OpenSSL
excluded), `rust-toolchain.toml` (1.99.0), `.rustfmt.toml`, `.clippy.toml`,
`.pre-commit-config.yaml` (prek; clippy and tests at pre-push), `Makefile`.

## Error model

- Each zero-argument check returns a `Report { passed, problems }`; every problem is a line
  naming the file and the rule, and any problem fails the command (`xtask/src/main.rs`).
- `cargo xtask ci` runs every step even after one fails and exits non-zero if any failed; an
  unknown or repeated `--skip` is a usage error.
- `cargo xtask bench` fails an active budget over its limit or more than 10% slower than the
  last passing run on the same host key.

## Verification

- Each check has unit tests beside it in `xtask/src/`; `clippy-fixtures` and `deny-fixtures`
  prove the lints and `deny.toml` reject what they target.
- `xtask/src/ci/changes_tests.rs` tests the light-path classifier and the job structure.
- `xtask` itself runs under the shipped-crate lints.

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Check runner | Rust `xtask` binary | Shell scripts, Make recipes | CONV-8 and `docs/conventions.md` adopt vouch's layout; one portable binary on three OSes [inferred] |
| Rule source | Parse the governing doc (`conventions.md`, `porting.md`, `ATTRIBUTION.md`, ADR template) | Duplicate rules in code | The doc and the check cannot drift; `adr.rs:186-190` checks template and files with the same constants |
| CI shape | Separate fmt, clippy, test, build, deny, dependency-review jobs plus `cargo xtask ci --skip` | One job running `make ci` | CONV-7: "separate fmt/clippy/test/build/deny/dependency-review jobs"; `docs/ci.md` maps each step to its job |
| Light path | Job-level `if:` and step-level skips, no `paths:` filter | `paths:` filters | `docs/ci.md` quotes GitHub: a workflow skipped by path filtering leaves required checks "Pending" |
| Readiness measurement | On demand in `readiness.yml`, not on every PR | Measured per PR | ADR-0009 option E |
| Differential bar | Zero divergences gated; coverage reported | 90% coverage gate | ADR-0013 |
| Asset licenses broader than source licenses | Seven asset licenses; GPL-3.0-or-later only for source SPDX | One list | Third-party assets keep their own license; our source is GPL-3.0-or-later (ADR-0002) [inferred] |
| `panic = "unwind"` in release | Differs from vouch | `abort` | CONV-6: "differs from vouch so the command guard works" |

## Open questions

### Resolved

None recorded.

### Deferred

1. **H4 and two upstreams.** `xtask/src/ported.rs:141-151` requires each cited upstream's
   copyright line to come before any other Copyright line. A file citing both
   id-Software/Quake and fte-team/fteqw cannot satisfy both; no shipped file does so yet (only a
   test in `xtask/src/licenses.rs`), but ADR-0017 makes this combination possible.
2. **xtask README drift.** `xtask/README.md:9-24` lists 13 commands and an old `ci` step list;
   `main.rs` routes 26 commands and `STEPS` has 16 entries including plan, plan-tests, specs,
   contrast and differential.
3. **docs/ci.md versus ADR-0013.** `docs/ci.md:184` says the Differential workflow "enforces
   zero divergences and the 90% statement-coverage bar (TEST-6)"; ADR-0013 removed the gate.
4. **Benchmarks are placeholders.** All four budgets are pending and measure nothing real
   (`crates/qs-bench/benches/budgets.rs:22-45`); filled in by P2-11, P2-18, P3-12 and verified
   by P4-23 (#134).
5. **Member discovery differs.** `conventions.rs:411-429` scans `crates/`, `apps/` and `xtask`;
   `layers.rs:123-168` reads the workspace `members` list. `fuzz/` is checked by neither.
6. **Process injection is partial.** `fixtures` and most of `readiness` take an `Exec`;
   `repo.rs`, `bench.rs` and `ratchet.rs` spawn `std::process::Command` directly, so their
   tests spawn real git, cargo and sysctl.
7. **Duplicated helpers.** `read_toml` in `conventions.rs:135`, `layers.rs:225` and
   `fixtures.rs:299`; `bench::{commit, unix_time}` reused by `ratchet.rs:32`.
8. **Pin drift.** `ci.yml:306, 346` pin `taiki-e/install-action` at v2.87.22 while `fuzz.yml:51`
   and `readiness.yml:49, 101` pin v2.87.26.
9. **Doc drift elsewhere.** `CONTRIBUTING.md:26-30` omits the plan checks and
   `refresh-specs.sh --verify` from the `ci` step list; `.claude/rules/layering.md:12-15` omits
   crates `layers.toml` maps (qc-diag, qs-install, qs-bsp, qs-map, qs-mdl, qs-wav, qc-engines,
   qc-sema); `docs/layers.md:78` names only qs-tests in the tests layer though qs-bench and
   qs-differential are there too; `conventions.md:362` calls `docs/` an mdBook with no
   `book.toml`.
10. **contrast.rs:241-244** marks a pair failed without its own problem line when a token is
    missing in a theme, relying on the theme-consistency problem.
11. **bench test portability.** `xtask/src/bench/tests.rs:443-449` would fail on a non-Windows
    machine with `PROCESSOR_IDENTIFIER` set.
12. `readiness`, `ratchet`, `corpus` and the corpus commands live in xtask and run in
    `readiness.yml` and `ci.yml`, but are not listed in this leaf; their owning segment is to be
    confirmed.

## References

- `xtask/src/{main,ci,repo,conventions,layers,licenses,ported,attribution,adr,contrast,fixtures,bench}.rs`, `xtask/src/ci/changes_tests.rs`, `xtask/README.md`
- `crates/qs-log/`, `crates/qs-bench/`
- `.github/workflows/*.yml`, `scripts/ci-changes.sh`, `scripts/refresh-specs.sh`
- `Cargo.toml`, `Makefile`, `layers.toml`, `deny.toml`, `rust-toolchain.toml`, `.clippy.toml`, `.rustfmt.toml`, `.pre-commit-config.yaml`
- `docs/ci.md`, `docs/conventions.md`, `docs/layers.md`, `docs/porting.md`, `docs/perf-budgets.md`, `docs/testing.md`
- ADR-0001, ADR-0002, ADR-0009, ADR-0010, ADR-0013, ADR-0017
