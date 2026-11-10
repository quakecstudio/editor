// SPDX-License-Identifier: GPL-3.0-or-later

//! The MCP server (P4-13, AGT-7, PRIN-6): the agent's tools, which are the IDE's commands,
//! offered to other agents over the Model Context Protocol, one JSON-RPC message per line on
//! stdio. Each tool is a command of the registry the window, the palette and the command line
//! run, so an outside agent does what the built-in one does and nothing else.
//!
//! The protocol is `rmcp`'s, the Rust SDK of the protocol's own project: it answers both eras,
//! modern requests carrying their version in `_meta` (MCP 2026-07-28) with `server/discover`,
//! and legacy clients that open with `initialize` (MCP 2025-11-25, 2025-06-18, 2025-03-26 and
//! 2024-11-05), and negotiates the version. This crate gives it the tools, the roots and the
//! session that runs them.
//!
//! The server keeps no state of the conversation: the project it serves and the roots it may
//! read and write are its configuration, given when it starts (2026-07-28, "Statelessness").
//! Every path a tool's arguments name is taken from the project when relative, and it, and the
//! project itself for a tool that writes, must lie under a root, or the call fails as a tool
//! execution error. The command session is not `Send` (the analysis caches `Rc` trees), so it
//! runs on a thread of its own, which the async handler sends each call to.
//!
//! The tools run on [`Commands`]: a [`Session`] the server makes for itself (headless), or
//! [`Connected`], the running app's session over its socket (`--connect`), so that a game the
//! agent plays or an edit it stages is the one the window shows.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

use qs_commands::Session;
use qs_commands::remote::{Client, Described, RemoteError};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool as McpTool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::runtime::{self, Runtime};
use tokio::sync::oneshot;

/// Whether a tool only reads or also writes files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    /// It reads the project and the paths it names.
    Read,
    /// It writes the project (a build, a checkpoint, an edit) or the paths it names.
    Write,
}

/// One tool offered: a command of the registry, and what it may touch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tool {
    /// The command's id, which is the tool's name.
    pub id: &'static str,
    /// What it touches.
    pub access: Access,
}

/// The agent's tools (AGT-2) and the commands its loop drives: plan, edit, compile, test,
/// preview and propose (AGT-1), with checkpoints to roll back (AGT-4).
pub const AGENT_TOOLS: [Tool; 26] = [
    tool("project.symbols", Access::Read),
    tool("project.check", Access::Read),
    tool("file.read", Access::Read),
    tool("symbol.definition", Access::Read),
    tool("symbol.references", Access::Read),
    tool("symbol.hover", Access::Read),
    tool("engine.builtins", Access::Read),
    tool("compile", Access::Write),
    tool("project.test", Access::Write),
    tool("vm.run", Access::Write),
    tool("scenario.run", Access::Write),
    tool("edit.apply", Access::Write),
    tool("edit.accept", Access::Write),
    tool("edit.reject", Access::Write),
    tool("edit.staged", Access::Read),
    tool("file.save", Access::Write),
    tool("checkpoint.create", Access::Write),
    tool("checkpoint.list", Access::Read),
    tool("checkpoint.restore", Access::Write),
    tool("preview.play", Access::Write),
    tool("preview.stop", Access::Read),
    tool("preview.poll", Access::Read),
    tool("engine.inspect", Access::Read),
    tool("engine.console", Access::Read),
    tool("engine.screenshot", Access::Write),
    tool("preview.reload", Access::Write),
];

/// A tool of `id` that touches as `access` says.
const fn tool(id: &'static str, access: Access) -> Tool {
    Tool { id, access }
}

/// What the server serves: its project, its roots and the name it gives.
#[derive(Clone, Debug)]
pub struct ServerSettings {
    /// The project directory, which relative paths are taken from.
    pub project: PathBuf,
    /// The directories a tool may read under; the project's at least.
    pub read: Vec<PathBuf>,
    /// The directories a tool may write under; each may be read too.
    pub write: Vec<PathBuf>,
    /// The server's name, for `serverInfo`.
    pub name: String,
    /// The server's version.
    pub version: String,
}

/// Why the server did not start.
#[derive(Debug)]
pub enum McpError {
    /// The session could not be made: the project did not open, or the commands did not
    /// register.
    Session(String),
    /// The session's thread ended before answering.
    Stopped,
    /// The transport failed.
    Io(std::io::Error),
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session(why) => f.write_str(why),
            Self::Stopped => f.write_str("the command session stopped"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for McpError {}

/// What the server's tools run on.
pub trait Commands {
    /// The summary and parameter schema of the command `id`, if there is one.
    fn describe(&self, id: &str) -> Option<(String, Value)>;

    /// Runs the command `id` with `arguments`: its output, or why it failed.
    ///
    /// # Errors
    ///
    /// The command's failure, or the connection's, as a message for the model.
    fn run(&mut self, id: &str, arguments: Value) -> Result<Value, String>;
}

impl<C: 'static> Commands for Session<C> {
    fn describe(&self, id: &str) -> Option<(String, Value)> {
        let info = self.registry().get(id)?;
        Some((info.summary().to_owned(), info.params().clone()))
    }

    fn run(&mut self, id: &str, arguments: Value) -> Result<Value, String> {
        self.execute(id, arguments)
            .map_err(|error| error.to_string())
    }
}

/// The running app's session, over the socket it answers on (AGT-7, `--connect`).
pub struct Connected {
    client: Client,
    commands: Vec<Described>,
}

impl Connected {
    /// Connects to the app answering on `socket` and reads its commands.
    ///
    /// # Errors
    ///
    /// [`RemoteError`] when no app answers there or its answer is not a list of commands.
    pub fn connect(socket: &Path) -> Result<Self, RemoteError> {
        let mut client = Client::connect(socket)?;
        let commands = client.commands()?;
        Ok(Self { client, commands })
    }
}

impl Commands for Connected {
    fn describe(&self, id: &str) -> Option<(String, Value)> {
        let command = self.commands.iter().find(|command| command.id() == id)?;
        Some((command.summary().to_owned(), command.params().clone()))
    }

