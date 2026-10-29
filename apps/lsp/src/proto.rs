// SPDX-License-Identifier: GPL-3.0-or-later

//! The protocol's structures this server reads and writes, from LSP 3.18 as cached in
//! `specs/lsp/3.18/`. Each is named as the specification names it, holds only the properties
//! the server uses, and is read leniently: a property the server does not use is ignored, as
//! "Clients should ignore server capabilities they don't understand" asks of the other side.

use serde::{Deserialize, Serialize};

use crate::lines::{Position, Range};

/// `TextDocumentIdentifier`.
#[derive(Clone, Debug, Deserialize)]
pub struct TextDocumentIdentifier {
    /// The document's URI.
    pub uri: String,
}

/// `TextDocumentItem` (`types/textDocumentItem.md`).
#[derive(Clone, Debug, Deserialize)]
pub struct TextDocumentItem {
    /// The document's URI.
    pub uri: String,
    /// Its version, which increases after each change.
    pub version: i32,
    /// Its content.
    pub text: String,
}

/// `VersionedTextDocumentIdentifier`.
#[derive(Clone, Debug, Deserialize)]
pub struct VersionedTextDocumentIdentifier {
    /// The document's URI.
    pub uri: String,
    /// Its version after the change.
    pub version: i32,
}

/// `DidOpenTextDocumentParams`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenParams {
    /// The document that was opened.
    pub text_document: TextDocumentItem,
}

/// `TextDocumentContentChangeEvent`: a range and its new text, or with no range the whole
/// document's.
#[derive(Clone, Debug, Deserialize)]
pub struct ContentChange {
    /// The range replaced; `None` for the whole document.
    pub range: Option<Range>,
    /// The new text.
    pub text: String,
}

/// `DidChangeTextDocumentParams`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidChangeParams {
    /// The document and its version after the changes.
    pub text_document: VersionedTextDocumentIdentifier,
    /// The changes, each applied to the text the one before it left.
    pub content_changes: Vec<ContentChange>,
}

/// `DidCloseTextDocumentParams` and `DidSaveTextDocumentParams`: the document.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentParams {
    /// The document.
    pub text_document: TextDocumentIdentifier,
}

/// `TextDocumentPositionParams`, the params of definition, hover, prepare-rename and
/// completion.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionParams {
    /// The document.
    pub text_document: TextDocumentIdentifier,
    /// The position in it.
    pub position: Position,
}

/// `ReferenceContext`.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceContext {
    /// Whether the declaration is listed too.
    pub include_declaration: bool,
}

/// `ReferenceParams`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceParams {
    /// The document.
    pub text_document: TextDocumentIdentifier,
    /// The position in it.
    pub position: Position,
    /// What to list.
    pub context: ReferenceContext,
}

/// `RenameParams`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameParams {
    /// The document.
    pub text_document: TextDocumentIdentifier,
    /// The position in it.
    pub position: Position,
    /// The new name.
    pub new_name: String,
}

/// `InlayHintParams`.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintParams {
    /// The document.
    pub text_document: TextDocumentIdentifier,
    /// The range the editor shows.
    pub range: Range,
}

/// `WorkspaceFolder`.
#[derive(Clone, Debug, Deserialize)]
pub struct WorkspaceFolder {
    /// The folder's URI.
    pub uri: String,
}

/// What the client may say in `initializationOptions`: the project settings a `quakec.toml`
/// would give, for a project that has none, such as the rerelease.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct InitializationOptions {
    /// The dialect, as `quakec.toml` writes it: `vanilla`, `fteqcc` or `gmqcc` (PRJ-2.8).
    pub dialect: Option<qc_project::Dialect>,
    /// The target engine, as `quakec.toml` writes it (PRJ-2.9).
    pub target: Option<qc_project::Target>,
    /// FTEQCC's `-Fautoproto` (FTE-22.3).
    pub autoproto: Option<bool>,
}

/// `general` of `ClientCapabilities`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralCapabilities {
    /// The position encodings the client supports, preferred first.
    pub position_encodings: Option<Vec<String>>,
}

/// `HoverClientCapabilities`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoverCapabilities {
    /// The content formats the client supports, preferred first.
    pub content_format: Option<Vec<String>>,
}

/// `RenameClientCapabilities`.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameCapabilities {
    /// Whether the client sends `textDocument/prepareRename`.
    pub prepare_support: Option<bool>,
    /// Whether the client honors the change annotations of a rename's workspace edit, "for
    /// example, presenting the workspace edit in the user interface and asking for
    /// confirmation".
    pub honors_change_annotations: Option<bool>,
}

/// `WorkspaceEditClientCapabilities`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEditCapabilities {
    /// Whether the client supports versioned document changes, `documentChanges`.
    pub document_changes: Option<bool>,
    /// Present when the client "in general supports change annotations on text edits".
    pub change_annotation_support: Option<serde_json::Value>,
}

