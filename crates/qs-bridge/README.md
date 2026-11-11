# qs-bridge

The IDE bridge protocol, [`bridge-protocol.md`](../../docs/spec/bridge-protocol.md): the
messages the preview host and the IDE's client send each other over a local socket, framed as
LSP frames them, as JSON-RPC 2.0, and the session's rules. The preview host (`apps/quakec-engine`,
P3-02) serves it; the IDE's bridge client (`qs-engine`, P3-06) speaks it.

- **Layer:** L4 bridges. It depends on `jsonschema` and `serde_json`.
- **Spec sections:** BRIDGE-1 to BRIDGE-4 and BRIDGE-7; the values of BRIDGE-5 are checked by
  the schemas of [`bridge-schema/`](../../docs/spec/bridge-schema/), built into the crate.
- **Shipped:** yes.

## Public shape

- `read_frame(reader)` and `write_frame(writer, content)`: one message's content, read from
  or written to a stream with its `Content-Length` header (BRIDGE-1.2).
- `Message::parse(content)` and `Message::to_bytes()`: a request, a notification or a
  response; a message that does not read gives the error response to send instead
  (BRIDGE-1.3, BRIDGE-1.4).
- `Session::handle(host, message)`: `initialize` first, the protocol version, the
  capabilities, each command's parameters checked against its schema, then the command run
  on the `Host` (BRIDGE-2, BRIDGE-4.2, BRIDGE-6.2); `Reply` says what to send and whether to
  close the connection.
- `event(method, params)`: an event's notification (BRIDGE-3).
- `ErrorCode` and `RpcError`: the errors of BRIDGE-7.
