# qs-agent

The agent loop (P4-06, AGT-1, AGT-5, PRIN-4): a model plans, edits, compiles, tests and
previews through the IDE's commands, and proposes its edits as staged hunks for a person to
accept. The model is any `Model`: one that answers a transcript with tool calls or "finished".
This crate is the runtime around it; the model providers come later (P4-07).

A run (`run`):

1. records a checkpoint of the project (`checkpoint.create`), so it can be undone (AGT-4);
2. gives the model the task and the tools of `AGENT_TOOLS` the registry has, and runs each
   call through the command session, answering the result or the error;
3. when the model says it is finished, checks the work itself: `compile` must be clean and
   `project.test` must pass, unit tests and scenarios alike (AGT-5). If not, the model is told
   why, one reason a line, and the run goes on;
4. ends `Done`, or `NotDone` with the reasons, within `Limits::turns`, leaving its edits staged
   (`edit.staged` in the report).

The model's tools leave out what is the person's to do: accepting or rejecting hunks, saving
files and restoring a checkpoint (PRIN-4).

The model is Claude Code's and Zed's agents, whose loop calls tools until the model stops. The
difference is the done criteria: here the runtime checks them and refuses a finish that does not
compile clean and pass its tests.

- **Layer:** L6 agent. It depends on `qs-commands` (L5) and `serde`.
- **Spec sections:** none; it implements plan requirements AGT-1, AGT-5 and PRIN-4.
- **Shipped:** yes.

## Tests

`src/tests.rs` runs the loop with a scripted fake model over the workbench's commands on a
project on disk:

- a failing scenario never lets a run be marked done (the acceptance criterion);
- a run told it is not done stages a fix, which the compile and tests read, and is then done,
  with its hunk proposed and the disk left alone;
- calls outside its tools, and failing calls, are error results;
- a model that never finishes runs out of turns, and a model's error ends the run.
