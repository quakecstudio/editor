---
name: gate-review
description: Review and close a QuakeC Studio phase gate (type:gate) - check that nothing in the phase straggles, prove each gate criterion with evidence in docs/gates/<phase>.md, and land the report with Closes. Escalates to Justin instead of deferring anything.
triggers: gate-review, gate review, /gate-review
---

# Gate review (Kiro pointer)

This is a thin pointer. The authoritative procedure is
[`.claude/skills/gate-review/SKILL.md`](../../../.claude/skills/gate-review/SKILL.md);
follow it step by step. This file only records the Kiro-specific translations.

- **Issue number:** where the source writes `#$ARGUMENTS`, use the gate issue number you
  were invoked with.
- **Scripts (referenced in place):** mark In Progress with
  `.claude/skills/work-issue/scripts/start.sh <issue-number>`; land the report PR with
  `.claude/skills/work-issue/scripts/land.sh <pr-number>`.
- **Guardrails apply:** you close the gate yourself only when the phase is finished with
  nothing straggling; you never defer work to a later phase to get a gate closed, and you
  never weaken a criterion to pass it. A criterion without evidence fails the gate. If the
  gate is `needs:human` (the MVP gate), assemble the evidence, open the PR without `Closes`,
  mention @jplock and stop. See `.kiro/steering/guardrails.md`.

The three steps, unchanged from the source: confirm nothing straggles (status, the GitHub
milestone, open PRs, closed-as-not-planned checks); write `docs/gates/<phase>.md` proving
each criterion with CI links and numbers and reviewing the risk register; then close it
(point the README at the next run, open the PR with `Closes`, land it, comment on the gate).
