# Plan: requirements, backlog and GitHub sync

**Crates:** `plan/` (Python). **Specs:** none of its own; PM-1, PM-5, PM-6.

## Context and design philosophy

The plan is the single source of the project's work. Every requirement the design doc
states or implies is a record in `plan/requirements.yaml`; every piece of work is an issue
record in `plan/backlog/*.yaml`. GitHub issues, labels, milestones and the Project board are
generated from these files and never edited by hand (CLAUDE.md, "How work is organised";
`.claude/rules/commits-and-issues.md`, "Issues"). Requirements PM-1, PM-5 and PM-6 in
`plan/requirements.yaml` are the intent this component serves.

The guiding principle is that the plan is data and its rules are code: `plan/check.py`
enforces coverage, dependency order and the shape of every issue, and `cargo xtask ci` runs
it with its tests on every change (`xtask/src/ci.rs:33-46`). Agents read phase progress from
`plan/status.py`, whose last line is the verdict a `/goal` loop tests (`docs/agents/README.md`).

## Components

### Data model

| File | Holds | Read by |
| --- | --- | --- |
| `plan/requirements.yaml` | `design_doc_sha256`; `sections[]` of `{id, kind: stated or implied, text}` (GOAL, LIC, PLAT, PRIN, ARCH, FND, LANG, COMP, ENG, AGT, UI, MAP, QA, MVP, OPEN, PM, CONV, TEST) | check.py, apply.py, review_page.py |
| `plan/backlog/_meta.yaml` | definition of done (7 items), milestones P0-P7 and LATER with gate text, label groups (layer, area, type, size, owner), sizes | check.py, apply.py, status.py |
| `plan/backlog/p0.yaml` ... `p5-p7.yaml` | issues `{key, title, labels, size, reqs, deliverable[], acceptance[], verify[], depends[]}`; `type:bug` issues also carry `summary` and `environment` | check.py |
| `plan/backlog/readiness.yaml` | generated `type:readiness` issues, one per layer per phase (33 issues, P1-R0 to P7-RG) | check.py, xtask ratchet |
| `plan/coverage.md` | generated requirement-to-issue table and per-milestone counts | check.py (staleness) |

Issue keys follow `P<n>-<NN>`, `P<n>-R<layer>` and `L-<NN>`, but a key's prefix does not have
to match its milestone: P5-01 and P5-02 sit in P1 (`plan/backlog/p1.yaml:1246, 1266`), P4-29
in P2, P5-05 in P3, P0-23 in P4, and P4-11, P4-14, P4-19 in LATER.

### Checking (`plan/check.py`)

`load_backlog` (`plan/check.py:72-93`) reads every backlog file, stamps each issue with its
milestone and file, and then rewrites dependencies in place:

- `add_gate_dependencies` (`plan/check.py:133-150`): every issue depends on the previous
  phase's gate; every LATER issue depends on `P4-25`, the MVP gate, hard-coded at
  `plan/check.py:139`.
- `add_readiness_dependencies` (`plan/check.py:100-131`): a feature or test issue that
  depends on a feature or test issue in a lower layer also depends on that layer's readiness
  issue for the dependency's phase (TEST-12, `docs/testing.md`); a missing readiness issue is
  an error naming `plan/gen_readiness.py`. Gates depend on every readiness issue in their phase.

`main` (`plan/check.py:387-403`) then runs six rule sets and fails on any problem:
`check` (fields, milestones, sizes, labels, requirement IDs, dependency validity and order),
`check_scope` (every LATER issue linked from `docs/scope.md`), `check_risks` (`docs/risks.md`
matches the design doc's risk table), `check_templates` (issue forms and
`.github/pull_request_template.md` match the definition of done), `check_design_hash` (the
recorded hash equals `docs/design/design-doc.md`) and `check_coverage` (`plan/coverage.md`
is current). `--write-coverage` regenerates `coverage.md` first.

### Sync (`plan/apply.py`)

`apply.py --repo R [--project org/N] [--dry-run]` syncs labels, milestones, issues, Project
fields (Phase, Layer, Size, Requirements) and views (Board, Roadmap, Decisions). An issue is
matched to its plan record by the body marker `<!-- plan-key: KEY -->` (`plan/apply.py:28`).
`render_body` (`plan/apply.py:102-140`) writes requirements, deliverable, acceptance,
verification, dependencies and the definition of done. Writes are throttled one second apart
(`plan/apply.py:56`). An open GitHub issue whose key left the plan makes the run exit 1 after
syncing (`plan/apply.py:423`).

### Progress (`plan/status.py`)

`track_of` (`plan/status.py:54-60`) puts `needs:human` or `escalated` issues in `human`,
readiness and gates in `review`, the rest in `build`; `agents` is `build` plus `review`.
`compute` (`plan/status.py:72-127`) yields a verdict line: `COMPLETE`, `IN PROGRESS, next #..`,
`BLOCKED #..` or `WAITING ON PEOPLE #..`. `--readme` rewrites README's Next run block between
the `next-goal` markers; `--check-readme` fails when it is stale.

