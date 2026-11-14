// SPDX-License-Identifier: GPL-3.0-or-later

//! The IDE bridge protocol (`docs/spec/bridge-protocol.md`): messages framed as LSP frames
//! them (BRIDGE-1), JSON-RPC 2.0 requests, notifications and responses, the session's
//! `initialize` and capabilities (BRIDGE-2), the commands and events (BRIDGE-3, BRIDGE-4),
//! and the errors (BRIDGE-7), each checked against the schemas of `docs/spec/bridge-schema/`.
//! The preview host serves it; the IDE's client (`qs-engine`) speaks it.

mod base64;
mod framing;
mod message;
mod schemas;
mod session;

pub use base64::base64;
pub use framing::{BRIDGE_CONTENT_TYPE, BRIDGE_MAX_CONTENT, FrameError, read_frame, write_frame};
pub use message::{ErrorCode, Id, Message, RpcError};
pub use session::{
    Answer, BRIDGE_COMMANDS, BRIDGE_PROTOCOL_VERSION, Capabilities, Host, HostInfo, Reply, Session,
    check_schema, event, is_flagged,
};
