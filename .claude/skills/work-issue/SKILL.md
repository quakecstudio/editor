---
name: work-issue
description: Take one QuakeC Studio GitHub issue from open to merged - read it, check its dependencies, implement it on a branch to its acceptance criteria and the definition of done, open a PR, wait for CI and add it to the merge queue. Use for any single plan issue.
argument-hint: "[issue-number]"
allowed-tools: Bash(${CLAUDE_SKILL_DIR}/scripts/land.sh *) Bash(python3 plan/*) Bash(gh issue view *) Bash(gh pr *)
---

# Work one issue

Issue: #$ARGUMENTS

## 1. Read before touching anything

1. `gh issue view $ARGUMENTS` - the deliverable, acceptance criteria, verification, requirement IDs, dependencies and the definition of done are all in the body.
2. Every dependency in "Depends on" must be closed. If one is open, stop and report it; do not work around it.
3. Look up each requirement ID in `plan/requirements.yaml` and read the matching section of `docs/design/design-doc.md`. For UI work, open the mockups in `docs/design/mockups/` and match them.
4. Check the labels:
   - `needs:human`: do not implement or merge. Use the `prepare-decision` skill (decisions) or prepare what a person needs and comment on the issue (hardware, credentials, legal, gate). Then stop.
   - `clean-room:impl`: work only from `docs/spec/`. Never open id's GPL source, the oracles repo or a GPL port's source. If the spec is missing something, stop and file a spec issue (`plan-change` skill) instead of guessing or looking.
   - `clean-room:spec` or `gpl-tools`: you may read the GPL source. Record every GPL file you read in `docs/clean-room/readers.md`. Never write the clean-room implementation of anything you specified.
   - `type:readiness`: use the `readiness-review` skill instead.
   - `type:gate`: use the `gate-review` skill instead.

## 2. Build it

1. `git switch main && git pull --ff-only`, then `git switch -c <type>/<issue-number>-<short-slug>`, for example `feat/45-qc-types`.
2. Do exactly what the deliverable says, at the paths it names. Follow `docs/conventions.md`: no panics, `unwrap` or `expect` in shipped crates; a crate depends only on lower layers; every source file starts with the SPDX line.
3. Meet every acceptance criterion with a test or a recorded result. Tests that pin a spec rule cite the spec section.
4. Run the checks that exist so far. Once #2 lands that is `make ci` (or `cargo xtask ci`); until then run `python3 plan/check.py` and the tests of whatever you touched.
5. If the work shows the plan is wrong or incomplete, change it with the `plan-change` skill in the same PR, or file a follow-up issue. Never leave it in a comment.

## 3. Land it

1. Commit with Conventional Commits. No Co-Authored-By trailers. Plain hyphens, not em or en dashes.
2. Push and open the PR: `gh pr create --title "<conventional title>" --body "<what changed, how each acceptance criterion is met>\n\nCloses #$ARGUMENTS"`.
3. Land it: `${CLAUDE_SKILL_DIR}/scripts/land.sh <pr-number>`. It waits for CI, adds the PR to the merge queue (agents may do this once CI passes) and waits for the merge.
4. If CI fails, fix the cause on the branch and run `land.sh` again. Never skip, disable or loosen a test, a lint or a ratchet to get green.
5. When it has merged, confirm the issue closed (`gh issue view $ARGUMENTS --json state`). Report the PR, what it delivered and any follow-up issues you filed.
