// SPDX-License-Identifier: GPL-3.0-or-later

//! The workbench commands: opening a project and a file, editing and saving a file, and the
//! language features over the open project.

use crate::git::register_git;
use crate::terminal::register_terminal;
use std::path::PathBuf;

use qc_project::{Loader, normalize};
use qs_commands::{BoxError, Command, NoParams, Outcome, RegisterError, Registry, UndoStep};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::checkpoints::register_checkpoints;
use crate::editing::{Change, Source};
use crate::entity_classes::register_entities;
use crate::export::ExportProject;
use crate::map_check::register_map;
use crate::search::register_search;
use crate::staged::register_staged;
use crate::testing::register_tests;
use crate::tools::register_tools;
use crate::vm_run::register_vm;
use crate::{
    Caution, DefKind, EntryKind, FileRange, Location, ModuleBuilt, Reported, Workbench,
    WorkbenchError,
};

/// A context the workbench commands work on: one that has a [`Workbench`].
pub trait HasWorkbench {
    /// How its workbench reads files.
    type Loader: Loader;

    /// The workbench.
    fn workbench(&mut self) -> &mut Workbench<Self::Loader>;
}

impl<L: Loader> HasWorkbench for Workbench<L> {
    type Loader = L;

    fn workbench(&mut self) -> &mut Self {
        self
    }
}

/// The id of the command that opens a project.
pub const OPEN_PROJECT: &str = "project.open";

/// The id of the command that builds the open project.
pub const BUILD_PROJECT: &str = "project.build";

/// The id of the command that reports the open project's problems.
pub const CHECK_PROJECT: &str = "project.check";

/// The id of the command that takes the background check's results (P3-35).
pub const TAKE_CHECKED: &str = "project.take_checked";

/// The id of the command that opens a file.
pub const OPEN_FILE: &str = "file.open";

/// The id of the command that takes an editor's text of a file.
pub const UPDATE_FILE: &str = "file.update";

/// The id of the command that writes a file to disk.
pub const SAVE_FILE: &str = "file.save";

/// The id of the command that formats a file (P4-34).
pub const FORMAT_FILE: &str = "file.format";

/// The id of the command that lists the quick fixes at a place (P4-34).
pub const CODE_ACTIONS: &str = "code.actions";

/// The id of the command that applies a quick fix (P4-34).
pub const CODE_FIX: &str = "code.fix";

/// The id of the command that gives a file's inlay hints (P4-34).
pub const INLAY_HINTS: &str = "file.inlay_hints";

/// The id of the command that reads a file's current text.
pub const READ_FILE: &str = "file.read";

/// The id of `file.close`.
pub const CLOSE_FILE: &str = "file.close";

/// The id of the command that finds where a name is defined.
pub const DEFINITION: &str = "symbol.definition";

/// The id of the command that finds every use of a name.
pub const REFERENCES: &str = "symbol.references";

/// The id of the command that says what a name is.
pub const HOVER: &str = "symbol.hover";

/// The id of the command that lists the names that may be written at a place.
pub const COMPLETE: &str = "symbol.complete";

/// The id of the command that renames a name everywhere it is written.
pub const RENAME: &str = "symbol.rename";

/// Registers the workbench commands.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub fn register<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(OpenProject)?;
    registry.register(BuildProject)?;
    registry.register(CheckProject)?;
    registry.register(TakeChecked)?;
    registry.register(OpenFileCommand)?;
    registry.register(UpdateFile)?;
    registry.register(SaveFile)?;
    registry.register(FormatFile)?;
    registry.register(CodeActions)?;
    registry.register(CodeFix)?;
    registry.register(InlayHints)?;
    registry.register(ReadFile)?;
    registry.register(CloseFile)?;
    registry.register(Definition)?;
    registry.register(References)?;
    registry.register(HoverCommand)?;
    registry.register(Complete)?;
    registry.register(RenameCommand)?;
    registry.register(ExportProject)?;
    register_terminal(registry)?;
    register_git(registry)?;
    register_staged(registry)?;
    register_tools(registry)?;
    register_entities(registry)?;
    register_map(registry)?;
    register_search(registry)?;
    register_tests(registry)?;
    register_checkpoints(registry)?;
    register_vm(registry)
}

