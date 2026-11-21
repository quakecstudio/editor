// SPDX-License-Identifier: GPL-3.0-or-later

//! The MCP server (P4-13, AGT-7, PRIN-6): the agent's tools, which are the IDE's commands,
//! offered to other agents over the Model Context Protocol, one JSON-RPC message per line on
//! stdio. Each tool is a command of the registry the window, the palette and the command line
//! run, and every command is a tool: an outside agent can do whatever a person at the window
//! can, the terminal and tasks included, where the built-in agent is held to its own list
//! (`qs-agent`). What holds an outside agent is the roots below, the review its map edits wait
//! in, and its client's own permissions, which read each tool's annotations.
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
//! agent plays or an edit it makes is the one the window shows.

use std::path::{Path, PathBuf};

use qs_paths::arguments::{absolute_paths, collect_paths};
use qs_paths::{lexical, lies_under};
use std::sync::Arc;
use std::sync::mpsc;

use qs_commands::remote::{Client, Described, RemoteError};
use qs_commands::{CommandError, Session};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool as McpTool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::runtime::{self, Runtime};
use tokio::sync::oneshot;

pub use qs_commands::Access;

/// One tool offered: a command of the registry, and whether it only reads (the project, the
/// paths it names, the game) or may write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tool {
    /// The command's id, which is the tool's name.
    pub id: Arc<str>,
    /// Whether it only reads.
    pub access: Access,
}

/// The prefix of the map's commands.
const MAP_COMMANDS: &str = "map.";
/// The workbench's command that names the open map's file.
const INSPECT_MAP: &str = "map.list_entities";
/// The workbench's command that opens a map in place of the open one.
const OPEN_MAP: &str = "map.open";
/// The workbench's command that makes a map and opens it in place of the open one.
const NEW_MAP: &str = "map.new";
/// The workbench's command that makes the last map edit again: a map edit under another name.
const REPEAT_EDIT: &str = "edit.repeat";
/// The workbench's commands that take back and make again the last change of the session's
/// history, which may be an edit of the open map.
const HISTORY: [&str; 2] = ["edit.undo", "edit.redo"];

impl Tool {
    /// Whether it changes the open map or writes its files.
    fn edits_map(&self) -> bool {
        self.access == Access::Write
            && (self.id.starts_with(MAP_COMMANDS) || &*self.id == REPEAT_EDIT)
    }

    /// Whether it changes whatever the session's history last changed: the open map's edit,
    /// when that is it.
    fn walks_history(&self) -> bool {
        HISTORY.contains(&&*self.id)
    }
}

/// Runs `tool` on `session` with `arguments`. A map tool that writes runs only when the open
/// map's file lies under a write root, since it may write beside it; `map.open` and `map.new`
/// replace the open map, and their path is checked as an argument; `edit.undo` and
/// `edit.redo`, which may take back or make again an edit of the open map, are held to the
/// same root while a map is open. Its edit is made at once,
/// as an outside agent's edit of a file is (L-43): no review is begun for it, and undo, or a
/// checkpoint restored, which reads the open map again from its file, takes it back. An edit made while a person has the map's
/// review open in the app joins that review, as any edit there does.
fn run_tool<K: Commands>(
    session: &mut K,
    tool: &Tool,
    arguments: Value,
    settings: &ServerSettings,
) -> Result<Value, ToolError> {
    let history = tool.walks_history();
    if !(tool.edits_map() || history) || &*tool.id == OPEN_MAP || &*tool.id == NEW_MAP {
        return session.run(&tool.id, arguments);
    }
    let open = match session.run(INSPECT_MAP, Value::Object(serde_json::Map::new())) {
        Ok(open) => open,
        // With no map open, undo and redo have no map edit to reach.
        Err(_) if history => return session.run(&tool.id, arguments),
        Err(error) => return Err(error),
    };
    let path = open.get("path").and_then(Value::as_str).unwrap_or_default();
    let full = lexical(Path::new(path));
    if !settings.write.iter().any(|root| lies_under(&full, root)) {
        return Err(ToolError::OutsideRoots {
            path: full,
            tool: tool.clone(),
        });
    }
    session.run(&tool.id, arguments)
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

/// Why a tool call was answered with an error instead of the command's output.
#[derive(Debug)]
pub enum ToolError {
    /// The command failed in the session.
    Command(CommandError),
    /// The running app's command failed: its message.
    Remote(String),
    /// The running app could not be reached or read.
    Connection(RemoteError),
    /// A path the call names lies outside the roots its tool may use.
    OutsideRoots {
        /// The path, made absolute and lexical.
        path: PathBuf,
        /// The tool.
        tool: Tool,
    },
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Command(error) => write!(f, "{error}"),
            Self::Remote(why) => f.write_str(why),
            Self::Connection(error) => write!(f, "{error}"),
            Self::OutsideRoots { path, tool } => {
                let kind = match tool.access {
                    Access::Read => "read",
                    Access::Write => "write",
                };
                write!(
                    f,
                    "{} is outside the roots {} may {kind} under",
                    path.display(),
                    tool.id
                )
            }
        }
    }
}

