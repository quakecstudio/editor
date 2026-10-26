# Test-suite readiness

No work may build on a layer until that layer's test suite is reliable. This document
defines "reliable" as a bar each layer must clear, and how the plan enforces it.
Requirement IDs (`TEST-n`) are tracked in `plan/requirements.yaml`.

## How it is enforced

- Every phase has one **readiness issue per layer** it builds (`type:readiness`, keys like
  `P1-R1` for Phase 1, layer L1). It depends on every feature and test issue of that layer in
  that phase, and on the same layer's readiness from earlier phases.
- If a feature or test issue depends on work in a **lower layer** (or a C tool), `plan/check.py`
  makes it depend on that layer's readiness issue too. Upward progress waits on the
  lower layer's suite, not just its code.
- Phase gates depend on every readiness issue in their phase, so a phase cannot close with
  a layer whose suite has not passed.
- Once a layer passes, its numbers become ratchets in `ratchets.toml` (TEST-10). A later change
  that lowers coverage, mutation score or spec-citation coverage fails the layer's next readiness
  run; moving the ratchet into pull-request CI is a later decision (ADR-0009).
- A defect found in a higher layer that originates in a lower one reopens the lower layer's
  readiness: it is fixed in the lower layer with a regression test there, and the review is
  repeated for the affected crate (TEST-11).

## The bar

The numbers below are confirmed in [ADR-0009](adr/0009-test-readiness-bar.md) and measured when
a layer's readiness review runs (the on-demand `Readiness` workflow), not on every pull request. Exceptions are listed per crate in its README with a reason, never waived silently.

| ID | Requirement | Threshold | Measured by |
| --- | --- | --- | --- |
| TEST-1 | Line and branch coverage per crate, excluding generated code | lines ≥ 90%, branches ≥ 80% | `make test-coverage` (cargo-llvm-cov), readiness workflow |
| TEST-2 | Mutation score per crate | ≥ 90% of mutants caught; every survivor killed by a new test or recorded as equivalent with a reason | `make test-mutants` (cargo-mutants), readiness workflow |
| TEST-3 | Spec traceability: every normative statement in the crate's `docs/spec/` sections has a test citing it | 100%, no baseline backlog for new crates | spec-citation ratchet in `qs-tests`; per crate in the readiness workflow |
| TEST-4 | Property tests for every encoder/decoder (round trip) and every stated invariant | one property per invariant listed in the crate README | proptest |
| TEST-5 | Fuzzing of every parser or decoder that reads files or network input | 24 h without a crash or divergence; minimized findings committed as regression tests | `make test-fuzz`, results in the readiness report |
| TEST-6 | Differential tests wherever an oracle exists (id VM, id qcc, pinned FTEQCC, id map tools, ericw-tools) | zero divergences; ≥ 90% of corpus statements exercised: the QuakeC statements of the differential corpus that the suite executes, as DIFF-11 measures them and [ADR-0013](adr/0013-differential-coverage-bar.md) reads the bar, not the line or branch coverage of the crate under test, which is TEST-1's; except the id VM differential suite, whose hard bar is zero divergences and whose statement coverage is measured and reported but not gated at 90% because its mock does not drive the running world ([ADR-0013](adr/0013-differential-coverage-bar.md)) | differential suites |
| TEST-7 | No flaky tests | full suite passes 20 consecutive runs on each of macOS, Windows and Linux; no wall-clock waits | readiness workflow, 20 runs per OS |
| TEST-8 | Performance where a budget applies | within budget on the reference machines; regression over 10% fails | `make bench` history |
| TEST-9 | Independent adversarial review | a reviewer who did not write the code (person or separate agent session) spends a fixed time trying to break the layer and writes at least five new tests; any defect found blocks readiness until fixed and re-reviewed | readiness report |
| TEST-10 | Ratchets | the readiness numbers become per-crate minimums in `ratchets.toml` | `cargo xtask ratchet` in the readiness workflow; in pull-request CI later, by a new decision |
| TEST-11 | Reopening | a lower-layer defect found from above reopens that layer's readiness | issue process |

## Measuring a layer

The reviewer runs the `Readiness` workflow
([`.github/workflows/readiness.yml`](../.github/workflows/readiness.yml)) from the Actions tab or
with `gh workflow run readiness.yml -f crates="qs-a qs-b"`, naming the layer's crates by package
name or directory. It is the only place the bar is measured (ADR-0009); it is not a required
check and never runs on pull requests or on a schedule.

