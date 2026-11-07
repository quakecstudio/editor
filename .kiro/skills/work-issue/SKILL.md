---
name: work-issue
description: Take one QuakeC Studio GitHub issue from open to merged - read it, check its dependencies, implement it on a branch to its acceptance criteria and the definition of done, open a PR, wait for CI and squash-merge it. Use for any single plan issue.
triggers: work-issue, work issue, /work-issue
---

# Work one issue (Kiro pointer)

This is a thin pointer. The authoritative procedure is
[`.claude/skills/work-issue/SKILL.md`](../../../.claude/skills/work-issue/SKILL.md); follow
it step by step. This file only records the Kiro-specific translations.

- **Issue number:** where the `.claude/` procedure writes `#$ARGUMENTS`, use the issue
  number you were invoked with. Kiro does not substitute `$ARGUMENTS`.
- **Scripts (referenced in place, never copied):**
  - Mark the issue In Progress: `.claude/skills/work-issue/scripts/start.sh <issue-number>`
    (the `.claude/` text calls this `${CLAUDE_SKILL_DIR}/scripts/start.sh`).
  - Land the PR: `.claude/skills/work-issue/scripts/land.sh <pr-number>` (waits for CI and
    squash-merges the PR, or queues it if the merge queue is turned back on).
- **Tools:** the `allowed-tools` list in the source is the intended scope: `gh issue`,
  `gh pr`, `python3 plan/*`, and the two scripts above.
- **`code-review` skill:** `code-review` is an external Claude Code plugin skill
  (`dev-agents@claude-dev-agents`), not a `.claude/skills/` file, and Kiro cannot load it.
  Where the source says to run `/code-review high <pr>`, a Kiro session performs that
  self-review itself: review the PR's own diff at high effort against `docs/conventions.md`,
  including "Type design (CONV-17)", fix or answer every finding, and record the outcome in the
  "Code review" section of the PR body. See the `code-review` note in
  `.kiro/steering/kiro-powers.md`.
- **Guardrails apply:** no panics in shipped code, no upward layer dependencies, specs
  quoted not recalled, the four-part porting checklist, never weaken a check to get green,
  and `needs:human` issues are prepared not merged. See `.kiro/steering/guardrails.md`.

The shape of the work, unchanged from the source: read the issue and confirm every
dependency is closed; build exactly the deliverable on a `<type>/<issue>-<slug>` branch;
meet every acceptance criterion with a test or a recorded result; run the checks (`make ci`);
commit with Conventional Commits and open a PR with `Closes #<issue>`; review the PR's diff
yourself with the `code-review` skill at high effort and append a "Code review" section to
the PR body giving each finding and whether it was fixed or answered; land it, watching CI
with `gh pr checks <pr> --watch --fail-fast --interval 30` or
`gh run watch <run-id> --exit-status --interval 60`, never a long `sleep`; confirm the issue
closed and report the PR, the code review's outcome and any follow-up issues.