### Generation

`plan/gen_readiness.py` regenerates `readiness.yaml` from the feature and test issues per
layer and phase P1-P7; each readiness issue requires TEST-1 to TEST-12 and depends on P0-24
(`plan/gen_readiness.py:45-52`). `plan/review_page.py` renders a self-contained HTML review
page of the plan.

## Error model

- `plan/check.py` collects every problem into one list, prints it and exits 1 (with one
  exception, an unknown milestone, in Open questions).
- `plan/apply.py` refuses to sync when `check()` reports a problem and exits 1 after syncing
  when an open GitHub issue's key has left the plan.
- `plan/status.py --check-readme` exits non-zero when README's Next run block is stale.

## Verification

- `plan/test_check.py`, `plan/test_status.py` and `plan/test_apply.py`, run by the
  `plan-tests` step of `cargo xtask ci`; `plan/check.py` itself is the `plan` step.

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Source of truth for work | YAML in `plan/`, GitHub generated from it | Issues edited on GitHub | CLAUDE.md "How work is organised": issue bodies are overwritten by the sync; plan changes go through PRs and checks |
| Gate and readiness dependencies | Injected by the loader, not written in YAML | Hand-written `depends` | TEST-12 and the phase order hold without each issue repeating them [inferred] |
| Readiness issues | Generated by `gen_readiness.py` | Hand-written | One per layer per phase stays consistent with the layers of feature issues [inferred] |
| Requirement coverage | Checked and rendered to `coverage.md` | Manual review | PM-6: "Nothing in the design doc is missed: requirement coverage checked automatically" |
| Design-doc drift | SHA-256 of the design doc recorded in `requirements.yaml` | No check | A design-doc edit forces a requirements review (`plan/README.md`) [inferred] |
| GitHub pagination | Page-number pagination | gh's Link-header pagination | The docstring says the session proxy rejects Link-header paging (`plan/apply.py:82-86`) |

## Open questions

### Resolved

None recorded.

### Deferred

1. `apply.py` refuses to sync only on `check()` errors (`plan/apply.py:321-324`); a plan
   failing `check_scope`, `check_risks`, `check_templates`, the design hash or coverage still
   syncs. Intended, or should it call the same rule set as `check.py main`?
2. `check.py:196` calls `PHASE_ORDER.index(milestone)`, which raises `ValueError` for an
   unknown milestone after `check.py:179-180` has already recorded the error, so the user
   sees a traceback instead of the problem list.
3. `P4-25` is hard-coded as the MVP gate (`plan/check.py:139`) and `P0-24` as every readiness
   issue's dependency (`plan/gen_readiness.py:49`).
4. `status.py --check-readme` exists but nothing runs it: no reference in `xtask/src` or
   `.github/`, so the README block can go stale until a gate reviewer reruns `--readme`.
5. The plan unittest commands differ by document: `cargo xtask ci` and `plan/README.md:38`
   discover every `test_*.py` (including `test_apply.py`); CLAUDE.md, `.claude/rules/branching.md:38`
   and `.claude/skills/plan-change/SKILL.md:16` list only `test_check.py` and `test_status.py`.
6. `plan/requirements.yaml:3` says requirements are covered by issues "in backlog.yaml"; the
   backlog is `plan/backlog/*.yaml`.
7. `plan/requirements.yaml:253` (CONV-15) asks for "dual license files"; ADR-0002 chose a
   single GPL-3.0-or-later license and only `LICENSE` exists. CONV-13
   (`plan/requirements.yaml:251`) lists six rule files; `.claude/rules/` has seven.
8. `plan/review_page.py:15-23` hard-codes a "Decisions for you" list of resolved Phase 0
   questions; no test covers the script.
9. `plan/status.py:225` opens the `--states` file without closing it; `plan/gen_readiness.py:14`
   has an unused `done = []`.
10. `plan/test_check.py` asserts on real keys (P0-01, P1-10, P1-25, P1-52, P0-08, P1-09) and
    risk rows (R-1, R-2, R-7, R-8, R-9), so editing those records can break the tests.
11. `type:bug` is defined in `_meta.yaml:57` and used by `.github/ISSUE_TEMPLATE/bug.yml:4`, but
    the label table in `.claude/rules/commits-and-issues.md` omits it.

## References

- `plan/README.md`, `plan/requirements.yaml`, `plan/backlog/*.yaml`, `plan/coverage.md`
- `plan/check.py`, `plan/apply.py`, `plan/status.py`, `plan/gen_readiness.py`, `plan/review_page.py`
- Tests: `plan/test_check.py`, `plan/test_status.py`, `plan/test_apply.py`
- ADR-0001 (decisions as issues and ADRs, PM-2), `docs/roadmap.md`, `docs/testing.md` (TEST-12)
- Consumers: `xtask/src/ci.rs` (plan and plan-tests steps), `xtask/src/ratchet.rs` (readiness keys),
  `.claude/skills/plan-change`, `.claude/skills/run-phase`, `.github/ISSUE_TEMPLATE/`
