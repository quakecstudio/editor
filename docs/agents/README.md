# Building with agents

QuakeC Studio is built by Claude Code agents working through the plan's issues, one phase at a time, with Justin deciding what only a person can decide. This page explains the moving parts and how to start a run.

## What is autonomous and what is not

Agents take an issue from open to merged on their own: they read it, build it on a branch, open a PR, wait for CI, add the PR to the merge queue and confirm the merge. They also run the independent readiness reviews, and they prepare everything a person has to act on.

Issues labelled `needs:human` wait for Justin. They are:

- every decision (`type:decision`): the agent drafts the ADR with options and a recommendation as a draft PR; Justin decides and merges it;
- every phase gate (`type:gate`): the agent writes the evidence report; Justin signs the gate off;
- work that needs hardware, money or a lawyer: checking FTEQW on Apple silicon (#13), code-signing credentials (#23), the MVP demo on three OSes (#135), the legal review of the GPL boundary.

Creating new GitHub repositories (`quakecstudio/oracles`, `quakecstudio/preview-host`, `quakecstudio/tree-sitter-quakec`) is also Justin's; an agent that needs one stops and says so.

## The pieces

| Piece | What it does |
| --- | --- |
| [`plan/status.py`](../../plan/status.py) | Reads the plan and GitHub and prints a phase's progress: closed, ready to work, waiting and on what. Its last line is the verdict a `/goal` condition tests: `COMPLETE`, `IN PROGRESS, next #..`, `WAITING ON PEOPLE #..` or `BLOCKED #..` |
| `/run-phase <phase>` | The orchestrator skill: delegates every ready issue to the right subagent and loops until the phase is complete or waiting on people |
| `/work-issue <n>` | Takes one issue from open to merged |
| `/prepare-decision <n>` | Drafts an ADR for a decision issue and asks Justin |
| `/readiness-review <n>` | Independent review of a layer's test suite against [`docs/testing.md`](../testing.md) |
| `/gate-review <n>` | Assembles the evidence for a phase gate |
| `/plan-change` | Changes `plan/` safely and syncs GitHub |
| `clean-room` skill | The clean-room rules, preloaded into the subagents |
| `spec-author` subagent | Clean-room specs and GPL tools; the only role that may read id's GPL source |
| `implementer` subagent | All other build work and decision drafts; a guard hook blocks it from GPL source |
| `reviewer` subagent | Readiness reviews and gate reports; never fixes what it reviews |
| [`.claude/hooks/clean_room_guard.py`](../../.claude/hooks/clean_room_guard.py) | Blocks implementer and reviewer tool calls that would reach GPL source |
| `.claude/skills/work-issue/scripts/land.sh` | Waits for CI, adds the PR to the merge queue, waits for the merge |

`status.py` sorts issues into tracks by who may work them: `spec` (clean-room specs and GPL tools), `build`, `review` (readiness), `human` (`needs:human`), and `agents` (everything except `human`). The orchestrator works the `agents` track and routes each issue by its track, so the spec author and the implementer are always separate agents with separate context.

## Merge policy

An agent may add its own PR to the merge queue once CI passes on it (`land.sh` does this). It never merges a `needs:human` issue's PR, never skips or loosens a test, lint or ratchet to get green, and never pushes to `main` directly.

## Before the first run

- Claude Code signed in with your claude.ai account, run from a clone of this repo.
- `gh auth login` with access to the `quakecstudio` org (issues, PRs, the merge queue).
- Python 3 with PyYAML (`pip install pyyaml`) for `plan/`.
- Auto mode on, so tool calls inside each turn do not wait for approval.

## Running a phase

```
/goal Run /run-phase P0 agents until python3 plan/status.py --phase P0 --track agents prints a last line starting "STATUS P0 agents: COMPLETE" or "STATUS P0 agents: WAITING ON PEOPLE". Run that command at the end of every turn so its last line is in the transcript. Every merged PR passed CI. Never read GPL source in this session. Stop after 150 turns.
```

Change `P0` to the phase. When the run stops on `WAITING ON PEOPLE`, the issues it lists have a draft ADR, a gate report or a comment explaining what they need. Act on them, then start the goal again; `status.py` picks up where it left off, because all state lives in GitHub and the repo, not in the session.

| Phase | What to expect |
| --- | --- |
| 0 | Mostly yours: the agent builds the repo foundation (#1, #2, #3, #24 tooling, #7) and drafts every decision, then waits for you on the licence, name, preview host, MVP scope, signing and the gate. |
| 1 | Long and mostly autonomous. Needs `quakecstudio/oracles` to exist before the oracle work (#35). Specs land first, then the clean-room implementations, then readiness reviews. |
| 2 | Autonomous except the UI work, which follows the framework chosen in #10 and the approved mockups, and the gate. Needs `quakecstudio/tree-sitter-quakec` for the grammar (#71). |
| 3 | Needs `quakecstudio/preview-host` for the fork (#90). Engine integration tests need the redistributable test assets chosen in #17. |
| 4 | Autonomous up to signing and release, which need the credentials from #23, then the legal review, the demo and the gate. |
| 5-7 | The map editor, the same pattern. |

## Watching a run

- `/goal` with no arguments shows the condition, turns, spend and the evaluator's latest reason.
- `python3 plan/status.py --phase P1` from any checkout shows where the phase stands.
- The [project board](https://github.com/orgs/quakecstudio/projects/1) moves as issues close; the Decisions view lists the decisions waiting on you.
