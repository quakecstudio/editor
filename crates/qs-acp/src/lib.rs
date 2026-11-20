// SPDX-License-Identifier: GPL-3.0-or-later

//! The client side of the Agent Client Protocol, version 1 (`specs/acp/v1/`): the IDE hosts an
//! external agent (Claude Code, Codex and others) and shows what it does in the agent panel
//! (P4-14, UI-11, PRIN-6).
//!
//! [`Client`] is the protocol with no process behind it: it writes the client's requests to
//! any [`Write`] and turns each line the agent writes into [`Event`]s. [`Agent`] runs the
//! agent as a subprocess and feeds a client from its standard output, as the protocol's
//! stdio transport has it (`transports.mdx`): "Messages are delimited by newlines (`\n`), and
//! **MUST NOT** contain embedded newlines."
//!
//! The one capability the client may advertise is the file system's ([`Files::ThroughClient`],
//! `file-system.mdx`): the agent then reads and writes text files through the client, which
//! sees unsaved text and can hold a write for review. Every other capability is left out
//! (`initialization.mdx`: "Clients and Agents **MUST** treat all capabilities omitted in the
//! `initialize` request as **UNSUPPORTED**"), so the agent runs commands itself; the IDE's own
//! tools reach it as an MCP server named in `session/new`.

mod process;
mod wire;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

pub use process::{Agent, AgentCommand};

/// The protocol version this client speaks: "a single integer that identifies a **MAJOR**
/// protocol version" (`initialization.mdx`, Protocol version).
pub const PROTOCOL_VERSION: u64 = 1;

/// JSON-RPC's "Method not found", answered to a request for a method the client does not
/// offer (JSON-RPC 2.0, section 5.1).
const METHOD_NOT_FOUND: i64 = -32601;

/// JSON-RPC's "Internal error", answered to a file request the client could not carry out
/// (JSON-RPC 2.0, section 5.1).
const INTERNAL_ERROR: i64 = -32603;

/// A session's id, as the agent gave it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SessionId(String);

impl SessionId {
    /// The id as the agent wrote it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A tool call's id within its session.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ToolCallId(String);

impl ToolCallId {
    /// The id as the agent wrote it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A permission option's id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionId(String);

/// The agent's request for permission that waits for an answer: its JSON-RPC id, a number
/// or a string as the agent chose.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct PermissionId(String);

/// A request of the agent's to read or write a file that waits for its answer.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FileRequestId(String);

/// Whose file tools the agent uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Files {
    /// The agent's own: it reads and writes the disk itself.
    #[default]
    AgentsOwn,
    /// The client's: `fs/read_text_file` and `fs/write_text_file` are offered
    /// (`file-system.mdx`).
    ThroughClient,
}

/// The part of a file a read asks for: lines from `line`, counted from 1, and at most `limit`
/// of them; the whole file when both are `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Lines {
    /// The first line, counted from 1.
    pub line: Option<u32>,
    /// The most lines.
    pub limit: Option<u32>,
}

impl Lines {
    /// The lines of `text` asked for, each with its line ending.
    #[must_use]
    pub fn of(self, text: &str) -> String {
        let skip = usize::try_from(self.line.unwrap_or(1).saturating_sub(1)).unwrap_or(usize::MAX);
        let take = self.limit.map_or(usize::MAX, |limit| {
            usize::try_from(limit).unwrap_or(usize::MAX)
        });
        text.split_inclusive('\n').skip(skip).take(take).collect()
    }
}

/// Who the client says it is (`initialization.mdx`: clients "**SHOULD** also provide a name
/// and version to the Agent").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientInfo {
    /// The program's name.
    pub name: String,
    /// The name to show a person.
    pub title: String,
    /// The program's version.
    pub version: String,
}

