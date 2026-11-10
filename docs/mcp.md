# MCP: QuakeC Studio's tools for other agents

`quakec-ide mcp` serves the agent's tools over the Model Context Protocol (P4-13, AGT-7,
PRIN-6). The tools are the IDE's own commands, the ones the palette, the command line and the
built-in agent run, so an outside agent can do what the built-in one does, and no more.

```text
quakec-ide mcp [<project>] [--dialect <vanilla|fteqcc>] [--read <dir>]... [--write <dir>]...
               [--read-only] [--game <folder>]
```

The server opens the project (the current directory when none is named) and then reads one
JSON-RPC message per line from stdin, answering each on stdout. It writes nothing else to
stdout. It runs headless: no window opens, and a game it plays runs in the preview host, as
Play starts it.

## Setup

### Claude Code

```bash
claude mcp add quakec -- quakec-ide mcp /path/to/your/mod
```

Add `--scope project` to share the server with everyone who opens the repository: Claude Code
then writes it to the repository's `.mcp.json`. Add `--dialect fteqcc` after the path for an
FTEQCC project with no `quakec.toml`, and `--game <folder>` when the game is not where
`quakec-ide` finds it.

### Other clients

Any MCP client that launches a stdio server takes the same command and arguments. In a
client configured by JSON:

```json
{
  "mcpServers": {
    "quakec": {
      "command": "quakec-ide",
      "args": ["mcp", "/path/to/your/mod"]
    }
  }
}
```

## Protocol

The protocol is [`rmcp`](https://crates.io/crates/rmcp)'s, the Rust SDK of the protocol's own
project (`modelcontextprotocol/rust-sdk`), so the server follows the specification as that SDK
tracks it. It answers both eras of MCP:

- **Modern clients** (MCP 2026-07-28) name the version and their capabilities in each request's
  `_meta`. `server/discover` lists the versions served, `2026-07-28` among them, and each
  result carries `"resultType": "complete"`.
- **Legacy clients** open with `initialize` (MCP 2025-11-25, 2025-06-18, 2025-03-26 or
  2024-11-05) and get the version they asked for back; rmcp answers one asking for another
  version with one it serves.

Either way the server keeps no state of the conversation. The project and the roots are set
when it starts.

## Tools

| Tool | What it does |
| --- | --- |
| `project.symbols`, `project.check` | The project's definitions; its errors and warnings |
| `symbol.definition`, `symbol.references`, `symbol.hover` | Where a name is defined and used, and what it is |
| `engine.builtins` | An engine's builtins for a module kind |
| `compile` | Builds the project: whether it is clean, its counts and each module's problems |
| `project.test` | Runs the project's `test_` functions and `tests/*.scenario` files ([`spec/qc-tests.md`](spec/qc-tests.md)) |
| `vm.run` | Calls one QuakeC function in the VM over a mock engine |
| `scenario.run` | Plays a scenario file in the server simulation ([`spec/scenario.md`](spec/scenario.md)) |
| `edit.apply`, `edit.accept`, `edit.reject`, `edit.staged` | Stages a change as hunks, decided one by one; nothing is written to disk until accepted and saved |
| `file.read`, `file.save` | A file's current text, unsaved edits included; writes a file's text to disk |
| `checkpoint.create`, `checkpoint.list`, `checkpoint.restore` | Checkpoints of the project, restored deletions and all |
| `preview.play`, `preview.reload`, `preview.stop`, `preview.poll` | The live game: start it, reload the program, stop it, read its events |
| `engine.inspect`, `engine.console`, `engine.screenshot` | The running game's edicts, globals and stack; a console command and its output; a frame as a PNG |

`tools/list` gives each tool's input schema, the command's parameter schema. A tool whose
command the server's registry lacks is not listed. A command that fails answers
`"isError": true` with its message, which the model can act on. An unknown tool is a protocol
error.

## Roots

A tool may only touch files under the roots the server was started with:

- **Reading:** the project, and each `--read` directory.
- **Writing:** the project, unless `--read-only`, and each `--write` directory.

Every path a tool's arguments name (a `path` field, or one ending in `_path`) must lie under a
root the tool may use, after `.` and `..` are worked out. A tool that writes (a build, an edit,
a checkpoint, the game) also needs the project under a write root. A call outside the roots is
refused as a tool error before the command runs.

## Tests

`crates/qs-mcp/src/tests.rs` drives the server with JSON lines, as a client writes them:

- the legacy handshake and calls;
- modern requests and `server/discover`;
- protocol and tool errors;
- the roots, and a read-only project;
- a session that does not start.

`apps/cli/src/tests.rs` runs `quakec-ide mcp` on a project end to end.