/// The parameters of `project.open`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenProjectParams {
    /// The project directory: the one holding `quakec.toml`, or `progs.src` and
    /// `csprogs.src`.
    pub path: PathBuf,
}

/// What `project.open` returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ProjectOpened {
    /// How many files the project reads.
    pub files: usize,
}

/// `project.open`: opens a project.
struct OpenProject;

impl<C: HasWorkbench + 'static> Command<C> for OpenProject {
    const ID: &'static str = OPEN_PROJECT;
    const SUMMARY: &'static str = "Opens the QuakeC project in a directory: its quakec.toml, \
        or its progs.src and csprogs.src.";
    type Params = OpenProjectParams;
    type Output = ProjectOpened;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: OpenProjectParams,
    ) -> Result<Outcome<ProjectOpened, C>, WorkbenchError> {
        let workbench = context.workbench();
        workbench.open_project(&params.path)?;
        let files = workbench
            .tree
            .iter()
            .filter(|entry| matches!(entry.kind, EntryKind::File(_)))
            .count();
        Ok(Outcome::outside_history(ProjectOpened { files }))
    }
}

/// `project.build`: builds the open project.
struct BuildProject;

impl<C: HasWorkbench + 'static> Command<C> for BuildProject {
    const ID: &'static str = BUILD_PROJECT;
    const SUMMARY: &'static str = "Compiles each module of the open project, with the editor's \
        text of each open file, and writes its progs.dat and .lno.";
    type Params = NoParams;
    type Output = Vec<ModuleBuilt>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        NoParams {}: NoParams,
    ) -> Result<Outcome<Vec<ModuleBuilt>, C>, WorkbenchError> {
        Ok(Outcome::outside_history(context.workbench().build()?))
    }
}

/// `project.check`: reports the open project's problems.
struct CheckProject;

impl<C: HasWorkbench + 'static> Command<C> for CheckProject {
    const ID: &'static str = CHECK_PROJECT;
    const SUMMARY: &'static str = "Lists every error and warning in the open project, as the \
        Problems panel does.";
    type Params = NoParams;
    type Output = Vec<Reported>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        NoParams {}: NoParams,
    ) -> Result<Outcome<Vec<Reported>, C>, WorkbenchError> {
        Ok(Outcome::outside_history(context.workbench().check()?))
    }
}

/// `project.take_checked`: takes what the background check worked out for the latest edit,
/// if it has finished (P3-35); the window runs it each frame.
struct TakeChecked;

impl<C: HasWorkbench + 'static> Command<C> for TakeChecked {
    const ID: &'static str = TAKE_CHECKED;
    const SUMMARY: &'static str = "Takes the problems, symbols and highlighting the background \
        check worked out for the latest edit, if it has finished; says whether anything changed.";
    type Params = NoParams;
    type Output = bool;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        NoParams {}: NoParams,
    ) -> Result<Outcome<bool, C>, WorkbenchError> {
        Ok(Outcome::outside_history(context.workbench().take_checked()))
    }
}

/// The parameters of `file.open`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenFileParams {
    /// The file.
    pub path: PathBuf,
    /// The byte offset to put the caret at; the start of the file when absent.
    #[serde(default)]
    pub offset: Option<usize>,
}

/// What `file.open` returns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct FileOpened {
    /// The file, as the project graph names it.
    pub path: PathBuf,
    /// Where the caret is.
    pub location: Location,
}

/// `file.open`: opens a file in the editor.
struct OpenFileCommand;

