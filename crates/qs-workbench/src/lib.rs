// SPDX-License-Identifier: GPL-3.0-or-later

//! The workbench (UI-2, P2-19): the open project and the open file, what the Project, Symbols
//! and Problems panels list, and the commands that open a project or a file, so the panels,
//! the palette, the CLI and the agent open them the same way.
//!
//! The analysis is `qc-ide`'s [`AnalysisHost`]; this crate turns its answers into what a panel
//! lists: a file tree relative to the project, symbols, and each diagnostic with its line and
//! column.
//!
//! # Layer
//!
//! L5 commands. It depends on `qs-commands` (L5), `qc-ide` (L3) and `qc-project` (L2).

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use qc_ide::AnalysisHost;
pub use qc_ide::{Code, Config, Diagnostic, Severity, Symbol, SymbolKind};
pub use qc_project::{Dialect, FsLoader, Target};
use qc_project::{Loader, ProjectError, normalize};
use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A place in a file as a person reads it: a line and a column, both from 1, the column
/// counting characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Location {
    line: u32,
    column: u32,
}

impl Location {
    /// The location of byte `offset` of `text`; an offset past the end is the end. Only LF
    /// ends a line, as the lexer reads it (QC-1.2), and text that is not UTF-8 counts one
    /// character for each replacement character a lossy decoding gives it, as the editor shows
    /// it.
    #[must_use]
    pub fn of(text: &[u8], offset: usize) -> Self {
        let before = text.get(..offset).unwrap_or(text);
        let start = before
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |newline| newline.saturating_add(1));
        let breaks = before.iter().filter(|&&b| b == b'\n').count();
        let chars = String::from_utf8_lossy(before.get(start..).unwrap_or_default())
            .chars()
            .count();
        Self {
            line: one_based(breaks),
            column: one_based(chars),
        }
    }

    /// The line, from 1.
    #[must_use]
    pub const fn line(self) -> u32 {
        self.line
    }

    /// The column, from 1, in characters.
    #[must_use]
    pub const fn column(self) -> u32 {
        self.column
    }
}

/// `count` plus one, as a `u32`, saturating at its largest value.
fn one_based(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX).saturating_add(1)
}

/// One problem the Problems panel lists: a diagnostic, the file it is in and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The file, as the project graph names it.
    pub path: PathBuf,
    /// Where its span starts.
    pub location: Location,
    /// The diagnostic.
    pub diagnostic: Diagnostic,
}

/// What a row of the file tree is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    /// A directory, whose entries follow it one level deeper.
    Directory,
    /// A file the project reads, with its path as the project graph names it.
    File(PathBuf),
}

/// One row of the file tree, in the order the tree is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntry {
    /// How deep it is: 0 for an entry of the project directory itself.
    pub depth: usize,
    /// The directory's or the file's name.
    pub name: String,
    /// Whether it is a directory or a file.
    pub kind: EntryKind,
}

/// A directory of the tree as it is built: its subdirectories and its files, by name.
#[derive(Default)]
struct Node {
    directories: BTreeMap<String, Node>,
    files: BTreeMap<String, PathBuf>,
}

impl Node {
    /// Adds `path`, whose components below the tree's root are `names`.
    fn insert(&mut self, names: &[String], path: PathBuf) {
        match names {
            [] => {}
            [name] => {
                self.files.insert(name.clone(), path);
            }
            [directory, rest @ ..] => self
                .directories
                .entry(directory.clone())
                .or_default()
                .insert(rest, path),
        }
    }

    /// Appends this directory's rows at `depth` to `out`: its directories, each followed by
    /// its own rows, then its files, each group by name.
    fn flatten(self, depth: usize, out: &mut Vec<TreeEntry>) {
        for (name, node) in self.directories {
            out.push(TreeEntry {
                depth,
                name,
                kind: EntryKind::Directory,
            });
            node.flatten(depth.saturating_add(1), out);
        }
        for (name, path) in self.files {
            out.push(TreeEntry {
                depth,
                name,
                kind: EntryKind::File(path),
            });
        }
    }
}

/// The file open in the editor: its text as it was opened, where its caret goes and its
/// outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenFile {
    path: PathBuf,
    text: Vec<u8>,
    offset: usize,
    generation: u64,
    outline: Vec<Symbol>,
}

impl OpenFile {
    /// The file, as the project graph names it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its text when it was opened.
    #[must_use]
    pub fn text(&self) -> &[u8] {
        &self.text
    }

    /// The byte offset the caret goes to.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.offset
    }

    /// Where the caret goes.
    #[must_use]
    pub fn location(&self) -> Location {
        Location::of(&self.text, self.offset)
    }

    /// Changes on every `file.open`, including one of the file already open, so the editor
    /// moves its caret again when the same problem is chosen twice.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// The symbols the file defines, in source order.
    #[must_use]
    pub fn outline(&self) -> &[Symbol] {
        &self.outline
    }
}

/// The open project and file, over the analysis of the project's modules. What the panels
/// list is worked out when the project or the file is opened, so a frame only reads it.
#[derive(Debug)]
pub struct Workbench<L> {
    host: AnalysisHost<L>,
    root: Option<PathBuf>,
    tree: Vec<TreeEntry>,
    problems: Vec<Problem>,
    symbols: Vec<Symbol>,
    open: Option<OpenFile>,
    generation: u64,
}

