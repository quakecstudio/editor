---
name: run-phase
description: Drive a whole QuakeC Studio phase to completion by delegating every unblocked issue to the right subagent (spec-author, implementer or reviewer) until plan/status.py reports the phase complete or waiting on people. Use with /goal for unattended runs.
argument-hint: "[phase, e.g. P1] [track: agents (default), spec, build or review]"
allowed-tools: Bash(python3 plan/status.py *) Bash(gh issue view *) Bash(gh issue comment *)
---

# Run a phase

Phase and track: $ARGUMENTS (track defaults to `agents`, which is everything an agent may close).

You are the orchestrator. You do not implement issues yourself and you never read GPL source; you keep the queue moving and the roles separate.

## Loop

1. Run `python3 plan/status.py --phase <phase> --track <track>`. Its last line is the verdict:
   - `COMPLETE`: every issue in the track is closed. Go to "Finish".
   - `WAITING ON PEOPLE #n ...`: everything left is blocked by `needs:human` issues. Make sure each of those has been prepared (below), then go to "Finish".
   - `BLOCKED #n ...`: blocked by open work outside the track (another track, or an earlier phase). Report it and go to "Finish".
   - `IN PROGRESS, next #n ...`: work the listed issues.
2. For each issue listed as next, delegate it with the Agent tool to the subagent for its track (shown in brackets in the report):
   - `[spec]` (clean-room specs and GPL tools) -> `spec-author`
   - `[build]` -> `implementer`
   - `[review]` (readiness reviews) -> `reviewer`
   Give the subagent only the issue number and the instruction to use its skills. Do not pass on GPL source, spec drafts or another role's notes.
3. Issues with no dependency between them can run in parallel, at most three at a time. Never run a `spec-author` and an `implementer` on the same component at once.
4. When a subagent reports back, check that its PR merged and the issue closed (`gh issue view <n> --json state`). If it failed, read why: fix the cause by delegating a new attempt with that information, or, if it needs a person, comment on the issue and move on.
5. Run the status again and repeat.

## Preparing people's work

For every open `needs:human` issue in the phase that blocks the track, make sure a person can act on it quickly:
- decisions: delegate to `implementer` with the `prepare-decision` skill (draft ADR, options, recommendation, draft PR);
- the phase gate: delegate to `reviewer` with the `gate-review` skill once everything else in the phase is closed;
- hardware, credentials or legal review: comment on the issue with exactly what is needed and why it blocks.
Do this once per issue; check the issue comments first so you do not repeat it.

## Finish

Print the final `plan/status.py` output, then a short summary: issues closed this run with their PRs, issues waiting on people and what each needs, and anything that failed.

## Using it with /goal

Set auto mode, then for example:

```
/goal Run /run-phase P1 agents until python3 plan/status.py --phase P1 --track agents prints a last line starting "STATUS P1 agents: COMPLETE" or "STATUS P1 agents: WAITING ON PEOPLE". Run that command at the end of every turn so its last line is in the transcript. Every merged PR passed CI. Never read GPL source in this session. Stop after 150 turns.
```
