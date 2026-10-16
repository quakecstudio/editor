# ADR-0009: Adopt the readiness bar, with branch coverage and full mutation runs nightly

- **Status:** Proposed
- **Date:** 2026-10-15
- **Decided by:** pending
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
stable Rust 1.99.0 (`rust-toolchain.toml`). CI on three operating systems is
[#3](https://github.com/quakecstudio/editor/issues/3), still open, so no workflow exists yet.

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

## Decision

Recommendation: **option B. Confirm every threshold in `docs/testing.md` unchanged, and run each
check where it can run well.** The numbers are reasonable for code that is ported against a spec
and an oracle, and lowering them now would cost more later, once layers are built on weak suites.
What needs deciding is placement:

| Check | Threshold | Pull requests | Nightly (pinned nightly toolchain, three OSes where noted) |
| --- | --- | --- | --- |
| Line coverage (TEST-1) | at least 90% per crate | Gate, stable, `cargo llvm-cov` | Recorded |
| Branch coverage (TEST-1) | at least 80% per crate | Not run (needs nightly) | Gate for readiness, `cargo llvm-cov --branch` |
| Mutation score (TEST-2) | at least 90% caught; every survivor killed or listed as equivalent with a reason | Gate on `--in-diff`: every mutant in changed code caught or justified | Full run per crate; the score is recorded |
| Spec citations (TEST-3) | 100% of normative statements cited | Gate | Recorded |
| Flakes (TEST-7) | 20 consecutive passing runs | Not run | 20 runs on macOS, Windows and Linux |
| Ratchets (TEST-10) | per-crate minimums | Gate | Gate |

The ratchet mechanism:

- **File:** `ratchets.toml` at the repository root, one table per crate with `lines`, `branches`,
  `mutation` and `spec` minimums and a `readiness` key naming the readiness issue that set them
  (for example `readiness = "P1-R1"`). A crate with no entry has no minimum yet.
- **Check:** `cargo xtask ratchet` (part of `cargo xtask ci`) compares the measured numbers with the
  file and fails below a minimum. It also compares the file with the merge base: a raised or lowered
  value must change that crate's `readiness` key to a key that exists in
  `plan/backlog/readiness.yaml`, and a lowered value also needs a `reason`. The check runs offline,
  against the plan data, without calling GitHub.
- **History:** every CI run writes the per-crate numbers as JSON (`target/metrics/<run>.json`) and
  uploads it as an artifact. The readiness report copies the numbers it relied on, with run links.
  `ratchets.toml` is the durable record. A committed history file would put a bot commit on `main`
  for every run, which the merge queue does not allow.
- **Fixtures:** as with the clippy fixtures from #1, a fixture crate under `xtask/fixtures/` sits
  below each threshold, and `cargo xtask ci` asserts that each matching check fails on it. That is
  the issue's acceptance test.

The nightly job pins its nightly toolchain by date, as `rust-toolchain.toml` pins stable, and
updates it on purpose, never through `nightly` floating.

## Consequences

If accepted as recommended:

- `docs/testing.md`: "proposed" becomes "confirmed in ADR-0009", and the Measured-by column names
  the pull-request and nightly jobs above. No threshold changes. `plan/requirements.yaml` TEST-1 and
  TEST-2 drop "(proposed ...)" from their text.
- The rest of #24's deliverable is then built on the issue's branch, after this ADR is accepted:
  `docs/readiness/_template.md`, the coverage, mutation, flake and spec-citation jobs, `ratchets.toml`
  and `cargo xtask ratchet`, and the fixture crates. The workflow files need #3's CI. If #3 has not
  landed, the jobs are `cargo xtask` subcommands that #3's workflows call, and the CI run links
  follow when #3 merges.
- `docs/conventions.md`: one line allowing a date-pinned nightly toolchain in the nightly job only
  (fuzzing already uses nightly).
- Every readiness issue (`plan/backlog/readiness.yaml`) already cites ADR-0009's thresholds; no plan
  change is needed.

## Links

- Issue: [#24](https://github.com/quakecstudio/editor/issues/24); pull request: this ADR's draft PR.
- [`docs/testing.md`](../testing.md), [`docs/conventions.md`](../conventions.md), [`plan/backlog/readiness.yaml`](../../plan/backlog/readiness.yaml).
- Related: [#2](https://github.com/quakecstudio/editor/issues/2) (Makefile and `cargo xtask ci`), [#3](https://github.com/quakecstudio/editor/issues/3) (CI on three OSes).
- [cargo-llvm-cov README](https://github.com/taiki-e/cargo-llvm-cov) (branch coverage requires nightly).
- cargo-mutants: [Testing code changed in a diff](https://mutants.rs/in-diff.html), [Continuous integration](https://mutants.rs/ci.html).
