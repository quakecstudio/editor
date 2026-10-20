# Readiness measurement and ratchets

**Crates:** `xtask` (`readiness`, `ratchet`), `ratchets.toml`, `xtask/fixtures/readiness/`. **Specs:** none of its own; TEST-1 to TEST-11 of `docs/testing.md`.

## Context and design philosophy

No work may build on a layer until that layer's test suite is reliable (`docs/testing.md`,
opening). This leaf defines "reliable" as numbers and enforces them: the bar in
`docs/testing.md` (TEST-1 to TEST-11), the `cargo xtask readiness` measurements, the
per-crate minimums in `ratchets.toml` judged by `cargo xtask ratchet`, the on-demand
`Readiness` workflow, the fixture crates that prove each check fails, and the readiness
reports in `docs/readiness/`.

Observed principles, each traced to a source:

- The thresholds are measured only when a layer's readiness review runs, not on pull requests
  or a schedule (ADR-0009, Decision option E).
- A passing review's numbers become minimums that only move up without a new reason, and any
  change names the readiness issue that set it (TEST-10; `ratchets.toml:24-26`).
- Measuring never fails because a number is low; only a tool that cannot run fails it. Judging
  is a separate step (`xtask/src/readiness.rs:17-18`).
- Each crate's numbers come from its own tests; `qs-tests` does not count toward another
  crate (CONV-11; `docs/testing.md`, Measuring a layer).
- Each check has a fixture below its threshold, and a unit test in pull-request CI shows the
  check fails on it (ADR-0009, Fixtures; `xtask/fixtures/readiness/README.md`).
- The VM differential's bar is zero divergences; its coverage is reported, not gated
  (ADR-0013; TEST-6).

## Components

### The bar

`docs/testing.md` is the authority; ADR-0009 confirms its numbers. In summary of what the
tooling enforces (`readiness.rs:79-85`):

| Check | `cargo xtask readiness` | Threshold | Tool |
| --- | --- | --- | --- |
| TEST-1 lines | `lines` | 90 | `cargo llvm-cov`, pinned stable |
| TEST-1 branches | `branches` | 80 | `cargo llvm-cov --branch` on `nightly-2026-10-01` |
| TEST-2 mutation | `mutation` | 90 | `cargo mutants`; timeouts caught, unbuilt mutants not counted |
| TEST-3 spec | `spec` | 100 | crate README `**Spec sections:**`, `**Spec statements:**`, `**Not this crate's:**`, `requirements.tsv`, `coverage-baseline.tsv` |
| TEST-7 flakes | `flakes` | 100 (20 of 20 runs, on linux, macos, windows) | `cargo test` |

TEST-4, TEST-5, TEST-6, TEST-8, TEST-9 and TEST-11 are reviewer-checked and recorded in the
report, not measured by this tooling.

### Measurement (`xtask/src/readiness.rs`)

`cargo xtask readiness <check> --out <file> [--os <os>] <crate>...` resolves each crate by
package name (`cargo metadata`) or directory, and writes a `Measurement {check, os, crates}`
JSON. Target kinds: a workspace Member, a Standalone workspace (the fixtures, each with its
own `CARGO_TARGET_DIR`), and `oracles`, the C under `oracles/`, measured with clang
source-based coverage and given no mutation score (`readiness.rs:116-126, 322`). Coverage runs
set a fixed proptest seed, ChaCha RNG and 256 cases (`readiness.rs:377-395`; P2-64). Mutation
runs use `--copy-vcs=true`, incremental builds, the `mutants` profile and a test timeout of at
least 10 s (`readiness.rs:475-509`). `mutant_key` drops line and column so an
equivalent-mutant entry survives edits (`readiness.rs:580-594`).

### Judging (`xtask/src/ratchet.rs`, `ratchets.toml`)

`cargo xtask ratchet [--history <file>] [<measurement.json>...]`:

- validates `ratchets.toml`: known crates only (`layers.toml`), `readiness` keys that exist in
  `plan/backlog/readiness.yaml`, non-empty reasons, no flake exception
  (`ratchet.rs:113-123, 197, 240-244`);