/// An MCP server the agent is asked to connect to over stdio, the transport every agent has
/// (`session-setup.mdx`: "All Agents **MUST** support connecting to MCP servers via stdio").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServer {
    /// The name the agent shows for it.
    pub name: String,
    /// The program to run.
    pub command: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Environment variables to set for it, each a name and a value.
    pub env: Vec<(String, String)>,
}

/// Where a tool call is in its life (`tool-calls.mdx`, Status).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolStatus {
    /// Not started: its input is streaming or it waits for permission.
    Pending,
    /// Running.
    InProgress,
    /// Ended well.
    Completed,
    /// Ended in an error.
    Failed,
}

/// A tool call as the agent reports it: every field of a first report, and of a later one
/// only what changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolCall {
    /// Its id.
    pub id: ToolCallId,
    /// What it is doing, for a person.
    pub title: Option<String>,
    /// Where it is in its life.
    pub status: Option<ToolStatus>,
    /// The text of its content so far, one entry per block; a block that is not text or a
    /// diff's path is left out.
    pub text: Vec<String>,
    /// The files it touches.
    pub paths: Vec<PathBuf>,
}

/// What a permission option does (`tool-calls.mdx`, Permission Options).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptionKind {
    /// Allow this once.
    AllowOnce,
    /// Allow this and remember it.
    AllowAlways,
    /// Reject this once.
    RejectOnce,
    /// Reject this and remember it.
    RejectAlways,
}

/// One answer the agent offers to a permission request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PermissionOption {
    /// Its id, sent back when it is chosen.
    pub id: OptionId,
    /// Its label.
    pub name: String,
    /// What it does.
    pub kind: OptionKind,
}

/// The person's answer to a permission request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// This option.
    Selected(OptionId),
    /// The turn was cancelled.
    Cancelled,
}

/// Why a prompt turn ended (`prompt-turn.mdx`, Stop Reasons).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The model finished.
    EndTurn,
    /// The token limit was reached.
    MaxTokens,
    /// The limit of model requests in one turn was reached.
    MaxTurnRequests,
    /// The agent refuses to continue.
    Refusal,
    /// The client cancelled the turn.
    Cancelled,
}

/// One step of the agent's plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanEntry {
    /// What the step is.
    pub content: String,
    /// Where the step stands.
    pub status: PlanStatus,
}

/// Where a step of the plan stands (`agent-plan.mdx`, Plan Entries).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanStatus {
    /// Not started; also a status this client does not know.
    Pending,
    /// Being worked on.
    InProgress,
    /// Done.
    Completed,
}

/// What a line from the agent means to the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The agent answered `initialize`: its name and version, when it gave them.
    Initialized {
        /// The agent's name and version, for a person.
        agent: Option<String>,
    },
    /// The agent made the session asked for.
    SessionReady(SessionId),
    /// A piece of the agent's reply.
    Message(String),
    /// A piece of the agent's reasoning.
    Thought(String),
    /// A new tool call.
    ToolCall(ToolCall),
    /// A change to a tool call reported before.
    ToolCallUpdate(ToolCall),
    /// The agent's plan, whole, replacing any before.
    Plan(Vec<PlanEntry>),
    /// The agent asks whether it may run a tool call.
    PermissionAsked {
        /// The request to answer.
        request: PermissionId,
        /// The call it asks about.
        tool: ToolCall,
        /// The answers offered.
        options: Vec<PermissionOption>,
    },
    /// The agent asks for a file's text; [`Client::answer_read`] answers it.
    ReadFile {
        /// The request to answer.
        request: FileRequestId,
        /// The file, by its absolute path.
        path: PathBuf,
    },
    /// The agent asks for a file to be written; [`Client::answer_write`] answers it.
    WriteFile {
        /// The request to answer.
        request: FileRequestId,
        /// The file, by its absolute path.
        path: PathBuf,
        /// Its whole new text.
        content: String,
    },
    /// The prompt turn ended.
    TurnEnded(StopReason),
    /// The agent refused one of the client's requests.
    Refused {
        /// The method refused.
        method: &'static str,
        /// The agent's reason.
        message: String,
    },
    /// The agent chose a protocol version this client does not speak; the connection is of
    /// no more use (`initialization.mdx`: "the Client **SHOULD** close the connection and
    /// inform the user about it").
    WrongVersion(u64),
    /// The agent's process ended, or its output closed.
    Exited,
}

