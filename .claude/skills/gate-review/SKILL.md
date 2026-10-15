---
name: gate-review
description: Review and close a QuakeC Studio phase gate (type:gate) - check that nothing in the phase straggles, prove each gate criterion with evidence in docs/gates/<phase>.md, and land the report with Closes. Escalates to Justin instead of deferring anything.
argument-hint: "[issue-number]"
allowed-tools: Bash(python3 plan/status.py *) Bash(gh issue *) Bash(gh pr *) Bash(gh run *) Bash(gh api *) Bash(.claude/skills/work-issue/scripts/land.sh *)
---

# Gate review

Issue: #$ARGUMENTS

Justin does not sign gates off. You close the gate yourself when the phase is finished with nothing straggling; when it is not, you escalate and stop. You never defer work to a later phase to get a gate closed.

## 1. Nothing straggles

1. `gh issue view $ARGUMENTS`. Find the phase and its gate text in `plan/backlog/_meta.yaml`. If the gate is labelled `needs:human` (the MVP gate, which publishes the release), assemble the evidence below, open the PR without `Closes`, mention @jplock on the issue and stop.
2. `python3 plan/status.py --phase <phase> --track all`: every issue other than this gate must be closed.
3. Every issue in the phase's GitHub milestone must be closed, including follow-ups agents filed during the phase that are not in the plan:
   `gh api "repos/quakecstudio/editor/issues?milestone=<milestone-number>&state=open" -q '.[] | select(.pull_request|not) | "#\(.number) \(.title)"'` must print only this gate.
4. No open PR may reference an issue in the phase: `gh pr list --state open --json number,title,body`.
5. No issue in the phase was closed as "not planned" or moved to a later milestone during the phase without the plan saying so (`gh api "repos/quakecstudio/editor/issues?milestone=<n>&state=closed"`, check `state_reason`).

If any check fails:
- work that can be finished in this phase: leave the gate open and report the issues; the orchestrator works them, and they become dependencies of the gate through `plan-change` if they are not in the plan;
- work that cannot be finished in this phase, or anything that would need deferring: add the `escalated` label to the gate (`gh issue edit $ARGUMENTS --add-label escalated`), comment with the list and what Justin needs to decide, mention @jplock, and stop.

## 2. Prove the gate

Write `docs/gates/<phase>.md`:
- the gate criterion, quoted;
- for each part of it, the evidence: the test, suite or demo that proves it, with CI run links and numbers;
- the readiness reports for each layer in the phase (`docs/readiness/`), with their verdicts;
- the acceptance criteria of this gate issue, each with its evidence;
- the straggler checks from step 1, with their output;
- the updated risk register and anything the next phase should know.

A criterion without evidence fails the gate: file the missing work as an issue in this phase, leave the gate open and report it. Do not weaken a criterion to pass it.

## 3. Close it

1. Open the PR with the report and `Closes #$ARGUMENTS`, then land it with `.claude/skills/work-issue/scripts/land.sh <pr-number>`.
2. Comment on the gate issue: three lines of summary, the report, and every issue in the phase with its state.
