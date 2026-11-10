# qs-mcp

The MCP server (P4-13, AGT-7, PRIN-6): the agent's tools, which are commands of the IDE's
registry, offered to other agents over the Model Context Protocol, one JSON-RPC message per
line. `quakec-ide mcp` serves it on stdin and stdout; [`docs/mcp.md`](../../docs/mcp.md) is the
setup for Claude Code and other clients.

- **Protocol:** both eras of MCP. A modern request (2026-07-28) names its version and the
  client's capabilities in `_meta` and is served on its own; `server/discover` lists the
  version. A legacy client opens with `initialize` (2025-11-25, 2025-06-18, 2025-03-26). The
  server keeps no conversation state; its project and roots are its configuration.
- **Tools:** `AGENT_TOOLS`, each a command id with whether it reads or writes. `Server` lists
  those the session's registry has and calls them through the session.
- **Roots:** every path a call names, and the project for a tool that writes, must lie under
  a root the tool may use, or the call is a tool error.
- **Layer:** L6 agent. It depends on `qs-commands` (L5) and `serde_json`.
- **Spec sections:** none of `docs/spec/`; it implements MCP 2026-07-28 and 2025-11-25,
  cached in `specs/mcp/`.
- **Shipped:** yes.

## Tests

`src/tests.rs`, over the workbench's commands on a project on disk:

- the legacy handshake and calls;
- modern requests and `server/discover`;
- `-32022` for another version and `-32602` for a missing capability or an unknown tool;
- `isError` for a failed command or a path outside the roots;
- JSON-RPC's own errors;
- a read-only project;
- stdio framing.
