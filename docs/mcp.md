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
stdout. It runs headless: no window opens, and a game it plays runs in the preview host with no
window and no sound either (`--headless`), its frames drawn offscreen for `engine.screenshot`,
so an agent that plays a map to look at it opens nothing on the person's screen. On a machine
with no GPU device the game still runs, and `engine.screenshot` is refused with "the preview
host does not offer screenshot". The player is moved from the console: `engine.console` with `+forward`, `+attack` or
`impulse 9` holds a button or sends an impulse, as a key would in the window. To watch the game
an agent plays, connect the server to the running app.

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

Every command of the app is a tool (PRIN-3): the server lists the session's registry, so a new
command is a tool with no list to add it to. [`commands.md`](commands.md) is the whole list,
generated from the registry, and `commands.list` answers it. Each tool tells the client
whether it only reads (MCP 2026-07-28, `ToolAnnotations.readOnlyHint`: "If true, the tool does
not modify its environment."), as its command declares (`Command::ACCESS`; a command that does
not say may write). A client can so run the reading tools without asking. Nothing is said of
`openWorldHint`, whose default the client then takes ("Default: true"): the terminal and the
tasks run whatever they are given.

The tools an agent's loop most often uses:

| Tool | What it does |
| --- | --- |
| `project.symbols`, `project.check` | The project's definitions; its errors and warnings |
| `symbol.definition`, `symbol.references`, `symbol.hover` | Where a name is defined and used, and what it is |
| `engine.builtins` | An engine's builtins for a module kind |
| `project.compile` | Builds the project: whether it is clean, its counts and each module's problems |
| `project.test` | Runs the project's `test_` functions and `tests/*.scenario` files ([`spec/qc-tests.md`](spec/qc-tests.md)) |
| `vm.run` | Calls one QuakeC function in the VM over a mock engine |
| `scenario.run` | Plays a scenario file in the server simulation ([`spec/scenario.md`](spec/scenario.md)) |
| `edit.stage_change`, `edit.accept`, `edit.reject`, `edit.staged` | Stages a change as hunks, decided one by one; nothing is written to disk until accepted and saved |
| `file.read`, `file.save` | A file's current text, unsaved edits included; writes a file's text to disk |
| `checkpoint.create`, `checkpoint.list`, `checkpoint.restore` | Checkpoints of the project, restored deletions and all |
| `preview.play`, `preview.reload`, `preview.stop`, `preview.poll` | The live game: start it, reload the program, stop it, read its events |
| `engine.inspect`, `engine.console`, `engine.screenshot` | The running game's edicts, globals and stack; a console command and its output; a frame as a PNG |
| `map.open`, `map.new`, `map.list_entities`, `map.staged_hunks` | Opens a `.map` file; makes a new one in the project, holding a world with the keys given, and opens it; its rooms and entities; the map edits waiting for review |
| `map.build_room`, `map.build_passage` | Builds one room of a shape of id's levels into the open map (a pillared hall, an eight-sided domed chamber, a hall around a pit, a room on two levels, a courtyard, a cross, a vault), sealed but for the doorways asked for, with its lights, and answers its doorways and where things may stand; builds a passage between two doorways, with stairs where their floors differ and a door when asked (L-49) |
| `map.generate` | Makes a random playable level in the project and opens it (L-48): rooms of seven shapes joined by passages, stairs and doors, from a start to an exit, with lights, monsters, weapons and a key, textured from the game's own levels; one seed and size give one level, and its answer names each room's shape and a place to stand in it |
| `map.brushes`, `map.summary` | An entity's brushes, each with its place, the box around it and its textures; the map in short: its bounds, brush and entity counts, entities by classname and textures by face |
| `map.place_entity`, `map.set_key` | Places an entity; sets one of an entity's keys |
| `map.brush_create`, `map.brush_shape`, `map.brush_delete`, `map.brush_move`, `map.brush_texture` | Adds a box brush, or a shape of brushes (cylinder, cone, arch, spheroid, rock, curve, stairs); deletes, moves or retextures brushes |
| `map.brush_clip`, `map.brush_vertex`, `map.brush_vertices`, `map.brush_split_edge`, `map.brush_split_face`, `map.brush_chamfer`, `map.brush_extrude`, `map.brush_scale`, `map.brush_shear`, `map.brush_sweep` | Reshapes a brush: clips it by a plane, moves its corners, splits, chamfers, extrudes, scales, shears or sweeps it |
| `map.brush_subtract`, `map.brush_merge`, `map.brush_intersect`, `map.brush_hollow` | CSG: carves a brush out of the others of its entity, merges brushes into their convex hull, keeps their common solid, hollows a brush into walls |
| `map.compile`, `map.find_leaks` | Compiles the map as it is in the editor to a BSP beside it, with its portal and leak files; reports whether it leaks without writing a file |
| `map.save`, `map.play_from_here` | Saves the open map; compiles and lights it and plays it with the player at a point |

A map tool that edits makes its edit at once, as an outside agent's edit of a file is made
(L-43): the server begins no review for it, so nothing waits for a person when the agent is
done, and `edit.undo`, or a restored checkpoint, which reads the open map again from its
file, takes it back. `edit.undo` and `edit.redo` are held to the write roots as the map
tools are while a map is open, since what they change may be the map. The built-in agent's
map edits are the ones staged as hunks (P6-21). `map.stage`, `map.accept_hunks` and
`map.reject_hunks` are still tools: an agent that wants its own edits held for a look can
begin a review and decide it.

