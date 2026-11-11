# qs-agent

The agent loop (P4-06, AGT-1, AGT-5, PRIN-4): a model plans, edits, compiles, tests and
previews through the IDE's commands, and proposes its edits as staged hunks for a person to
accept. The model is any `Model`: one that answers a transcript with tool calls or "finished".
This crate is the runtime around it; `qs-models` has the model providers (P4-07).

A run (`run`) gives the model what it needs to be productive on its first pass, and holds it
to the result:

- **Every tool:** every command of the registry but `DENIED`. Denied are the person's decisions
  (accepting or rejecting hunks, saving, restoring a checkpoint), writes around the staged
  edits (`file.update`, `symbol.rename`) and the window's own controls (`view.*`, tabs, the
  palette). The model can read and search the codebase (`project.files`, `project.search`,
  `file.read`, the symbol tools), compile, test, run QuakeC in the VM, play scenarios and
  drive the running game.
- **A brief before the first turn:**
  1. the runtime records a checkpoint (AGT-4);
  2. it compiles the project and runs its tests;
  3. it gives the model the task with the project's `AGENTS.md` when it has one, each
     module's kind, dialect and target engine (`project.info`) with that engine's builtins as
     a program declares them (`engine.builtins`), the project's source files, its compile
     errors and its failing tests (P4-08). The brief of a test project is held to
     `src/snapshots/brief.txt`.
- **The system prompt** (`system_prompt`): the rules it works under (`SYSTEM`), then the
  reference written for it, [`docs/agent-reference/`](../../docs/agent-reference/README.md):
  the language and the idioms of id's code, and how to work, test and play scenarios here.
  It is the same on every turn, so a provider's prompt cache keeps it.
- **One turn at a time:** `Run::start` records the checkpoint and writes the brief,
  `Run::request` is what to ask the model next (owned, so it can go to the thread that waits
  on the model), `Run::answer` runs the reply's calls in the session or checks the work, and
  `Run::finish` ends it. The agent panel drives a `Run` a reply at a time so the frame never
  waits on the model; `run` drives one to the end. `Model::reply_streaming` gives the reply's
  text as it is written.
- **Guardrails:**
  - a relative path is taken from the project, and an edit outside it is refused;
  - a tool's output longer than `RunConfig::output_limit` is cut, saying so;
  - the run has at most `RunConfig::turns` turns.
- **Done means done (AGT-5):** when the model says it is finished, the runtime compiles and runs
  the tests itself. The run is done only when the compile is clean and every test, unit and
  scenario, passes; otherwise the model is told why, each error with its place and each
  failing test with its message, and goes on.
- **Proposed, not applied (PRIN-4):** the run ends `Done` or `NotDone` with its edits staged
  for review (`edit.staged` in the report). Nothing is written to disk, and the checkpoint
  undoes everything.
- **Context the modder points at (P4-12, UI-8, UI-9, PRIN-2):** `expand_mentions` attaches what a
  task's `@file:`, `@symbol:`, `@builtin:`, `@entity` and `@error` mentions name, read through
  the session's commands and the running game's `Live` state. `ask_about` is the task of Ask
  about this: an `EntityState` with its classname, its think function and when it is due, and
  every field.

- **Tool permissions and profiles (P4-30, UI-22):** `Permissions` decides each call from the
  profile and the rules in the settings (`agent.profile`, `agent.tool_permissions`, kept in
  `qs-settings`). The Ask profile offers the reading tools (`READ_TOOLS`) only. Write offers
  every tool and runs reads and edits (`EDIT_TOOLS`) unasked, asking before the rest. Full runs
  everything. A rule's patterns are globs over the call's paths in the project, decided in Zed's
  order: `deny`, then `ask`, then `allow`, then the tool's default, then the global default,
  then the profile. A denied call is not run, and the model reads which rule denied it. A call
  that asks stops the run (`Run::asking`) until `Run::permit` answers it: Allow once, Always
  allow (an `allow` pattern for exactly what it names) or Deny. `run`, with no one to ask,
  denies it.

The model is Claude Code's and Zed's agents, whose loop calls tools until the model stops. The
difference is the done criteria: here the runtime checks them and refuses a finish that does not
compile clean and pass its tests.

- **Layer:** L6 agent. It depends on `qs-commands` and `qs-settings` (L5) and `serde`.
- **Spec sections:** none; it implements plan requirements AGT-1, AGT-5, PRIN-2, PRIN-4, UI-8
  and UI-9.
- **Shipped:** yes.

## Tests

`src/tests.rs` runs the loop with a scripted fake model over the workbench's commands on a
project on disk:

- a failing scenario never lets a run be marked done (the acceptance criterion);
- the brief names the files and the failing tests, and the tools leave out the denied
  commands;
- the guardrails: an edit outside the project, a relative path, a cut output, compile errors
  with their place;
- a run told it is not done stages a fix, which the compile and tests read, and is then done,
  with its hunk proposed and the disk left alone;
- calls outside its tools, and failing calls, are error results;
- a model that never finishes runs out of turns, and a model's error ends the run.
