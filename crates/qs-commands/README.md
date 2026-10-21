# qs-commands

The command engine (PRIN-3, PRIN-7): one registry of every command QuakeC Studio has, each
with a stable id, typed parameters whose JSON Schema the registry exports, and an undo step
for what it changes; and a session that runs commands by id behind a guard, so a bug fails
one command and never the session.

- **Layer:** L5 commands. It depends on `schemars` (`derive` and `std`), `serde`,
  `serde_json` and `tracing`, and on no crate of ours.
- **Decision:** plan requirements PRIN-3 and PRIN-7, P2-12; not engine behaviour.
- **Spec sections:** none; it implements plan requirements, not engine behaviour. The MCP
  rules it follows are quoted below from `specs/mcp/2026-07-28/`.
- **Shipped:** yes.

## Prior art

The model is **Zed's actions** with **VS Code's string-id command registry**, adapted so the
same registry also serves MCP.

- Zed (`crates/gpui/src/action.rs`, zed-industries/zed@544780422affd11699fb531c49af5cc6e82a3fd7):
  an action is a Rust type that derives `serde::Deserialize` and `schemars::JsonSchema`, is
  built from JSON with `Action::build(value: serde_json::Value)`, and exports its schema with
  `action_json_schema`; "In Zed these trait impls are used to load keymaps from JSON." Here a
  command's `Params` type does the same, so the schema comes from the type and arguments are
  checked by deserialising them.
- VS Code ([Commands guide](https://code.visualstudio.com/api/extension-guides/command)): a
  command is registered under a string id and "executed, be it programmatically with
  executeCommand, from the VS Code UI, or through a keybinding." Here the id is a
  `CommandId`, and the UI, CLI, agent and MCP all call `Session::execute(id, params)`.
- Neither puts undo on the command: Zed and VS Code keep it in the text buffer. PRIN-3 asks
  for an undo step on every command, so a changing command returns one
  (`Outcome::undoable`), as in [WordCraft](https://github.com/storytold/wordcraft), which the
  design doc names for PRIN-3.

## Writing a command

```rust
use qs_commands::{BoxError, Command, Outcome, UndoStep};

/// The parameters of `doc.append`.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct AppendParams {
    /// The text to add at the end.
    text: String,
}

struct Append;

impl Command<Doc> for Append {
    const ID: &'static str = "doc.append";
    const SUMMARY: &'static str = "Appends text to the document.";
    type Params = AppendParams;
    type Output = usize;
    type Error = DocError;

    fn run(&self, doc: &mut Doc, params: AppendParams) -> Result<Outcome<usize, Doc>, DocError> {
        doc.text.push_str(&params.text);
        Ok(Outcome::undoable(doc.text.len(), Truncate::new(&params.text)))
    }
}
```

- **Ids** are dotted lowercase segments (`edit.undo`, `vm.run`, `compile`), at most 128
  bytes. Every such id is a valid MCP tool name: "The following SHOULD be the only allowed
  characters: uppercase and lowercase ASCII letters (A-Z, a-z), digits (0-9), underscore
  (\_), hyphen (-), and dot (.)" and "Tool names SHOULD be between 1 and 128 characters in
  length (inclusive)" (MCP 2026-07-28, `specs/mcp/2026-07-28/server/tools.mdx`, Tool Names).
- **Parameters** are always a JSON object: the registry refuses a `Params` whose schema is
  not `"type": "object"`, because MCP's `Tool` declares
  `inputSchema: { $schema?: string; type: "object"; [key: string]: unknown }`
  (`specs/mcp/2026-07-28/schema/schema.ts`). `NoParams` is the schema MCP recommends for a
  tool with none, `{ "type": "object", "additionalProperties": false }`
  (`server/tools.mdx`, Tool). schemars emits JSON Schema 2020-12, the dialect MCP assumes
  "if no `$schema` field is present". Doc comments become the schema's descriptions.
- **Outcome.** `Outcome::unchanged` for a command that only reads; `Outcome::undoable` with
  an `UndoStep` for one that changes the context. A command or step that returns `Err` must
  leave the context as it found it.
- **Context.** `Registry<C>` and `Session<C>` are generic over the state `C` commands work
  on, so the app, the CLI and tests each bring their own; a command needing only part of a
  context is generic over a trait, `impl<C: HasDoc> Command<C> for Append`.

## The session and the guard

`Session::execute(id, params)` looks the id up, deserialises the arguments, runs the
command and serialises its output, all inside `std::panic::catch_unwind`. A changing
command goes on the undo history and empties the redo history.

| What happens | Result | History |
| --- | --- | --- |
| Unknown id | `CommandError::Unknown` | unchanged |
| Arguments do not match the schema | `CommandError::InvalidParams` | unchanged |
| The command returns `Err` | `CommandError::Failed` | unchanged |
| The output is not JSON | `CommandError::Output` | the change is recorded and can be undone |
| The command, or an undo or redo step, panics | `CommandError::Panicked` with the message | cleared: the guard cannot know how much of the change was made |
| An undo or redo step returns `Err` | `CommandError::UndoFailed` or `RedoFailed` | the step stays next in line |

The guard needs `panic = "unwind"`, which the workspace sets in every profile, release
included (`docs/conventions.md`, CONV-6); the crate fails to compile under
`panic = "abort"`. Panics stay denied by lint in shipped code: the guard is the last line of
defence (PRIN-7). `cargo xtask ci` runs this crate's tests under `--profile release` as well
as the test profile, so the release build is proven to catch a panic.

A caught panic still runs the process's panic hook first: with `qs-log` installed it writes a
crash record, and the default hook prints the message to standard error. The guard's own log
line names the command and never the message, which can hold user content (QA-7); the message
is in `CommandError::Panicked` for the person to see.

## Built-in commands

Every registry starts with three commands the session runs itself, since they work on the
registry and the history rather than the context:

- `commands.list`: every command with its id, summary and parameter schema;
- `edit.undo` and `edit.redo`: walk the undo history.

## The reference

`Registry::markdown` renders `docs/commands.md`. `cargo xtask commands` writes it from the
registry, and `cargo xtask commands --check`, which `cargo xtask ci` runs, fails when the
checked-in file differs.
