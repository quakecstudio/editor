# The IDE bridge protocol

Prefix: `BRIDGE` (registered in [`README.md`](README.md)). IDs follow the rules there: never
renumbered, never reused, withdrawn rather than deleted.

The protocol the IDE's bridge client (`qs-engine`, P3-06) speaks to the preview host
(`apps/preview-host`, P3-02) over a local socket: versioned JSON messages that carry the
host's events (a progs loaded, an edict spawned, a breakpoint hit) and the client's commands
(reload, step, inspect, set a field). It is this project's own design, after the
[Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/) (requests,
responses and events, capabilities announced at initialization) carried the way the
[Language Server Protocol 3.18](../../specs/lsp/3.18/specification.md) carries JSON-RPC 2.0
(`specs/lsp/3.18/specification.md`, "Base Protocol"). Its schemas are in
[`bridge-schema/`](bridge-schema/), JSON Schema 2020-12, one file per message, with an
example instance of each under `bridge-schema/examples/`; the tests in
`crates/qs-tests/tests/it/bridge_schema.rs` hold the schemas, the examples and this document
to each other.

Requirements: ENG-1, ENG-2 and ENG-14 ([`../../plan/requirements.yaml`](../../plan/requirements.yaml)).
Issue: [#89](https://github.com/quakecstudio/editor/issues/89) (P3-01).

## BRIDGE-1 Transport and framing

**Source:** this design; LSP 3.18, "Base Protocol": "The base protocol consists of a header
and a content part (comparable to HTTP). The header and content part are separated by a
'\r\n'." and "Content-Length | number | The length of the content part in bytes. This header
is required."

### Behaviour

- **BRIDGE-1.1** The client launches the host with `-bridge <path>` and the host listens at
  `<path>`: a Unix domain socket on macOS and Linux, a named pipe `\\.\pipe\<name>` on
  Windows. The host serves one client; a second connection is closed at once. The host exits
  when the client's connection closes.
- **BRIDGE-1.2** Each message is a header part and a content part as LSP's base protocol
  frames them: header fields `Name: value` each ended by `\r\n`, an empty line, then the
  content. `Content-Length` is required and gives the content's length in bytes;
  `Content-Type`, when present, is `application/vscode-jsonrpc; charset=utf-8`, the only
  encoding, and any other is refused by closing the connection. The header is ASCII.
- **BRIDGE-1.3** The content is one JSON-RPC 2.0 object: a request (`method` and `id`), a
  notification (`method`, no `id`) or a response (`id` with `result` or `error`). Batches
  (a JSON array) are not supported: one is answered with a parse error (`-32600`) and no
  message is read from it. An `id` is an integer or a string the sender chooses; the host's
  own requests, if any, use ids the client never does, since the client numbers from 1 and
  the host from -1 downward.
- **BRIDGE-1.4** The envelope is `bridge-schema/message.json`: `jsonrpc` is `"2.0"`, a
  request has `method` and `id` and no `result`, a notification has `method` and no `id`, a
  response has `id` and exactly one of `result` and `error`, where an error has an integer
  `code`, a `message` and an optional `data`.

## BRIDGE-2 The session

**Source:** this design; DAP's `initialize` request and capabilities.

### Behaviour

- **BRIDGE-2.1** The client's first request is `initialize`, with the protocol version it
  speaks and its name and version (`initialize.params.json`). Any other request before it is
  answered with `-32003` (not initialized) and any notification before it is ignored.
- **BRIDGE-2.2** The host answers `initialize` with the protocol version it speaks, its own
  name, version and engine, and its capabilities (`initialize.result.json`). The host sends no
  event before it has answered.
- **BRIDGE-2.3** The capabilities are one boolean per capability-flagged command (BRIDGE-4.1,
  the ENG-14 commands and those the host may leave out of a build): `pause`, `resume`,
  `field/set`, `pick`, `entity/spawn`, `entity/remove`, `entity/edit`, `player/spawn_at`,
  `screenshot` and `evaluate`. A command whose flag is absent or `false` is answered with
  `-32005` (unsupported); the client shows it disabled.
- **BRIDGE-2.4** A protocol version is `major.minor` in decimal. The host accepts a client
  whose `major` is its own and answers with its own version; a different `major` is answered
  with `-32001` (incompatible version) and the host closes the connection. A client reading a
  higher `minor` than it knows proceeds; it ignores what it does not know (BRIDGE-6). This
  document describes version `1.0`.
- **BRIDGE-2.5** `shutdown` (no parameters, result `null`) asks the host to stop its game and
  answer; the client then closes the connection, which ends the host (BRIDGE-1.1). A client
  that closes without `shutdown` ends it too.

## BRIDGE-3 Events

**Source:** this design; ADR-0004, "What the bridge needs": "hooks in `PR_LoadProgs` (progs
loaded), `ED_Alloc` and `ED_Free` (edict spawned and removed), the field writes in the
interpreter (edict changed), `PR_RunError` (QC error) and the statement loop of
`PR_ExecuteProgram` (breakpoints and stepping)".