/// Why a message could not be written or a line could not be read.
#[derive(Debug)]
pub enum AcpError {
    /// Writing to the agent failed.
    Io(std::io::Error),
    /// A line was not a JSON-RPC message.
    Json(serde_json::Error),
    /// The agent could not be started.
    Spawn(PathBuf, std::io::Error),
}

impl core::fmt::Display for AcpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "the agent could not be written to: {error}"),
            Self::Json(error) => write!(f, "the agent wrote a line that is not a message: {error}"),
            Self::Spawn(program, error) => {
                write!(f, "{} could not be started: {error}", program.display())
            }
        }
    }
}

impl core::error::Error for AcpError {}

impl From<std::io::Error> for AcpError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for AcpError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// A request of the client's that waits for the agent's answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asked {
    /// `initialize`.
    Initialize,
    /// `session/new`.
    NewSession,
    /// `session/prompt`.
    Prompt,
}

impl Asked {
    /// The method's name on the wire.
    const fn method(self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::NewSession => "session/new",
            Self::Prompt => "session/prompt",
        }
    }
}

/// The protocol's client: writes requests to `out` and reads the agent's lines.
#[derive(Debug)]
pub struct Client<W> {
    out: W,
    /// The id of the next request.
    next: u64,
    /// The client's requests not yet answered, by id.
    asked: BTreeMap<u64, Asked>,
    /// The agent's permission requests not yet answered: the JSON-RPC id to answer with,
    /// and the session each is in.
    permissions: BTreeMap<PermissionId, (Value, SessionId)>,
    /// Whose file tools the agent was told to use.
    files: Files,
    /// The agent's file requests not yet answered: the JSON-RPC id to answer with, and for a
    /// read the lines asked for.
    file_requests: BTreeMap<FileRequestId, (Value, Lines)>,
}

impl<W: Write> Client<W> {
    /// A client writing to `out`, the agent's standard input.
    pub const fn new(out: W) -> Self {
        Self {
            out,
            next: 0,
            asked: BTreeMap::new(),
            permissions: BTreeMap::new(),
            files: Files::AgentsOwn,
            file_requests: BTreeMap::new(),
        }
    }

    /// Writes one message as one line.
    fn write(&mut self, message: &Value) -> Result<(), AcpError> {
        // serde_json writes no newline inside a value, as the transport requires.
        let mut line = serde_json::to_vec(message)?;
        line.push(b'\n');
        self.out.write_all(&line)?;
        self.out.flush()?;
        Ok(())
    }