/// `WorkspaceClientCapabilities`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceCapabilities {
    /// Workspace edits.
    pub workspace_edit: Option<WorkspaceEditCapabilities>,
}

/// `TextDocumentClientCapabilities`.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct TextDocumentCapabilities {
    /// Hover.
    pub hover: Option<HoverCapabilities>,
    /// Rename.
    pub rename: Option<RenameCapabilities>,
}

/// `ClientCapabilities`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientCapabilities {
    /// General capabilities.
    pub general: Option<GeneralCapabilities>,
    /// Text document capabilities.
    pub text_document: Option<TextDocumentCapabilities>,
    /// Workspace capabilities.
    pub workspace: Option<WorkspaceCapabilities>,
}

/// `InitializeParams`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// The deprecated root path.
    pub root_path: Option<String>,
    /// The deprecated root URI.
    pub root_uri: Option<String>,
    /// The workspace folders.
    pub workspace_folders: Option<Vec<WorkspaceFolder>>,
    /// The project settings the client gives.
    pub initialization_options: Option<InitializationOptions>,
    /// What the client supports.
    #[serde(default)]
    pub capabilities: ClientCapabilities,
}

/// `Location`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Location {
    /// The document.
    pub uri: String,
    /// The range in it.
    pub range: Range,
}

/// `MarkupContent`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MarkupContent {
    /// `plaintext` or `markdown`.
    pub kind: &'static str,
    /// The content.
    pub value: String,
}

/// `Hover`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hover {
    /// What to show.
    pub contents: MarkupContent,
    /// The range the hover is about.
    pub range: Range,
}

/// `CompletionItem`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompletionItem {
    /// The name.
    pub label: String,
    /// A `CompletionItemKind`.
    pub kind: u32,
    /// Its type.
    pub detail: String,
}

/// `SemanticTokens`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SemanticTokens {
    /// Five integers for each token, relative to the one before.
    pub data: Vec<u32>,
}

/// `InlayHint`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InlayHint {
    /// Where it is shown.
    pub position: Position,
    /// What it says.
    pub label: String,
    /// An `InlayHintKind`: 2 for a parameter.
    pub kind: u32,
    /// Whether a space follows it.
    pub padding_right: bool,
}

/// `TextEdit`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextEdit {
    /// The range replaced.
    pub range: Range,
    /// Its new text.
    pub new_text: String,
}

/// `WorkspaceEdit`: plain `changes`, which every client supports (`types/workspaceEdit.md`: "If
/// a client neither supports `documentChanges` nor `workspace.workspaceEdit.resourceOperations`
/// then only plain `TextEdit`s using the `changes` property are supported"), or
/// `documentChanges` with the `changeAnnotations` their edits name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEdit {
    /// The edits of each document, by URI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changes: Option<std::collections::BTreeMap<String, Vec<TextEdit>>>,
    /// The edits of each document, as `TextDocumentEdit`s.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_changes: Option<Vec<TextDocumentEdit>>,
    /// The annotations the edits name, by identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_annotations: Option<std::collections::BTreeMap<String, ChangeAnnotation>>,
}

/// `OptionalVersionedTextDocumentIdentifier`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OptionalVersionedTextDocumentIdentifier {
    /// The document.
    pub uri: String,
    /// The version of the open document, `null` for one that is not open.
    pub version: Option<i32>,
}

/// `TextDocumentEdit` whose edits are `AnnotatedTextEdit`s.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentEdit {
    /// The document to change.
    pub text_document: OptionalVersionedTextDocumentIdentifier,
    /// The edits.
    pub edits: Vec<AnnotatedTextEdit>,
}

/// `AnnotatedTextEdit`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotatedTextEdit {
    /// The range replaced.
    pub range: Range,
    /// Its new text.
    pub new_text: String,
    /// The `ChangeAnnotationIdentifier` of its annotation.
    pub annotation_id: String,
}

/// `ChangeAnnotation`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeAnnotation {
    /// "A human-readable string describing the actual change."
    pub label: String,
    /// "A flag which indicates that user confirmation is needed before applying the change."
    pub needs_confirmation: bool,
    /// "A human-readable string which is rendered less prominently".
    pub description: String,
}

/// `Diagnostic`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    /// Where it applies.
    pub range: Range,
    /// A `DiagnosticSeverity`: 1 for an error, 2 for a warning.
    pub severity: u32,
    /// Its code, such as `E0301`.
    pub code: &'static str,
    /// What produced it.
    pub source: &'static str,
    /// What is wrong.
    pub message: String,
}

/// `PublishDiagnosticsParams`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PublishDiagnosticsParams {
    /// The document.
    pub uri: String,
    /// The version of the open document they are for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<i32>,
    /// Every diagnostic of the document.
    pub diagnostics: Vec<Diagnostic>,
}
