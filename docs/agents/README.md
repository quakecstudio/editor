# Building with agents

QuakeC Studio is built by Claude Code agents working through the plan's issues, one phase at a time, with Justin deciding what only a person can decide. This page explains the moving parts and how to start a run.

## What is autonomous and what is not

Agents take an issue from open to merged on their own: they read it, build it on a branch, open a PR, wait for CI, add the PR to the merge queue and confirm the merge. They also run the independent readiness reviews, close phase gates when nothing in the phase straggles, and prepare everything a person has to act on.

Issues labelled `needs:human` wait for Justin. They are:

- every decision (`type:decision`): the agent drafts the ADR with options and a recommendation as a draft PR; Justin decides and merges it;
- work that needs hardware, credentials or a lawyer: the MVP demo on three OSes (#135), the license compliance review before release;
- the MVP gate (#136), because it publishes the release.

Other phase gates are not yours: the reviewer agent closes a gate once every issue in the phase's milestone is closed, including follow-ups filed along the way, nothing was deferred to a later phase, and every gate criterion has evidence in `docs/gates/<phase>.md`. If something would have to be deferred, it labels the gate `escalated` and asks you instead.

Any issue an agent cannot finish without a person gets the `escalated` label and a comment saying what is needed. `status.py` treats it like `needs:human`; remove the label once it is resolved.

Creating new GitHub repositories (such as `quakecstudio/tree-sitter-quakec`) is also Justin's; an agent that needs one stops and says so.

## The pieces

| Piece | What it does |
| --- | --- |
| [`plan/status.py`](../../plan/status.py) | Reads the plan and GitHub and prints a phase's progress: closed, ready to work, waiting and on what. Its last line is the verdict a `/goal` condition tests: `COMPLETE`, `IN PROGRESS, next #..`, `WAITING ON PEOPLE #..` or `BLOCKED #..` |
| `/run-phase <phase>` | The orchestrator skill: delegates every ready issue to the right subagent and loops until the phase is complete or waiting on people |
| `/work-issue <n>` | Takes one issue from open to merged |
| `/prepare-decision <n>` | Drafts an ADR for a decision issue and asks Justin |
| `/readiness-review <n>` | Independent review of a layer's test suite against [`docs/testing.md`](../testing.md) |
| `/gate-review <n>` | Checks that nothing in the phase straggles, writes the gate report with its evidence and closes the gate |
| `/plan-change` | Changes `plan/` safely and syncs GitHub |
| `implementer` subagent | All build work (specs, ports, features, tests, tooling, docs) and decision drafts |
| `reviewer` subagent | Readiness reviews and phase gates; never fixes what it reviews |
| `.claude/skills/work-issue/scripts/start.sh` | Sets the issue's Status on the project board to In Progress when work starts; closing it moves it to Done |
| `.claude/skills/work-issue/scripts/land.sh` | Waits for CI, adds the PR to the merge queue, waits for the merge |

`status.py` sorts issues into tracks by who may work them: `build`, `review` (readiness and gates), `human` (`needs:human` or `escalated`), and `agents` (`build` and `review`). The orchestrator works the `agents` track and routes each issue by its track, so a readiness review is always done by a different agent from the one that wrote the code.

The project is GPL-3.0-or-later ([ADR-0002](../adr/0002-license-model.md)), so agents may read and port id's source directly; ports keep id's copyright line and cite where they came from.

## Merge policy

An agent may add its own PR to the merge queue once CI passes on it (`land.sh` does this). An implementer's PR also needs its own diff reviewed with the `code-review` skill at high effort and the outcome recorded in the PR body (`work-issue`, "Land it"); that is a self-review, not an independent one. Readiness and gate PRs are reviews already and skip it. It never merges a `needs:human` issue's PR, never skips or loosens a test, lint or ratchet to get green, and never pushes to `main` directly.

## Before the first run

- Claude Code signed in with your claude.ai account, run from a clone of this repo.
- `gh auth login` with access to the `quakecstudio` org (issues, PRs, the merge queue).
- Python 3 with PyYAML (`pip install pyyaml`) for `plan/`.
- Auto mode on, so tool calls inside each turn do not wait for approval.

## Running a phase

The [README](../../README.md#status) always shows the current phase and the exact command for the next run. `python3 plan/status.py --readme` writes it, and the gate reviewer updates it to the next phase in the PR that closes each gate. The general form is:

```
/goal Run /run-phase P0 agents until python3 plan/status.py --phase P0 --track agents prints a last line starting "STATUS P0 agents: COMPLETE" or "STATUS P0 agents: WAITING ON PEOPLE". Run that command at the end of every turn so its last line is in the transcript. Every merged PR passed CI. Stop after 150 turns.
```

Change `P0` to the phase. When the run stops on `WAITING ON PEOPLE`, the issues it lists have a draft ADR or a comment explaining what they need. Act on them, then start the goal again; `status.py` picks up where it left off, because all state lives in GitHub and the repo, not in the session.

| Phase | What to expect |
| --- | --- |
| 0 | Mostly yours: the agent builds the repo foundation (#1, #2, #3, #24 tooling, #7, the porting process #9) and drafts every decision, then waits for you on the name, preview host, MVP scope and the signing secrets. The license is decided; the agent closes the gate. |
| 1 | Long and mostly autonomous: specs and ports of id's formats and VM, the oracles in `oracles/` (#35), the differential runner, then readiness reviews. |
| 2 | Autonomous except the UI work, which follows the framework chosen in #10 and the approved mockups. The grammar (#71) lives in `quakecstudio/tree-sitter-quakec`. |
| 3 | The preview host is our Rust app in `apps/preview-host` (ADR-0004), checked against id's engine built in `oracles/`. Engine integration tests need the redistributable test assets chosen in #17. |
| 4 | Autonomous through signing (#23: the secrets are set; agents build the signing workflow) and release, then the license review, the demo and the MVP gate. |
| 5-7 | The map editor, the same pattern. |

## Watching a run

- `/goal` with no arguments shows the condition, turns, spend and the evaluator's latest reason.
- `python3 plan/status.py --phase P1` from any checkout shows where the phase stands.
- The [project board](https://github.com/orgs/quakecstudio/projects/1) moves as issues close; the Decisions view lists the decisions waiting on you.
