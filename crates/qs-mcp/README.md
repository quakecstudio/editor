# qs-mcp

The MCP server (P4-13, AGT-7, PRIN-6): the agent's tools, which are commands of the IDE's
registry, offered to other agents over the Model Context Protocol, one JSON-RPC message per
line. `quakec-ide mcp` serves it on stdin and stdout; [`docs/mcp.md`](../../docs/mcp.md) is the
setup for Claude Code and other clients.

- **Protocol:** `rmcp`, the SDK of the protocol's own project, answers both eras: modern
  requests (2026-07-28) that carry their version and capabilities in `_meta`, with
  `server/discover`, and legacy clients that open with `initialize`. The server keeps no
  conversation state; its project and roots are its configuration.
- **Tools:** `AGENT_TOOLS`, each a command id with whether it reads or writes. `Server`
  lists those the session's registry has and calls them through the session, which runs on a
  thread of its own because the analysis is not `Send`; rmcp's async handler sends it each
  call.
- **Roots:** paths are made absolute from the project, then every path a call names, and the
  project for a tool that writes, must lie under a root the tool may use, or the call is a tool
  error.
- **Layer:** L6 agent. It depends on `qs-commands` (L5), `rmcp`, `tokio` and `serde_json`.
- **Spec sections:** none of `docs/spec/`; it implements MCP 2026-07-28 and 2025-11-25,
  cached in `specs/mcp/`.
- **Shipped:** yes.

## Tests

`src/tests.rs` writes JSON lines to the server, as a client does, over the workbench's commands
on a project on disk:

- the legacy handshake, tool listing and calls, and paths relative to the project;
- modern requests and `server/discover`;
- `-32602` for an unknown tool, and `isError` for a failed command or a path outside the roots;
- a read-only project refusing tools that write;
- a session that does not start.