impl<C: HasWorkbench + 'static> Command<C> for OpenFileCommand {
    const ID: &'static str = OPEN_FILE;
    const SUMMARY: &'static str = "Opens a file in the editor, with the caret at a byte \
        offset or at the start.";
    type Params = OpenFileParams;
    type Output = FileOpened;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: OpenFileParams,
    ) -> Result<Outcome<FileOpened, C>, WorkbenchError> {
        let workbench = context.workbench();
        let location = workbench.open_file(&params.path, params.offset.unwrap_or(0))?;
        let path = normalize(&params.path);
        Ok(Outcome::outside_history(FileOpened { path, location }))
    }
}

/// A file and a place in it: a span, with where it starts as a person reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Place {
    /// The file, as the project graph names it.
    pub path: PathBuf,
    /// The span's first byte.
    pub start: usize,
    /// The byte after the span.
    pub end: usize,
    /// Where the span starts.
    pub location: Location,
}

impl<L: Loader> Workbench<L> {
    /// `range` as a [`Place`], with its location in the file's current text.
    fn place(&mut self, range: FileRange) -> Place {
        let text = self.host.text(&range.path).unwrap_or_default();
        Place {
            location: Location::of(&text, range.span.start),
            start: range.span.start,
            end: range.span.end,
            path: range.path,
        }
    }
}

/// The parameters of a command asked at a place: a file and a byte offset in it.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AtParams {
    /// The file.
    pub path: PathBuf,
    /// The byte offset of the place, in the file's current text.
    pub offset: usize,
}

/// The parameters of `file.update`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateFileParams {
    /// The file.
    pub path: PathBuf,
    /// Its whole new text.
    pub text: String,
}

/// The parameters of `file.close`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CloseFileParams {
    /// The file.
    pub path: PathBuf,
}

/// `file.close`: closes a file's tab.
struct CloseFile;

impl<C: HasWorkbench + 'static> Command<C> for CloseFile {
    const ID: &'static str = CLOSE_FILE;
    const SUMMARY: &'static str = "Closes a file's tab, opening the tab beside it when it was         the open file; its unsaved text is kept.";
    type Params = CloseFileParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: CloseFileParams,
    ) -> Result<Outcome<(), C>, WorkbenchError> {
        context.workbench().close_file(&params.path)?;
        Ok(Outcome::outside_history(()))
    }
}

/// `file.update`: takes an editor's text of a file, unsaved.
struct UpdateFile;

impl<C: HasWorkbench + 'static> Command<C> for UpdateFile {
    const ID: &'static str = UPDATE_FILE;
    const SUMMARY: &'static str = "Takes the whole text of a file as an editor has it, unsaved, \
        and checks the project again.";
    type Params = UpdateFileParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: UpdateFileParams,
    ) -> Result<Outcome<(), C>, WorkbenchError> {
        context
            .workbench()
            .update(&params.path, params.text.into_bytes())?;
        Ok(Outcome::outside_history(()))
    }
}

/// What `file.read` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileText {
    /// The file.
    pub path: PathBuf,
    /// Its current text: the editor's when it has one, else what is on disk; bytes that are
    /// not UTF-8 are replaced.
    pub text: String,
    /// Whether the text is the editor's, not yet saved.
    pub unsaved: bool,
}

/// `file.read`: a file's current text, for the agent and other tools that do not show it.
struct ReadFile;

impl<C: HasWorkbench + 'static> Command<C> for ReadFile {
    const ID: &'static str = READ_FILE;
    const SUMMARY: &'static str = "Reads a file's current text: the editor's when it has \
        unsaved changes, else what is on disk.";
    type Params = SaveFileParams;
    type Output = FileText;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SaveFileParams,
    ) -> Result<Outcome<FileText, C>, WorkbenchError> {
        let workbench = context.workbench();
        let path = normalize(&params.path);
        let text = workbench
            .text(&path)
            .ok_or_else(|| WorkbenchError::NoFile { path: path.clone() })?;
        let unsaved = workbench.is_unsaved(&path);
        Ok(Outcome::unchanged(FileText {
            path,
            text: String::from_utf8_lossy(&text).into_owned(),
            unsaved,
        }))
    }
}

