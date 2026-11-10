// SPDX-License-Identifier: GPL-3.0-or-later

//! The MCP server (P4-13, AGT-7, PRIN-6): the agent's tools, which are the IDE's commands,
//! offered to other agents over the Model Context Protocol, one JSON-RPC message per line on
//! stdio. Each tool is a command of the registry the window, the palette and the command line
//! run, so an outside agent does what the built-in one does and nothing else.
//!
//! The server answers both eras of the protocol (MCP 2026-07-28, "Backward Compatibility with
//! Initialization-Based Versions"):
//!
//! - a **modern** request carries `_meta["io.modelcontextprotocol/protocolVersion"]` and is
//!   served on its own, as 2026-07-28 says; `server/discover` names the versions served;
//! - a **legacy** client opens with `initialize` (MCP 2025-11-25, "Lifecycle"), and its
//!   requests carry no `_meta` version.
//!
//! The server holds no state of the conversation: the project it serves and the roots it may
//! read and write are its configuration, given when it starts (2026-07-28, "Statelessness").
//! Every path a tool's arguments name, and the project itself for a tool that writes, must lie
//! under a root, or the call fails as a tool execution error.

use std::path::{Component, Path, PathBuf};

use qs_commands::{CommandError, Session};
use serde_json::{Map, Value, json};

/// The protocol version a modern request names.
pub const MODERN_VERSION: &str = "2026-07-28";

/// The legacy versions an `initialize` may name and get back; any other gets the first.
pub const LEGACY_VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

/// The `_meta` key of a modern request's protocol version.
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
/// The `_meta` key of a modern request's client capabilities, which it must carry.
const CAPABILITIES_KEY: &str = "io.modelcontextprotocol/clientCapabilities";

/// JSON-RPC's parse error.
const PARSE_ERROR: i64 = -32700;
/// JSON-RPC's invalid request.
const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC's method not found.
const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC's invalid params: also MCP's unknown tool and missing `_meta` field.
const INVALID_PARAMS: i64 = -32602;
/// MCP 2026-07-28's `UnsupportedProtocolVersionError`.
const UNSUPPORTED_VERSION: i64 = -32022;

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
pub struct ServerConfig {
    /// The project directory, which relative paths are read from.
    pub project: PathBuf,
    /// The directories a tool may read under; the project's at least.
    pub read: Vec<PathBuf>,
    /// The directories a tool may write under; each may be read too.
    pub write: Vec<PathBuf>,
    /// The server's name and version, for `serverInfo`.
    pub name: String,
    /// The server's version.
    pub version: String,
}

/// The MCP server over a command session.
pub struct Server<C> {
    session: Session<C>,
    config: ServerConfig,
    tools: Vec<Tool>,
}

/// A JSON-RPC error: its code, message and data.
struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// Which era a request is served in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Era {
    Modern,
    Legacy,
}

