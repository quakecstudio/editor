# The agent

The agent, in the right panel, changes your project to do a task you give it in words, and
does not call the task done until the project compiles clean and its tests pass. It uses the
same commands you do, so you can watch what it does, and nothing it changes is written until
you accept it.

The agent runs on a model provider you choose. QuakeC Studio has no account and no model of
its own, and it sends nothing anywhere until you set a provider up ([Privacy](../privacy.md)).

## Set up a provider

Until a provider is set up, the Agent section says **The agent is not set up yet.** and shows
its form:

- **Anthropic**, with an API key from the Anthropic Console;
- **OpenAI-compatible**, for OpenAI or any service with the same API, with its **Address
  (optional)** and an API key;
- **Ollama (local)**, a model running on your own computer, which needs no key and keeps
  everything on it.

Fill in the **Model** and the key, and press **Save**.

At work, your company may give you **workload identity** instead of a key, which both Anthropic
and OpenAI support. Choose **Workload identity**, then give the **Identity token file** your
identity provider keeps current, or **Or a command that prints it**. Then give the ids your
administrator set up:
- for Anthropic, the **Federation rule**, **Organization**, **Service account** and, if the rule
  names one, **Workspace**;
- for OpenAI, the **Identity provider** and **Service account**.

The token is exchanged for a short-lived API token and renewed before it expires. No key is
kept at all, and usage is counted against that service account. The key is kept in the system keychain,
never in a file; the rest goes to `agent.json` in the app's data directory. **Settings** opens
the form again later.

## Give it a task

Type the task in **What should the agent do?** and press **Start**. For example:

> Make the axe do 200 damage, and add a test that proves it.

Each task is a thread, and several can run at once; the row above the task box switches
between them. Before the agent changes anything, the IDE records a checkpoint of the project.

The agent starts with a brief of your project: its `AGENTS.md` if it has one (write your notes
for the agent there), each module's dialect and engine, the engine's builtins, the files, and
whether the project compiles and its tests pass now.

While it works, the thread shows what the model writes as it writes it, and each command it
calls with whether it ran. When it says it is finished, the IDE compiles and runs the tests
itself:

- **Done**: the compile is clean and every test passes. Its summary follows.
- **Not done**: what still fails. The agent is told and goes on, until it runs out of turns.

## Review its changes

The agent's edits are not written to your files. They are staged, and **Proposed changes**
lists each change, hunk by hunk, with **Accept** and **Reject**, and **Accept all** and
**Reject all**. Once every hunk of a file is decided, the accepted ones are
the editor's text of it, unsaved: save as usual.

**Restore checkpoint** puts the project back as it was before the thread started.

## Point it at something

A task can name what the agent should look at, with an `@` mention. Each one is read when the
thread starts and attached after the task:

| Mention | What is attached |
| --- | --- |
| `@file:<path>` | The file's text, its unsaved edits included; the path is relative to the project |
| `@symbol:<name>` | Where the name is defined, file and line |
| `@builtin:<name>` | The engine's declaration of the builtin |
| `@entity` | The edict selected in Entities, or picked in the paused game, with every field |
| `@error` | The last error the game's console printed |

For example: `Why does @symbol:W_FireAxe miss? See @error.`

## Ask about this

Right-click a row in **Entities** and choose **Ask about this**, or press **Ask about this** in
**Properties**. The IDE selects that edict and starts a thread that explains what it is doing
and why: what spawned it, what its think function does next, and anything in its state that
looks wrong. The thread holds its classname, its current think function and when it is due,
and every field, as the game has them at that moment. The agent is told not to change code
unless you ask.

## Inline edit

Select some code in the editor and press Cmd+K. A box asks **Change the selection to…**: say
what you want and press Enter. The model rewrites only the selection, and the answer is shown
in place as a diff from the text before. **Accept** keeps it, and one undo still takes it back;
**Reject** puts the text back. Inline edit uses the provider the agent panel has.

## The command line and other agents

The same agent tools are served to Claude Code and other agents over MCP: see [MCP](mcp.md).
