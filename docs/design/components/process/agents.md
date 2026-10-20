# Agent harness: rules, skills, subagents and hooks

**Crates:** `.claude/`, `.kiro/`. **Specs:** none of its own; CONV-13, PM-4.

## Context and design philosophy

QuakeC Studio is built by agents working the plan's issues one phase at a time, with Justin
deciding what only a person can decide (`docs/agents/README.md`). This component is the
instruction set and tooling that makes that safe: the rules every agent reads
(`.claude/rules/`), the procedures (`.claude/skills/`), the two subagent roles
(`.claude/agents/`), the session hook (`.claude/hooks/session-setup.sh`), the two GitHub
scripts (`start.sh`, `land.sh`), and the Kiro pointers in `.kiro/` that let Kiro run the same
roles. CONV-13 in `docs/conventions.md` and `plan/requirements.yaml` is the convention it
implements; PM-4 asks for the agent instructions file.

Principles visible in the files:

- **One source, many pointers.** `.claude/` holds each rule, skill and role once; `.kiro/`
  files only name their `.claude/` counterpart and summarise it, except
  `.kiro/steering/guardrails.md`, which inlines the hard constraints on purpose
  (`.kiro/steering/kiro-powers.md`, "The one rule").
- **Separation of roles.** The `implementer` builds; the `reviewer` reviews code it did not
  write and never fixes it (TEST-9, `docs/testing.md`). `plan/status.py` routes issues to them
  by track, so a readiness review is never done by the author (`docs/agents/README.md`).
- **People decide.** `needs:human` issues are prepared, never closed, by agents; an agent that
  cannot finish without a person labels the issue `escalated`, comments and stops (CLAUDE.md,
  "Working autonomously"; ADR-0001 for decisions).
- **Evidence over claims.** Rules require quoting specs, not recalling them
  (`specs-are-source-of-truth.md`), and completion claims backed by command output
  (`development-discipline.md`, rule 8).

## Components

### Rules (.claude/rules/)

Seven files: `branching.md`, `commits-and-issues.md`, `development-discipline.md`,
`layering.md`, `porting.md`, `specs-are-source-of-truth.md`, `testing.md`. CLAUDE.md
summarises them; `AGENTS.md` covers environment setup and points at both.

### Roles (.claude/agents/)

| Role | Skills | Constraint |
| --- | --- | --- |
| `implementer` | work-issue, plan-change, prepare-decision | `disallowedTools: Agent`; one issue only; self-review with `code-review` at high effort recorded in the PR body (`.claude/agents/implementer.md:16`) |
| `reviewer` | readiness-review, gate-review, plan-change | `disallowedTools: Agent`; judges from evidence; never fixes what it reviews (`.claude/agents/reviewer.md`) |

### Skills (.claude/skills/)

| Skill | Flow |
| --- | --- |
| `work-issue` | read the issue, require closed dependencies, stop on `needs:human`/`escalated`, branch `<type>/<n>-<slug>`, `start.sh`, build to the deliverable, `make ci`, PR with `Closes #n`, `code-review` section, `land.sh` |
| `run-phase` | each loop: check failed scheduled runs on main, `status.py` verdict, delegate `[build]` to implementer and `[review]` to reviewer, at most three in parallel, confirm merge and code-review section, `wt remove -y` finished worktrees |
| `prepare-decision` | draft the ADR as Proposed from the template, index it, `cargo xtask adr`, open a **draft** PR, comment with options and recommendation, mention @jplock, stop |
| `readiness-review` | collect TEST-1 to TEST-12 evidence from CI, try to break the layer (at least five new tests), file each defect as a blocking issue, write `docs/readiness/<phase>-<layer>.md`; `Closes` only on a pass |
| `gate-review` | straggler checks (status, open milestone issues, open PRs, not-planned closures), `docs/gates/<phase>.md` with evidence, risk register review, `status.py --readme --phase <next>`, land; escalate rather than defer |
| `plan-change` | edit `plan/`, run the plan checks, sync with `apply.py` after merge |

### Scripts and hooks

- `.claude/skills/work-issue/scripts/start.sh <issue> [project]` sets the issue's board Status
  to In Progress, adding it to the project if missing; it reads the first 20 project items
  (`start.sh:41`).
- `.claude/skills/work-issue/scripts/land.sh <pr> [timeout-minutes]` waits for checks, runs
  `gh pr merge --auto` (falling back to `gh pr merge`, `land.sh:83`), then watches merge-queue
  runs until merged, conflicting or timed out; gh watch commands are bounded with perl
  `alarm` (`land.sh:27-33`); two consecutive "not in queue" polls count as removal
  (`land.sh:106-116`).
- `.claude/hooks/session-setup.sh` (SessionStart, `.claude/settings.json`) prepares a Linux
  container: apt packages, rustup, prek, uv, git hooks. It does nothing outside Linux and
  always exits 0 (`session-setup.sh:1-14`).
- `.claude/settings.json` also sets `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`,
  `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` and enables four plugins from two marketplaces, one of
  them third-party (`bug-ops/claude-plugins`).

### Kiro (.kiro/)

Agent JSONs for `implementer` and `reviewer` that point at the `.claude/agents/` files and run
`session-setup.sh` on `agentSpawn`; six skill pointers; a `SessionStart` hook running the same
setup script; a `PostFileSave` guard that warns when a pointer is edited; steering files
`guardrails.md` (inlined constraints), `kiro-powers.md` (the map) and `rules.md` (summaries of
the seven rules).

