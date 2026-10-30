// SPDX-License-Identifier: GPL-3.0-or-later

//! The workbench commands: opening a project and a file, editing and saving a file, and the
//! language features over the open project.

use std::path::PathBuf;

use qc_project::{Loader, normalize};
use qs_commands::{BoxError, Command, Outcome, RegisterError, Registry, UndoStep};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::editing::{Change, Source};
use crate::{Caution, DefKind, EntryKind, FileRange, Location, Workbench, WorkbenchError};

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

/// The id of the command that opens a file.
pub const OPEN_FILE: &str = "file.open";

/// The id of the command that takes an editor's text of a file.
pub const UPDATE_FILE: &str = "file.update";

/// The id of the command that writes a file to disk.
pub const SAVE_FILE: &str = "file.save";

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
    registry.register(OpenFileCommand)?;
    registry.register(UpdateFile)?;
    registry.register(SaveFile)?;
    registry.register(Definition)?;
    registry.register(References)?;
    registry.register(HoverCommand)?;
    registry.register(Complete)?;
    registry.register(RenameCommand)
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
pub struct ProjectOpened {
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
pub struct FileOpened {
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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

/// The parameters of `file.save`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveFileParams {
    /// The file.
    pub path: PathBuf,
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// The name.
    pub label: String,
    /// What it is.
    pub kind: CandidateKind,
    /// Its type or declaration.
    pub detail: String,
}

/// What a completion candidate is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RenameDone {
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
