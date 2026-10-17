# ADR-0013: Make zero divergences the differential suite's bar and report coverage without a flat 90% gate

- **Status:** Accepted
- **Date:** 2026-10-17
- **Decided by:** Justin Plock, 2026-10-17
- **Issue:** [#37](https://github.com/quakecstudio/editor/issues/37)
- **Requirements:** FND-8, QA-1, TEST-6

## Question

Does the differential VM suite enforce the flat "at least 90% of statements" coverage bar of
TEST-6 and DIFF-11, or a differential-specific bar that fits a mock engine specified not to
drive a running world?

## Context

The differential VM suite (P1-13, [#37](https://github.com/quakecstudio/editor/issues/37))
runs each corpus progs in `qc-vm` and in id's VM oracle and compares the traces, to prove the
two VMs execute the same QuakeC the same way. Two statements written for P1-13 conflict.

The coverage bar, `docs/spec/differential.md` DIFF-11, verbatim:

> The bar of [`../testing.md`](../testing.md) (TEST-6) is at least 90% of the statements of each
> corpus codebase executed; this is how the runner measures it.

The mock engine, by design, does not drive physics or interaction, `docs/spec/differential.md`
DIFF-5.8, verbatim:

> **DIFF-5.8** The mock frame leaves out everything that needs the map or another entity's
> box: client movement (`SV_ClientThink`), gravity, `SV_CheckVelocity`, water checks, the
> moves of every movetype, pushing and blocking, trigger and impact touches and `blocked`
> functions, and the client updates. So `origin`, `velocity`, `angles`, `groundentity`,
> `flags`, `waterlevel` and the other fields of EDICT-12 that physics writes change only when
> QuakeC or a builtin writes them; `touch` and `blocked` are never called; and nothing runs
> `ClientKill`, `SetChangeParms` or `ClientDisconnect` except DIFF-8.3.

The suite's purpose, `docs/spec/differential.md` DIFF-1.5, verbatim:

> **DIFF-1.5** The mock engine stands in for id's server; it does not model it. Nothing moves
> and nothing collides (DIFF-5.8, DIFF-7), so a scenario shows that the two VMs execute the
> same QuakeC the same way, not that a mod behaves as it does in Quake. The server simulation
> (`qs-sim`, [#238](https://github.com/quakecstudio/editor/issues/238)) is checked against id's
> engine by its own suite ([#239](https://github.com/quakecstudio/editor/issues/239)).

Because DIFF-5.8 never calls `touch` or `blocked`, never moves or collides, and never runs the
client connect, kill or disconnect paths, the large movement, combat, projectile and monster-AI
bodies of a Quake progs are unreachable under the mock as specified. The measurement in CI
([#388](https://github.com/quakecstudio/editor/issues/388), the `Test Assets (linux)` job),
verbatim:

> id-1.01: 4 scenarios, 0 divergences, 6698 of 20731 statements (32.3%)

So id 1.01 reaches 32.3%, with zero divergences. More hand-written scenarios raise the figure
but cannot cross into code that only a running world reaches, so the flat 90% is unreachable
under the mock without building the running-world simulation now. That simulation is `qs-sim`'s
job ([#238](https://github.com/quakecstudio/editor/issues/238)), with its own differential
suite against id's engine ([#239](https://github.com/quakecstudio/editor/issues/239)); building
it inside the differential mock would duplicate that effort and undercut DIFF-1.5.

The MVP is a modder opening the rerelease QuakeC, asking the agent for a weapon, reviewing the
diff and seeing it fire in a live game, closing at the Phase 4 gate ([ADR-0007](0007-mvp-scope.md),
MVP-1). What proves the agent's edits are safe for that demo is that `qc-vm` runs the same
QuakeC as id's VM, which is zero divergences, not a statement percentage chasing a running
world the mock is specified not to provide.

## Options

| Option | Consequences |
| --- | --- |
| A. Widen the mock engine to drive movement, collision and touch | Changes DIFF-5.8 to run `SV_Physics`-equivalent movetypes, `SV_CheckVelocity`, touches and `blocked`; reaches the combat and movement code and approaches 90%. Large, and it undercuts DIFF-1.5; it duplicates `qs-sim` (#238) and its suite (#239) and risks the two VMs diverging on physics the suite was built not to test. Throwaway work that delays the MVP |
| B. Add many more hand-written scenarios | Keeps DIFF-5.8; writes scenarios that call more functions directly. Partial: raises coverage but cannot reach code gated on movement, touch or a real map, so 90% stays out of reach for id 1.01, and the scenarios become a large hand-maintained corpus |
| C. Make zero divergences the differential bar and report coverage without a flat 90% gate (chosen) | Keeps DIFF-5.8 and DIFF-1.5 as the design intent. The differential suite enforces zero divergences on every corpus codebase (the guarantee that matters for working code, already met on id 1.01), and measures and reports statement coverage (DIFF-11.1 to DIFF-11.3 unchanged) without gating it against a flat 90%. Full-world coverage is left to `qs-sim`'s suite (#239). A spec and ADR change, no running-world work now |

## Decision

Decided: **option C.** The differential VM suite's hard bar is **zero divergences on every
corpus codebase**, enforced in CI (QA-1): the fast subset on id 1.01 on every change, the full
corpus nightly. The suite continues to **measure and report** statement coverage by DIFF-11.1
to DIFF-11.3, but does not gate on a flat 90% figure, because the mock engine is specified
(DIFF-5.8, DIFF-1.5) not to drive the running world, so most of a Quake progs is unreachable
under it and 90% presumes a running world the mock does not provide. Running-world statement
coverage is `qs-sim`'s, measured by its own differential suite against id's engine
([#239](https://github.com/quakecstudio/editor/issues/239)).

This changes only the differential VM oracle's bar. The flat 90% of TEST-6 still applies to the
other oracles (id qcc, pinned FTEQCC, id map tools, ericw-tools), where no running world is
needed to exercise the code.

The owner's instruction (2026-10-17): working code over percentages, and the shortest path to a
working MVP. Zero divergences is the guarantee the MVP's agent edits rest on; chasing 90%
through a running world the mock is specified not to model is throwaway work that delays the
MVP.

## Consequences

Carried by this ADR's pull request (`Closes #37`):

- `docs/spec/differential.md` DIFF-11: the intro bar becomes zero divergences as the gate and
  coverage measured and reported per DIFF-11.1 to DIFF-11.3, citing this ADR; the measurement
  statements DIFF-11.1 to DIFF-11.3 are unchanged.
- `docs/testing.md` TEST-6: the differential row records that zero divergences is the hard bar,
  that the running-world statement figure does not apply to the VM differential suite (this
  ADR), and that the flat 90% still applies to the other oracles.
- `crates/qs-differential`: the enforced check becomes zero divergences in every mode
  (`Fast` and `Full`); coverage stays measured and printed. `MIN_COVERAGE` and the
  coverage-only `enforces_coverage` gate are removed; `CodebaseReport::passes` enforces zero
  divergences over at least one scenario. Tests citing the revised DIFF-11 are added.
- `plan/backlog/p1.yaml` P1-13 acceptance (2): references this ADR's bar instead of a flat 90%.
- `docs/adr/README.md`: this ADR is indexed.

What becomes impossible or harder: the differential VM suite no longer fails on low statement
coverage, so a drop in how many statements the scenarios exercise is not caught by this suite;
it is caught by `qc-vm`'s own coverage at readiness (TEST-1) and by `qs-sim`'s suite (#239) for
the running world.

## Links

- Issue: [#37](https://github.com/quakecstudio/editor/issues/37); pull request: this ADR's pull
  request.
- Measurement: [#388](https://github.com/quakecstudio/editor/issues/388) (the CI wiring that
  measured 32.3%).
- Related: [ADR-0007](0007-mvp-scope.md) (MVP scope, MVP-1), [ADR-0009](0009-test-readiness-bar.md)
  (the readiness bar and TEST-6).
- Sources: [`docs/spec/differential.md`](../spec/differential.md) (DIFF-1.5, DIFF-5.8, DIFF-11),
  [`docs/testing.md`](../testing.md) (TEST-6), `qs-sim`
  ([#238](https://github.com/quakecstudio/editor/issues/238),
  [#239](https://github.com/quakecstudio/editor/issues/239)).
