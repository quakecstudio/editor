// SPDX-License-Identifier: GPL-3.0-or-later

//! The server: the lifecycle, the document state and one handler per request.
//!
//! Like rust-analyzer's main loop, it reads one message at a time and answers it before reading
//! the next, so every answer is computed on the state the messages before it left: LSP 3.18,
//! "Language Features": "The language features should be computed on the synchronized state of
//! the document." The analysis is the [`qc_ide::AnalysisHost`]'s; this module only translates
//! between its byte offsets and paths and the protocol's positions and URIs.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;
use std::path::PathBuf;

use qc_ide::{
    AnalysisHost, Config, DefKind, Diagnostic, FileRange, HighlightKind, Modifier, RenameError,
    Severity, Span,
};
use qc_project::Loader;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::lines::{Encoding, LineIndex, Position};
use crate::render::Markup;
use crate::transport::write_message;
use crate::uri;
use crate::{proto, render};

/// JSON-RPC's and the protocol's error codes this server sends (LSP 3.18, "Response Message").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    /// `ParseError`, -32700: the content is not JSON.
    ParseError,
    /// `InvalidRequest`, -32600: not a request, or one after `shutdown`.
    InvalidRequest,
    /// `MethodNotFound`, -32601.
    MethodNotFound,
    /// `InvalidParams`, -32602.
    InvalidParams,
    /// `ServerNotInitialized`, -32002: a request before `initialize`.
    ServerNotInitialized,
    /// `RequestFailed`, -32803: "A request failed but it was syntactically correct".
    RequestFailed,
}

impl ErrorCode {
    /// The code's number.
    #[must_use]
    pub const fn code(self) -> i32 {
        match self {
            Self::ParseError => -32700,
            Self::InvalidRequest => -32600,
            Self::MethodNotFound => -32601,
            Self::InvalidParams => -32602,
            Self::ServerNotInitialized => -32002,
            Self::RequestFailed => -32803,
        }
    }
}

/// An error answer to a request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResponseError {
    /// What kind of error.
    pub code: ErrorCode,
    /// What went wrong, for a person.
    pub message: String,
}

impl ResponseError {
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Where the server is in its lifecycle (LSP 3.18, "Server lifecycle").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Before `initialize`.
    Uninitialized,
    /// Between `initialize` and `shutdown`.
    Running,
    /// After `shutdown`.
    ShutDown,
}

/// How the server's process should end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// `exit` after `shutdown`: "The server should exit with `success` code 0 if the shutdown
    /// request has been received before".
    Success,
    /// `exit` with no `shutdown` before it, or the input ended: "otherwise with `error` code 1".
    Failure,
}

/// The token types of the semantic-token legend, in the order of their numbers.
pub const TOKEN_TYPES: [&str; 4] = ["function", "property", "variable", "parameter"];

/// The token modifiers of the legend, in the order of their bits, as [`Modifier::ALL`].
pub const TOKEN_MODIFIERS: [&str; 3] = ["declaration", "readonly", "defaultLibrary"];

/// The server's state.
pub struct Server<L, W> {
    out: W,
    phase: Phase,
    host: AnalysisHost<L>,
    /// What a position's character offset counts.
    encoding: Encoding,
    /// How hovers are written.
    markup: Markup,
    /// The version of each open document.
    versions: HashMap<PathBuf, i32>,
    /// What was last published for each document.
    published: HashMap<PathBuf, Vec<Diagnostic>>,
    /// Notifications to send after the answer being computed.
    queued: Vec<(String, Value)>,
    /// The project error last shown, if the project does not open.
    project_error: Option<String>,
}

impl<L: Loader, W: Write> Server<L, W> {
    /// A server reading files through `loader` and writing messages to `out`.
    pub fn new(loader: L, out: W) -> Self {
        Self {
            out,
            phase: Phase::Uninitialized,
            host: AnalysisHost::new(loader, Config::default()),
            encoding: Encoding::Utf16,
            markup: Markup::PlainText,
            versions: HashMap::new(),
            published: HashMap::new(),
            queued: Vec::new(),
            project_error: None,
        }
    }

