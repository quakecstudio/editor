---
name: run-phase
description: Drive a whole QuakeC Studio phase to completion, gate included, by working every unblocked issue in this session, one at a time, until plan/status.py reports the phase complete or waiting on people. Use with /goal for unattended runs.
argument-hint: "[phase, e.g. P1] [track: agents (default), build or review]"
allowed-tools: Bash(python3 plan/status.py *) Bash(gh run list *) Bash(gh issue view *) Bash(gh issue comment *) Bash(gh issue edit *) Bash(wt remove -y *)
---

# Run a phase

Phase and track: $ARGUMENTS (track defaults to `agents`, which is everything an agent may close).

You are the coordinator, and you work every issue yourself: no subagents, forks or teammates for issue work, fixes or reviews (Justin, 2026-10-29). They cost tokens out of proportion to what they deliver and repeat context this session already has. Readiness reviews and gates still need a reviewer who did not write the code, which this session cannot be; see step 3.

## Loop

1. Check the scheduled workflows on `main` (the nightly Differential run and any other `schedule` workflow), which no PR runs and so nothing else watches. List the latest scheduled run of each workflow that did not succeed:

   ```bash
   gh run list --branch main --event schedule --limit 40 --json workflowName,conclusion,createdAt,url --jq 'group_by(.workflowName)|map(max_by(.createdAt))|.[]|select(.conclusion!="success")'
   ```

   Empty output means every one passed. A run still going (an empty `conclusion`) has not failed; look again on the next pass. A failure whose fix has merged, with a green `workflow_dispatch` run of that workflow on the fix, stays listed until the next scheduled run and needs nothing more. If anything else failed, it is fixed (or filed with `plan-change` and worked next) before new work starts: a red nightly on `main` is a defect in work already merged.
2. Run `python3 plan/status.py --phase <phase> --track <track>`. Its last line is the verdict:
   - `COMPLETE`: every issue in the track is closed. Go to "Finish".
   - `WAITING ON PEOPLE #n ...`: everything left is blocked by `needs:human` issues. Make sure each of those has been prepared (below), then go to "Finish".
   - `BLOCKED #n ...`: blocked by open work outside the track (another track, or an earlier phase). Report it and go to "Finish".
   - `IN PROGRESS, next #n ...`: work the listed issues.
3. Take the issues listed as next one at a time, the one with the most IDE value first:
   - `[build]` (specs, ports, features, tests, tooling, docs): work it with the `work-issue` skill from start to merge before starting the next.
   - `[review]` (readiness reviews and the phase gate): do not review work this session wrote, and do not spawn a reviewer. Ask Justin how he wants the review done; in an unattended run, add the `escalated` label, comment on the issue that it needs an independent reviewer, mention @jplock and move on (`status.py` then counts it as waiting on people). When Justin has named the runs that decide it (for example green Readiness, Fuzz and Differential runs on `main`), those runs are the evidence and no reviewer round is needed.
4. While a long job runs (CI on a PR, a dispatched workflow, a local mutation run), wait on its background watch (`gh pr checks --watch`, `gh run watch`) and its completion notification: no polling and no sleeps. Run one `make ci` at a time.
5. When an issue's PR has merged, confirm the issue closed (`gh issue view <n> --json state`) and that the PR body has the "Code review" section `work-issue` asks for; if it is missing, list the PR under "Finish" as merged without its code review. Merge `main` into any other open branch. If the issue was worked in a worktree, remove it with `wt remove -y <path>`: each finished worktree keeps its own `target/`, and about 60 of them once used 64 GB. Keep a worktree with uncommitted changes (`git -C <path> status --porcelain` prints anything) or one that holds unfinished work for an issue still open. If an issue needs a person, add the `escalated` label, comment on the issue with what is needed, mention @jplock and move on. If the gate review reports stragglers that can be finished in this phase, work them before retrying the gate.
6. Repeat from step 1.

## Preparing people's work

For every open `needs:human` issue in the phase that blocks the track, make sure a person can act on it quickly:
- decisions: prepare it yourself with the `prepare-decision` skill (draft ADR, options, recommendation, draft PR);
- hardware, credentials, legal review or the MVP release: comment on the issue with exactly what is needed and why it blocks.
Do this once per issue; check the issue comments first so you do not repeat it.

## Finish

Print the final `plan/status.py` output, then a short summary: issues closed this run with their PRs, issues waiting on people and what each needs, any PR merged without its code review, and anything that failed.

## Using it with /goal

Set auto mode, then for example:

```
/goal Run /run-phase P1 agents until python3 plan/status.py --phase P1 --track agents prints a last line starting "STATUS P1 agents: COMPLETE" or "STATUS P1 agents: WAITING ON PEOPLE". Run that command at the end of every turn so its last line is in the transcript. Every merged PR passed CI. Stop after 150 turns.
```
