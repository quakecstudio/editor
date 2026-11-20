// SPDX-License-Identifier: GPL-3.0-or-later

//! The agent's messages as they are on the wire (`specs/acp/v1/schema/schema.json`), read
//! leniently: a field or a variant this client does not know is passed over, since the
//! protocol grows by adding them (`initialization.mdx`: "The introduction of new capabilities
//! is not considered a breaking change").

use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::{
    Event, Lines, OptionId, OptionKind, PROTOCOL_VERSION, PermissionOption, PlanEntry, PlanStatus,
    SessionId, StopReason, ToolCall, ToolCallId, ToolStatus,
};

/// A JSON-RPC message of any kind: a request has a method and an id, a notification a
/// method alone, a response an id with a result or an error.
#[derive(Debug, Deserialize)]
pub(crate) struct Message {
    pub(crate) method: Option<String>,
    pub(crate) id: Option<Value>,
    pub(crate) params: Option<Value>,
    pub(crate) result: Option<Value>,
    pub(crate) error: Option<RpcError>,
}

/// A JSON-RPC error.
#[derive(Debug, Deserialize)]
pub(crate) struct RpcError {
    pub(crate) message: String,
}

/// `SessionNotification`.
#[derive(Deserialize)]
struct Notification {
    update: Told,
}

/// `SessionUpdate`, the variants the panel shows.
#[derive(Deserialize)]
#[serde(tag = "sessionUpdate", rename_all = "snake_case")]
enum Told {
    AgentMessageChunk {
        content: Block,
    },
    AgentThoughtChunk {
        content: Block,
    },
    ToolCall(RawToolCall),
    ToolCallUpdate(RawToolCall),
    Plan {
        entries: Vec<RawPlanEntry>,
    },
    #[serde(other)]
    Other,
}

/// `ContentBlock`, of which the panel shows text.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Block {
    Text {
        text: String,
    },
    #[serde(other)]
    Other,
}

/// `ToolCall` and `ToolCallUpdate`, which differ in what is required.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawToolCall {
    tool_call_id: String,
    title: Option<String>,
    status: Option<RawStatus>,
    content: Option<Vec<RawContent>>,
    locations: Option<Vec<RawLocation>>,
}

/// `ToolCallStatus`.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    #[serde(other)]
    Other,
}

/// `ToolCallContent`.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RawContent {
    Content {
        content: Block,
    },
    Diff {
        path: PathBuf,
    },
    #[serde(other)]
    Other,
}

/// `ToolCallLocation`.
#[derive(Deserialize)]
struct RawLocation {
    path: PathBuf,
}

/// `PlanEntry`.
#[derive(Deserialize)]
struct RawPlanEntry {
    content: String,
    status: RawPlanStatus,
}

/// `PlanEntryStatus`.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawPlanStatus {
    Pending,
    InProgress,
    Completed,
    #[serde(other)]
    Other,
}

/// `RequestPermissionRequest`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPermission {
    session_id: String,
    tool_call: RawToolCall,
    options: Vec<RawOption>,
}

/// `PermissionOption`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawOption {
    option_id: String,
    name: String,
    kind: RawOptionKind,
}

/// `PermissionOptionKind`.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

/// `InitializeResponse`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawInitialized {
    protocol_version: u64,
    agent_info: Option<RawImplementation>,
}

/// `Implementation`.
#[derive(Deserialize)]
struct RawImplementation {
    name: String,
    title: Option<String>,
    version: String,
}

/// `NewSessionResponse`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSession {
    session_id: String,
}

/// `PromptResponse`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTurn {
    stop_reason: RawStop,
}

/// `StopReason`.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawStop {
    EndTurn,
    MaxTokens,
    MaxTurnRequests,
    Refusal,
    Cancelled,
}

impl From<RawToolCall> for ToolCall {
    fn from(raw: RawToolCall) -> Self {
        let mut text = Vec::new();
        let mut paths: Vec<PathBuf> = raw
            .locations
            .unwrap_or_default()
            .into_iter()
            .map(|location| location.path)
            .collect();
        for content in raw.content.unwrap_or_default() {
            match content {
                RawContent::Content {
                    content: Block::Text { text: piece },
                } => text.push(piece),
                RawContent::Diff { path } => {
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                }
                RawContent::Content {
                    content: Block::Other,
                }
                | RawContent::Other => {}
            }
        }
        Self {
            id: ToolCallId(raw.tool_call_id),
            title: raw.title,
            status: raw.status.and_then(|status| match status {
                RawStatus::Pending => Some(ToolStatus::Pending),
                RawStatus::InProgress => Some(ToolStatus::InProgress),
                RawStatus::Completed => Some(ToolStatus::Completed),
                RawStatus::Failed => Some(ToolStatus::Failed),
                RawStatus::Other => None,
            }),
            text,
            paths,
        }
    }
}