    /// Handles one message's content; `Some` when the server should exit.
    ///
    /// # Errors
    ///
    /// When writing an answer fails.
    pub fn handle(&mut self, content: &[u8]) -> std::io::Result<Option<Exit>> {
        let message: Value = match serde_json::from_slice(content) {
            Ok(message) => message,
            Err(err) => {
                self.respond(
                    &Value::Null,
                    Err(ResponseError::new(ErrorCode::ParseError, err.to_string())),
                )?;
                return Ok(None);
            }
        };
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response to a request the server never sends, or no message at all.
            if id.is_none() || message.get("result").is_some() || message.get("error").is_some() {
                return Ok(None);
            }
            self.respond(
                &id.unwrap_or(Value::Null),
                Err(ResponseError::new(ErrorCode::InvalidRequest, "no method")),
            )?;
            return Ok(None);
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match id {
            Some(id) => {
                let result = self.request(method, params);
                self.respond(&id, result)?;
                Ok(None)
            }
            None => self.notification(method, params),
        }
    }

    /// Answers a request.
    fn request(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        match (self.phase, method) {
            (Phase::Uninitialized, "initialize") => self.initialize(params),
            // LSP 3.18, "Initialize Request": "If the server receives a request or
            // notification before the `initialize` request, it should act as follows: For a
            // request, the response should be an error with `code: -32002`."
            (Phase::Uninitialized, _) => Err(ResponseError::new(
                ErrorCode::ServerNotInitialized,
                "the server is not initialized",
            )),
            (Phase::Running, "initialize") => Err(ResponseError::new(
                ErrorCode::InvalidRequest,
                "the initialize request may only be sent once",
            )),
            (Phase::Running, "shutdown") => {
                self.phase = Phase::ShutDown;
                Ok(Value::Null)
            }
            (Phase::Running, method) => self.feature(method, params),
            // LSP 3.18, "Shutdown Request": "If a server receives requests after a shutdown
            // request those requests should error with `InvalidRequest`."
            (Phase::ShutDown, _) => Err(ResponseError::new(
                ErrorCode::InvalidRequest,
                "the server is shut down",
            )),
        }
    }

    /// Answers a language-feature request.
    fn feature(&mut self, method: &str, params: Value) -> Result<Value, ResponseError> {
        match method {
            "textDocument/definition" => self.definition(parse(params)?),
            "textDocument/references" => self.references(parse(params)?),
            "textDocument/prepareRename" => self.prepare_rename(parse(params)?),
            "textDocument/rename" => self.rename(parse(params)?),
            "textDocument/hover" => self.hover(parse(params)?),
            "textDocument/completion" => self.completion(parse(params)?),
            "textDocument/semanticTokens/full" => self.semantic_tokens(parse(params)?),
            "textDocument/inlayHint" => self.inlay_hints(parse(params)?),
            // LSP 3.18, "$ Notifications and Requests": "If a server or client receives a
            // request starting with '$/' it must error the request with error code
            // `MethodNotFound`."
            _ => Err(ResponseError::new(
                ErrorCode::MethodNotFound,
                format!("no method {method}"),
            )),
        }
    }

    /// Acts on a notification; `Some` for `exit`.
    fn notification(&mut self, method: &str, params: Value) -> std::io::Result<Option<Exit>> {
        if method == "exit" {
            return Ok(Some(if self.phase == Phase::ShutDown {
                Exit::Success
            } else {
                Exit::Failure
            }));
        }
        // LSP 3.18: before `initialize`, "Notifications should be dropped, except for the exit
        // notification"; after `shutdown` the client sends none but `exit`.
        if self.phase != Phase::Running {
            return Ok(None);
        }
        let changed = match method {
            "initialized" => true,
            // A notification cannot be answered, so one with params of the wrong shape is
            // dropped.
            "textDocument/didOpen" => match parse(params) {
                Ok(p) => self.did_open(p)?,
                Err(_) => false,
            },
            "textDocument/didChange" => match parse(params) {
                Ok(p) => self.did_change(p)?,
                Err(_) => false,
            },
            "textDocument/didClose" => match parse(params) {
                Ok(p) => self.did_close(&p)?,
                Err(_) => false,
            },
            "textDocument/didSave" => {
                // A save may change what a `#include` brings in, which the project graph reads
                // on opening.
                let reloaded = self.host.reload();
                self.report_project(reloaded)?;
                true
            }
            // Every other notification, `$/` ones included, is free to ignore.
            _ => false,
        };
        if changed {
            self.publish()?;
        }
        Ok(None)
    }

