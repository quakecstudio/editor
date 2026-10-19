---
name: run-phase
description: Drive a whole QuakeC Studio phase to completion, gate included, by delegating every unblocked issue to the right subagent (implementer or reviewer) until plan/status.py reports the phase complete or waiting on people. Use with /goal for unattended runs.
argument-hint: "[phase, e.g. P1] [track: agents (default), build or review]"
allowed-tools: Bash(python3 plan/status.py *) Bash(gh issue view *) Bash(gh issue comment *) Bash(gh issue edit *) Bash(wt remove -y *)
---

# Run a phase

Phase and track: $ARGUMENTS (track defaults to `agents`, which is everything an agent may close).

You are the orchestrator. You do not implement issues yourself; you keep the queue moving and keep reviewers independent of the code they review.

## Loop

1. Run `python3 plan/status.py --phase <phase> --track <track>`. Its last line is the verdict:
   - `COMPLETE`: every issue in the track is closed. Go to "Finish".
   - `WAITING ON PEOPLE #n ...`: everything left is blocked by `needs:human` issues. Make sure each of those has been prepared (below), then go to "Finish".
   - `BLOCKED #n ...`: blocked by open work outside the track (another track, or an earlier phase). Report it and go to "Finish".
   - `IN PROGRESS, next #n ...`: work the listed issues.
2. For each issue listed as next, delegate it with the Agent tool to the subagent for its track (shown in brackets in the report):
   - `[build]` (specs, ports, features, tests, tooling, docs) -> `implementer`
   - `[review]` (readiness reviews and the phase gate) -> `reviewer`
   Give the subagent the issue number and the instruction to use its skills. Never hand a reviewer the implementer's notes; it works from the evidence.
3. Issues with no dependency between them can run in parallel, at most three at a time. Do not run two implementers on issues that change the same crate at once.
4. When a subagent reports back, check that its PR merged and the issue closed (`gh issue view <n> --json state`), and that an implementer's PR body has the "Code review" section `work-issue` asks for, recording the `code-review` run and what became of each finding (`gh pr view <pr> --json body`). If the section is missing, list the PR under "Finish" as merged without its code review. Once all are confirmed, remove the agent's worktree with `wt remove -y <path>` (the path the Agent tool reported, under `.claude/worktrees/`): each finished worktree keeps its own `target/`, and about 60 of them once used 64 GB. Skip a worktree with uncommitted changes (`git -C <path> status --porcelain` prints anything) or whose agent is still running; worktrees Claude Code has locked clear when the session ends. If it failed, read why: fix the cause by delegating a new attempt with that information, or, if it needs a person, add the `escalated` label, comment on the issue with what is needed, mention @jplock and move on (`status.py` then counts it as waiting on people). If the gate reviewer reports stragglers that can be finished in this phase, work them before retrying the gate.
5. Run the status again and repeat.

## Preparing people's work

For every open `needs:human` issue in the phase that blocks the track, make sure a person can act on it quickly:
- decisions: delegate to `implementer` with the `prepare-decision` skill (draft ADR, options, recommendation, draft PR);
- hardware, credentials, legal review or the MVP release: comment on the issue with exactly what is needed and why it blocks.
Do this once per issue; check the issue comments first so you do not repeat it.

## Finish

Print the final `plan/status.py` output, then a short summary: issues closed this run with their PRs, issues waiting on people and what each needs, any PR merged without its code review, and anything that failed.

## Using it with /goal

Set auto mode, then for example:

```
/goal Run /run-phase P1 agents until python3 plan/status.py --phase P1 --track agents prints a last line starting "STATUS P1 agents: COMPLETE" or "STATUS P1 agents: WAITING ON PEOPLE". Run that command at the end of every turn so its last line is in the transcript. Every merged PR passed CI. Stop after 150 turns.
```