/// The parameters of `file.save` and `file.read`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveFileParams {
    /// The file.
    pub path: PathBuf,
}

/// What `file.format` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Formatted {
    /// Whether formatting changed the text.
    pub changed: bool,
}

/// `file.format`: formats a file's current text, which the editor takes; it is not saved.
struct FormatFile;

impl<C: HasWorkbench + 'static> Command<C> for FormatFile {
    const ID: &'static str = FORMAT_FILE;
    const SUMMARY: &'static str = "Formats a QuakeC file: each line indented with tabs to its \
        depth, trailing white space removed. Only white space at the ends of lines changes, so \
        it compiles to the same progs.dat; the file is not saved.";
    type Params = SaveFileParams;
    type Output = Formatted;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SaveFileParams,
    ) -> Result<Outcome<Formatted, C>, WorkbenchError> {
        let workbench = context.workbench();
        let before = workbench.text(&params.path);
        let after = workbench
            .formatted(&params.path)
            .ok_or_else(|| WorkbenchError::NoFile {
                path: params.path.clone(),
            })?;
        let changed = before.as_deref() != Some(after.as_slice());
        if changed {
            workbench.set_text(&params.path, after, Source::Elsewhere)?;
        }
        Ok(Outcome::outside_history(Formatted { changed }))
    }
}

/// The parameters of `code.actions` and `code.fix`.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeActionParams {
    /// The file.
    pub path: PathBuf,
    /// The byte offset the fixes are for: the diagnostics there.
    pub offset: usize,
    /// With `code.fix`, which fix, by its index in `code.actions`' list at the same place.
    #[serde(default)]
    pub index: Option<usize>,
}

/// One quick fix, as `code.actions` lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ActionInfo {
    /// What it does.
    pub title: String,
    /// The diagnostic it fixes.
    pub code: String,
    /// The file it edits.
    pub path: PathBuf,
}

/// `code.actions`: the quick fixes for the diagnostics at a place.
struct CodeActions;

impl<C: HasWorkbench + 'static> Command<C> for CodeActions {
    const ID: &'static str = CODE_ACTIONS;
    const SUMMARY: &'static str = "Lists the quick fixes for the diagnostics at a place in a \
        file: a missing builtin declared, a misspelled field corrected, an unused local removed, \
        a missing precache added.";
    type Params = CodeActionParams;
    type Output = Vec<ActionInfo>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: CodeActionParams,
    ) -> Result<Outcome<Vec<ActionInfo>, C>, WorkbenchError> {
        let actions = context
            .workbench()
            .code_actions(&params.path, params.offset, params.offset);
        Ok(Outcome::outside_history(
            actions
                .into_iter()
                .map(|action| ActionInfo {
                    title: action.title,
                    code: action.code.as_str().to_owned(),
                    path: action.edit.path,
                })
                .collect(),
        ))
    }
}

/// `code.fix`: applies one quick fix at a place.
struct CodeFix;

impl<C: HasWorkbench + 'static> Command<C> for CodeFix {
    const ID: &'static str = CODE_FIX;
    const SUMMARY: &'static str = "Applies a quick fix for the diagnostics at a place in a file, \
        the first unless one is named by its index in code.actions' list; the file is not saved.";
    type Params = CodeActionParams;
    type Output = ActionInfo;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: CodeActionParams,
    ) -> Result<Outcome<ActionInfo, C>, WorkbenchError> {
        let workbench = context.workbench();
        let actions = workbench.code_actions(&params.path, params.offset, params.offset);
        let action =
            actions
                .get(params.index.unwrap_or(0))
                .ok_or_else(|| WorkbenchError::NoFix {
                    path: params.path.clone(),
                    offset: params.offset,
                })?;
        workbench.apply_action(action)?;
        Ok(Outcome::outside_history(ActionInfo {
            title: action.title.clone(),
            code: action.code.as_str().to_owned(),
            path: action.edit.path.clone(),
        }))
    }
}