| Job | Command | Measures |
| --- | --- | --- |
| `Readiness coverage` | `cargo xtask readiness lines`, then `branches` | line coverage with `cargo llvm-cov` on the pinned stable toolchain; branch coverage with `cargo llvm-cov --branch` on the date-pinned nightly ([`conventions.md`](conventions.md)); both run property tests on a fixed seed, ChaCha RNG and 256 cases unless a test sets its own (`PROPTEST_RNG_SEED`, `PROPTEST_RNG_ALGORITHM`, `PROPTEST_CASES`), so a line or branch reached only by a generated input counts the same on every run of one commit |
| `Readiness mutation (<crate> <k/n>)` | `cargo xtask readiness mutation --shard k/n` | a full `cargo mutants` run per crate, its mutants dealt round robin into shards (`--shard k/n`, k from 0, as cargo mutants counts them), each shard on a runner of its own (a matrix over the crates and their shards; the count per crate is set in the workflow from the time its whole run took, 10 for qc-preproc, 4 for qc-syntax, 1 otherwise), with incremental builds and, for a workspace member, the `mutants` profile (no debug info, opt-level 1, debug assertions and overflow checks on) and a test timeout of five times the baseline's and at least 10 s; a mutant that times out counts as caught, one that does not build does not count; on a Linux host, in the workflow or locally, xtask gives each test binary 3 GiB of data memory and no core file (`prlimit --data --core=0` as cargo's runner for the host's triple, `readiness.rs` `mutant_runner`), so a mutant whose test grows past it aborts that binary, which counts as caught, rather than ending the runner; macOS and Windows enforce no such limit and get none; the ratchet adds a crate's shards back into one score (the mutant counts add, the survivors are listed in shard order) and fails when a shard is missing, measured twice or of another count |
| `Readiness flakes (linux, macos, windows)` | `cargo xtask readiness flakes` | each crate's tests 20 times on each OS; every run must pass |
| `Readiness ratchet` | `cargo xtask readiness spec`, then `cargo xtask ratchet` | the active statements, in the sections the crate README lists under **Spec sections** (`A to B` being every section between, less those it lists under **Not this crate's**) and the single statements it lists under **Spec statements** (none of which may be in the baseline), that a `// spec` test cites; then judges every number against the bar and `ratchets.toml` |

Each crate's numbers come from its own tests (CONV-11), so tests in `qs-tests` do not count
towards another crate's coverage or mutation score. `cargo xtask ratchet` fails when a crate is
below the bar or its minimum, when a check was not measured, when a surviving mutant is
neither killed nor listed as equivalent, when `ratchets.toml` is malformed, and when any commit
that touched `ratchets.toml` changed a crate's minimums without a new `readiness` key or
lowered one without a new `reason`. A passing run prints the minimums to commit. The run
uploads every measurement and the JSON history of readiness runs (kept between runs in the
Actions cache, saved from `main`) as the `readiness` artifact.

Each check has a fixture crate under
[`xtask/fixtures/readiness/`](../xtask/fixtures/readiness/README.md) that sits below its
threshold. The xtask unit tests, which run in pull-request CI, assert that each check fails on
its fixture and that lowering a minimum without a readiness issue fails `cargo xtask ratchet`.
Running the workflow on the fixture directories shows the same failures with the real tools.

## Layer notes

- **L0 formats:** round-trip property tests and fuzzing carry most of the weight;
  every corpus file of the format round-trips.
- **L1 VM:** differential testing against id's VM is the primary evidence; coverage and
  mutation scores are necessary but not sufficient.
- **L2 syntax:** lossless round trip of the whole corpus, error-recovery tests generated
  by deleting each token, parser fuzzing.
- **L3 semantics:** golden indexes (definitions, references, diagnostics) for the corpus
  (`cargo xtask sema-corpus`, `docs/spec/vectors/sema/`); incrementality asserted by query
  counters.
- **L4 bridges:** compiler differential suites; bridge client suites run against the headless
  preview host; protocol conformance tests on both sides of the bridge.
- **L5 commands:** every command has a schema test, an undo test and a guard test.
- **L6 agent:** tests use a scripted fake model; evals (P4-15) measure quality, not correctness.
- **L7 UI and apps:** coverage applies to non-rendering code; every command and panel has
  an offscreen snapshot test; accessibility checks run in CI.
- **C tools (`oracles/`, including id's engine as the engine oracle):** the same bar. The
  workflow measures the C when `oracles` is among its crates (`gh workflow run readiness.yml
  -f crates="oracles xtask"`, with `xtask` for the Rust that drives the oracles): clang's
  source-based coverage of our C under every vector and the id 1.01 scenarios, the spec
  sections [`oracles/README.md`](../oracles/README.md) lists, and `cargo xtask oracles` 20
  times on each OS. No mutation tool of the bar runs on C; that exception, with its reason,
  is in `oracles/README.md` and `ratchets.toml`. The preview host is Rust,
  `apps/preview-host` in this repository ([ADR-0004](adr/0004-preview-host-base.md)), and is
  covered by the L4 and apps notes above.

## Readiness report

Each readiness issue produces `docs/readiness/<phase>-<layer>.md`, copied from
[`readiness/_template.md`](readiness/_template.md):

1. Crates covered and their spec sections.
2. Coverage, mutation score and spec-citation numbers, with links to the CI runs.
3. Property, fuzz and differential results.
4. Flake-job results on each OS.
5. Independent review: who reviewed, time spent, tests added, defects found and their fixes.
6. Ratchet values committed to CI.
7. Known gaps, each with an issue and the reason it does not block.
