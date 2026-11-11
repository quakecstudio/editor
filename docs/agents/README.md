# Building with agents

QuakeC Studio is built by Claude Code agents working through the plan's issues, one phase at a time, with Justin deciding what only a person can decide. This page explains the moving parts and how to start a run.

## What is autonomous and what is not

An agent session takes an issue from open to merged on its own: it reads it, builds it on a branch, opens a PR, waits for CI and squash-merges it. It works every issue, fix and review itself, one at a time, with no subagents (Justin, 2026-10-29: they cost tokens out of proportion to what they deliver). It also prepares everything a person has to act on. Readiness reviews and phase gates need a reviewer who did not write the code, so the session asks Justin how each one is done instead of reviewing its own work.

Issues labelled `needs:human` wait for Justin. They are:

- every decision (`type:decision`): the agent drafts the ADR with options and a recommendation as a draft PR; Justin decides and merges it;
- work that needs hardware, credentials or a lawyer: the MVP demo on three OSes (#135), the license compliance review before release;
- the MVP gate (#136), because it publishes the release.

Other phase gates are not yours to do, though you say who reviews them: the reviewer closes a gate once every issue in the phase's milestone is closed, including follow-ups filed along the way, nothing was deferred to a later phase, and every gate criterion has evidence in `docs/gates/<phase>.md`. If something would have to be deferred, it labels the gate `escalated` and asks you instead.

Any issue an agent cannot finish without a person gets the `escalated` label and a comment saying what is needed. `status.py` treats it like `needs:human`; remove the label once it is resolved.

Creating new GitHub repositories (such as `quakecstudio/tree-sitter-quakec`) is also Justin's; an agent that needs one stops and says so.

## The pieces

| Piece | What it does |
| --- | --- |
| [`plan/status.py`](../../plan/status.py) | Reads the plan and GitHub and prints a phase's progress: closed, ready to work, waiting and on what. Its last line is the verdict a `/goal` condition tests: `COMPLETE`, `IN PROGRESS, next #..`, `WAITING ON PEOPLE #..` or `BLOCKED #..` |
| `/run-phase <phase>` | The coordinator skill: works every ready issue itself, one at a time, and loops until the phase is complete or waiting on people |
| `/work-issue <n>` | Takes one issue from open to merged |
| `/prepare-decision <n>` | Drafts an ADR for a decision issue and asks Justin |
| `/readiness-review <n>` | Independent review of a layer's test suite against [`docs/testing.md`](../testing.md) |
| `/gate-review <n>` | Checks that nothing in the phase straggles, writes the gate report with its evidence and closes the gate |
| `/plan-change` | Changes `plan/` safely and syncs GitHub |
| `.claude/agents/implementer.md`, `reviewer.md` | Role definitions from when issues were delegated to subagents; not used while the no-subagents rule stands |
| `.claude/skills/work-issue/scripts/start.sh` | Sets the issue's Status on the project board to In Progress when work starts; closing it moves it to Done |
| `.claude/skills/work-issue/scripts/land.sh` | Waits for CI and squash-merges the PR (or, with the merge queue on, queues it and waits for the merge); first refuses a body with a closing keyword in a negated sentence (`check-pr-body.sh`) |

`status.py` sorts issues into tracks by who may work them: `build`, `review` (readiness and gates), `human` (`needs:human` or `escalated`), and `agents` (`build` and `review`). The coordinator works the `agents` track: `build` issues itself, and for `review` issues it asks Justin how the review is done, so a readiness review is never done by the session that wrote the code.

The project is GPL-3.0-or-later ([ADR-0002](../adr/0002-license-model.md)), so agents may read and port id's source directly; ports keep id's copyright line and cite where they came from.

## Merge policy

The merge queue on `main` is off, so an agent squash-merges its own PR once CI passes on it (`land.sh` does this) and then merges `main` into any other open branch. A build PR also needs its own diff reviewed with the `code-review` skill at high effort and the outcome recorded in the PR body (`work-issue`, "Land it"); that is a self-review, not an independent one. Readiness and gate PRs are reviews already and skip it. It never merges a `needs:human` issue's PR, never skips or loosens a test, lint or ratchet to get green, and never pushes to `main` directly.

## Before the first run

- Claude Code signed in with your claude.ai account, run from a clone of this repo.
- `gh auth login` with access to the `quakecstudio` org (issues and PRs).
- Python 3 with PyYAML (`pip install pyyaml`) for `plan/`.
- Auto mode on, so tool calls inside each turn do not wait for approval.

## Running a phase

The [README](../../README.md#status) always shows the current phase and the exact command for the next run. `python3 plan/status.py --readme` writes it, and the gate reviewer updates it to the next phase in the PR that closes each gate. The general form is:

```
/goal Run /run-phase P0 agents until python3 plan/status.py --phase P0 --track agents prints a last line starting "STATUS P0 agents: COMPLETE" or "STATUS P0 agents: WAITING ON PEOPLE". Run that command at the end of every turn so its last line is in the transcript. Every merged PR passed CI. Stop after 150 turns.
```

Change `P0` to the phase. Each pass of the loop starts by checking the scheduled workflows on `main`, such as the nightly Differential run, which no PR runs: it lists the latest scheduled run of each workflow that did not succeed,

```bash
gh run list --branch main --event schedule --limit 40 --json workflowName,conclusion,createdAt,url --jq 'group_by(.workflowName)|map(max_by(.createdAt))|.[]|select(.conclusion!="success")'
```

and anything it lists is fixed, or filed and worked next, before new work starts.

When the run stops on `WAITING ON PEOPLE`, the issues it lists have a draft ADR or a comment explaining what they need. Act on them, then start the goal again; `status.py` picks up where it left off, because all state lives in GitHub and the repo, not in the session.

| Phase | What to expect |
| --- | --- |
| 0 | Mostly yours: the agent builds the repo foundation (#1, #2, #3, #24 tooling, #7, the porting process #9) and drafts every decision, then waits for you on the name, preview host, MVP scope and the signing secrets. The license is decided; the agent closes the gate. |
| 1 | Long and mostly autonomous: specs and ports of id's formats and VM, the oracles in `oracles/` (#35), the differential runner, then readiness reviews. |
| 2 | Autonomous except the UI work, which follows egui ([ADR-0003](../adr/0003-ui-framework.md)) and the approved mockups. The grammar (#71) lives in `quakecstudio/tree-sitter-quakec`. |
| 3 | The preview host is our Rust app in `apps/quakec-engine` (ADR-0004), checked against id's engine built in `oracles/`. Engine integration tests need the redistributable test assets chosen in #17. |
| 4 | Autonomous through signing (#23: the secrets are set; agents build the signing workflow) and release, then the license review, the demo and the MVP gate. |
| 5-7 | The map editor, the same pattern. |

## The knowledge base

A sibling checkout, `../quakecstudio-kb` (the `quakecstudio/kb` repository), holds durable
knowledge distilled from past sessions across the editor, site, infra, tree-sitter-quakec and
.github repositories: decisions and their rationale, rejected alternatives, dead ends, user
corrections, gotchas with their fixes, runbooks, integration quirks, testing and readiness
knowledge. It is an Open Knowledge Format v0.2 bundle: one markdown concept per file with YAML
frontmatter, in `architecture/`, `decisions/`, `conventions/`, `gotchas/`, `runbooks/`,
`integrations/`, `testing/` and `references/`.

**Read-only from here.** `.claude/settings.json` allows reading `../quakecstudio-kb` and denies
Edit and Write there (#577). Nothing in this repository writes to it: the base is refreshed from
session transcripts by its own `/quakecstudio-kb-refresh` skill, run by hand in that checkout,
and every change lands there through a pull request.

Query it before designing, debugging a tool or CI failure, or repeating a decision:

```bash
KB=../quakecstudio-kb
cat $KB/index.md                              # the bundle map: one line per directory
cat $KB/gotchas/index.md                      # a directory's concepts, one line each with its summary
rg -il 'merge queue|land.sh' $KB --glob '*.md' # concepts that mention a term (the frontmatter is searchable too)
rg -l '^- editor' $KB/decisions                # concepts scoped to this repository (the first tag is the scope)
rg -n '^title:|^description:' $KB/testing/*.md # titles and summaries of a directory
```

Open a concept file only once its index line looks relevant; the `sources` list in its
frontmatter names the transcript or artifact to re-check a fact against. A concept with
`status: deprecated` names its replacement in `superseded_by`; one with `stale_after` in the
past is a claim to re-verify, not to repeat. When a session shows a concept to be wrong, say so
in the hand-back or the PR so the next refresh corrects it; do not edit the base from here.

## Cost

What a run spends tokens on, and the rules that keep it down (`.claude/skills/run-phase`, `work-issue`, `readiness-review` and the agent definitions carry them):

- No subagents, forks, teammates or helper agents (2026-10-29): each one carries its own context on every tool call and repeats what the coordinator already knows. The session works one issue at a time itself.
- The session waits on the completion of its own background watches (`gh pr checks --watch`, `gh run watch`, a background local job), never on polling or sleeps.
- Defects are found before a PR, where they cost minutes: cargo-mutants on the diff (`--in-diff`) for L0 to L3 crates, the differential families for parser and model work, targeted local checks while other agents share the machine. A defect found by a readiness review instead costs a measurement and a review round, about an hour each.
- A readiness review measures its own branch once, with its tests on it.

## Watching a run

- `/goal` with no arguments shows the condition, turns, spend and the evaluator's latest reason.
- `python3 plan/status.py --phase P1` from any checkout shows where the phase stands.
- The [project board](https://github.com/orgs/quakecstudio/projects/1) moves as issues close; the Decisions view lists the decisions waiting on you.