    /// `initialize`: reads the client's settings, opens the project, answers the capabilities.
    fn initialize(&mut self, params: Value) -> Result<Value, ResponseError> {
        let params: proto::InitializeParams = parse(params)?;
        let encodings = params
            .capabilities
            .general
            .as_ref()
            .and_then(|g| g.position_encodings.clone())
            .unwrap_or_default();
        // LSP 3.18, `positionEncoding`: "If the client didn't provide any position encodings
        // the only valid value that a server can return is 'utf-16'." UTF-8 counts bytes, as
        // the analysis does, so it is taken when offered.
        self.encoding = if encodings.iter().any(|e| e == Encoding::Utf8.kind()) {
            Encoding::Utf8
        } else {
            Encoding::Utf16
        };
        let document = params
            .capabilities
            .text_document
            .clone()
            .unwrap_or_default();
        // LSP 3.18, `HoverClientCapabilities.contentFormat`: "The order describes the preferred
        // format of the client."
        let markdown = document
            .hover
            .as_ref()
            .and_then(|h| h.content_format.as_ref())
            .and_then(|formats| formats.first())
            .is_some_and(|f| f == Markup::Markdown.kind());
        self.markup = if markdown {
            Markup::Markdown
        } else {
            Markup::PlainText
        };
        let prepare = document
            .rename
            .and_then(|r| r.prepare_support)
            .unwrap_or(false);
        let options = params.initialization_options.unwrap_or_default();
        self.host.set_config(Config {
            dialect: options.dialect,
            target: options.target,
            autoproto: options.autoproto.unwrap_or(false),
        });
        let root = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .and_then(|f| uri::to_path(&f.uri))
            .or_else(|| params.root_uri.as_deref().and_then(uri::to_path))
            .or_else(|| params.root_path.map(PathBuf::from));
        let opened = match &root {
            Some(root) => self.host.open(root),
            None => Ok(()),
        };
        self.phase = Phase::Running;
        // LSP 3.18: the server "is not allowed to send any requests or notifications to the
        // client until it has responded with an `InitializeResult`", so the message waits for
        // the answer.
        if let Err(err) = opened {
            let message = format!("QuakeC project: {err}");
            self.project_error = Some(message.clone());
            self.queued.push((
                "window/showMessage".to_owned(),
                json!({ "type": 1, "message": message }),
            ));
        }
        // LSP 3.18, `renameProvider`: "`RenameOptions` may only be specified if the client
        // states that it supports `prepareSupport` in its initial `initialize` request."
        let rename = if prepare {
            json!({ "prepareProvider": true })
        } else {
            json!(true)
        };
        Ok(json!({
            "capabilities": {
                "positionEncoding": self.encoding.kind(),
                "textDocumentSync": { "openClose": true, "change": 2, "save": {} },
                "definitionProvider": true,
                "referencesProvider": true,
                "renameProvider": rename,
                "hoverProvider": true,
                "completionProvider": { "triggerCharacters": ["."] },
                "semanticTokensProvider": {
                    "legend": { "tokenTypes": TOKEN_TYPES, "tokenModifiers": TOKEN_MODIFIERS },
                    "full": true,
                },
                "inlayHintProvider": true,
            },
            "serverInfo": { "name": "quakec-lsp", "version": env!("CARGO_PKG_VERSION") },
        }))
    }
}

impl<L: Loader, W: Write> Server<L, W> {
    /// `textDocument/didOpen`: the client now owns the document's text. Returns whether the
    /// document is the server's, a `file` URI.
    fn did_open(&mut self, params: proto::DidOpenParams) -> std::io::Result<bool> {
        let Some(path) = uri::to_path(&params.text_document.uri) else {
            return Ok(false);
        };
        let path = qc_project::normalize(&path);
        self.versions
            .insert(path.clone(), params.text_document.version);
        let opened = self
            .host
            .set_overlay(&path, params.text_document.text.into_bytes());
        self.report_project(opened)?;
        Ok(true)
    }

    /// `textDocument/didChange`: applies each change in order, as LSP 3.18 asks: "apply the
    /// `TextDocumentContentChangeEvent`s in a single notification in the order you receive
    /// them."
    fn did_change(&mut self, params: proto::DidChangeParams) -> std::io::Result<bool> {
        let Some(path) = uri::to_path(&params.text_document.uri) else {
            return Ok(false);
        };
        let path = qc_project::normalize(&path);
        let mut text = self.host.text(&path).unwrap_or_default();
        for change in params.content_changes {
            text = match change.range {
                None => change.text.into_bytes(),
                Some(range) => {
                    let lines = LineIndex::new(&text, self.encoding);
                    let start = lines.offset(range.start);
                    let end = lines.offset(range.end).max(start);
                    let mut edited = text.get(..start).unwrap_or_default().to_vec();
                    edited.extend_from_slice(change.text.as_bytes());
                    edited.extend_from_slice(text.get(end..).unwrap_or_default());
                    edited
                }
            };
        }
        self.versions
            .insert(path.clone(), params.text_document.version);
        let changed = self.host.set_overlay(&path, text);
        self.report_project(changed)?;
        Ok(true)
    }

