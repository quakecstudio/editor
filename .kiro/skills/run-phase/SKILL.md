---
name: run-phase
description: Drive a whole QuakeC Studio phase to completion, gate included, by working every unblocked issue in this session, one at a time, until plan/status.py reports the phase complete or waiting on people. Use with a goal loop for unattended runs.
triggers: run-phase, run phase, /run-phase
---

# Run a phase (Kiro pointer)

This is a thin pointer. The authoritative procedure is
[`.claude/skills/run-phase/SKILL.md`](../../../.claude/skills/run-phase/SKILL.md); follow it
step by step. This file only records the Kiro-specific translations.

- **Phase and track:** where the `.claude/` procedure writes `$ARGUMENTS`, use the phase
  (for example `P1`) and track (`agents` by default, or `build`/`review`) you were invoked
  with.
- **No delegation:** as in the source, do not hand issues to the `.kiro/agents/` agents.
  Work each `[build]` issue yourself with `work-issue`, one at a time. For a `[review]` issue
  (readiness reviews and the phase gate), ask Justin how the review is done, or in an
  unattended run label it `escalated` with a comment; a readiness review is never done by the
  session that wrote the code.
- **Status and cleanup:** `python3 plan/status.py --phase <phase> --track <track>` drives the
  loop; its last line is the verdict (`COMPLETE`, `IN PROGRESS`, `WAITING ON PEOPLE`,
  `BLOCKED`). Remove a finished issue's worktree with `wt remove -y <path>` only after its
  PR merged and its issue closed, skipping any with uncommitted changes.
- **Guardrails apply:** the coordinator implements issues itself and never reviews its own
  work. `needs:human` work is prepared, not merged. See `.kiro/steering/guardrails.md`.

- **Scheduled runs and code review:** each pass starts by checking the scheduled workflows
  on `main` (the nightly Differential run and any other `schedule` workflow), which no PR
  runs, with the source's `gh run list --branch main --event schedule ...` command, and
  treats a failed one as work to do. When a PR merges, check that its body has the "Code
  review" section `work-issue` asks for.

The loop, the "preparing people's work" step and the finish summary are all as described in
the source. See `docs/agents/README.md` for how a whole phase is run under a goal loop.