/// One inlay hint: a parameter's name shown before an argument.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InlayHint {
    /// The byte offset of the argument.
    pub offset: usize,
    /// The parameter's name.
    pub name: String,
}

/// `file.inlay_hints`: the parameter names before a file's call arguments.
struct InlayHints;

impl<C: HasWorkbench + 'static> Command<C> for InlayHints {
    const ID: &'static str = INLAY_HINTS;
    const SUMMARY: &'static str = "Lists a file's inlay hints: the name of the parameter each \
        argument of a call is passed as.";
    type Params = SaveFileParams;
    type Output = Vec<InlayHint>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SaveFileParams,
    ) -> Result<Outcome<Vec<InlayHint>, C>, WorkbenchError> {
        let hints = context.workbench().inlay_hints(&params.path);
        Ok(Outcome::outside_history(
            hints
                .into_iter()
                .map(|hint| InlayHint {
                    offset: hint.offset,
                    name: hint.name,
                })
                .collect(),
        ))
    }
}

/// `file.save`: writes a file's current text to disk.
struct SaveFile;

impl<C: HasWorkbench + 'static> Command<C> for SaveFile {
    const ID: &'static str = SAVE_FILE;
    const SUMMARY: &'static str = "Writes a file's current text, as the editor has it, to disk.";
    type Params = SaveFileParams;
    type Output = ();
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SaveFileParams,
    ) -> Result<Outcome<(), C>, WorkbenchError> {
        context.workbench().save(&params.path)?;
        Ok(Outcome::outside_history(()))
    }
}

/// `symbol.definition`: where a name is defined.
struct Definition;

impl<C: HasWorkbench + 'static> Command<C> for Definition {
    const ID: &'static str = DEFINITION;
    const SUMMARY: &'static str = "Finds where the name at a place is defined.";
    type Params = AtParams;
    type Output = Option<Place>;
    type Error = std::convert::Infallible;

    fn run(
        &self,
        context: &mut C,
        params: AtParams,
    ) -> Result<Outcome<Option<Place>, C>, Self::Error> {
        let workbench = context.workbench();
        let place = workbench
            .definition(&params.path, params.offset)
            .map(|range| workbench.place(range));
        Ok(Outcome::unchanged(place))
    }
}

/// `symbol.references`: every declaration and use of a name.
struct References;

impl<C: HasWorkbench + 'static> Command<C> for References {
    const ID: &'static str = REFERENCES;
    const SUMMARY: &'static str = "Finds every declaration and use of the name at a place, in \
        every file.";
    type Params = AtParams;
    type Output = Vec<Place>;
    type Error = std::convert::Infallible;

    fn run(
        &self,
        context: &mut C,
        params: AtParams,
    ) -> Result<Outcome<Vec<Place>, C>, Self::Error> {
        let workbench = context.workbench();
        let places = workbench
            .references(&params.path, params.offset)
            .into_iter()
            .map(|range| workbench.place(range))
            .collect();
        Ok(Outcome::unchanged(places))
    }
}

/// What `symbol.hover` returns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HoverInfo {
    /// The name's span.
    pub place: Place,
    /// Its declaration, as written.
    pub declaration: String,
    /// The comment written with it, if any.
    pub docs: Option<String>,
}

/// `symbol.hover`: what a name is.
struct HoverCommand;

impl<C: HasWorkbench + 'static> Command<C> for HoverCommand {
    const ID: &'static str = HOVER;
    const SUMMARY: &'static str = "Says what the name at a place is: its declaration and its \
        comment.";
    type Params = AtParams;
    type Output = Option<HoverInfo>;
    type Error = std::convert::Infallible;

    fn run(
        &self,
        context: &mut C,
        params: AtParams,
    ) -> Result<Outcome<Option<HoverInfo>, C>, Self::Error> {
        let workbench = context.workbench();
        let info = workbench
            .hover(&params.path, params.offset)
            .map(|hover| HoverInfo {
                place: workbench.place(hover.range),
                declaration: hover.declaration,
                docs: hover.docs,
            });
        Ok(Outcome::unchanged(info))
    }
}

