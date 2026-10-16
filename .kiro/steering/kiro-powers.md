---
inclusion: always
---

# Kiro powers for QuakeC Studio

This repository is driven by agents from two toolchains: Claude Code (from `.claude/`)
and Kiro CLI (from `.kiro/`). The `.claude/` tree is the single source of truth. The
`.kiro/` tree is a thin layer of pointers so Kiro CLI has the same powers without a second
copy of the content to keep in sync.

## The one rule

**Edit `.claude/`, never the Kiro pointer.** Rules, skill procedures, agent prompts, the
session hook and the scripts live once in `.claude/`. A `.kiro/` file only names its
authoritative `.claude/` counterpart and summarises it. If a power changes, change the
`.claude/` file; the Kiro pointer keeps working because it points rather than copies.

The exception is `.kiro/steering/guardrails.md`, which inlines the hard constraints on
purpose so Kiro always has them in context. When a guardrail's underlying rule changes in
`.claude/rules/`, update that one steering file too.

## What maps to what

### Steering (Kiro-only; loaded into every session)

| Kiro file | Role | Authoritative source |
| --- | --- | --- |
| `.kiro/steering/kiro-powers.md` | This map | (none; Kiro-only) |
| `.kiro/steering/guardrails.md` | Inlined hard constraints | `CLAUDE.md` and `.claude/rules/` |
| `.kiro/steering/rules.md` | Pointers to the seven rules | `.claude/rules/*.md` |

### Rules

Each is summarised in `.kiro/steering/rules.md` and inlined where it is a hard guardrail in
`.kiro/steering/guardrails.md`. The authoritative text is in `.claude/rules/`.

| Rule | Authoritative source |
| --- | --- |
| Branching | `.claude/rules/branching.md` |
| Commits and issues | `.claude/rules/commits-and-issues.md` |
| Development discipline | `.claude/rules/development-discipline.md` |
| Layering | `.claude/rules/layering.md` |
| Porting from id's source | `.claude/rules/porting.md` |
| Specs are the source of truth | `.claude/rules/specs-are-source-of-truth.md` |
| Testing | `.claude/rules/testing.md` |

### Skills

| Kiro skill | Authoritative source |
| --- | --- |
| `.kiro/skills/work-issue/SKILL.md` | `.claude/skills/work-issue/SKILL.md` |
| `.kiro/skills/run-phase/SKILL.md` | `.claude/skills/run-phase/SKILL.md` |
| `.kiro/skills/gate-review/SKILL.md` | `.claude/skills/gate-review/SKILL.md` |
| `.kiro/skills/readiness-review/SKILL.md` | `.claude/skills/readiness-review/SKILL.md` |
| `.kiro/skills/prepare-decision/SKILL.md` | `.claude/skills/prepare-decision/SKILL.md` |
| `.kiro/skills/plan-change/SKILL.md` | `.claude/skills/plan-change/SKILL.md` |

### Agents

| Kiro agent | Authoritative source |
| --- | --- |
| `.kiro/agents/implementer.json` | `.claude/agents/implementer.md` |
| `.kiro/agents/reviewer.json` | `.claude/agents/reviewer.md` |

### Hook

| Kiro wiring | Authoritative source |
| --- | --- |
| `agentSpawn` hook in each `.kiro/agents/*.json` | `.claude/hooks/session-setup.sh` |

The script is referenced in place, not copied. In Claude Code it runs from
`.claude/settings.json` `SessionStart`; in Kiro it runs from the `agentSpawn` hook of each
agent. It is Linux-only, idempotent and always exits 0, so running it under either tool is
safe.

### Scripts

Referenced in place from the Kiro skills, never copied:

- `.claude/skills/work-issue/scripts/start.sh` - mark an issue In Progress on the board.
- `.claude/skills/work-issue/scripts/land.sh` - wait for CI, add the PR to the merge queue,
  wait for the merge.

## Claude to Kiro translation notes

The `.claude/` skills and agents use a few Claude Code conventions. When following a
`.claude/` procedure from Kiro, read them this way:

- **`$ARGUMENTS` / `#$ARGUMENTS`** - the argument you were invoked with (for a skill, the
  issue number or phase). Kiro does not substitute this token; take the value from the
  invocation.
- **`${CLAUDE_SKILL_DIR}`** - the skill's own directory. The scripts it names live under
  `.claude/skills/work-issue/scripts/`; call them at that path.
- **`$CLAUDE_PROJECT_DIR`** - the repository root (the current workspace root).
- **`allowed-tools:` in a skill's front matter** - the tools that skill expects to use
  (mostly `gh`, `python3 plan/*` and the two scripts). Kiro governs tool access through the
  agent's `tools`/`allowedTools` and the permission prompt; treat the list as the intended
  scope.
- **The Claude `Agent` tool / subagents** - delegation to the `implementer` and `reviewer`
  subagents. In Kiro, delegate by invoking the `implementer` and `reviewer` agents defined
  in `.kiro/agents/`.
- **Running the `plan/` scripts** - they need PyYAML. The session hook
  (`.claude/hooks/session-setup.sh`) installs `uv`, so when a plain `python3 plan/check.py`
  cannot import yaml (a sandbox with a read-only `HOME`, where the hook could not install
  PyYAML into `$HOME`), run the same script through uv instead, which supplies the
  dependency per run: `uv run --no-project --with pyyaml python3 plan/check.py` (and the same
  for `status.py`, `apply.py`, `gen_readiness.py` and the `plan` unittests). Where a normal
  `python3` already imports yaml, keep using it; the commands in the `.claude/` procedures,
  `AGENTS.md` and `CLAUDE.md` stay as written.
- **The `/goal ...` run block** - the `README.md` "Next run" block and the examples in
  `docs/agents/README.md` are Claude Code auto-mode commands (`/goal Run /run-phase ...`).
  Kiro has the same unattended, goal-driven loop as its Autonomous mode, so run the identical
  instruction as the session goal there: `Run run-phase <phase> <track> until python3
  plan/status.py ... prints a last line starting "STATUS ... COMPLETE" or "... WAITING ON
  PEOPLE". Run that command at the end of every turn. Stop after 150 turns.` The verbatim
  text of the instruction and its stop condition are the same; only the invocation differs
  (a session goal in Autonomous mode, not a `/goal` slash command). Kiro's mode is chosen when
  the session starts and does not change within it, so start a new Autonomous session to run a
  phase rather than switching mode mid-session.

See `docs/agents/README.md` for how the pieces fit together and how a whole phase is run.