    /// Sends a request and remembers it until it is answered.
    fn request(&mut self, asked: Asked, params: &Value) -> Result<(), AcpError> {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        self.asked.insert(id, asked);
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": asked.method(),
            "params": params,
        }))
    }

    /// Sends `initialize` with this client's version and, for [`Files::ThroughClient`], the
    /// file system capability (`initialization.mdx`: "Clients **MUST** initialize the
    /// connection by calling the `initialize` method"). [`Event::Initialized`] answers it.
    ///
    /// # Errors
    ///
    /// [`AcpError`] when the agent cannot be written to.
    pub fn initialize(&mut self, info: &ClientInfo, files: Files) -> Result<(), AcpError> {
        self.files = files;
        let capabilities = match files {
            Files::AgentsOwn => json!({}),
            Files::ThroughClient => {
                json!({ "fs": { "readTextFile": true, "writeTextFile": true } })
            }
        };
        self.request(
            Asked::Initialize,
            &json!({
                "protocolVersion": PROTOCOL_VERSION,
                "clientCapabilities": capabilities,
                "clientInfo": {
                    "name": info.name,
                    "title": info.title,
                    "version": info.version,
                },
            }),
        )
    }

    /// Sends `session/new` for the working directory `cwd`, with the MCP servers the agent
    /// should connect to. [`Event::SessionReady`] answers it.
    ///
    /// # Errors
    ///
    /// [`AcpError`] when the agent cannot be written to.
    pub fn new_session(&mut self, cwd: &Path, servers: &[McpServer]) -> Result<(), AcpError> {
        let servers: Vec<Value> = servers
            .iter()
            .map(|server| {
                let env: Vec<Value> = server
                    .env
                    .iter()
                    .map(|(name, value)| json!({ "name": name, "value": value }))
                    .collect();
                json!({
                    "name": server.name,
                    "command": server.command,
                    "args": server.args,
                    "env": env,
                })
            })
            .collect();
        self.request(
            Asked::NewSession,
            &json!({ "cwd": cwd, "mcpServers": servers }),
        )
    }

    /// Sends `session/prompt`: the person's message as one text block, which every agent
    /// takes. Updates follow as events, and [`Event::TurnEnded`] ends the turn.
    ///
    /// # Errors
    ///
    /// [`AcpError`] when the agent cannot be written to.
    pub fn prompt(&mut self, session: &SessionId, text: &str) -> Result<(), AcpError> {
        self.request(
            Asked::Prompt,
            &json!({
                "sessionId": session.0,
                "prompt": [{ "type": "text", "text": text }],
            }),
        )
    }

    /// Answers the agent's permission request `request` with `outcome`; a request already
    /// answered is not answered twice.
    ///
    /// # Errors
    ///
    /// [`AcpError`] when the agent cannot be written to.
    pub fn answer(&mut self, request: &PermissionId, outcome: &Outcome) -> Result<(), AcpError> {
        let Some((id, _)) = self.permissions.remove(request) else {
            return Ok(());
        };
        let outcome = match outcome {
            Outcome::Selected(option) => json!({ "outcome": "selected", "optionId": option.0 }),
            Outcome::Cancelled => json!({ "outcome": "cancelled" }),
        };
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": { "outcome": outcome } }))
    }

    /// Answers the agent's read `request` with the lines it asked for of `text`, the file's
    /// whole text; a request already answered is not answered twice.
    ///
    /// # Errors
    ///
    /// [`AcpError`] when the agent cannot be written to.
    pub fn answer_read(&mut self, request: &FileRequestId, text: &str) -> Result<(), AcpError> {
        let Some((id, lines)) = self.file_requests.remove(request) else {
            return Ok(());
        };
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "content": lines.of(text) },
        }))
    }

    /// Answers the agent's write `request`: the file was taken (`file-system.mdx`: "The
    /// Client responds with an empty result on success").
    ///
    /// # Errors
    ///
    /// [`AcpError`] when the agent cannot be written to.
    pub fn answer_write(&mut self, request: &FileRequestId) -> Result<(), AcpError> {
        let Some((id, _)) = self.file_requests.remove(request) else {
            return Ok(());
        };
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": {} }))
    }

    /// Answers the agent's file `request` with an error saying `why` it could not be done.
    ///
    /// # Errors
    ///
    /// [`AcpError`] when the agent cannot be written to.
    pub fn refuse_file(&mut self, request: &FileRequestId, why: &str) -> Result<(), AcpError> {
        let Some((id, _)) = self.file_requests.remove(request) else {
            return Ok(());
        };
        self.write(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": INTERNAL_ERROR, "message": why },
        }))
    }

    /// Cancels the prompt turn of `session`: the `session/cancel` notification, and the
    /// `cancelled` outcome to every permission request of the session that waits
    /// (`prompt-turn.mdx`, Cancellation: "The Client **MUST** respond to all pending
    /// `session/request_permission` requests with the `cancelled` outcome."). The turn ends
    /// when the agent answers the prompt, with [`StopReason::Cancelled`].
    ///
    /// # Errors
    ///
    /// [`AcpError`] when the agent cannot be written to.
    pub fn cancel(&mut self, session: &SessionId) -> Result<(), AcpError> {
        self.write(&json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": session.0 },
        }))?;
        let waiting: Vec<PermissionId> = self
            .permissions
            .iter()
            .filter(|(_, (_, of))| of == session)
            .map(|(request, _)| request.clone())
            .collect();
        for request in waiting {
            self.answer(&request, &Outcome::Cancelled)?;
        }
        Ok(())
    }

    /// Reads one line the agent wrote and says what it means. A request for a method the
    /// client does not offer is answered with JSON-RPC's "Method not found"; a notification
    /// or an update of a kind the panel does not show gives no event.
    ///
    /// # Errors
    ///
    /// [`AcpError`] when the line is not a JSON-RPC message, or an answer cannot be written.
    pub fn handle(&mut self, line: &str) -> Result<Vec<Event>, AcpError> {
        let message: wire::Message = serde_json::from_str(line)?;
        match (message.method, message.id) {
            (Some(method), Some(id)) => self.asked_by_agent(&method, id, message.params),
            (Some(method), None) => Ok(wire::update(&method, message.params)),
            (None, Some(id)) => Ok(self
                .answered(&id, message.result, message.error)
                .into_iter()
                .collect()),
            (None, None) => Ok(Vec::new()),
        }
    }

    /// A request from the agent: a permission request waits for the person, a file request
    /// for the client when it offered the file system; any other method is not offered.
    fn asked_by_agent(
        &mut self,
        method: &str,
        id: Value,
        params: Option<Value>,
    ) -> Result<Vec<Event>, AcpError> {
        if self.files == Files::ThroughClient
            && let Some(asked) = params
                .clone()
                .and_then(|params| wire::file_request(method, params))
        {
            let request = FileRequestId(id.to_string());
            let (lines, event) = match asked {
                wire::FileAsked::Read { path, lines } => (
                    lines,
                    Event::ReadFile {
                        request: request.clone(),
                        path,
                    },
                ),
                wire::FileAsked::Write { path, content } => (
                    Lines::default(),
                    Event::WriteFile {
                        request: request.clone(),
                        path,
                        content,
                    },
                ),
            };
            self.file_requests.insert(request, (id, lines));
            return Ok(vec![event]);
        }
        let asked = (method == "session/request_permission")
            .then(|| params.and_then(wire::permission))
            .flatten();
        let Some((session, tool, options)) = asked else {
            self.write(&json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": METHOD_NOT_FOUND, "message": "Method not found" },
            }))?;
            return Ok(Vec::new());
        };
        let request = PermissionId(id.to_string());
        self.permissions.insert(request.clone(), (id, session));
        Ok(vec![Event::PermissionAsked {
            request,
            tool,
            options,
        }])
    }

    /// The agent's answer to the request `id` of the client's.
    fn answered(
        &mut self,
        id: &Value,
        result: Option<Value>,
        error: Option<wire::RpcError>,
    ) -> Option<Event> {
        let asked = self.asked.remove(&id.as_u64()?)?;
        if let Some(error) = error {
            return Some(Event::Refused {
                method: asked.method(),
                message: error.message,
            });
        }
        let result = result.unwrap_or(Value::Null);
        let event = match asked {
            Asked::Initialize => wire::initialized(result),
            Asked::NewSession => wire::session_ready(result),
            Asked::Prompt => wire::turn_ended(result),
        };
        Some(event.unwrap_or_else(|| Event::Refused {
            method: asked.method(),
            message: "the answer is not the protocol's".to_owned(),
        }))
    }
}

#[cfg(test)]
mod tests;