    fn run(&mut self, id: &str, arguments: Value) -> Result<Value, String> {
        self.client
            .execute(id, arguments)
            .map_err(|error| error.to_string())?
    }
}

/// One call for the session's thread: a command, its arguments, and where to send the result.
struct Job {
    name: String,
    arguments: Value,
    reply: oneshot::Sender<CallToolResult>,
}

/// The MCP server: rmcp's handler over a command session on a thread of its own.
#[derive(Clone)]
pub struct Server {
    jobs: mpsc::Sender<Job>,
    tools: Arc<[(Tool, McpTool)]>,
    settings: Arc<ServerSettings>,
}

impl Server {
    /// Starts the commands `make` builds (a session, or a connection to the app's) on a
    /// thread of its own and serves the tools of `tools` they have.
    ///
    /// # Errors
    ///
    /// [`McpError::Session`] with `make`'s error, or [`McpError::Stopped`].
    pub fn start<K: Commands>(
        make: impl FnOnce() -> Result<K, String> + Send + 'static,
        settings: ServerSettings,
        tools: &'static [Tool],
    ) -> Result<Self, McpError> {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (ready, listed) = mpsc::channel::<Result<Vec<(Tool, McpTool)>, String>>();
        std::thread::spawn(move || {
            let mut session = match make() {
                Ok(session) => session,
                Err(why) => {
                    // The starting thread waits on this; if it has gone, nothing is owed.
                    let _sent = ready.send(Err(why));
                    return;
                }
            };
            let offered = tools
                .iter()
                .filter_map(|tool| {
                    let (summary, params) = session.describe(tool.id)?;
                    let schema = params.as_object().cloned().unwrap_or_default();
                    Some((*tool, McpTool::new(tool.id, summary, schema)))
                })
                .collect();
            if ready.send(Ok(offered)).is_err() {
                return;
            }
            while let Ok(job) = inbox.recv() {
                let result = match session.run(&job.name, job.arguments) {
                    Ok(output) => tool_result(output),
                    Err(why) => tool_error(&why),
                };
                // A client that went away no longer waits for the result.
                let _sent = job.reply.send(result);
            }
        });
        let offered: Vec<(Tool, McpTool)> = listed
            .recv()
            .map_err(|_| McpError::Stopped)?
            .map_err(McpError::Session)?;
        Ok(Self {
            jobs,
            tools: offered.into(),
            settings: Arc::new(settings),
        })
    }

