# Readiness: <phase> <layer> (<plan key>)

<!--
Copy this file to docs/readiness/<phase>-<layer>.md (for example p1-r1.md) and fill in every
section. The bar is in docs/testing.md and ADR-0009; how to measure it is under "Measuring a
layer" there. Delete these comments.
-->

- **Issue:** #<number> (`<plan key>`)
- **Reviewer:** <name or agent session>; did not write the layer's code
- **Date:** <YYYY-MM-DD>
- **Commit measured:** <full SHA>
- **Verdict:** Ready | Not ready

## 1. Crates and spec sections

| Crate | Layer | Spec sections |
| --- | --- | --- |
| `qs-...` | L<n> | <PREFIX-n, ... or none> |

## 2. Coverage, mutation score and spec citations

Readiness workflow run: <link to the `Readiness` workflow run>.

| Crate | Lines (bar 90%) | Branches (bar 80%) | Mutation (bar 90%) | Spec citations (bar 100%) |
| --- | --- | --- | --- | --- |
| `qs-...` | <n>% (<covered> of <total>) | | | |

Surviving mutants, each killed by a new test or recorded in `ratchets.toml` under
`equivalent_mutants` with the reason it cannot be killed:

| Crate | Mutant | Outcome |
| --- | --- | --- |
| | | <killed by `test name` / equivalent: reason> |

## 3. Property, fuzz and differential results

| Crate | Property tests (TEST-4) | Fuzzing (TEST-5) | Differential (TEST-6) |
| --- | --- | --- | --- |
| `qs-...` | <one per invariant in the README: yes / gaps> | <targets, hours, crashes> | <oracle, cases, divergences, corpus statements executed (DIFF-11, ADR-0013)> |

## 4. Flakes

20 consecutive runs of each crate's tests (TEST-7), from the same workflow run:

| Crate | Linux | macOS | Windows |
| --- | --- | --- | --- |
| `qs-...` | 20 of 20 | 20 of 20 | 20 of 20 |

No test waits on the wall clock: <how this was checked>.

## 5. Independent review (TEST-9)

- **Reviewer and time spent:** <who>, <hours>
- **Tests added:** <at least five; list them with their files>
- **Defects found:** <each with its issue, fix and regression test, or "none">

## 6. Ratchets

The minimums committed to `ratchets.toml` in this review's pull request (`cargo xtask ratchet`
prints them for a passing run), with `readiness = "<plan key>"`:

```toml
[crates.qs-...]
readiness = "<plan key>"
lines = <n>
branches = <n>
mutation = <n>
spec = 100
```

## 7. Known gaps

| Gap | Issue | Why it does not block |
| --- | --- | --- |
| <none> | | |

Exceptions to the bar are listed in the crate README with a reason and under `exceptions` in
`ratchets.toml`; list each one here too.
