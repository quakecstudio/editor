# Readiness fixtures

One crate below each threshold of the readiness bar ([ADR-0009](../../../docs/adr/0009-test-readiness-bar.md),
[`docs/testing.md`](../../../docs/testing.md)). Each is its own workspace, outside the main
one, so nothing in pull-request CI builds it.

| Fixture | Below | Why |
| --- | --- | --- |
| [`lines/`](lines) `fx-lines` | lines, 90% | no test calls `describe`: 6 of 18 lines |
| [`branches/`](branches) `fx-branches` | branches, 80% | every line runs but the test takes one side of each `if`: 2 of 4 branches |
| [`mutation/`](mutation) `fx-mutation` | mutation, 90% | the test checks nothing, so both mutants survive |
| [`spec/crates/fx-spec/`](spec/crates/fx-spec) | spec citations, 100% | its README lists section FX-1 and nothing cites FX-1.2; `spec/` is a small repository with its own `docs/spec/requirements.tsv` |
| [`flakes/`](flakes) `fx-flakes` | flakes, 20 of 20 | the test passes at random, half the time |

The xtask unit tests (`xtask/src/readiness/tests.rs`) run in pull-request CI and assert that
each check fails on its fixture. They read the tool output recorded in each fixture's
`recorded/` directory, except the spec check, which needs no tool and is measured live. The
recordings were made with the commands below, then had absolute paths made relative to the
repository and, for `outcomes.json`, everything but the scenarios, summaries and counts removed:

```bash
cargo xtask readiness lines    --out target/measured/lines.json    xtask/fixtures/readiness/lines
cargo xtask readiness branches --out target/measured/branches.json xtask/fixtures/readiness/branches
cargo xtask readiness mutation --out target/measured/mutation.json xtask/fixtures/readiness/mutation
cargo xtask readiness flakes   --out target/measured/flakes.json   xtask/fixtures/readiness/flakes
```

- `lines/recorded/llvm-cov.json` and `branches/recorded/llvm-cov.json`: from
  `target/xtask/readiness/<check>-<crate>.json` (cargo-llvm-cov 0.9.1; branches on the pinned
  nightly).
- `mutation/recorded/outcomes.json`: from
  `target/xtask/readiness/mutants-fx-mutation/mutants.out/outcomes.json` (cargo-mutants 27.1.0).
- `flakes/recorded/flakes.json`: the measurement file itself.

To see each check fail with the real tools, run the `Readiness` workflow on
`xtask/fixtures/readiness/lines xtask/fixtures/readiness/branches xtask/fixtures/readiness/mutation xtask/fixtures/readiness/spec/crates/fx-spec`.
Measure `xtask/fixtures/readiness/flakes` in a run of its own: its tests fail half the time,
which also stops the coverage and mutation tools.
