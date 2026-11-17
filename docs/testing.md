# Test-suite readiness

No work may build on a layer until that layer's test suite is reliable. This document
defines "reliable" as a bar each layer must clear, and how the plan enforces it.
Requirement IDs (`TEST-n`) are tracked in `plan/requirements.yaml`.

From Phase 3 on, a phase has its gate review alone: the per-layer readiness reviews below are
the P1 and P2 bar, and no new ones are generated (`plan/gen_readiness.py`). The compiler and VM
differentials, the fuzz targets and the property tests keep running in CI, the evidence the
gates read (Justin, 2026-11-04: focus on the MVP and move through the phases faster).

## How it is enforced

- Every phase has one **readiness issue per layer** it builds (`type:readiness`, keys like
  `P1-R1` for Phase 1, layer L1). It depends on every feature and test issue of that layer in
  that phase, and on the same layer's readiness from earlier phases.
- If a feature or test issue depends on work in a **lower layer** (or a C tool), `plan/check.py`
  makes it depend on that layer's readiness issue too. Upward progress waits on the
  lower layer's suite, not just its code.
- Phase gates depend on every readiness issue in their phase, so a phase cannot close with
  a layer whose suite has not passed.
- Once a layer passes, its numbers are recorded in its readiness report (`docs/readiness/`,
  TEST-10). The spec-citation ratchet in `qs-tests` holds citations in pull-request CI; the
  per-crate coverage and mutation minimums (`ratchets.toml`) and the workflow that judged them
  were removed once every phase's layers had passed (2026-11-16).
- A defect found in a higher layer that originates in a lower one reopens the lower layer's
  readiness: it is fixed in the lower layer with a regression test there, and the review is
  repeated for the affected crate (TEST-11).

## The bar

The numbers below are confirmed in [ADR-0009](adr/0009-test-readiness-bar.md) and measured when
a layer's readiness review runs, not on every pull request. Exceptions are listed per crate in its README with a reason, never waived silently.

| ID | Requirement | Threshold | Measured by |
| --- | --- | --- | --- |
| TEST-1 | Line and branch coverage per crate, excluding generated code | lines ≥ 90%, branches ≥ 80% | `make test-coverage` (cargo-llvm-cov) |
| TEST-2 | Mutation score per crate | ≥ 90% of mutants caught; every survivor killed by a new test or recorded as equivalent with a reason | `make test-mutants` (cargo-mutants) |
| TEST-3 | Spec traceability: every normative statement in the crate's `docs/spec/` sections has a test citing it | 100%, no baseline backlog for new crates | spec-citation ratchet in `qs-tests` |
| TEST-4 | Property tests for every encoder/decoder (round trip) and every stated invariant | one property per invariant listed in the crate README | proptest |
| TEST-5 | Fuzzing of every parser or decoder that reads files or network input | 24 h without a crash or divergence; minimized findings committed as regression tests | `make test-fuzz`, results in the readiness report |
| TEST-6 | Differential tests wherever an oracle exists (id VM, id qcc, pinned FTEQCC, id map tools, ericw-tools) | zero divergences; ≥ 90% of corpus statements exercised: the QuakeC statements of the differential corpus that the suite executes, as DIFF-11 measures them and [ADR-0013](adr/0013-differential-coverage-bar.md) reads the bar, not the line or branch coverage of the crate under test, which is TEST-1's; except the id VM differential suite, whose hard bar is zero divergences and whose statement coverage is measured and reported but not gated at 90% because its mock does not drive the running world ([ADR-0013](adr/0013-differential-coverage-bar.md)) | differential suites |
| TEST-7 | No flaky tests | full suite passes 20 consecutive runs on each of macOS, Windows and Linux; no wall-clock waits | 20 runs per OS, recorded in the readiness report |
| TEST-8 | Performance where a budget applies | within budget on the reference machines; regression over 10% fails | `make bench` history |
| TEST-9 | Independent adversarial review | a reviewer who did not write the code (person or separate agent session) spends a fixed time trying to break the layer and writes at least five new tests; any defect found blocks readiness until fixed and re-reviewed | readiness report |
| TEST-10 | Ratchets | the readiness numbers are recorded in the layer's readiness report, and a later review measures against them | readiness report |
| TEST-11 | Reopening | a lower-layer defect found from above reopens that layer's readiness | issue process |

## Measuring a layer

The reviewer measures the layer's crates with the tools themselves and records the commands and
numbers in the readiness report (`docs/readiness/_template.md`): `make test-coverage`
(`cargo llvm-cov`, lines on the pinned stable toolchain and `--branch` on the date-pinned
nightly, [`conventions.md`](conventions.md)), `make test-mutants` (`cargo mutants` with nextest
and the `mutants` profile; a mutant that times out counts as caught, one that does not build
does not count), the crate's tests 20 times on each OS, and the spec-citation ratchet in
`qs-tests`. Each crate's numbers come from its own tests (CONV-11), so tests in `qs-tests` do
not count towards another crate's coverage or mutation score.

The `Readiness` workflow, `cargo xtask readiness` and `cargo xtask ratchet`, which automated
these measurements while the layers were being built, were removed on 2026-11-16 with
`ratchets.toml`; the reports in `docs/readiness/` link the runs that passed each layer.

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
  C is measured by clang's source-based coverage of our C under every vector and the id 1.01
  scenarios (`QS_ORACLE_COVERAGE`), the spec sections
  [`oracles/README.md`](../oracles/README.md) lists, and `cargo xtask oracles` 20 times on each
  OS. No mutation tool of the bar runs on C; that exception, with its reason, is in
  `oracles/README.md`. The preview host is Rust,
  `apps/quakec-engine` in this repository ([ADR-0004](adr/0004-preview-host-base.md)), and is
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