    /// The names of the tools offered.
    #[must_use]
    pub fn tool_names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|(tool, _)| tool.id).collect()
    }

    /// Serves on `read` and `write` until the client closes `read`, on a runtime of its own.
    ///
    /// # Errors
    ///
    /// [`McpError::Io`] when the runtime or the transport fails.
    pub fn serve_on<R, W>(self, read: R, write: W) -> Result<(), McpError>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        runtime()?.block_on(self.run(read, write))
    }

    /// Serves on stdin and stdout until stdin ends.
    ///
    /// # Errors
    ///
    /// As [`Server::serve_on`].
    pub fn serve_stdio(self) -> Result<(), McpError> {
        runtime()?.block_on(async move {
            let (read, write) = rmcp::transport::stdio();
            self.run(read, write).await
        })
    }

    /// The service on `read` and `write`, until the client closes `read`.
    async fn run<R, W>(self, read: R, write: W) -> Result<(), McpError>
    where
        R: AsyncRead + Send + Unpin + 'static,
        W: AsyncWrite + Send + Unpin + 'static,
    {
        let failed = |error: String| McpError::Io(std::io::Error::other(error));
        let running = self
            .serve((read, write))
            .await
            .map_err(|error| failed(error.to_string()))?;
        running
            .waiting()
            .await
            .map_err(|error| failed(error.to_string()))?;
        Ok(())
    }

    /// Serves the messages of `input`, one per line, and answers what the server wrote back,
    /// once `input` has ended and every call has been answered: for tests and scripts.
    ///
    /// # Errors
    ///
    /// As [`Server::serve_on`].
    pub fn serve_bytes(self, input: Vec<u8>) -> Result<Vec<u8>, McpError> {
        runtime()?.block_on(async move {
            let (client, server) = tokio::io::duplex(1 << 16);
            let (server_read, server_write) = tokio::io::split(server);
            let (mut client_read, mut client_write) = tokio::io::split(client);
            let serving = tokio::spawn(async move {
                let running = self.serve((server_read, server_write)).await;
                if let Ok(running) = running {
                    // The service ends when the client's side closes.
                    let _ended = running.waiting().await;
                }
            });
            client_write.write_all(&input).await.map_err(McpError::Io)?;
            client_write.shutdown().await.map_err(McpError::Io)?;
            let mut output = Vec::new();
            client_read
                .read_to_end(&mut output)
                .await
                .map_err(McpError::Io)?;
            serving
                .await
                .map_err(|error| McpError::Io(std::io::Error::other(error.to_string())))?;
            Ok(output)
        })
    }

    /// Whether every path `arguments` names lies under a root the tool may use, and, for a
    /// tool that writes, the project too.
    fn check_roots(&self, tool: Tool, arguments: &Value) -> Result<(), String> {
        let settings = &self.settings;
        let roots: Vec<&PathBuf> = match tool.access {
            Access::Read => settings.read.iter().chain(&settings.write).collect(),
            Access::Write => settings.write.iter().collect(),
        };
        let kind = match tool.access {
            Access::Read => "read",
            Access::Write => "write",
        };
        let mut paths = Vec::new();
        collect_paths(arguments, &mut paths);
        if tool.access == Access::Write {
            paths.push(settings.project.to_string_lossy().into_owned());
        }
        for path in paths {
            let full = lexical(Path::new(&path));
            if !roots.iter().any(|root| full.starts_with(lexical(root))) {
                return Err(format!(
                    "{} is outside the roots {} may {kind} under",
                    full.display(),
                    tool.id
                ));
            }
        }
        Ok(())
    }
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        let mut config = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        config.server_info = Implementation::new(&self.settings.name, &self.settings.version);
        config.instructions = Some(INSTRUCTIONS.to_owned());
        config
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = self.tools.iter().map(|(_, tool)| tool.clone()).collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let tool = self
            .tools
            .iter()
            .map(|(tool, _)| *tool)
            .find(|tool| tool.id == name)
            .ok_or_else(|| ErrorData::invalid_params(format!("Unknown tool: {name}"), None))?;
        let mut arguments = Value::Object(request.arguments.unwrap_or_default());
        absolute_paths(&mut arguments, &self.settings.project);
        if let Err(why) = self.check_roots(tool, &arguments) {
            return Ok(tool_error(&why).into());
        }
        let (reply, answer) = oneshot::channel();
        let job = Job {
            name,
            arguments,
            reply,
        };
        self.jobs
            .send(job)
            .map_err(|_| ErrorData::internal_error("the command session stopped", None))?;
        let result = answer
            .await
            .map_err(|_| ErrorData::internal_error("the command session stopped", None))?;
        Ok(result.into())
    }
}

/// A runtime on this thread, with I/O and time.
fn runtime() -> Result<Runtime, McpError> {
    runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(McpError::Io)
}

/// What the server tells a client's model about using it.
const INSTRUCTIONS: &str = "QuakeC Studio's tools over the project this server was started on: \
    compile, test, run QuakeC in the VM, play scenarios, inspect and drive the running game, \
    read files, stage edits as hunks and keep checkpoints. Paths are the project's, absolute or \
    relative to it.";

/// A tool's result for `output`: its JSON as text, and as structured content when it is an
/// object (MCP 2026-07-28, "Structured Content").
fn tool_result(output: Value) -> CallToolResult {
    let text = serde_json::to_string_pretty(&output).unwrap_or_default();
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    if output.is_object() {
        result.structured_content = Some(output);
    }
    result
}

/// A tool execution error saying `why` (2026-07-28, "Error Handling").
fn tool_error(why: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(why)])
}

/// Every string under a key named `path`, or ending in `_path`, anywhere in `value`.
fn collect_paths(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, inner) in object {
                if is_path_key(key)
                    && let Some(path) = inner.as_str()
                {
                    out.push(path.to_owned());
                }
                collect_paths(inner, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| collect_paths(item, out)),
        _ => {}
    }
}

/// Whether a parameter named `key` holds a path.
fn is_path_key(key: &str) -> bool {
    key == "path" || key.ends_with("_path")
}

/// Makes every path `arguments` names absolute, a relative one taken from `project`, with `.`
/// and `..` worked out, so the commands, which name files by their full paths, find them.
fn absolute_paths(arguments: &mut Value, project: &Path) {
    match arguments {
        Value::Object(object) => {
            for (key, inner) in object.iter_mut() {
                if is_path_key(key)
                    && let Some(path) = inner.as_str()
                {
                    let full = lexical(&project.join(path));
                    *inner = Value::String(full.to_string_lossy().into_owned());
                } else {
                    absolute_paths(inner, project);
                }
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| absolute_paths(item, project)),
        _ => {}
    }
}

/// `path` with `.` and `..` worked out by its text, as a check of where it points needs
/// before the file exists.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests;
