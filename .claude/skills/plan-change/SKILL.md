---
name: plan-change
description: Change the QuakeC Studio plan safely - add or edit requirements and issues in plan/, keep every check passing, and sync GitHub. Use when work shows the plan is wrong or incomplete, or to file a follow-up issue.
allowed-tools: Bash(python3 plan/*) Bash(cd plan && python3 -m unittest *)
---

# Change the plan

GitHub issues are generated from `plan/`. Never edit an issue body on GitHub; the next sync overwrites it.

1. Edit the data:
   - a requirement: `plan/requirements.yaml` (`kind: stated` if the design doc says it, `implied` if it is needed but unstated; update `docs/design/design-doc.md` in the same PR for a stated one);
   - an issue: the phase file in `plan/backlog/` (`p0.yaml` to `p4.yaml`, `p5-p7.yaml`). Give it the next free key in its phase, a title, one `type:*` label, its layer and area labels, a size, the requirement IDs, dependencies, a concrete deliverable with paths, acceptance criteria that a test or recorded result can prove, and verification. Add `needs:human` if a person must act before it closes. Decisions name a `docs/adr/` file.
   - a new issue in a phase also goes into that phase's gate issue `depends` list.
2. If you added or relabelled a feature or test issue: `python3 plan/gen_readiness.py`.
3. `python3 plan/check.py --write-coverage` and `cd plan && python3 -m unittest discover -p 'test_*.py'` (every plan test, as `cargo xtask ci` runs them) must pass.
4. Commit the plan change with the work that needed it, in that work's PR, and land it. A standalone plan PR is only for a plan change with no other open work to join, since every PR runs the full CI matrix.
5. After it merges, sync GitHub: `python3 plan/apply.py --repo quakecstudio/editor`. It is idempotent: it creates new issues, updates changed ones and prints what it did. New issues join the project board automatically.