    /// `textDocument/didClose`: the file on disk is the text again. Its diagnostics stay, as
    /// for "a language [that] has a project system" (LSP 3.18, "PublishDiagnostics
    /// Notification").
    fn did_close(&mut self, params: &proto::DocumentParams) -> std::io::Result<bool> {
        let Some(path) = uri::to_path(&params.text_document.uri) else {
            return Ok(false);
        };
        let path = qc_project::normalize(&path);
        self.versions.remove(&path);
        let closed = self.host.remove_overlay(&path);
        self.report_project(closed)?;
        Ok(true)
    }

    /// Shows a project that does not open as an error message.
    ///
    /// A project file being typed fails to open at many keystrokes in a row, so an error is
    /// shown once, until the project opens or fails another way.
    fn report_project(
        &mut self,
        result: Result<(), qc_project::ProjectError>,
    ) -> std::io::Result<()> {
        let message = result.err().map(|err| format!("QuakeC project: {err}"));
        if message == self.project_error {
            return Ok(());
        }
        self.project_error.clone_from(&message);
        match message {
            Some(message) => self.notify(
                "window/showMessage",
                &json!({ "type": 1, "message": message }),
            ),
            None => Ok(()),
        }
    }

    /// Publishes the diagnostics of every file whose list changed since the last publication,
    /// an empty list clearing a file's: LSP 3.18, "If the computed set is empty, the server has
    /// to push the empty array to clear former diagnostics."
    fn publish(&mut self) -> std::io::Result<()> {
        let mut all = self.host.all_diagnostics();
        for path in self.published.keys() {
            all.entry(path.clone()).or_default();
        }
        for (path, diagnostics) in all {
            if self.published.get(&path) == Some(&diagnostics)
                || (diagnostics.is_empty() && !self.published.contains_key(&path))
            {
                continue;
            }
            let text = self.host.text(&path).unwrap_or_default();
            let lines = LineIndex::new(&text, self.encoding);
            let params = proto::PublishDiagnosticsParams {
                uri: uri::from_path(&path),
                version: self.versions.get(&path).copied(),
                diagnostics: diagnostics
                    .iter()
                    .map(|d| proto::Diagnostic {
                        range: lines.range(d.span.start, d.span.end),
                        severity: match d.code.severity() {
                            Severity::Error => 1,
                            Severity::Warning => 2,
                        },
                        code: d.code.as_str(),
                        source: "quakec",
                        message: d.message.clone(),
                    })
                    .collect(),
            };
            self.notify("textDocument/publishDiagnostics", &params)?;
            if diagnostics.is_empty() {
                self.published.remove(&path);
            } else {
                self.published.insert(path, diagnostics);
            }
        }
        Ok(())
    }

    /// The path and byte offset a document position names.
    fn locate(
        &mut self,
        document: &proto::TextDocumentIdentifier,
        position: Position,
    ) -> Option<(PathBuf, usize)> {
        let path = qc_project::normalize(&uri::to_path(&document.uri)?);
        let text = self.host.text(&path)?;
        let offset = LineIndex::new(&text, self.encoding).offset(position);
        Some((path, offset))
    }

    /// The protocol location of a range of a file.
    fn location(&mut self, range: &FileRange) -> proto::Location {
        let text = self.host.text(&range.path).unwrap_or_default();
        let lines = LineIndex::new(&text, self.encoding);
        proto::Location {
            uri: uri::from_path(&range.path),
            range: lines.range(range.span.start, range.span.end),
        }
    }

    /// The protocol locations of `ranges`, in order, each file's lines read once.
    fn locations(&mut self, ranges: &[FileRange]) -> Vec<proto::Location> {
        let mut texts: HashMap<PathBuf, Vec<u8>> = HashMap::new();
        for range in ranges {
            if !texts.contains_key(&range.path) {
                let text = self.host.text(&range.path).unwrap_or_default();
                texts.insert(range.path.clone(), text);
            }
        }
        let lines: HashMap<&PathBuf, LineIndex<'_>> = texts
            .iter()
            .map(|(path, text)| (path, LineIndex::new(text, self.encoding)))
            .collect();
        ranges
            .iter()
            .filter_map(|range| {
                let lines = lines.get(&range.path)?;
                Some(proto::Location {
                    uri: uri::from_path(&range.path),
                    range: lines.range(range.span.start, range.span.end),
                })
            })
            .collect()
    }

