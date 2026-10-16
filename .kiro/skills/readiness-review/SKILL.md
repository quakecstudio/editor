---
name: readiness-review
description: Run an independent readiness review of one layer's test suite against the bar in docs/testing.md and write docs/readiness/<phase>-<layer>.md. Use for type:readiness issues; the reviewer must not have written the layer's code.
triggers: readiness-review, readiness review, /readiness-review
---

# Readiness review (Kiro pointer)

This is a thin pointer. The authoritative procedure is
[`.claude/skills/readiness-review/SKILL.md`](../../../.claude/skills/readiness-review/SKILL.md);
follow it step by step. This file only records the Kiro-specific translations.

- **Issue number:** where the source writes `#$ARGUMENTS`, use the readiness issue number
  you were invoked with.
- **Scripts (referenced in place):** mark In Progress with
  `.claude/skills/work-issue/scripts/start.sh <issue-number>`; land a passing report with
  `.claude/skills/work-issue/scripts/land.sh <pr-number>`.
- **Independence (TEST-9):** you must not have written this layer's code; you are here to
  break it. Judge from CI evidence, not from memory or the author's description. See
  `.kiro/steering/guardrails.md`.

The shape of the work, unchanged from the source: read `docs/testing.md` in full; list the
layer's crates and the `docs/spec/` sections they implement; collect every TEST requirement's
evidence from CI (coverage, mutation score, spec citations, property tests, fuzzing,
differential results, the flake job, performance); write at least five new tests trying to
break the layer; file an issue for every defect and make the review depend on it with the
`plan-change` skill; write `docs/readiness/<phase>-<layer>.md` from the template; land it
with `Closes` only if the layer passes, otherwise open the PR without `Closes` and report
the blocking issues. Do not fix the code yourself.
