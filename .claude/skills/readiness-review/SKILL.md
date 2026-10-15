---
name: readiness-review
description: Run an independent readiness review of one layer's test suite against the bar in docs/testing.md and write docs/readiness/<phase>-<layer>.md. Use for type:readiness issues; the reviewer must not have written the layer's code.
argument-hint: "[issue-number]"
allowed-tools: Bash(gh issue view *) Bash(gh pr *) Bash(gh run *)
---

# Readiness review

Issue: #$ARGUMENTS

You are the independent reviewer (TEST-9). You did not write this layer's code, and you are here to break it.

1. `gh issue view $ARGUMENTS`. Note the phase, the layer and the issues it depends on (all must be closed). Read `docs/testing.md` in full: the bar, the layer notes and the report template.
2. List the layer's crates and the `docs/spec/` sections they implement.
3. Collect the evidence for every TEST requirement from CI, not from memory: coverage, mutation score, spec citations, property tests, fuzzing (24 hours), differential results, the 20-run flake job on each OS, performance against budget. Link each CI run. A number that has not been measured is a failure, not a pass.
4. Spend real effort trying to break the layer: edge cases from the spec, malformed inputs, limits, error paths. Write at least five new tests; keep every one that is meaningful.
5. Every defect you find blocks readiness. File an issue for each (label it with the layer and `type:feature` or `type:test`), link it, and make this review depend on it with the `plan-change` skill. Do not fix the code yourself.
6. Write `docs/readiness/<phase>-<layer>.md` following the template in `docs/testing.md`, including the ratchet values to commit and the known gaps (each with an issue and why it does not block).
7. If the layer passes: commit the report, the new tests and the ratchet values, open a PR with `Closes #$ARGUMENTS`, and land it with `.claude/skills/work-issue/scripts/land.sh <pr-number>`. If it fails: open the PR with the report and new tests but without `Closes`, and report the blocking issues.
