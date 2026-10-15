# CLAUDE.md

Starter instructions for agents working in this repository. The full version, with AGENTS.md, `.claude/rules` and issue and PR templates, is [#4](https://github.com/quakecstudio/editor/issues/4).

## What this is

QuakeC Studio: a Rust IDE for QuakeC with its own compiler, live in-game preview, a built-in agent and a map editor. Read [`docs/roadmap.md`](docs/roadmap.md) first: it is the overall plan and says where we are. Then [`docs/design/design-doc.md`](docs/design/design-doc.md) for the why.

## How work is organised

- Every piece of work is a GitHub issue generated from [`plan/`](plan/README.md). Each issue states its deliverable, acceptance criteria, verification, requirements and dependencies, and carries the shared definition of done from `plan/backlog/_meta.yaml`.
- Phases run in order (P0 to P7, then LATER). An issue depends on the previous phase's gate, and on the readiness review of any lower layer it builds on (`docs/testing.md`). Do not start an issue whose dependencies are open.
- To change the plan, edit `plan/requirements.yaml` or `plan/backlog/*.yaml`, run the checks below, open a PR, and sync GitHub with `plan/apply.py` after it merges. Do not edit issue bodies by hand; the sync overwrites them.
- Decisions go in `docs/adr/` (set up by [#7](https://github.com/quakecstudio/editor/issues/7)).
- `python3 plan/status.py --phase P0` shows what is closed, ready and waiting in a phase.

## Working autonomously

Use the project skills and subagents; [`docs/agents/README.md`](docs/agents/README.md) explains them and how to run a whole phase under `/goal`.

- `/work-issue <n>` for one issue, `/run-phase <phase>` to drive a phase through the `implementer` and `reviewer` subagents.
- `needs:human` issues (decisions, hardware, credentials, legal, the MVP release) are prepared, never closed, by agents: `/prepare-decision`. When an agent stops on something only a person can resolve, it labels the issue `escalated` and comments.
- `/readiness-review` for `type:readiness` issues, `/gate-review` to close a phase gate once nothing in the phase straggles, `/plan-change` to change the plan.

```bash
python3 plan/check.py --write-coverage            # every requirement covered, dependencies valid
(cd plan && python3 -m unittest test_check.py)    # the checker's own tests
python3 plan/gen_readiness.py                     # after adding or relabelling feature or test issues
python3 plan/apply.py --repo quakecstudio/editor  # sync labels, milestones and issues (idempotent)
python3 plan/status.py                            # the current phase's progress; the last line is the verdict
python3 plan/status.py --readme                   # refresh the README's Next run block (current phase and /goal command)
(cd plan && python3 -m unittest test_status.py)   # status tests
```

## Rules

- **License and porting.** The whole project is GPL-3.0-or-later ([ADR-0002](docs/adr/0002-license-model.md)). Port from id's source freely: keep id's copyright line, cite the file and function, document the behaviour in `docs/spec/` and prove it with a differential test against the oracle. Dependencies must be GPL-3.0-compatible. The process is written up in `docs/porting.md` by [#9](https://github.com/quakecstudio/editor/issues/9).
- **Rust conventions.** Follow [`docs/conventions.md`](docs/conventions.md): pinned toolchain, exact-pinned workspace dependencies, no panics, unwraps or expects in shipped crates, crates depend only on lower layers.
- **Tests before climbing.** A layer's work above it waits until that layer meets the bar in [`docs/testing.md`](docs/testing.md).
- **UI.** Match the approved mockups in [`docs/design/`](docs/design/README.md). The game runs in its own docked window in v1; the embedded game view is post-MVP.
- **Git.** `main` only accepts changes through the merge queue: work on a branch and open a PR. An agent may add its own PR to the merge queue once CI passes (`.claude/skills/work-issue/scripts/land.sh`), except for `needs:human` issues, which wait for Justin. Use Conventional Commits. Do not add Co-Authored-By trailers.
- **Writing.** Plain hyphens, not em or en dashes, in docs and messages.
- **Never** bundle Quake game data (pak0/pak1) or QuakeC 1.06, which has no license.
