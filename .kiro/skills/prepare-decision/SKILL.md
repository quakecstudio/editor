---
name: prepare-decision
description: Prepare a needs:human decision issue for Justin - research the options, draft the ADR with a recommendation, open it as a draft PR and summarise it on the issue. Never decides or merges. Use for type:decision issues.
triggers: prepare-decision, prepare decision, /prepare-decision
---

# Prepare a decision (Kiro pointer)

This is a thin pointer. The authoritative procedure is
[`.claude/skills/prepare-decision/SKILL.md`](../../../.claude/skills/prepare-decision/SKILL.md);
follow it step by step. This file only records the Kiro-specific translations.

- **Issue number:** where the source writes `#$ARGUMENTS`, use the decision issue number you
  were invoked with.
- **Scripts (referenced in place):** mark In Progress with
  `.claude/skills/work-issue/scripts/start.sh <issue-number>`.
- **Guardrails apply:** a decision belongs to Justin. You research, draft and recommend; you
  never decide and never merge. The ADR is opened as a draft PR and is not added to the merge
  queue; the issue stays open until Justin decides. See `.kiro/steering/guardrails.md`.

The shape of the work, unchanged from the source: read the issue with its comments (update
an existing draft ADR PR rather than opening a second); read the requirement IDs and the
matching design-doc sections; research licenses, prices, platform support and prior art and
cite the pages you rely on; write the ADR at the deliverable's path from
`docs/adr/0000-template.md` with status Proposed, add its index row and run `cargo xtask
adr`; open it as a draft PR; comment on the issue with the question, the options one line
each, the recommendation and the PR link, mention @jplock, and stop.