impl std::error::Error for ToolError {}

/// What the server's tools run on.
pub trait Commands {
    /// Every command: its id, summary, parameter schema and whether it only reads.
    fn offered(&self) -> Vec<Described>;

    /// Runs the command `id` with `arguments`: its output, or why it failed.
    ///
    /// # Errors
    ///
    /// The command's failure, or the connection's ([`ToolError`]).
    fn run(&mut self, id: &str, arguments: Value) -> Result<Value, ToolError>;
}

impl<C: 'static> Commands for Session<C> {
    fn offered(&self) -> Vec<Described> {
        self.registry().commands().map(Described::of).collect()
    }

    fn run(&mut self, id: &str, arguments: Value) -> Result<Value, ToolError> {
        self.execute(id, arguments).map_err(ToolError::Command)
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
    fn offered(&self) -> Vec<Described> {
        self.commands.clone()
    }

    fn run(&mut self, id: &str, arguments: Value) -> Result<Value, ToolError> {
        self.client
            .execute(id, arguments)
            .map_err(ToolError::Connection)?
            .map_err(ToolError::Remote)
    }
}

/// One call for the session's thread: a command, its arguments, and where to send the result.
struct Job {
    tool: Tool,
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
    /// thread of its own and serves every one of them as a tool (PRIN-3), each told to the
    /// client as reading only or not (MCP 2026-07-28, `ToolAnnotations.readOnlyHint`).
    ///
    /// # Errors
    ///
    /// [`McpError::Session`] with `make`'s error, or [`McpError::Stopped`].
    pub fn start<K: Commands>(
        make: impl FnOnce() -> Result<K, String> + Send + 'static,
        settings: ServerSettings,
    ) -> Result<Self, McpError> {
        let settings = Arc::new(settings);
        let shared = Arc::clone(&settings);
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
            let offered = session
                .offered()
                .into_iter()
                .map(|command| {
                    let schema = command.params().as_object().cloned().unwrap_or_default();
                    let tool = Tool {
                        id: command.id().into(),
                        access: command.access(),
                    };
                    // Nothing is said of the open world: the terminal and tasks run whatever
                    // they are given, which no command declares.
                    let annotations = ToolAnnotations::new().read_only(tool.access == Access::Read);
                    let listed = McpTool::new(
                        command.id().to_owned(),
                        command.summary().to_owned(),
                        schema,
                    )
                    .annotate(annotations);
                    (tool, listed)
                })
                .collect();
            if ready.send(Ok(offered)).is_err() {
                return;
            }
            while let Ok(job) = inbox.recv() {
                let result = match run_tool(&mut session, &job.tool, job.arguments, &shared) {
                    Ok(output) => tool_result(output),
                    Err(why) => tool_error(&why.to_string()),
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
            settings,
        })
    }

    /// The tools offered, each a command of the session.
    pub fn tools(&self) -> impl Iterator<Item = &Tool> {
        self.tools.iter().map(|(tool, _)| tool)
    }

