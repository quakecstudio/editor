# ADR-0009: Adopt the readiness bar, measured at readiness reviews

- **Status:** Accepted
- **Date:** 2026-10-15
- **Decided by:** Justin Plock, 2026-10-15, choosing option E (keep CI lean until there is product code) over the recommended option B
- **Issue:** [#24](https://github.com/quakecstudio/editor/issues/24)
- **Requirements:** TEST-1, TEST-2, TEST-3, TEST-7, TEST-10, TEST-12

## Question

Are the thresholds proposed in `docs/testing.md` the bar a layer's test suite must clear before
anything builds on it, and how does CI measure and ratchet them?

## Context

[`docs/testing.md`](../testing.md) sets out the readiness rule (TEST-12: no issue that builds on a
lower layer starts until that layer's readiness issue passes) and proposes a bar: lines 90% and
branches 80% per crate (TEST-1), a 90% mutation score with every survivor killed or justified
(TEST-2), a citing test for every normative spec statement (TEST-3), 20 consecutive clean runs per
OS (TEST-7), and ratchets so a ready layer cannot decay (TEST-10). It says the numbers "must be
confirmed in ADR-0009 (P0-24) before Phase 1 starts". Every readiness issue in the plan (32 of them,
`plan/backlog/readiness.yaml`) depends on this one, and each phase gate depends on its readiness
issues, so these numbers set the pace of the whole build.

What exists: `make test-coverage` (`cargo llvm-cov`) and `make test-mutants` (`cargo mutants`) are
in the Makefile from #2, and `make test-fuzz` already uses a nightly toolchain. The workspace pins
stable Rust 1.99.0 (`rust-toolchain.toml`). CI on three operating systems landed with
[#3](https://github.com/quakecstudio/editor/issues/3) ([`docs/ci.md`](../ci.md)).

Facts the choice rests on, from the tools' documentation (fetched 2026-10-15):

- **cargo-llvm-cov** measures line, region and branch coverage, but "branch coverage is currently
  optional and requires nightly". On the pinned stable toolchain only lines and regions are
  available.
- **cargo-mutants** can test only the mutants in a diff (`--in-diff`), which "is typically much
  faster than testing all mutants", while its authors warn that incremental runs are "not a
  substitute for a full test run". It emits GitHub annotations and recommends `--in-place` in CI.
- A full mutation run costs one build and test run per mutant, so it grows with the codebase; a
  20-run flake job costs 20 full suites on each of three runners.

## Options

| Option | Consequences |
| --- | --- |
| A. Adopt `docs/testing.md` as written, every number gating every pull request | Strongest guarantee, but branch coverage would force the whole PR pipeline onto nightly Rust (against the pinned toolchain in `docs/conventions.md`), and full mutation and 20-run flake jobs on every PR would make CI slow and costly as the code grows |
| B. Adopt the numbers as written, split by where they run (recommended) | Same bar as A. Pull requests gate on line coverage (stable), mutants in the diff and spec citations. A nightly job on a pinned nightly measures branch coverage, the full mutation score and the 20-run flake check, and a readiness review requires green nightly results. A regression in a nightly-only number can land and is caught the next morning, not at merge |
| C. Lower the bar (for example lines 80%, no branch number, mutation 75%) | Faster readiness reviews and earlier phase starts, but weaker evidence for layers that are meant to be proved (the VM, the compiler). The plan's premise that higher layers can trust lower ones weakens |
| D. Raise the bar (mutation 100%, every survivor killed, no "equivalent" exemptions) | Some mutants are truly equivalent and cannot be killed, so 100% would force contorted tests or `#[mutants::skip]` noise. Not recommended |
| E. Adopt the numbers as written, measured only when a layer's readiness review runs (decided) | Same bar as A and B. No new pull-request gates, no nightly job: the reviewer runs one on-demand workflow that measures everything for the layer's crates. Pull-request CI stays as fast as it is now. A regression between reviews is not caught at merge; it is caught at the layer's next readiness review, or turned into a pull-request gate later by its own decision |

## Decision

Decided: **option E. Confirm every threshold in `docs/testing.md` unchanged, and measure them only
when a layer's readiness review runs.** The agent recommended option B (pull-request gates plus a
nightly job); Justin chose to keep CI lean until the product has real code, so nothing new runs on
pull requests or on a schedule.

| Check | Threshold | Pull requests | Readiness review (on-demand workflow, three OSes where noted) |
| --- | --- | --- | --- |
| Line coverage (TEST-1) | at least 90% per crate | Not run | Gate, `cargo llvm-cov` |
| Branch coverage (TEST-1) | at least 80% per crate | Not run | Gate, `cargo llvm-cov --branch` on a date-pinned nightly |
| Mutation score (TEST-2) | at least 90% caught; every survivor killed or listed as equivalent with a reason | Not run | Gate, full `cargo mutants` run per crate |
| Spec citations (TEST-3) | 100% of normative statements cited | The existing spec-citation ratchet test in `qs-tests` (already part of the unit tests) | Gate |
| Flakes (TEST-7) | 20 consecutive passing runs | Not run | 20 runs on macOS, Windows and Linux |
| Ratchets (TEST-10) | per-crate minimums | Not run | Gate: no crate below its recorded minimum |

How it works:

- **Workflow:** one `workflow_dispatch` workflow, `Readiness`, taking a list of crates. It is not a
  required check and has no schedule. The readiness reviewer runs it and links the run in the
  report.
- **Ratchets:** `ratchets.toml` at the repository root, one table per crate with `lines`,
  `branches`, `mutation` and `spec` minimums and a `readiness` key naming the readiness issue that
  set them. `cargo xtask ratchet` compares measured numbers with the file; the readiness workflow
  runs it. A passing review records its numbers there.
- **Fixtures:** a fixture crate under `xtask/fixtures/` sits below each threshold, and a unit test
  asserts that each check fails on it. That is the issue's acceptance test, and it is cheap enough
  for pull-request CI.
- **Later:** moving any of these checks into pull-request CI (TEST-10's "CI ratchets") is a new
  decision once layers hold real code; until then a ready layer's decay is caught at its next
  readiness review.

## Consequences

- `docs/testing.md`: "proposed" becomes "confirmed in ADR-0009"; the Measured-by column names the
  readiness workflow instead of nightly jobs. No threshold changes. `plan/requirements.yaml`
  TEST-1 and TEST-2 drop "(proposed ...)".
- The rest of #24's deliverable is built on the issue's branch after this ADR merges:
  `docs/readiness/_template.md`, the `Readiness` workflow, `ratchets.toml`, `cargo xtask ratchet`
  and the fixture crates. No new required checks.
- `docs/conventions.md`: one line allowing a date-pinned nightly toolchain in the readiness
  workflow only (fuzzing already uses nightly).
- Every readiness issue (`plan/backlog/readiness.yaml`) already cites ADR-0009's thresholds; no plan
  change is needed.

## Links

- Issue: [#24](https://github.com/quakecstudio/editor/issues/24); pull request: this ADR's draft PR.
- [`docs/testing.md`](../testing.md), [`docs/conventions.md`](../conventions.md), [`plan/backlog/readiness.yaml`](../../plan/backlog/readiness.yaml).
- Related: [#2](https://github.com/quakecstudio/editor/issues/2) (Makefile and `cargo xtask ci`), [#3](https://github.com/quakecstudio/editor/issues/3) (CI on three OSes).
- [cargo-llvm-cov README](https://github.com/taiki-e/cargo-llvm-cov) (branch coverage requires nightly).
- cargo-mutants: [Testing code changed in a diff](https://mutants.rs/in-diff.html), [Continuous integration](https://mutants.rs/ci.html).