An event is a notification from the host. Each carries `time`, the server's clock in seconds
(`sv.time`) when it happened, except `console/line` outside a game.

### Behaviour

- **BRIDGE-3.1** The events, each with the schema of its parameters under
  `bridge-schema/event.<name>.json` (`/` as `.`):

  | Method | When | Carries |
  | --- | --- | --- |
  | `progs/loaded` | a progs.dat is loaded or reloaded | its path, CRC, version, the counts of functions, globals and fields, whether a `.lno` was found, and the VM time |
  | `map/loaded` | a map's entities are spawned | the map name and the number of entities |
  | `edict/spawned` | an edict is allocated (`ED_Alloc`) | its number and classname |
  | `edict/changed` | an edict's fields were written during a server frame | its number and each changed field with its new value, once per frame (BRIDGE-3.2) |
  | `edict/removed` | an edict is freed (`ED_Free`) | its number |
  | `qc/error` | the VM stops with a run error (`PR_RunError`) | the message, where it stopped and the stack |
  | `breakpoint/hit` | execution reaches a breakpoint | the breakpoint's id, where, and `self` |
  | `console/line` | the host prints a line | the text and its level |
  | `state/changed` | the game pauses or resumes | the state, why, and where it paused |

- **BRIDGE-3.2** The host sends `edict/changed` at most once per edict per server frame,
  after the frame, with every field written during it and its value then; a field written
  back to its old value is still listed. While paused, a field set by the client (BRIDGE-4.4)
  is reported when it is applied.
- **BRIDGE-3.3** A `breakpoint/hit` or `qc/error` is followed by `state/changed` to `paused`
  with the matching reason; `step` ends with `state/changed` to `paused` with reason `step`
  at the new location; `resume` sends `state/changed` to `running`. A `reload` while paused
  keeps the host paused and sends `progs/loaded` then `state/changed` with reason `reload`.
- **BRIDGE-3.4** A location names the function and, when the progs has a `.lno` (LNO-1),
  the file and line and the statement index; without one it names the function alone.

## BRIDGE-4 Commands

**Source:** this design; ENG-1 and ENG-14.

A command is a request from the client, answered with a result on success or an error
(BRIDGE-7). The parameters and results are under `bridge-schema/<name>.params.json` and
`<name>.result.json` (`/` as `.`).

### Behaviour

- **BRIDGE-4.1** Every operation of the bridge client (ENG-1, ENG-14) maps to a command, and
  every command to one of them:

  | Operation | Method | Flagged | Does |
  | --- | --- | --- | --- |
  | launch | none | | the client starts the host process with its arguments (P3-06); the session starts with `initialize` (BRIDGE-2) |
  | reload | `reload` | no | loads the progs named, or the running one's path again, keeping the map, migrating globals and fields by name and type (P3-03); the result counts what migrated and lists what was lost and why |
  | set_breakpoint | `breakpoint/set` | no | a breakpoint at a source line, resolved through the `.lno` to the first statement on that line of the file; the result names it and where it landed; `breakpoint/clear` removes one by id |
  | step | `step` | no | runs one statement (`statement`), into the next call (`in`), to the next statement of this function (`over`), to the caller (`out`) or to the end of the server frame (`frame`); the host must be paused (BRIDGE-4.3) |
  | inspect | `inspect` | no | an edict's classname and every field with its value (`edict`), every named global (`globals`), the call stack (`stack`, paused only) or the list of edicts in use with classname, model and origin (`entities`) |
  | watch | `evaluate` | yes | the value of a QuakeC expression: a global, a field path such as `self.owner.health`, or a constant, in the frame given (paused only) |
  | console | `console` | no | runs a console command; its output comes back as `console/line` events |
  | screenshot | `screenshot` | yes | the frame the host last drew, or draws for the size asked, as a PNG in base64 |
  | pause, resume | `pause`, `resume` | yes | stop and start the server clock; the state events follow (BRIDGE-3.3) |
  | set field | `field/set` | yes | writes one field of an edict with a typed value; while paused it is applied when the game resumes (`on_resume`), else at once (`now`) |
  | pick | `pick` | yes | the edict under a point of the host's window, `x` and `y` as fractions of its width and height from the top left, and the point hit; no edict when the ray hits the world or nothing |
  | spawn entity | `entity/spawn` | yes | spawns an edict of a classname with the key-values given, as a map entity is spawned (EDICT-8); the result is its number |
  | remove entity | `entity/remove` | yes | frees an edict |
  | edit entity | `entity/edit` | yes | sets key-values on an edict as its spawn would parse them (EDICT-8) |
  | spawn player at | `player/spawn_at` | yes | moves the player to an origin and angles, as a teleporter would |

