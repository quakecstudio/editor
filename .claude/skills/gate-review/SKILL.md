---
name: gate-review
description: Assemble the evidence for a phase gate (docs/gates/<phase>.md) so Justin can sign it off - every gate criterion with proof, every issue in the phase closed, readiness reports linked. Use for type:gate issues; never closes the gate itself.
argument-hint: "[issue-number]"
allowed-tools: Bash(python3 plan/status.py *) Bash(gh issue view *) Bash(gh issue comment *) Bash(gh pr *) Bash(gh run *)
---

# Gate review

Issue: #$ARGUMENTS

1. `gh issue view $ARGUMENTS`. Find the phase and its gate text in `plan/backlog/_meta.yaml`.
2. `python3 plan/status.py --phase <phase> --track all`. Every issue other than this gate must be closed. If any is open, report it and stop.
3. Write `docs/gates/<phase>.md`:
   - the gate criterion, quoted;
   - for each part of it, the evidence: the test, suite or demo that proves it, with CI run links and numbers;
   - the readiness reports for each layer in the phase (`docs/readiness/`), with their verdicts;
   - the acceptance criteria of this gate issue, each with its evidence;
   - known gaps and the issues that track them;
   - anything the next phase should know.
4. Open the PR with the report (no `Closes`; Justin closes the gate) and do not add it to the merge queue.
5. Comment on the gate issue with a three-line summary and the PR link, and mention @jplock.