/// The events of the notification `method` with `params`: those of a `session/update` the
/// panel shows, none for any other.
pub(crate) fn update(method: &str, params: Option<Value>) -> Vec<Event> {
    if method != "session/update" {
        return Vec::new();
    }
    let Some(notification) =
        params.and_then(|params| serde_json::from_value::<Notification>(params).ok())
    else {
        return Vec::new();
    };
    let event = match notification.update {
        Told::AgentMessageChunk {
            content: Block::Text { text },
        } => Event::Message(text),
        Told::AgentThoughtChunk {
            content: Block::Text { text },
        } => Event::Thought(text),
        Told::ToolCall(call) => Event::ToolCall(call.into()),
        Told::ToolCallUpdate(call) => Event::ToolCallUpdate(call.into()),
        Told::Plan { entries } => Event::Plan(
            entries
                .into_iter()
                .map(|entry| PlanEntry {
                    content: entry.content,
                    status: match entry.status {
                        RawPlanStatus::Pending | RawPlanStatus::Other => PlanStatus::Pending,
                        RawPlanStatus::InProgress => PlanStatus::InProgress,
                        RawPlanStatus::Completed => PlanStatus::Completed,
                    },
                })
                .collect(),
        ),
        Told::AgentMessageChunk {
            content: Block::Other,
        }
        | Told::AgentThoughtChunk {
            content: Block::Other,
        }
        | Told::Other => return Vec::new(),
    };
    vec![event]
}

/// The session, the call and the options of a `session/request_permission`.
pub(crate) fn permission(params: Value) -> Option<(SessionId, ToolCall, Vec<PermissionOption>)> {
    let raw: RawPermission = serde_json::from_value(params).ok()?;
    let options = raw
        .options
        .into_iter()
        .map(|option| PermissionOption {
            id: OptionId(option.option_id),
            name: option.name,
            kind: match option.kind {
                RawOptionKind::AllowOnce => OptionKind::AllowOnce,
                RawOptionKind::AllowAlways => OptionKind::AllowAlways,
                RawOptionKind::RejectOnce => OptionKind::RejectOnce,
                RawOptionKind::RejectAlways => OptionKind::RejectAlways,
            },
        })
        .collect();
    Some((SessionId(raw.session_id), raw.tool_call.into(), options))
}

/// `ReadTextFileRequest`.
#[derive(Deserialize)]
struct RawRead {
    path: PathBuf,
    line: Option<u32>,
    limit: Option<u32>,
}

/// `WriteTextFileRequest`.
#[derive(Deserialize)]
struct RawWrite {
    path: PathBuf,
    content: String,
}

/// A file request of the agent's.
pub(crate) enum FileAsked {
    /// `fs/read_text_file`.
    Read {
        /// The file.
        path: PathBuf,
        /// The lines of it.
        lines: Lines,
    },
    /// `fs/write_text_file`.
    Write {
        /// The file.
        path: PathBuf,
        /// Its new text.
        content: String,
    },
}

/// The file request `method` with `params` is, if it is one and is the protocol's.
pub(crate) fn file_request(method: &str, params: Value) -> Option<FileAsked> {
    match method {
        "fs/read_text_file" => {
            let raw: RawRead = serde_json::from_value(params).ok()?;
            Some(FileAsked::Read {
                path: raw.path,
                lines: Lines {
                    line: raw.line,
                    limit: raw.limit,
                },
            })
        }
        "fs/write_text_file" => {
            let raw: RawWrite = serde_json::from_value(params).ok()?;
            Some(FileAsked::Write {
                path: raw.path,
                content: raw.content,
            })
        }
        _ => None,
    }
}

/// The answer to `initialize`: the version must be this client's.
pub(crate) fn initialized(result: Value) -> Option<Event> {
    let raw: RawInitialized = serde_json::from_value(result).ok()?;
    if raw.protocol_version != PROTOCOL_VERSION {
        return Some(Event::WrongVersion(raw.protocol_version));
    }
    let agent = raw.agent_info.map(|info| {
        let name = info.title.unwrap_or(info.name);
        format!("{name} {}", info.version)
    });
    Some(Event::Initialized { agent })
}

/// The answer to `session/new`.
pub(crate) fn session_ready(result: Value) -> Option<Event> {
    let raw: RawSession = serde_json::from_value(result).ok()?;
    Some(Event::SessionReady(SessionId(raw.session_id)))
}

/// The answer to `session/prompt`.
pub(crate) fn turn_ended(result: Value) -> Option<Event> {
    let raw: RawTurn = serde_json::from_value(result).ok()?;
    Some(Event::TurnEnded(match raw.stop_reason {
        RawStop::EndTurn => StopReason::EndTurn,
        RawStop::MaxTokens => StopReason::MaxTokens,
        RawStop::MaxTurnRequests => StopReason::MaxTurnRequests,
        RawStop::Refusal => StopReason::Refusal,
        RawStop::Cancelled => StopReason::Cancelled,
    }))
}