- walks every commit that touched `ratchets.toml` and fails a change of a reviewed table without
  a new `readiness` key, or a lowered minimum or new exception or equivalent mutant without a
  new `reason`; a table may go only when its crate left `layers.toml` (`ratchet.rs:256-310`);
- judges each measurement against `max(bar, minimum)`, the bar dropping to 0 under an
  exception; fails unlisted surviving mutants, fewer than 20 flake runs, a missing OS, and an
  unmeasured check unless an exception leaves no minimum (`ratchet.rs:400-470`);
- appends a run to the `--history` JSON and prints the minimums to commit.

With no measurement files it still validates the file and its history; the xtask test
`a_command_with_arguments_reports_its_outcome` runs it on the real repository in every
`cargo test` (`xtask/src/main.rs:322-325`).

`ratchets.toml` holds tables for the L0 crates (P1-R0), L1 crates (P2-R1), L2 crates
(P1-R2), `qs-map` (P1-R0) and `oracles` (P1-RG, mutation exception), plus reason-only tables
for `xtask` (P1-55) and `qc-sema` (P2-43) with no minimums.

### The workflow (`.github/workflows/readiness.yml`)

`workflow_dispatch` with a `crates` input; jobs `coverage`, `mutation`, `flakes` (three OSes)
and `ratchet`; uploads measurements and the readiness history (Actions cache, saved from
`main`). It enables clang only when `oracles` or `xtask` is named (`readiness.yml:60, 111`)
and has no concurrency group by design (`readiness.yml:21-22`).

### Plan and reports

- `plan/backlog/readiness.yaml`, generated by `plan/gen_readiness.py`: one `P<n>-R<layer>`
  issue per layer per phase; `plan/check.py` makes upward work depend on lower-layer readiness.
- `docs/readiness/_template.md` and one report per review: `p1-r0`, `p1-r1`, `p1-r2`, `p1-rg`,
  `p2-r1` (Ready), `p2-r2`, `p2-r3` (Not ready).
- `.claude/skills/readiness-review/SKILL.md` and the `reviewer` subagent run reviews by an
  agent that did not write the code (TEST-9).

### Fixtures (`xtask/fixtures/readiness/`)

`fx-lines`, `fx-branches`, `fx-mutation`, `fx-spec` (a small repository with its own spec
files) and `fx-flakes`, each below one threshold, with recorded tool output in `recorded/`
so the unit tests in `xtask/src/readiness/tests.rs` run without the tools.

## Error model

- Measuring returns `Err` only when a tool cannot run; low numbers are data
  (`readiness.rs:17-18`).
- Judging returns a `Report {passed, problems}`; any problem fails the command
  (`xtask/src/main.rs`, `Report`).
- `percent` and `floor_percent` treat 0 of 0 as 100% (`readiness.rs:1146-1167`).

## Verification

- `xtask/src/readiness/tests.rs` and `xtask/src/ratchet/tests.rs` run the measurement and
  judging code on the fixtures in `xtask/fixtures/readiness/`, each below one threshold, with
  recorded tool output, so pull-request CI shows each check fails on its fixture.
- `a_command_with_arguments_reports_its_outcome` runs `cargo xtask ratchet` on the real
  `ratchets.toml` and its history in every `cargo test` (`xtask/src/main.rs:322-325`).

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| When the bar is measured | On demand at readiness reviews | PR gates plus nightly (option B, recommended) | ADR-0009: keep CI lean until the product has real code |
| What PR CI runs | Spec-citation ratchet (`qs-tests`) and the fixture tests | Full readiness checks | ADR-0009 table |
| Branch coverage toolchain | Date-pinned nightly `nightly-2026-10-01` | Stable | ADR-0009; `readiness.rs:32-36` (branch coverage needs nightly) |
| Ratchet history check | Every commit that touched `ratchets.toml` | Only the working copy | [inferred] a lowered minimum cannot be hidden across commits |
| C oracles mutation | Exception, mutation = 0 | Port a C mutation tool | `ratchets.toml:30-37`; `oracles/README.md` Readiness |
| Flakes exceptions | Never allowed | Allow with reason | `ratchet.rs:240-244` [inferred: TEST-7 has no waiver] |
| VM differential coverage | Reported, not gated | 90% gate | ADR-0013 |
| Equivalent mutant keys | File plus mutant text, no line:col | Exact cargo-mutants name | `readiness.rs:580-594`: survive unrelated edits |

