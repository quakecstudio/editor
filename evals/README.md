# Agent evals

Fixed modding tasks for QuakeC Studio's agent (P4-15, QA-5). Each one is scored by a
scenario that defines success, then read by a reviewer against a rubric.

| Task | Starter | What the agent is asked |
| --- | --- | --- |
| `spawn-health` | id1 | Spawn with 150 health |
| `fix-crash` | id1 | Fix a run error on spawn (a call through `th_pain` before it is set) |
| `fix-compile` | id1 | Fix a compile error (a missing parenthesis in `ImpulseCommands`) |
| `add-impulse` | id1 | Add impulse 50, which gives 100 nails |
| `port-to-rerelease` | rerelease | Port the 150-health change to the rerelease, keeping its Nightmare rule |

## A task

`evals/<name>/` holds:

- `task.toml`: its `title` and `template` (`id1` or `rerelease`, made by `qs-templates` from
  the pinned QuakeC), the `prompt` the agent is given, its `turns`, how many `runs` and the
  `threshold` (how many runs must pass), and two lists of edits, each a `file`, the text to
  `find` there once and what `replace`s it:
  - `setup`: what makes the task's starting point, such as the bug to fix;
  - `solution`: a reference solution, which proves the scenario can pass.
- `success.scenario`: what success is (`docs/spec/scenario.md`). It is put into the project's
  `tests/` only after the agent's run, and played on the agent's work, unsaved edits included.
- `rubric.md`: what a reviewer reads a passing run against.

## Running them

```bash
cargo xtask evals --check   # each scenario fails at its task's start and passes with its solution
QS_EVAL_API_KEY=... cargo xtask evals --run [--task <name>] [--provider anthropic] [--model <id>]
    [--max-usd 2] [--input-usd-per-mtok 4] [--output-usd-per-mtok 20] [--history <file.jsonl>]
```

Both take the starters' QuakeC and the game from `$QS_TEST_ASSETS` (`cargo xtask corpus fetch
id-1.01 rerelease`, `cargo xtask assets fetch`), or `--id1`, `--rerelease` and `--game`. Both
write `target/evals/report.md`.

- **The cost cap:** `--run` estimates each run's cost from the tokens its provider reports, at
  the prices given (Claude Opus 5.5's list prices by default). It stops the run once the
  estimate reaches `--max-usd`.
- **The history:** `--history` appends one JSON line per run (task, model, passed, turns,
  tokens, cost).

CI runs `--check` in Test Assets (linux) on every full run. The `Evals` workflow runs `--run`
weekly and by hand, only when the `QS_EVAL_API_KEY` secret is set. It keeps the history in the
Actions cache and uploads the report and history.
