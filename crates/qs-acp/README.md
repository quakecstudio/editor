# qs-acp

The client side of the Agent Client Protocol (P4-14, UI-11, PRIN-6): the IDE runs an external
agent, such as Claude Code through its ACP adapter, as a subprocess, and the agent panel shows
what it says and does. The agent brings its own account, so the IDE needs no model provider
for it.

- **Layer:** L6 agent. It depends on `serde`, `serde_json` and `tracing` only; the protocol is
  written here on `serde_json` rather than taken from the protocol's own Rust crate, since the
  client needs a handful of messages and that crate changes weekly.
- **Spec sections:** none in `docs/spec/`. The protocol is ACP version 1, cached verbatim in
  `specs/acp/v1/`; each test quotes the sentence it holds the client to, and the messages the
  client writes are checked against `specs/acp/v1/schema/schema.json`.
- **Shipped:** yes.

## What it does

- `Client` is the protocol with no process behind it. It writes `initialize`, `session/new`
  (with the MCP servers the agent should connect to, which is how the IDE's own tools reach
  it), `session/prompt` and `session/cancel`, and answers `session/request_permission`. Each
  line the agent writes becomes `Event`s: the reply and the reasoning as they stream, tool
  calls and their updates, the plan, a permission request, and the turn's end with its stop
  reason.
- `Agent` starts the agent with its standard input and output piped, one message a line, and
  feeds a `Client` from the lines; what the agent writes to standard error goes to the log.
  Dropping it ends the process.

The client advertises no capability, so the agent reads and writes files and runs commands
itself. A request for a method the client did not offer (`fs/read_text_file`, `terminal/*`)
is answered with JSON-RPC's "Method not found". Cancelling a turn answers every permission
request that waits with the `cancelled` outcome, as the protocol requires.

## Tests

`src/tests.rs`: each request the client writes, against the schema; the agent's answers and
updates as events; permission requests answered once; cancellation; methods not offered; one
message a line.