## Error model

The harness has no error type. A blocked agent labels the issue `escalated`, comments with what
it needs and stops; `land.sh` exits non-zero when checks fail, the PR conflicts or the merge
queue times out; `session-setup.sh` always exits 0 and does nothing outside Linux.

## Verification

No automated tests cover the rules, skills or scripts. They are checked in use: every PR's
self-review section, the readiness and gate reports, and `cargo xtask ci` on the files they
produce. The light CI path still runs `cargo xtask ci` for changes under `.claude/` and
`.kiro/`.

## Decisions and alternatives

| Decision | Chosen | Alternatives considered | Rationale |
|----------|--------|------------------------|-----------|
| Agent roles | Two subagents (implementer, reviewer) plus an orchestrator skill | One general agent | TEST-9 needs a reviewer independent of the author (`docs/agents/README.md`) |
| Who merges | Agents land their own PRs through the merge queue once CI passes; never `needs:human` | Human merges all | CLAUDE.md "Git"; `.claude/rules/branching.md` |
| Self-review | `code-review` at high effort recorded in the PR body | None, or independent review per PR | `docs/agents/README.md` "Merge policy": a self-review that does not replace readiness review |
| Waiting on CI | `gh pr checks --watch`, `gh run watch`, `land.sh` | `sleep` polling | `development-discipline.md` rule 12 |
| Kiro support | Pointers to `.claude/`, guardrails inlined | Duplicated content | `.kiro/steering/kiro-powers.md`, "The one rule" |
| Parallelism | At most three issues, never two implementers on one crate, separate worktrees | Unbounded | `run-phase/SKILL.md` step 4; `development-discipline.md` rule 14 |
| Worktree cleanup | Remove finished worktrees each loop | Leave them | `run-phase/SKILL.md` step 5 records about 60 worktrees using 64 GB |

## Open questions

### Resolved

None recorded.

### Deferred

1. **Kiro SessionStart contradiction.** `.kiro/steering/guardrails.md:72-74` says Kiro does
   not fire this repository's SessionStart hook and tells the agent to run the script by
   hand; `.kiro/hooks/session-setup.json` and `.kiro/steering/kiro-powers.md:74-79` say it runs.
2. **Kiro pointers lag their sources.** `.kiro/agents/implementer.json:37` omits the
   code-review self-review and the `--interval` values of `.claude/agents/implementer.md:16-17`;
   `.kiro/agents/reviewer.json:38` omits the intervals; `.kiro/skills/work-issue/SKILL.md:28-29`
   and `.kiro/skills/run-phase/SKILL.md` omit the code-review and scheduled-run steps.
3. **Stale conditionals.** `.claude/agents/implementer.md:15` "follow docs/porting.md (from
   #9)"; `.claude/skills/work-issue/SKILL.md` "Follow docs/porting.md once #9 lands" and "Once
   #2 lands that is make ci"; `AGENTS.md:26-27` "once .pre-commit-config.yaml exists";
   `session-setup.sh:63` "(#2)"; `docs/agents/README.md:74` "the framework chosen in #10".
4. **Plan-test commands.** `.claude/skills/plan-change/SKILL.md:16` and
   `.claude/rules/branching.md:38` run only `test_check.py test_status.py`; CI discovers
   `test_apply.py` too.
5. **Label tables.** `.claude/rules/commits-and-issues.md` omits `type:bug`, which
   `plan/backlog/_meta.yaml:57` and `.github/ISSUE_TEMPLATE/bug.yml:4` use.
6. **Layer table.** `.claude/rules/layering.md:12-15` omits crates `layers.toml` maps
   (qc-diag, qs-install, qs-bsp, qs-map, qs-mdl, qs-wav in L0; qc-engines, qc-sema in L3).
7. **work-issue versus plan-change on PR scope.** `work-issue/SKILL.md` step 2.5 puts plan
   changes in the issue's own PR, "never in a PR of their own"; CLAUDE.md "How work is
   organised" says to change the plan by opening a PR. Both may hold (a standalone plan change
   versus one found during an issue) [inferred]; worth stating.
8. **Supply chain of the harness.** `session-setup.sh:40-41, 59-60` pipe remote installers
   into `sh`; `.claude/settings.json:25-32` trusts a third-party plugin marketplace.
9. **land.sh** needs perl and silently falls back from `--auto` to a direct merge
   (`land.sh:83`); `start.sh:41` reads only 20 project items.
10. `docs/agents/README.md` describes only Claude Code, though P1 readiness reviews ran in Kiro
    (`docs/readiness/p1-r0.md:4`).
11. CONV-13 (`plan/requirements.yaml:251`) lists six rule files; `testing.md` is the seventh.

## References

- `.claude/rules/*.md`, `.claude/skills/*/SKILL.md`, `.claude/skills/work-issue/scripts/{start,land}.sh`
- `.claude/agents/{implementer,reviewer}.md`, `.claude/hooks/session-setup.sh`, `.claude/settings.json`
- `.kiro/agents/*.json`, `.kiro/hooks/*.json`, `.kiro/skills/*/SKILL.md`, `.kiro/steering/*.md`
- `CLAUDE.md`, `AGENTS.md`, `docs/agents/README.md`
- Depends on: `plan/status.py`, `plan/apply.py` (plan leaf); `cargo xtask ci`, `cargo xtask adr` (repo-checks-ci leaf)
- ADR-0001, `docs/testing.md` (TEST-9), `docs/conventions.md` (CONV-10, CONV-13)