    /// `textDocument/definition`: a `Location`, or `null`.
    fn definition(&mut self, params: proto::PositionParams) -> Result<Value, ResponseError> {
        let Some((path, offset)) = self.locate(&params.text_document, params.position) else {
            return Ok(Value::Null);
        };
        match self.host.definition(&path, offset) {
            Some(range) => to_value(&self.location(&range)),
            None => Ok(Value::Null),
        }
    }

    /// `textDocument/references`: `Location[]`.
    fn references(&mut self, params: proto::ReferenceParams) -> Result<Value, ResponseError> {
        let Some((path, offset)) = self.locate(&params.text_document, params.position) else {
            return Ok(json!([]));
        };
        let ranges = self
            .host
            .references(&path, offset, params.context.include_declaration);
        to_value(&self.locations(&ranges))
    }

    /// `textDocument/prepareRename`: the `Range` to rename, `null` where no name is, and an
    /// error where the name cannot be renamed, which "Clients should show ... in their user
    /// interface".
    fn prepare_rename(&mut self, params: proto::PositionParams) -> Result<Value, ResponseError> {
        let Some((path, offset)) = self.locate(&params.text_document, params.position) else {
            return Ok(Value::Null);
        };
        match self.host.prepare_rename(&path, offset) {
            Ok(range) => to_value(&self.location(&range).range),
            Err(RenameError::NoName) => Ok(Value::Null),
            Err(err) => Err(rename_error(&err)),
        }
    }

    /// `textDocument/rename`: a `WorkspaceEdit`. LSP 3.18: "If the given name is not valid, the
    /// request must return a ResponseError with an appropriate message set."
    fn rename(&mut self, params: proto::RenameParams) -> Result<Value, ResponseError> {
        let Some((path, offset)) = self.locate(&params.text_document, params.position) else {
            return Err(rename_error(&RenameError::NoName));
        };
        let rename = self
            .host
            .rename(&path, offset, &params.new_name)
            .map_err(|err| rename_error(&err))?;
        if let Some(caution) = rename.caution {
            self.queued.push((
                "window/showMessage".to_owned(),
                json!({ "type": 2, "message": format!("Renamed: {caution}.") }),
            ));
        }
        let mut changes = BTreeMap::new();
        for file in rename.edits {
            let text = self.host.text(&file.path).unwrap_or_default();
            let lines = LineIndex::new(&text, self.encoding);
            let edits = file
                .edits
                .iter()
                .map(|(span, new_text)| proto::TextEdit {
                    range: lines.range(span.start, span.end),
                    new_text: new_text.clone(),
                })
                .collect();
            changes.insert(uri::from_path(&file.path), edits);
        }
        to_value(&proto::WorkspaceEdit { changes })
    }

    /// `textDocument/hover`: a `Hover`, or `null`.
    fn hover(&mut self, params: proto::PositionParams) -> Result<Value, ResponseError> {
        let Some((path, offset)) = self.locate(&params.text_document, params.position) else {
            return Ok(Value::Null);
        };
        let Some(hover) = self.host.hover(&path, offset) else {
            return Ok(Value::Null);
        };
        let range = self.location(&hover.range).range;
        let contents = proto::MarkupContent {
            kind: self.markup.kind(),
            value: render::hover(&hover, self.markup),
        };
        to_value(&proto::Hover { contents, range })
    }

    /// `textDocument/completion`: `CompletionItem[]`, which the client filters.
    fn completion(&mut self, params: proto::PositionParams) -> Result<Value, ResponseError> {
        let Some((path, offset)) = self.locate(&params.text_document, params.position) else {
            return Ok(json!([]));
        };
        let items: Vec<proto::CompletionItem> = self
            .host
            .completions(&path, offset)
            .into_iter()
            .map(|c| proto::CompletionItem {
                label: c.label,
                kind: completion_kind(c.kind),
                detail: c.detail,
            })
            .collect();
        to_value(&items)
    }

