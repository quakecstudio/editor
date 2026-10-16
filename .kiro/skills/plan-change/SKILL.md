---
name: plan-change
description: Change the QuakeC Studio plan safely - add or edit requirements and issues in plan/, keep every check passing, and sync GitHub. Use when work shows the plan is wrong or incomplete, or to file a follow-up issue.
triggers: plan-change, plan change, /plan-change
---

# Change the plan (Kiro pointer)

This is a thin pointer. The authoritative procedure is
[`.claude/skills/plan-change/SKILL.md`](../../../.claude/skills/plan-change/SKILL.md);
follow it step by step. This file only records the Kiro-specific translations.

- **Tools:** the `allowed-tools` list in the source is the intended scope: `python3 plan/*`
  and the plan unit tests (`cd plan && python3 -m unittest ...`).
- **Guardrails apply:** GitHub issues are generated from `plan/`; never edit an issue body on
  GitHub, because the next sync overwrites it. Follow-up work becomes a new issue, never a
  comment. See `.kiro/steering/guardrails.md`.

The shape of the work, unchanged from the source: edit the data (`plan/requirements.yaml`
for a requirement, the phase file in `plan/backlog/` for an issue, and add a new issue to its
phase gate's `depends` list); run `python3 plan/gen_readiness.py` if you added or relabelled
a feature or test issue; make `python3 plan/check.py --write-coverage` and the plan unit
tests pass; commit the plan change with the work that needed it and land it; after it merges,
sync GitHub with `python3 plan/apply.py --repo quakecstudio/editor` (idempotent).