impl<C: 'static> Server<C> {
    /// A server of the tools of `tools` the session's registry has, over `session`.
    pub fn new(session: Session<C>, config: ServerConfig, tools: &[Tool]) -> Self {
        let tools = tools
            .iter()
            .copied()
            .filter(|tool| session.registry().get(tool.id).is_some())
            .collect();
        Self {
            session,
            config,
            tools,
        }
    }

    /// The session, for the caller that opens the project before serving.
    pub fn session_mut(&mut self) -> &mut Session<C> {
        &mut self.session
    }

    /// The names of the tools offered.
    #[must_use]
    pub fn tool_names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|tool| tool.id).collect()
    }

    /// Answers one line read from the client: the line to write back, or `None` for a
    /// notification, which is never answered.
    pub fn handle_line(&mut self, line: &str) -> Option<String> {
        let message: Value = match serde_json::from_str(line) {
            Ok(message) => message,
            Err(error) => {
                let error = RpcError::new(PARSE_ERROR, format!("not JSON: {error}"));
                return Some(reply(&Value::Null, Err(error)));
            }
        };
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            let error = RpcError::new(INVALID_REQUEST, "a request names its method");
            return Some(reply(&id.unwrap_or(Value::Null), Err(error)));
        };
        let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
        let id = id?;
        if id.is_null() {
            let error = RpcError::new(INVALID_REQUEST, "a request's id is not null");
            return Some(reply(&id, Err(error)));
        }
        Some(reply(&id, self.answer(method, &params)))
    }

    /// The result of the request `method` with `params`.
    fn answer(&mut self, method: &str, params: &Value) -> Result<Value, RpcError> {
        if method == "initialize" {
            return Ok(self.initialize(params));
        }
        let era = era(params)?;
        let mut result = match method {
            "ping" => json!({}),
            "server/discover" => {
                if era == Era::Legacy {
                    return Err(RpcError::new(
                        INVALID_PARAMS,
                        format!("server/discover needs _meta[\"{VERSION_KEY}\"]"),
                    ));
                }
                self.discover()
            }
            "tools/list" => json!({ "tools": self.list() }),
            "tools/call" => self.call(params)?,
            _ => {
                return Err(RpcError::new(
                    METHOD_NOT_FOUND,
                    format!("no method {method}"),
                ));
            }
        };
        if era == Era::Modern
            && let Some(object) = result.as_object_mut()
        {
            object.insert("resultType".to_owned(), json!("complete"));
        }
        Ok(result)
    }

    /// The answer to a legacy `initialize`: the version asked when it is one served, else the
    /// latest legacy version (2025-11-25, "Version Negotiation").
    fn initialize(&self, params: &Value) -> Value {
        let asked = params.get("protocolVersion").and_then(Value::as_str);
        let version = asked
            .filter(|asked| LEGACY_VERSIONS.contains(asked))
            .unwrap_or(LEGACY_VERSIONS[0]);
        json!({
            "protocolVersion": version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": self.config.name, "version": self.config.version },
            "instructions": instructions(),
        })
    }

    /// The answer to `server/discover`.
    fn discover(&self) -> Value {
        json!({
            "supportedVersions": [MODERN_VERSION],
            "capabilities": { "tools": {} },
            "_meta": {
                "io.modelcontextprotocol/serverInfo": {
                    "name": self.config.name,
                    "version": self.config.version,
                }
            },
            "instructions": instructions(),
        })
    }

    /// Each tool offered, as `tools/list` lists it.
    fn list(&self) -> Vec<Value> {
        self.tools
            .iter()
            .filter_map(|tool| self.session.registry().get(tool.id))
            .map(|info| {
                json!({
                    "name": info.id().as_str(),
                    "description": info.summary(),
                    "inputSchema": info.params(),
                })
            })
            .collect()
    }

    /// Calls the tool `params` names: a protocol error for an unknown tool, else its result,
    /// with `isError` when it failed or its paths are outside the roots.
    fn call(&mut self, params: &Value) -> Result<Value, RpcError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "tools/call names a tool"))?;
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.id == name)
            .copied()
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, format!("Unknown tool: {name}")))?;
        let mut arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        absolute_paths(&mut arguments, &self.config.project);
        if let Err(why) = self.check_roots(tool, &arguments) {
            return Ok(tool_error(&why));
        }
        Ok(match self.session.execute(name, arguments) {
            Ok(output) => tool_result(output),
            Err(error) => tool_error(&describe(&error)),
        })
    }

    /// Whether every path `arguments` names lies under a root the tool may use, and, for a
    /// tool that writes, the project too.
    fn check_roots(&self, tool: Tool, arguments: &Value) -> Result<(), String> {
        let roots: Vec<&PathBuf> = match tool.access {
            Access::Read => self.config.read.iter().chain(&self.config.write).collect(),
            Access::Write => self.config.write.iter().collect(),
        };
        let kind = match tool.access {
            Access::Read => "read",
            Access::Write => "write",
        };
        let mut paths = Vec::new();
        collect_paths(arguments, &mut paths);
        if tool.access == Access::Write {
            paths.push(self.config.project.to_string_lossy().into_owned());
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

/// What the server tells a client's model about using it.
const fn instructions() -> &'static str {
    "QuakeC Studio's tools over the project this server was started on: compile, test, run \
     QuakeC in the VM, play scenarios, inspect and drive the running game, stage edits as hunks \
     and keep checkpoints. Paths are the project's, absolute or relative to it."
}

/// The era of a request with `params`: modern when its `_meta` names a version, which must be
/// the one served and must come with the client's capabilities.
fn era(params: &Value) -> Result<Era, RpcError> {
    let meta = params.get("_meta");
    let Some(version) = meta.and_then(|meta| meta.get(VERSION_KEY)) else {
        return Ok(Era::Legacy);
    };
    if version.as_str() != Some(MODERN_VERSION) {
        return Err(RpcError {
            code: UNSUPPORTED_VERSION,
            message: "Unsupported protocol version".to_owned(),
            data: Some(json!({ "supported": [MODERN_VERSION], "requested": version })),
        });
    }
    if meta.and_then(|meta| meta.get(CAPABILITIES_KEY)).is_none() {
        return Err(RpcError::new(
            INVALID_PARAMS,
            format!("a request carries _meta[\"{CAPABILITIES_KEY}\"]"),
        ));
    }
    Ok(Era::Modern)
}

/// A tool's result for `output`: its JSON as text, and as structured content when it is an
/// object.
fn tool_result(output: Value) -> Value {
    let text = serde_json::to_string_pretty(&output).unwrap_or_default();
    let mut result = Map::new();
    result.insert(
        "content".to_owned(),
        json!([{ "type": "text", "text": text }]),
    );
    if output.is_object() {
        result.insert("structuredContent".to_owned(), output);
    }
    result.insert("isError".to_owned(), json!(false));
    Value::Object(result)
}

/// A tool execution error saying `why` (2026-07-28, "Error Handling").
fn tool_error(why: &str) -> Value {
    json!({ "content": [{ "type": "text", "text": why }], "isError": true })
}

/// What went wrong, as the model reads it.
fn describe(error: &CommandError) -> String {
    error.to_string()
}

/// The line answering `id`: its result or its error.
fn reply(id: &Value, outcome: Result<Value, RpcError>) -> String {
    let message = match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(error) => {
            let mut body = json!({ "code": error.code, "message": error.message });
            if let (Some(data), Some(object)) = (error.data, body.as_object_mut()) {
                object.insert("data".to_owned(), data);
            }
            json!({ "jsonrpc": "2.0", "id": id, "error": body })
        }
    };
    // A JSON value serialises without line feeds, as stdio's framing asks.
    message.to_string()
}

/// Every string under a key named `path`, or ending in `_path`, anywhere in `value`.
fn collect_paths(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            for (key, inner) in object {
                if (key == "path" || key.ends_with("_path"))
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

/// Makes every path `arguments` names absolute, a relative one taken from `project`, with `.`
/// and `..` worked out, so the commands, which name files by their full paths, find them.
fn absolute_paths(arguments: &mut Value, project: &Path) {
    match arguments {
        Value::Object(object) => {
            for (key, inner) in object.iter_mut() {
                if (key == "path" || key.ends_with("_path"))
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

/// Serves `server` on `input` and `output`, one message per line, until `input` ends.
///
/// # Errors
///
/// The first error reading `input` or writing `output`.
pub fn serve<C: 'static>(
    server: &mut Server<C>,
    input: impl std::io::BufRead,
    mut output: impl std::io::Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(answer) = server.handle_line(&line) {
            writeln!(output, "{answer}")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