    /// The names of the tools offered.
    #[must_use]
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.iter().map(|(tool, _)| &*tool.id).collect()
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
            let (client, server) = tokio::io::duplex(DUPLEX_BUFFER);
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
    fn check_roots(&self, tool: &Tool, arguments: &Value) -> Result<(), ToolError> {
        let settings = &self.settings;
        let roots: Vec<&PathBuf> = match tool.access {
            Access::Read => settings.read.iter().chain(&settings.write).collect(),
            Access::Write => settings.write.iter().collect(),
        };
        let mut paths = Vec::new();
        collect_paths(arguments, &mut paths);
        if tool.access == Access::Write {
            paths.push(settings.project.to_string_lossy().into_owned());
        }
        for path in paths {
            let full = lexical(Path::new(&path));
            if !roots.iter().any(|root| lies_under(&full, root)) {
                return Err(ToolError::OutsideRoots {
                    path: full,
                    tool: tool.clone(),
                });
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
            .map(|(tool, _)| tool.clone())
            .find(|tool| *tool.id == name)
            .ok_or_else(|| ErrorData::invalid_params(format!("Unknown tool: {name}"), None))?;
        let mut arguments = Value::Object(request.arguments.unwrap_or_default());
        absolute_paths(&mut arguments, &self.settings.project);
        if let Err(why) = self.check_roots(&tool, &arguments) {
            return Ok(tool_error(&why.to_string()).into());
        }
        let (reply, answer) = oneshot::channel();
        let job = Job {
            tool,
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

/// The bytes `serve_bytes`' in-memory pipe buffers each way.
const DUPLEX_BUFFER: usize = 1 << 16;

/// What the server tells a client's model about using it.
const INSTRUCTIONS: &str = "Every command of QuakeC Studio over the project this server was \
    started on (commands.list names them all). The kinds of tools, by the start of their \
    names: map.* builds, reads and reviews a level (map.brush_* its brushes, the rest its \
    entities, textures, layers, compile and review hunks); project.* compiles, checks, \
    tests and searches the QuakeC; file.*, edit.*, symbol.* and code.* read, change, \
    navigate and fix source files; preview.* and engine.* run the game, debug it and take \
    screenshots; vm.* and scenario.* run QuakeC without the game; texture.* imports an \
    image as a texture; checkpoint.* and git.* keep and restore work; terminal.* and \
    task.* run shell commands; paths.* and files.* are the app's own, for what it is asked \
    to open and what changed on disk. A server connected to the running app (--connect) \
    also has the app's window: view.*, editor.*, settings.* and help.*. Before you write or \
    debug QuakeC, read project.reference: topic quakec for the language and how a mod runs, \
    topic entities for what the engine calls, how entities move and collide, the builtins \
    and debugging, topic monsters before adding a monster, topic models before changing how \
    something looks, topic workflow for tests and \
    scenarios; and topic maps before building a map. Then read the project's own code for the same thing \
    (project.search, project.symbols, file.read) and follow it. In short: \
    compile, test, run QuakeC in the VM, play scenarios, inspect and drive the running game, \
    read files, stage edits as hunks and keep checkpoints; make or open a map, place and \
    key its entities, build its brushes, compile it and find its leaks, save it and play it. \
    A map edit is made at once and edit.undo takes it back. Paths are the project's, absolute or relative to it. \
    To build a map: map.new (or map.open), then map.list_textures for the texture names there \
    are. A room is map.brush_create for a box and map.brush_hollow for its walls; rooms are \
    joined by a box across the walls between them given to map.brush_subtract. The \
    map.brush_* tools take a brush by its place among its entity's brushes: each \
    answers the places of the brushes it made or changed (`brushes`) and the entity's count \
    (`total`), so use those for the next tool. A tool that changes how many brushes the \
    entity has (hollow, clip, subtract, merge, delete and the like) moves the places of the \
    brushes after the one it worked on, so an answer from before it is stale for those; \
    map.brushes lists the places with each brush's box when they are not known. \
    map.place_entity puts in the player start, lights, monsters and items, with keys such as \
    `light` and `angle`; map.make_brush_entity makes a door of a brush. Then map.issues and \
    map.find_leaks must find nothing, and map.save writes the file. To review a map: \
    map.summary counts what it holds (with a path, a .map file read without opening it, to \
    compare many maps; most_textures keeps its texture list short), map.list_entities lists \
    its entities, and \
    map.play_from_here with engine.screenshot shows it as the game draws it from a point: \
    give it start paused, so the monsters stand where the map put them, and read its `dark` \
    list, the entities no light reaches; look at the picture and fix what is dark, empty or \
    blocked.";

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

#[cfg(test)]
mod tests;
