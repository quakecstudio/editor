---
name: project-conv17-audit
description: [rust] How CONV-17 audits are filed in quakecstudio-editor - plan/ entries not gh issues, type:chore, one PR per crate group; last audit 2026-10-20
metadata:
  type: project
---

[rust] In this repo, audit findings go into `plan/backlog/<phase>.yaml` (the plan-change skill), never `gh issue create`: `plan/apply.py` generates issues from plan/ after merge. Refactors are labelled `type:chore` so `plan/check.py` adds no readiness dependencies (only type:feature/type:test do), and the phase gate's `depends` list gets each new key. Defects that change behaviour are filed separately as `type:bug` (which needs `summary` and `environment` fields).

Group refactors as ONE issue per layer, delivered as one PR with a commit per crate, chained L0 to L1 to L2 to L3 (Justin, 2026-10-20: every PR runs the full CI matrix, so as few PRs as possible). The first draft's one-issue-per-crate grouping was replaced by P2-80 to P2-83; bugs stay separate type:bug entries (P2-84, P2-85) closed by the same layer PR, because an issue has exactly one type label.

**Why:** the user (via the coordinator) asked for this shape for P2-60 (#452) on 2026-10-20; the record is docs/audits/conv-17-l0-l3.md, PR #487 (draft, held until readiness reviews #83/#84 close; do not land or run apply.py).

**How to apply:** for a later audit cycle, read docs/audits/ first and pick crates not covered there (L4 and up did not exist on 2026-10-20). Use IDs like `qc-sema/3` (not `MAP-3`, which collides with spec statement IDs). Parallel read-only general-purpose subagents per crate group worked well; spot-check their line numbers. See [[feedback-worktree-bash]].
