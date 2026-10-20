---
name: run-phase
description: Drive a whole QuakeC Studio phase to completion, gate included, by delegating every unblocked issue to the right agent (implementer or reviewer) until plan/status.py reports the phase complete or waiting on people. Use with a goal loop for unattended runs.
triggers: run-phase, run phase, /run-phase
---

# Run a phase (Kiro pointer)

This is a thin pointer. The authoritative procedure is
[`.claude/skills/run-phase/SKILL.md`](../../../.claude/skills/run-phase/SKILL.md); follow it
step by step. This file only records the Kiro-specific translations.

- **Phase and track:** where the `.claude/` procedure writes `$ARGUMENTS`, use the phase
  (for example `P1`) and track (`agents` by default, or `build`/`review`) you were invoked
  with.
- **Delegation:** the source delegates each issue through Claude Code's `Agent` tool to the
  `implementer` and `reviewer` subagents. In Kiro, delegate by invoking the agents defined
  in `.kiro/agents/` instead:
  - `[build]` issues (specs, ports, features, tests, tooling, docs) -> the `implementer`
    agent.
  - `[review]` issues (readiness reviews and the phase gate) -> the `reviewer` agent.
  Never hand a reviewer the implementer's notes; a reviewer works from the evidence, and a
  readiness review is always done by a different agent from the one that wrote the code.
- **Status and cleanup:** `python3 plan/status.py --phase <phase> --track <track>` drives the
  loop; its last line is the verdict (`COMPLETE`, `IN PROGRESS`, `WAITING ON PEOPLE`,
  `BLOCKED`). Remove a finished agent's worktree with `wt remove -y <path>` only after its
  PR merged and its issue closed, skipping any with uncommitted changes.
- **Guardrails apply:** the orchestrator keeps the queue moving and keeps reviewers
  independent; it does not implement issues itself. `needs:human` work is prepared, not
  merged. See `.kiro/steering/guardrails.md`.

- **Scheduled runs and code review:** each pass starts by checking the scheduled workflows
  on `main` (the nightly Differential run and any other `schedule` workflow), which no PR
  runs, with the source's `gh run list --branch main --event schedule ...` command, and
  treats a failed one as work to do. When an implementer reports back, check that its PR
  body has the "Code review" section `work-issue` asks for.

The loop, parallelism (at most three issues at once, never two implementers on the same
crate), the "preparing people's work" step and the finish summary are all as described in
the source. See `docs/agents/README.md` for how a whole phase is run under a goal loop.
