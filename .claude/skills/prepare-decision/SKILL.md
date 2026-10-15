---
name: prepare-decision
description: Prepare a needs:human decision issue for Justin - research the options, draft the ADR with a recommendation, open it as a draft PR and summarise it on the issue. Never decides or merges. Use for type:decision issues.
argument-hint: "[issue-number]"
allowed-tools: Bash(gh issue view *) Bash(gh issue comment *) Bash(gh pr *)
---

# Prepare a decision

Issue: #$ARGUMENTS

A decision belongs to a person. Your job is to make it a five-minute read.

1. `gh issue view $ARGUMENTS --comments`. If a comment already links a draft ADR PR, update that PR instead of opening another.
2. Read the requirement IDs it cites in `plan/requirements.yaml`, the matching sections of `docs/design/design-doc.md`, and any open question about it in the design doc's "Risks and open questions".
3. Research what the decision depends on: licenses, prices, platform support, prior art. Fetch the pages you rely on and cite them. Label anything you could not verify.
4. Write the ADR at the path the issue's deliverable names (in `docs/adr/`, from `docs/adr/0000-template.md` once #7 exists) with status **Proposed**:
   - the question, in one sentence;
   - two to four real options, each with its consequences for the plan (which issues, layers or phases it changes);
   - your recommendation and why, in one paragraph;
   - what changes in `plan/` if the recommendation is accepted.
5. Open it as a **draft** PR: `gh pr create --draft --title "docs(adr): propose <decision>" --body "Proposes a decision for #$ARGUMENTS ..."`. Do not add it to the merge queue.
6. Comment on the issue with the question, the options in one line each, the recommendation, and the PR link, then mention @jplock.
7. Stop. The issue stays open until Justin decides, marks the ADR Accepted and merges.