impl<L: Loader> Workbench<L> {
    /// A workbench with no project, reading files through `loader` and analysing them as
    /// `config` says over the project file.
    pub fn new(loader: L, config: Config) -> Self {
        Self {
            host: AnalysisHost::new(loader, config),
            root: None,
            tree: Vec::new(),
            problems: Vec::new(),
            symbols: Vec::new(),
            open: None,
            generation: 0,
        }
    }

    /// Replaces what the editor says over the project file; it takes effect when a project is
    /// next opened.
    pub fn set_config(&mut self, config: Config) {
        self.host.set_config(config);
    }

    /// Opens the project in directory `root`, as [`AnalysisHost::open`] does, works out its
    /// tree, problems and symbols, and closes the open file.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::Project`] when the project file cannot be read or is invalid. The
    /// project open before, if any, is then read again, so the workbench stays as it was.
    pub fn open_project(&mut self, root: &Path) -> Result<(), WorkbenchError> {
        if let Err(error) = self.host.open(root) {
            if let Some(previous) = &self.root
                && let Err(again) = self.host.open(previous)
            {
                tracing::warn!(%again, "the project open before could not be read again");
            }
            return Err(WorkbenchError::Project(error));
        }
        let root = normalize(root);
        self.tree = tree(&self.host.files(), &root);
        self.root = Some(root);
        self.problems = problems(&mut self.host);
        self.symbols = self.host.workspace_symbols();
        self.open = None;
        Ok(())
    }

    /// Opens `path` with its caret at byte `offset`, and returns where that is.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoFile`] when the file cannot be read, and
    /// [`WorkbenchError::Offset`] when `offset` is past its end.
    pub fn open_file(&mut self, path: &Path, offset: usize) -> Result<Location, WorkbenchError> {
        let path = normalize(path);
        let text = self
            .host
            .text(&path)
            .ok_or_else(|| WorkbenchError::NoFile { path: path.clone() })?;
        if offset > text.len() {
            return Err(WorkbenchError::Offset {
                path,
                offset,
                len: text.len(),
            });
        }
        self.generation = self.generation.wrapping_add(1);
        let outline = self.host.document_symbols(&path);
        let open = OpenFile {
            path,
            text,
            offset,
            generation: self.generation,
            outline,
        };
        let location = open.location();
        self.open = Some(open);
        Ok(location)
    }

    /// The project directory, once a project is open.
    #[must_use]
    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    /// The open file, if any.
    #[must_use]
    pub const fn open(&self) -> Option<&OpenFile> {
        self.open.as_ref()
    }

    /// The QuakeC files the project compiles, as a tree under the project directory:
    /// directories before files, each by name. A file outside the project directory is placed
    /// by its whole path.
    #[must_use]
    pub fn tree(&self) -> &[TreeEntry] {
        &self.tree
    }

    /// Every problem in the project: each file's diagnostics, the files by path and each
    /// file's in source order.
    #[must_use]
    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    /// The symbols every file of the project defines.
    #[must_use]
    pub fn project_symbols(&self) -> &[Symbol] {
        &self.symbols
    }
}

/// `files` as a tree under `root`, as [`Workbench::tree`] lists it.
fn tree(files: &[PathBuf], root: &Path) -> Vec<TreeEntry> {
    let mut top = Node::default();
    for path in files {
        let below = path.strip_prefix(root).unwrap_or(path);
        let names: Vec<String> = below
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        top.insert(&names, path.clone());
    }
    let mut out = Vec::new();
    top.flatten(0, &mut out);
    out
}

/// Every diagnostic `host` has, as [`Workbench::problems`] lists them.
fn problems<L: Loader>(host: &mut AnalysisHost<L>) -> Vec<Problem> {
    let mut out = Vec::new();
    for (path, diagnostics) in host.all_diagnostics() {
        let text = host.text(&path).unwrap_or_default();
        for diagnostic in diagnostics {
            out.push(Problem {
                path: path.clone(),
                location: Location::of(&text, diagnostic.span.start),
                diagnostic,
            });
        }
    }
    out
}

/// Why the workbench could not open a project or a file.
#[derive(Debug)]
pub enum WorkbenchError {
    /// The project file could not be read or is invalid.
    Project(ProjectError),
    /// The file could not be read.
    NoFile {
        /// The file.
        path: PathBuf,
    },
    /// The offset is past the end of the file.
    Offset {
        /// The file.
        path: PathBuf,
        /// The offset asked for.
        offset: usize,
        /// The file's length in bytes.
        len: usize,
    },
}

impl fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Project(error) => error.fmt(f),
            Self::NoFile { path } => write!(f, "{}: cannot be read", path.display()),
            Self::Offset { path, offset, len } => write!(
                f,
                "{}: offset {offset} is past the end of the file, {len} bytes",
                path.display()
            ),
        }
    }
}

impl Error for WorkbenchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Project(error) => Some(error),
            Self::NoFile { .. } | Self::Offset { .. } => None,
        }
    }
}

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

/// Registers the workbench commands.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub fn register<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(OpenProject)?;
    registry.register(OpenFileCommand)
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