## Open questions

### Resolved
1. Thresholds confirmed unchanged (ADR-0009, 2026-10-15).
2. VM differential coverage not gated (ADR-0013, 2026-10-17).

### Deferred
1. **Ratchets in pull-request CI.** TEST-10's "CI ratchets" needs a new decision (ADR-0009,
   Later). Until then decay is found at the next review: the L2 run 39455878689 after #486
   failed `cargo xtask ratchet` on qc-lexer and qc-preproc, which P2-86 fixes on branch
   `fix/p2-86-l2-readiness-ratchets` (not in the mapped commit).
2. **Open reviews.** P2-R2 (#83) and P2-R3 (#84) are Not ready as written
   (`docs/readiness/p2-r2.md`, `p2-r3.md`); their proposed minimums are not committed, and
   `qc-sema` and `qc-engines` have no minimums.
3. **Crates with no review.** `xtask` has never had a full review (`ratchets.toml:39-42`);
   `qc-diag` (L0) is in no report and no P2-R0 exists; `qs-differential`, `qs-bench` and
   `qs-tests` (tests layer) have no table, and `docs/testing.md` does not say whether the bar
   applies to them.
4. **TEST-2 exceptions.** `docs/testing.md:27` allows per-crate exceptions with a reason, while
   TEST-2's row has no exception clause; `qc-lno` mutation 89% is waived this way
   (`ratchets.toml:54-58`; `docs/readiness/p1-r0.md:58`).
5. **TEST-6 wording.** "statements or code paths" is flagged as ambiguous and "for a person to
   settle" (`docs/readiness/p2-r2.md:236`, `p2-r3.md:190`).
6. **TEST-5 24-hour fuzz** is a recorded gap in every report (`docs/gates/p1.md:90`).
7. **Stale comments in `ratchets.toml`.** Lines 47-53, 64-66, 76-77, 87-88, 98-100, 110-116
   and 224-229 say "a later review adds the minimums" above tables that have them; lines
   245-246, 278-279, 287-288 and 300-301 name P1-R1 where the key is P2-R1.
8. **Text-matched inputs.** Readiness keys are read by matching `- key: ` lines, not YAML
   (`ratchet.rs:163-173`); tests assert `readiness.yml` contains the nightly text
   (`readiness/tests.rs:150-162`).
9. **Silent history gaps.** A `git show` failure for one version is treated as "file absent"
   (`ratchet.rs:322`).
10. **Process seam.** `member_dir` runs `cargo metadata` with `std::process::Command`, not the
    injected `Exec` (`readiness.rs:280-284`); coverage calls the rustup proxy `cargo`, not
    `$CARGO` (`readiness.rs:341`); ratchet reuses `bench::{commit, unix_time}`
    (`ratchet.rs:32`).
11. **Spec check reading.** Citations are read from `crates/`, `apps/` and `xtask/` textually,
    as the `qs-tests` ratchet does, so `// spec` lines in non-test code count
    (`readiness.rs:876, 1132`; see [corpora-fuzzing](corpora-fuzzing.md), Open questions).
12. **Doc drift.** `xtask/README.md:11` lists an outdated `ci` step set; ADR-0004 vs ADR-0005
    on the engine oracle that `docs/testing.md:90` follows (ADR-0005).

## References

- `docs/testing.md`, `docs/adr/0009-test-readiness-bar.md`,
  `docs/adr/0013-differential-coverage-bar.md`
- `ratchets.toml`, `layers.toml`, `xtask/fixtures/readiness/README.md`, `xtask/README.md`
- `docs/readiness/`, `docs/gates/p1.md`, `.claude/skills/readiness-review/SKILL.md`
- `plan/backlog/readiness.yaml`, `plan/gen_readiness.py`, `plan/check.py`
- Requirements: TEST-1 to TEST-11, CONV-11
- Plan: P0-24, P1-35, P1-55, P1-57, P2-43, P2-62, P2-64, P2-68, P2-86 (in flight); reviews
  P1-R0, P1-R1, P1-R2, P1-RG, P2-R1, P2-R2 (#83), P2-R3 (#84)