/// One name `symbol.complete` offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidate {
    /// The name.
    pub label: String,
    /// What it is.
    pub kind: CandidateKind,
    /// Its type or declaration.
    pub detail: String,
}

/// What a completion candidate is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateKind {
    /// A variable or constant.
    Global,
    /// A function.
    Function,
    /// A field.
    Field,
    /// A parameter.
    Parameter,
    /// A local.
    Local,
}

impl From<DefKind> for CandidateKind {
    fn from(kind: DefKind) -> Self {
        match kind {
            DefKind::Global => Self::Global,
            DefKind::Function => Self::Function,
            DefKind::Field => Self::Field,
            DefKind::Param => Self::Parameter,
            DefKind::Local => Self::Local,
        }
    }
}

/// `symbol.complete`: the names that may be written at a place.
struct Complete;

impl<C: HasWorkbench + 'static> Command<C> for Complete {
    const ID: &'static str = COMPLETE;
    const SUMMARY: &'static str = "Lists the names that may be written at a place.";
    type Params = AtParams;
    type Output = Vec<Candidate>;
    type Error = std::convert::Infallible;

    fn run(
        &self,
        context: &mut C,
        params: AtParams,
    ) -> Result<Outcome<Vec<Candidate>, C>, Self::Error> {
        let candidates = context
            .workbench()
            .completions(&params.path, params.offset)
            .into_iter()
            .map(|c| Candidate {
                label: c.label,
                kind: c.kind.into(),
                detail: c.detail,
            })
            .collect();
        Ok(Outcome::unchanged(candidates))
    }
}

/// The parameters of `symbol.rename`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RenameParams {
    /// The file.
    pub path: PathBuf,
    /// The byte offset of the name.
    pub offset: usize,
    /// The new name.
    pub name: String,
}

/// What `symbol.rename` returns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RenameDone {
    /// The files changed, unsaved.
    pub files: Vec<PathBuf>,
    /// What a map may still name by the old name: a function's or field's name, if any.
    pub caution: Option<String>,
}

/// The files a rename changed, to put back and to change again.
struct RenameStep {
    changes: Vec<Change>,
}

impl RenameStep {
    /// Gives each changed file its text before, or after.
    fn set<C: HasWorkbench>(&self, context: &mut C, before: bool) -> Result<(), BoxError> {
        let workbench = context.workbench();
        for change in &self.changes {
            let text = if before {
                &change.before
            } else {
                &change.after
            };
            workbench.set_text(&change.path, text.clone(), Source::Elsewhere)?;
        }
        Ok(())
    }
}

impl<C: HasWorkbench + 'static> UndoStep<C> for RenameStep {
    fn undo(&mut self, context: &mut C) -> Result<(), BoxError> {
        self.set(context, true)
    }

    fn redo(&mut self, context: &mut C) -> Result<(), BoxError> {
        self.set(context, false)
    }
}

/// `symbol.rename`: renames a name everywhere it is written.
struct RenameCommand;

impl<C: HasWorkbench + 'static> Command<C> for RenameCommand {
    const ID: &'static str = RENAME;
    const SUMMARY: &'static str = "Renames the name at a place everywhere it is written, in the \
        editor's text of each file; nothing is saved.";
    type Params = RenameParams;
    type Output = RenameDone;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: RenameParams,
    ) -> Result<Outcome<RenameDone, C>, WorkbenchError> {
        let renamed = context
            .workbench()
            .rename(&params.path, params.offset, &params.name)?;
        let done = RenameDone {
            files: renamed.changes.iter().map(|c| c.path.clone()).collect(),
            caution: renamed.caution.map(|caution| match caution {
                Caution::MapFunction(name) | Caution::MapField(name) => name,
            }),
        };
        Ok(Outcome::undoable(
            done,
            RenameStep {
                changes: renamed.changes,
            },
        ))
    }
}