- **BRIDGE-4.2** A command's parameters are validated against its schema before it runs; a
  message that fails is answered with `-32602` (invalid params) and nothing runs.
- **BRIDGE-4.3** `step`, `inspect` with `stack` and `evaluate` need the host paused; while
  running they are answered with `-32002` (not paused) and nothing changes. `inspect` with
  `edict`, `globals` or `entities` runs at any time and reads the state between two server
  frames, never during one.
- **BRIDGE-4.4** `field/set` and `entity/edit` while paused are applied in order when the
  game resumes, and their results say `on_resume`; while running they apply before the next
  server frame and say `now`. A value of a type other than the field's is answered with
  `-32006` (bad value) and not applied.
- **BRIDGE-4.5** An edict number that is not in use is answered with `-32004` (no such
  edict); a breakpoint id the host does not hold, or a file and line with no statement, with
  `-32007` (no such breakpoint). A file path is relative to the project's root, as the `.lno`
  and the compiler name files.

## BRIDGE-5 Values

**Source:** this design; PROGS-3, the QuakeC types.

### Behaviour

- **BRIDGE-5.1** A QuakeC value is an object with its `type`, one of `float`, `vector`,
  `string`, `entity`, `field`, `function` and `void`, and its `value`: a number for a float,
  an array of three numbers for a vector, a string, an edict number for an entity, a field's
  name for a field, a function's name for a function, and nothing for `void`
  (`bridge-schema/value.json`).
- **BRIDGE-5.2** An edict is its number, 0 the world; a time is seconds as a number; a vector
  is `[x, y, z]`; an engine stack frame names the engine function and is marked `engine`;
  a key-value is the two strings as a map entity holds them.

## BRIDGE-6 Compatibility

**Source:** this design.

### Behaviour

- **BRIDGE-6.1** A member a receiver does not know is ignored: no schema here forbids
  additional properties, and a client or host validating a message must not reject one for
  an unknown member.
- **BRIDGE-6.2** A notification whose method a receiver does not know is ignored; a request
  whose method the host does not know is answered with `-32601` (method not found).
- **BRIDGE-6.3** A change that adds an optional member, a new event or a new capability-flagged
  command raises the `minor` version; a change that removes or renames a member, method or
  capability, makes an optional member required, or changes a member's type or meaning
  raises the `major` version and withdraws the statements it changes.
- **BRIDGE-6.4** A new command is capability-flagged from the version that adds it; a flag is
  removed only by a `major` change.

## BRIDGE-7 Errors

**Source:** this design; JSON-RPC 2.0, section 5.1, "Error object".

### Behaviour

- **BRIDGE-7.1** The error codes are JSON-RPC's and the host's own in JSON-RPC's reserved
  range for implementation-defined errors, `-32000` to `-32099`:

  | Code | Name | When |
  | --- | --- | --- |
  | `-32700` | parse error | the content is not JSON |
  | `-32600` | invalid request | the content is not a JSON-RPC 2.0 object, or is a batch |
  | `-32601` | method not found | a request's method is unknown (BRIDGE-6.2) |
  | `-32602` | invalid params | the parameters fail their schema (BRIDGE-4.2) |
  | `-32603` | internal error | the host failed in a way the others do not name |
  | `-32001` | incompatible version | `initialize` with another major (BRIDGE-2.4) |
  | `-32002` | not paused | a command that needs a paused host (BRIDGE-4.3) |
  | `-32003` | not initialized | a request before `initialize` (BRIDGE-2.1) |
  | `-32004` | no such edict | an edict number not in use (BRIDGE-4.5) |
  | `-32005` | unsupported | a command whose capability is off (BRIDGE-2.3) |
  | `-32006` | bad value | a value of the wrong type for a field (BRIDGE-4.4) |
  | `-32007` | no such breakpoint | a breakpoint id or line the host cannot resolve (BRIDGE-4.5) |

- **BRIDGE-7.2** An error's `message` is for the person; its `data`, when present, is an
  object the schema of the command may describe; a client acts on the `code`, never on the
  message's text.