    /// `textDocument/semanticTokens/full`: the tokens, each relative to the one before (LSP
    /// 3.18, "Integer Encoding for Tokens").
    fn semantic_tokens(&mut self, params: proto::DocumentParams) -> Result<Value, ResponseError> {
        let Some(path) = uri::to_path(&params.text_document.uri) else {
            return to_value(&proto::SemanticTokens { data: Vec::new() });
        };
        let path = qc_project::normalize(&path);
        let highlights = self.host.highlights(&path);
        let text = self.host.text(&path).unwrap_or_default();
        let lines = LineIndex::new(&text, self.encoding);
        let mut data = Vec::with_capacity(highlights.len().saturating_mul(5));
        let mut last = Position::default();
        for h in highlights {
            let range = lines.range(h.span.start, h.span.end);
            // A name never spans lines; one that would is left out rather than wrapped.
            if range.start.line != range.end.line {
                continue;
            }
            let delta_line = range.start.line.saturating_sub(last.line);
            let delta_start = if delta_line == 0 {
                range.start.character.saturating_sub(last.character)
            } else {
                range.start.character
            };
            let modifiers = Modifier::ALL
                .iter()
                .enumerate()
                .filter(|(_, m)| h.modifiers.contains(**m))
                .fold(0_u32, |bits, (index, _)| {
                    bits | 1_u32
                        .checked_shl(u32::try_from(index).unwrap_or(31))
                        .unwrap_or(0)
                });
            data.extend([
                delta_line,
                delta_start,
                range.end.character.saturating_sub(range.start.character),
                token_type(h.kind),
                modifiers,
            ]);
            last = range.start;
        }
        to_value(&proto::SemanticTokens { data })
    }

    /// `textDocument/inlayHint`: the parameter names in the range, `InlayHint[]`.
    fn inlay_hints(&mut self, params: proto::InlayHintParams) -> Result<Value, ResponseError> {
        let Some(path) = uri::to_path(&params.text_document.uri) else {
            return Ok(json!([]));
        };
        let path = qc_project::normalize(&path);
        let text = self.host.text(&path).unwrap_or_default();
        let lines = LineIndex::new(&text, self.encoding);
        let span = Span::new(
            lines.offset(params.range.start),
            lines.offset(params.range.end),
        );
        let hints: Vec<proto::InlayHint> = self
            .host
            .parameter_hints(&path, span)
            .into_iter()
            .map(|h| proto::InlayHint {
                position: lines.position(h.offset),
                label: format!("{}:", h.name),
                kind: 2,
                padding_right: true,
            })
            .collect();
        to_value(&hints)
    }

    /// Writes the answer to request `id`.
    fn respond(&mut self, id: &Value, result: Result<Value, ResponseError>) -> std::io::Result<()> {
        let message = match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(err) => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": err.code.code(), "message": err.message },
            }),
        };
        self.send(&message)?;
        for (method, params) in std::mem::take(&mut self.queued) {
            self.notify(&method, &params)?;
        }
        Ok(())
    }

    /// Writes a notification.
    fn notify<P: Serialize>(&mut self, method: &str, params: &P) -> std::io::Result<()> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// Writes one message.
    fn send(&mut self, message: &Value) -> std::io::Result<()> {
        let content = serde_json::to_vec(message).map_err(std::io::Error::other)?;
        write_message(&mut self.out, &content)
    }
}

/// The params of a request or notification, or `InvalidParams`.
fn parse<T: DeserializeOwned>(params: Value) -> Result<T, ResponseError> {
    serde_json::from_value(params)
        .map_err(|err| ResponseError::new(ErrorCode::InvalidParams, err.to_string()))
}

/// A result as JSON.
fn to_value<T: Serialize>(value: &T) -> Result<Value, ResponseError> {
    serde_json::to_value(value)
        .map_err(|err| ResponseError::new(ErrorCode::RequestFailed, err.to_string()))
}

/// The error answer for a rename that cannot be made.
fn rename_error(err: &RenameError) -> ResponseError {
    ResponseError::new(ErrorCode::RequestFailed, err.to_string())
}

/// The `CompletionItemKind` of a definition's kind.
const fn completion_kind(kind: DefKind) -> u32 {
    match kind {
        DefKind::Function => 3,
        DefKind::Field => 5,
        DefKind::Global | DefKind::Param | DefKind::Local => 6,
    }
}

/// The legend index of a highlight's kind ([`TOKEN_TYPES`]).
const fn token_type(kind: HighlightKind) -> u32 {
    match kind {
        HighlightKind::Function => 0,
        HighlightKind::Field => 1,
        HighlightKind::Global | HighlightKind::Local => 2,
        HighlightKind::Parameter => 3,
    }
}