`map.open`, `map.new` and `map.generate` write, since they replace the open map, its unsaved and staged edits
with it. A map tool that edits, compiles, saves or plays also needs the open map's file under
a write root, since `map.compile` and `map.save` write beside it: the server asks `map.list_entities`
for it first. `map.save` writes the map as it is in the editor. `preview.play` of a map the
project compiled reads its BSP from beside the open map of that name, or from the project's
`maps`. With `--connect`, an edit made while the person has the map's review open in the app
joins that review, as an edit of theirs does.

### Writing QuakeC

The server's instructions send an agent to `project.reference` before it writes or debugs
QuakeC (L-45). It answers one topic of the agent reference (`docs/agent-reference/`): `quakec`,
the language and how a mod runs; `entities`, what the engine calls, how an entity moves and
collides, the builtins by what they are for, what the language does that C does not, and
debugging; `monsters`, how a monster is written and every place a new one is added; `models`,
what a model is and how to recolour, scale and reframe one; `workflow`, tests and scenarios;
and `maps`. The built-in agent reads the same text.

### Changing how something looks

A monster's, a weapon's or an item's shape, skin and poses are its `.mdl` model (L-47). The
`model.*` tools read one and change it, each change one step with undo: `model.inspect`;
`model.copy_from_game`, which makes a project's own file of a model of the game;
`model.import`, which makes one of a mesh in an `.obj`, `.iqe`, `.fbx`, `.md5mesh` or
`.smd` file;
`model.recolor`, `model.export_skin` and `model.set_skin` for its skins; `model.scale` and
`model.move`; and `model.rename_frames`, `model.remove_frames` and `model.add_frames`. The
reference's `models` topic says what a model is and how to give a monster a look of its own.
Play puts a project's own `progs`, `sound` and other asset files where the game reads them.

### Building and reviewing a map

The server's instructions give an agent the order (L-40); it is the one a person follows:

1. `map.new` makes the map, or `map.open` opens one; `map.list_textures` lists the textures there
   are, those of the game's own levels among them, so a map needs no WAD of its own.
2. A room is one of seven shapes built whole by `map.build_room`, joined to the next by
   `map.build_passage` (L-49): the reference's `maps` topic says what each is and needs. Or
   it is a box (`map.brush_create`) hollowed into walls (`map.brush_hollow`). Rooms are
   joined by a box across the walls between them given to `map.brush_subtract`, which carves
   it out of every brush it overlaps and removes it.
3. The `map.brush_*` tools take a brush by its place among its entity's brushes, and each
   answers the places of the brushes it made or changed (`brushes`) with how many the entity
   now has (`total`): the box just added, the walls of a hollowed room, the pieces a cut
   left. A tool that changes how many brushes the entity has (a hollow, a clip, a cut, a
   merge, a delete) moves the places of the brushes after the one it worked on, so an answer
   from before it is stale for those. `map.brushes`
   lists the places, each with the box around the brush and its textures, all of them or those
   touching a box, for a map someone else built.
4. `map.place_entity` puts in the player start, the lights, the monsters and the items, with keys
   such as `light` and `angle`; `map.make_brush_entity` makes a door or a platform of a brush.
5. `map.issues` and `map.find_leaks` say what is wrong: a missing player start, an entity the
   QuakeC does not know, a leak with its trail. `map.save` writes the file.
6. To review it, `map.summary` counts what the map holds and `map.list_entities` lists its entities;
   with a `path`, `map.summary` reads a `.map` file from the disk without opening it, which
   is how a set of maps is compared, under a read root, and `most_textures` cuts its list of
   textures to the most used, the count of them all still given;
   `map.play_from_here` compiles and lights it and starts the game at a point, and
   `engine.screenshot` writes the frame as a PNG the agent can look at. With `start` paused
   the game waits where the player was put, its monsters where the map put them and not yet
   awake, which is the picture to review a room by; `preview.resume` plays on. Its answer
   names the light where the player stands and the entities that stand in the dark (`dark`),
   so an unlit room is found without a picture.

Each edit of these is made at once, as above.

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
  and compiled through them alone, nothing left waiting for review and its last edit undone; a read-only
  server refusing every map tool that writes; and, connected to an app, an edit refused for a
  map outside the write roots.

`crates/qs-commands/src/tests/remote.rs` tests the socket's host and client,
`apps/desktop/src/tests.rs` a command sent to the window over it, and `apps/cli/src/tests.rs`
runs `quakec-ide mcp` on a project end to end, and `--connect` with no app.

### A recorded session

Claude Code, given only this server's tools, on id's QuakeC 1.01 over LibreQuake's `start`
map (`quakec-ide mcp <mod> --game librequake-lite`), was asked to make the player spawn with
150 health, compile, and prove it with a scenario. In 11 turns it read `client.qc`, staged
the change and `tests/health.scenario` with `edit.stage_change`, accepted and saved them, compiled
(`"clean": true`) and ran the scenario (`expect field player health == 150` passed).
