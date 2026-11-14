# MCP: QuakeC Studio's tools for other agents

`quakec-ide mcp` serves the agent's tools over the Model Context Protocol (P4-13, AGT-7,
PRIN-6). The tools are the IDE's own commands, the ones the palette, the command line and the
built-in agent run, so an outside agent can do what the built-in one does, and no more.

```text
quakec-ide mcp [<project>] [--dialect <vanilla|fteqcc>] [--read <dir>]... [--write <dir>]...
               [--read-only] [--game <folder> | --connect [<socket>]]
```

The server opens the project (the current directory when none is named) and then reads one
JSON-RPC message per line from stdin, answering each on stdout. It writes nothing else to
stdout. It runs headless: no window opens, and a game it plays runs in the preview host, as
Play starts it.

## Connecting to the running app

With `--connect`, the tools run in the open QuakeC Studio window's session instead of one of
the server's own (AGT-7): an edit the agent stages shows in the window's diff, a build is the
window's build, and the game it plays is the window's game. The app answers on a local socket
of the user's own, `app.sock` in its cache directory (on Windows, a named pipe named after
that directory); `--connect <socket>` names another. The app must be open first, or the server
stops with `no QuakeC Studio answers on <socket>`. Name the project the window has open: the
roots are still the server's. `--connect` and `--game` do not go together, since the app plays
its own game.

```bash
claude mcp add quakec -- quakec-ide mcp /path/to/your/mod --connect
```

The socket's wire is one JSON object per line each way (`qs_commands::remote`): a request is
`"commands"` or `{"execute": {"command": <id>, "params": <object>}}`, and the answer is
`{"commands": [...]}`, `{"output": <value>}` or `{"error": <message>}`. The window runs each
request on its next frame.

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
| `map.open`, `map.inspect`, `map.staged` | Opens a `.map` file; its rooms and entities; the map edits waiting for review |
| `map.place`, `map.set_key` | Places an entity; sets one of an entity's keys |
| `map.brush_create`, `map.brush_shape`, `map.brush_delete`, `map.brush_move`, `map.brush_texture` | Adds a box brush, or a shape of brushes (cylinder, cone, arch, spheroid, rock, curve, stairs); deletes, moves or retextures brushes |
| `map.brush_clip`, `map.brush_vertex`, `map.brush_vertices`, `map.brush_split_edge`, `map.brush_split_face`, `map.brush_chamfer`, `map.brush_extrude`, `map.brush_scale`, `map.brush_shear`, `map.brush_sweep` | Reshapes a brush: clips it by a plane, moves its corners, splits, chamfers, extrudes, scales, shears or sweeps it |
| `map.brush_subtract`, `map.brush_merge`, `map.brush_intersect`, `map.brush_hollow` | CSG: carves a brush out of the others of its entity, merges brushes into their convex hull, keeps their common solid, hollows a brush into walls |
| `map.compile`, `map.leaks` | Compiles the map as it is in the editor to a BSP beside it, with its portal and leak files; reports whether it leaks without writing a file |

A map tool that edits runs as the built-in agent's do (P6-21): the server begins the map's
review with `map.stage` before it and ends it with `map.unstage` after, so every edit waits as a
hunk for a person to accept or reject in the app, and a call that changed nothing leaves no
review open. When review cannot begin the edit is refused, not run unreviewed. The map tools
that read open no review. `map.accept`, `map.reject`, `map.stage` and `map.unstage` are not
offered: the decision is the person's.

`map.open` writes, since it replaces the open map, its unsaved and staged edits with it. A map
tool that edits or compiles also needs the open map's file under a write root, since
`map.compile` writes beside it: the server asks `map.inspect` for it first. With `--connect`,
the stage, the edit and the unstage are three calls to the app, so an edit the person makes in
the app between them joins the review.

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
- a session that does not start;
- a server connected to an app's session, its staged edit there;
- the map tools listed with their schemas and the review's decisions not; a sealed room built
  and compiled through them alone, its edits staged and a read opening no review; a read-only
  server refusing every map tool that writes; and, connected to an app, an edit refused for a
  map outside the write roots.

`crates/qs-commands/src/tests/remote.rs` tests the socket's host and client,
`apps/desktop/src/tests.rs` a command sent to the window over it, and `apps/cli/src/tests.rs`
runs `quakec-ide mcp` on a project end to end, and `--connect` with no app.

### A recorded session

Claude Code, given only this server's tools, on id's QuakeC 1.01 over LibreQuake's `start`
map (`quakec-ide mcp <mod> --game librequake-lite`), was asked to make the player spawn with
150 health, compile, and prove it with a scenario. In 11 turns it read `client.qc`, staged
the change and `tests/health.scenario` with `edit.apply`, accepted and saved them, compiled
(`"clean": true`) and ran the scenario (`expect field player health == 150` passed).
