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
//! L5 commands. It depends on `qs-commands` (L5), `qc-codegen` (L4), `qc-ide` and `qc-sema`
//! (L3), `qc-project` (L2), and `qc-progs` (L0).

mod build;
mod check;
pub mod checkpoints;
mod commands;
mod editing;
pub mod entity_classes;
pub mod export;
pub mod files_changed;
pub mod full_build;
pub mod git;
pub mod ids;
pub mod map_balance;
pub mod map_budget;
pub mod map_check;
pub mod map_compile;
pub mod map_filter;
pub mod map_logic;
pub mod map_playable;
pub mod map_profile;
pub mod models;
pub mod next_edit;
pub mod open_paths;
mod preview;
mod recent;
pub mod reference;
pub mod search;
pub mod staged;
pub mod terminal;
pub mod testing;
#[cfg(test)]
mod tests;
pub mod tools;
pub mod vm_run;

use qs_map::qbsp::QbspError;
use qs_wad::write::WadWriteError;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use qc_ide::AnalysisHost;
pub use qc_ide::Caution;
pub use qc_ide::{
    Code, Completion, Config, DefKind, Diagnostic, FileEdit, FileRange, Highlight, HighlightKind,
    Hover, Modifier, Modifiers, RenameError, Severity, Span, Symbol, SymbolKind, SyntaxClass,
};
pub use qc_project::{Dialect, FsLoader, Loader, Target};
// The map seam's types, for the workbench's own modules; dependants import them from
// qs_map_edit.
use qs_map_edit::{
    EntityCheck, EntityIndex, EntityProblem, GameFolders, HasMap, LiveMap, MapEditError,
    MapProject, MapStore,
};

pub use crate::build::{Gravity, ModuleBuilt, Reported};
pub use crate::commands::*;
pub use crate::editing::{Change, Renamed};
pub use crate::preview::{
    BREAKPOINT, BreakpointParams, CONSOLE, ConsoleParams, DOCK, SCENARIO_RUN, STEP_BY,
    ScenarioRunParams, StepBy, StepByParams, UNWATCH, UnwatchParams, WATCH, WATCH_NAMES, WatchName,
    WatchParams, debug, dock, engine, game_files, scenario,
};
pub use crate::preview::{
    CHOOSE_GAME, ChooseGameParams, ConsoleLine, FieldSet, GAME, GameStart, HasPreview, HeldEdit,
    OnLoss, PAUSE, PICK, PLAY, POLL, PickParams, PlayParams, PlayState, Preview, PreviewConfig,
    PreviewError, PreviewStatus, RELOAD, RESTART, RESUME, ReloadParams, SELECT, SET_FIELD, STEP,
    STOP, SelectParams, SelectedEntity, SendLive, SetFieldParams, Stopped, entities,
    register_preview,
};
pub use crate::recent::{RECENT_LIMIT, RecentProjects, recent_path};
pub use crate::staged::{
    ACCEPT_HUNKS, APPLY_EDIT, Decision, HunkId, REJECT_HUNKS, STAGED_EDITS, StagedHunk, Waiting,
    register_staged,
};
pub use crate::terminal::{
    TASK_LIST, TASK_RUN, TERMINAL_CLOSE, TERMINAL_NEW, TERMINAL_POLL, TERMINAL_SEND,
    register_terminal,
};
pub use crate::testing::{
    RUN_TESTS, RunTestsParams, TestKind, TestResult, TestRun, Verdict, register_tests,
};
pub use crate::vm_run::{VM_RUN, VmRunParams, register_vm};
use qc_project::{ProjectError, normalize};
use serde::{Deserialize, Serialize};

/// A place in a file as a person reads it: a line and a column, both from 1, the column
/// counting characters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
        let breaks = before
            .iter()
            .map(|&b| usize::from(b == b'\n'))
            .sum::<usize>();
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

/// The file open in the editor: its current text, where its caret goes, its outline and its
/// semantic highlighting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenFile {
    path: PathBuf,
    text: Vec<u8>,
    offset: usize,
    generation: u64,
    version: u64,
    revision: u64,
    outline: Vec<Symbol>,
    highlights: Vec<Highlight>,
    classes: Vec<(Span, SyntaxClass)>,
    folds: Vec<Span>,
}

impl OpenFile {
    /// The file, as the project graph names it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its current text: as it was opened, then as each edit left it.
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

    /// Changes when the text changes other than through the editor's own `file.update`: a
    /// rename, or its undo, so the editor takes the new text.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// The symbols the file defines, in source order.
    #[must_use]
    pub fn outline(&self) -> &[Symbol] {
        &self.outline
    }

    /// The definition byte `offset` is in, as the breadcrumb names it: the last whose name is
    /// written at or before it. A QuakeC file is a run of top-level definitions, so the text
    /// after one name and before the next belongs to the first.
    #[must_use]
    pub fn symbol_at(&self, offset: usize) -> Option<&Symbol> {
        self.outline
            .iter()
            .take_while(|symbol| symbol.range.span.start <= offset)
            .last()
    }

    /// The semantic highlighting of its names, in source order.
    #[must_use]
    pub fn highlights(&self) -> &[Highlight] {
        &self.highlights
    }

    /// Its keywords, literals and comments, in source order.
    #[must_use]
    pub fn classes(&self) -> &[(Span, SyntaxClass)] {
        &self.classes
    }

    /// The spans an editor folds: the blocks and block comments over several lines, outer
    /// before inner (P4-32).
    #[must_use]
    pub fn folds(&self) -> &[Span] {
        &self.folds
    }

    /// Changes whenever its text or what was worked out from it does, from any source.
    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

/// Where the check after an edit from the editor runs (P3-35).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CheckAfterTyping {
    /// On a thread of its own, its results taken by [`Workbench::take_checked`]: the frame
    /// only sets the text. The app's choice.
    #[default]
    OffTheFrame,
    /// In the edit, before it returns: for tests and tools that drive frames by hand.
    OnTheFrame,
}

/// The open project and file, over the analysis of the project's modules. What the panels
/// list is worked out when the project or the file is opened, so a frame only reads it.
#[derive(Debug)]
pub struct Workbench<L> {
    host: AnalysisHost<L>,
    /// The program `preview.breakpoint` last looked a line up in (BRIDGE-4.9).
    line_program: Option<preview::LineProgram>,
    root: Option<PathBuf>,
    tree: Vec<TreeEntry>,
    problems: Vec<Problem>,
    symbols: Vec<Symbol>,
    open: Option<OpenFile>,
    /// The files opened since the project was, in the order they were first opened: the
    /// editor's tabs.
    tabs: Vec<PathBuf>,
    /// The tabs' files, the one opened most recently first.
    history: Vec<PathBuf>,
    /// The files whose text an editor or a command changed since they were last saved.
    unsaved: BTreeSet<PathBuf>,
    generation: u64,
    /// The check after typing, off the frame, when the workbench was made with one (P3-35).
    checker: Option<check::Checker>,
    /// The generation of the last text the checker was sent, whose result is the one to take.
    sent: u64,
    /// The staged edits waiting for a decision (P4-04).
    staged: staged::Staged,
    /// The edits the editor's last one implies (P4-11).
    next_edits: next_edit::NextEdits,
    /// The terminal tabs (P4-36).
    terminals: terminal::Terminals,
    /// The map open in the map view (P5-06).
    maps: MapStore,
    /// The compile profile chosen, which `map.run_profile` runs when given none (P6-20).
    profile: Option<String>,
    /// What the last `paths.open` opened, until the window takes it.
    opened_paths: Option<open_paths::PathsOpened>,
    /// The issues `map.logic_issues` last found, which `map.issues` lists (FIRE-5.3).
    logic: map_logic::LogicFound,
    /// What `map.playability` last found, for `map.issues` (L-77).
    playable: map_playable::PlayFound,
    /// The issues `map.budget` last found, which `map.issues` lists (BUDGET-3.3).
    budget: map_budget::BudgetFound,
    /// A budget being made on its own thread (BUDGET-1).
    budget_run: Option<map_budget::BudgetRun>,
    /// A `map.playability` search running on its own thread (L-77).
    play_run: Option<map_playable::PlayRun>,
    /// The whole open map as it was last compiled, which `map.playability` reads again.
    last_compile: Option<map_playable::LastCompile>,
    /// The world the open `.map` was last compiled to for spawning (FIRE-5.4).
    spawn_world: map_filter::SpawnWorld,
}

impl<L: Loader> Workbench<L> {
    /// A workbench with no project, reading files through `loader` and analysing them as
    /// `config` says over the project file.
    pub fn new(loader: L, config: Config) -> Self {
        Self {
            host: AnalysisHost::new(loader, config),
            line_program: None,
            root: None,
            tree: Vec::new(),
            problems: Vec::new(),
            symbols: Vec::new(),
            open: None,
            tabs: Vec::new(),
            history: Vec::new(),
            unsaved: BTreeSet::new(),
            generation: 0,
            checker: None,
            sent: 0,
            staged: staged::Staged::default(),
            next_edits: next_edit::NextEdits::default(),
            terminals: terminal::Terminals::default(),
            maps: MapStore::default(),
            profile: None,
            opened_paths: None,
            logic: map_logic::LogicFound::default(),
            playable: map_playable::PlayFound::default(),
            budget: map_budget::BudgetFound::default(),
            budget_run: None,
            play_run: None,
            last_compile: None,
            spawn_world: map_filter::SpawnWorld::default(),
        }
    }

    /// A workbench that checks an edit from the editor as `check` says (P3-35).
    pub fn with_check(loader: L, config: Config, check: CheckAfterTyping) -> Self
    where
        L: Clone + Send + 'static,
    {
        match check {
            CheckAfterTyping::OffTheFrame => Self::checked_in_background(loader, config),
            CheckAfterTyping::OnTheFrame => Self::new(loader, config),
        }
    }

    /// A workbench whose check after typing runs off the frame (P3-35): an edit from the
    /// editor sets the text at once, and the problems, symbols and the open file's outline and
    /// highlighting follow when [`Workbench::take_checked`] finds them worked out on another
    /// thread, which reads files through its own copy of `loader`.
    pub fn checked_in_background(loader: L, config: Config) -> Self
    where
        L: Clone + Send + 'static,
    {
        let checker = check::Checker::spawn(loader.clone(), config);
        let mut workbench = Self::new(loader, config);
        workbench.checker = Some(checker);
        workbench
    }

    /// Takes the background check's newest result, if one has arrived for the latest edit:
    /// the problems, the symbols and the open file's outline and highlighting it worked out.
    /// Returns whether anything changed. A result for an older edit is dropped, a newer one
    /// being on its way.
    pub fn take_checked(&mut self) -> bool {
        let Some(checked) = self.checker.as_ref().and_then(check::Checker::take) else {
            return false;
        };
        self.apply(checked)
    }

    /// Waits for the background check to finish the latest edit and takes its result, as
    /// [`Workbench::take_checked`] does; for tests and benchmarks, never the frame. `false`
    /// with no background check.
    pub fn wait_checked(&mut self) -> bool {
        let Some(checker) = &self.checker else {
            return false;
        };
        while let Some(checked) = checker.wait() {
            if checked.generation == self.sent {
                return self.apply(checked);
            }
        }
        false
    }

    fn apply(&mut self, checked: check::Checked) -> bool {
        if checked.generation != self.sent {
            return false;
        }
        self.problems = checked.problems;
        self.symbols = checked.symbols;
        // The edit has been checked: the typing has paused long enough to ask the rename.
        self.settle_next_edit();
        if let (Some(open), Some(file)) = (self.open.as_mut(), checked.file)
            && open.path == file.path
        {
            open.outline = file.outline;
            open.highlights = file.highlights;
            open.classes = file.classes;
            open.folds = file.folds;
            self.generation = self.generation.wrapping_add(1);
            open.revision = self.generation;
        }
        true
    }

    /// Replaces what the editor says over the project file; it takes effect when a project is
    /// next opened.
    pub fn set_config(&mut self, config: Config) {
        self.host.set_config(config);
        if let Some(checker) = &self.checker {
            checker.config(config);
        }
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
        if let Some(checker) = &self.checker {
            checker.open(&root);
        }
        self.tree = tree(&self.host.files(), &root);
        self.root = Some(root);
        self.problems = problems(&mut self.host);
        self.symbols = self.host.workspace_symbols();
        self.open = None;
        self.tabs.clear();
        self.history.clear();
        self.unsaved.clear();
        self.next_edits.forget();
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
        let highlights = self.host.highlights(&path);
        let classes = self.host.syntax_classes(&path);
        let folds = self.host.folding_ranges(&path);
        let open = OpenFile {
            path,
            text,
            offset,
            generation: self.generation,
            version: self.generation,
            revision: self.generation,
            outline,
            highlights,
            classes,
            folds,
        };
        let location = open.location();
        if !self.tabs.contains(&open.path) {
            self.tabs.push(open.path.clone());
        }
        self.history.retain(|path| *path != open.path);
        self.history.insert(0, open.path.clone());
        self.open = Some(open);
        Ok(location)
    }

    /// Closes the tab of `path`. When it is the open file, the tab open before it opens again,
    /// as Zed's default `activate_on_close: "history"` does, or no file when it was the only
    /// one. Its unsaved text is kept: opening it again shows it.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoFile`] when `path` has no tab, or the tab to open next cannot be
    /// read.
    pub fn close_file(&mut self, path: &Path) -> Result<(), WorkbenchError> {
        let path = normalize(path);
        let at = self
            .tabs
            .iter()
            .position(|tab| *tab == path)
            .ok_or_else(|| WorkbenchError::NoFile { path: path.clone() })?;
        self.tabs.remove(at);
        self.history.retain(|tab| *tab != path);
        if self.open.as_ref().is_some_and(|open| open.path == path) {
            self.open = None;
            if let Some(next) = self.history.first().cloned() {
                self.open_file(&next, 0)?;
            }
        }
        Ok(())
    }

    /// The editor's tabs: the files opened since the project was, first opened first.
    #[must_use]
    pub fn tabs(&self) -> &[PathBuf] {
        &self.tabs
    }

    /// The tabs' files, the one opened most recently first: the order the file finder lists
    /// them in before anything is typed.
    #[must_use]
    pub fn history(&self) -> &[PathBuf] {
        &self.history
    }

    /// Whether the text of `path` changed since it was last saved.
    #[must_use]
    pub fn is_unsaved(&self, path: &Path) -> bool {
        self.unsaved.contains(path)
    }

    /// The terminal tabs (P4-36).
    #[must_use]
    pub const fn terminals(&self) -> &terminal::Terminals {
        &self.terminals
    }

    /// The terminal tabs, to poll or write to.
    pub const fn terminals_mut(&mut self) -> &mut terminal::Terminals {
        &mut self.terminals
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

    /// The engine the project's first module targets (ENGT-3.5), `None` with no project.
    pub fn engine(&mut self) -> Option<qc_engines::Engine> {
        self.host
            .project_modules()
            .first()
            .map(|module| module.engine)
    }
}

/// `files` as a tree under `root`, as [`Workbench::tree`] lists it.
pub(crate) fn tree(files: &[PathBuf], root: &Path) -> Vec<TreeEntry> {
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
pub(crate) fn problems<L: Loader>(host: &mut AnalysisHost<L>) -> Vec<Problem> {
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

impl<L: Loader> Workbench<L> {
    /// The map open in the map view.
    #[must_use]
    pub const fn map(&self) -> Option<&qs_map_edit::MapDocument> {
        self.maps.map()
    }

    /// The store of the map open in the map view, to open, change or save it.
    pub const fn maps_mut(&mut self) -> &mut MapStore {
        &mut self.maps
    }
}

/// Why the workbench could not open, change or save a project or a file.
#[derive(Debug)]
pub enum WorkbenchError {
    /// A checkpoint could not be made, listed or restored (P4-05).
    Checkpoint(qs_checkpoint::CheckpointError),
    /// A rename was refused.
    Rename(RenameError),
    /// The file could not be written.
    Save {
        /// The file.
        path: PathBuf,
        /// Why.
        error: std::io::Error,
    },
    /// The project file could not be read or is invalid.
    Project(ProjectError),
    /// The file could not be read.
    NoFile {
        /// The file.
        path: PathBuf,
    },
    /// No project is open.
    NoProject,
    /// There is no quick fix at the place.
    NoFix {
        /// The file.
        path: PathBuf,
        /// The byte offset.
        offset: usize,
    },
    /// A staged hunk's number is not one waiting for a decision.
    NoHunk {
        /// The number.
        id: staged::HunkId,
    },
    /// A search's pattern is not a regular expression (P4-33).
    Pattern {
        /// The pattern.
        pattern: String,
        /// Why it is not one.
        reason: String,
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
    /// A range that ends before it starts.
    Reversed {
        /// The file.
        path: PathBuf,
        /// The range's start.
        start: usize,
        /// The range's end, before its start.
        end: usize,
    },
    /// No map is open in the map view.
    NoMap,
    /// A `.map` that does not parse, or a key it cannot hold.
    Map {
        /// The file.
        path: PathBuf,
        /// Why.
        error: qs_map::MapError,
    },
    /// The project's server QuakeC does not compile to a program to spawn a map with.
    NotBuilt,
    /// A WAD could not be read (P6-16, WAD-2).
    Wad {
        /// The WAD.
        path: PathBuf,
        /// Why.
        error: qs_wad::WadError,
    },
    /// A WAD could not be written (P6-16, WAD-5).
    WadWrite {
        /// The WAD.
        path: PathBuf,
        /// Why.
        error: WadWriteError,
    },
    /// The open map is a BSP with no `.map` source, so there is nothing to compile (P6-13).
    NoMapSource {
        /// The BSP.
        path: PathBuf,
    },
    /// The project's compile profiles file could not be read (P6-20).
    ProfilesRead {
        /// The file.
        path: PathBuf,
        /// Why.
        error: std::io::Error,
    },
    /// The project's compile profiles file is not a list of profiles (P6-20).
    ProfilesParse {
        /// The file.
        path: PathBuf,
        /// Why.
        error: toml::de::Error,
    },
    /// The open map's portal file is not one vis reads (P6-20).
    PortalFile {
        /// The file.
        path: PathBuf,
        /// Why.
        error: qs_map_vis::VisError,
    },
    /// No compile profile has this name (P6-20).
    UnknownProfile {
        /// The name.
        name: String,
    },
    /// The map compiler stopped (P6-13, QBSP-1).
    MapCompile {
        /// Why.
        error: qs_map_bsp::CompileError,
    },
    /// The light compiler stopped (P6-12, LIGHT-1).
    MapLight {
        /// Why.
        error: qs_map_light::LightError,
    },
    /// The open map's entity lump cannot be written as qbsp writes it.
    MapLump {
        /// The map.
        path: PathBuf,
        /// Why.
        error: QbspError,
    },
    /// The open map's entities do not spawn with the project's program.
    Spawn {
        /// The map.
        path: PathBuf,
        /// Why.
        error: qs_sim::WorldError,
    },
    /// The game did not keep the entity at the skill and in the kind of game given, so it
    /// cannot be fired (FIRE-5.1).
    NotSpawned {
        /// The entity.
        entity: EntityIndex,
    },
    /// The open map's playability could not be checked (L-77).
    Playability {
        /// Why.
        error: map_playable::PlayError,
    },
    /// The entity cannot be fired (FIRE-1.3).
    NotFired {
        /// The entity.
        entity: EntityIndex,
        /// Why.
        error: qs_sim::FireError,
    },
    /// A map command could not open, change or save the open map.
    MapEdit(MapEditError),
    /// A region compile's box has no volume, or its rooms no brushes (MAP-16.1).
    NoRegion,
    /// A region compile names an entity that is in no room (MAP-16.5).
    NotInRoom {
        /// The entity.
        entity: EntityIndex,
    },
    /// Play from here with a region puts the player outside it (MAP-16.5).
    EyeOutsideRegion,
    /// The map is played from its own start and has none (L-52).
    NoPlayerStart,
    /// The map is played from its own start, in a region the start is outside (L-52).
    StartOutsideRegion,
    /// A region's sealing brushes could not be written (MAP-16.3).
    RegionSeal(qs_map::BrushError),
}

impl fmt::Display for WorkbenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Checkpoint(error) => error.fmt(f),
            Self::Rename(error) => error.fmt(f),
            Self::Save { path, error } => {
                write!(f, "{}: cannot be written: {error}", path.display())
            }
            Self::Project(error) => error.fmt(f),
            Self::NoFile { path } => write!(f, "{}: cannot be read", path.display()),
            Self::NoProject => f.write_str("no project is open"),
            Self::NoFix { path, offset } => {
                write!(f, "{}: no quick fix at byte {offset}", path.display())
            }
            Self::NoHunk { id } => write!(f, "{id} is not staged"),
            Self::Pattern { pattern, reason } => {
                write!(f, "{pattern}: not a regular expression: {reason}")
            }
            Self::Offset { path, offset, len } => write!(
                f,
                "{}: offset {offset} is past the end of the file, {len} bytes",
                path.display()
            ),
            Self::Reversed { path, start, end } => write!(
                f,
                "{}: the range ends at {end}, before its start at {start}",
                path.display()
            ),
            Self::NoMap => f.write_str("no map is open"),
            Self::Map { path, error } => write!(f, "{}: {error}", path.display()),
            Self::NotBuilt => f.write_str("the project's server QuakeC does not compile"),
            Self::Wad { path, error } => write!(f, "{}: {error}", path.display()),
            Self::WadWrite { path, error } => write!(f, "{}: {error}", path.display()),
            Self::NoMapSource { path } => {
                write!(
                    f,
                    "{}: a BSP with no .map source to compile",
                    path.display()
                )
            }
            Self::ProfilesRead { path, error } => {
                write!(f, "cannot read {}: {error}", path.display())
            }
            Self::ProfilesParse { path, error } => {
                write!(
                    f,
                    "{} is not a list of compile profiles: {error}",
                    path.display()
                )
            }
            Self::PortalFile { path, error } => write!(f, "{}: {error}", path.display()),
            Self::UnknownProfile { name } => write!(f, "no compile profile is called {name:?}"),
            Self::MapCompile { error } => write!(f, "the map does not compile: {error}"),
            Self::MapLight { error } => write!(f, "the map does not light: {error}"),
            Self::MapLump { path, error } => write!(f, "{}: {error}", path.display()),
            Self::Spawn { path, error } => write!(f, "{}: {error}", path.display()),
            Self::NotSpawned { entity } => write!(
                f,
                "the game does not keep entity {} at that skill and in that kind of game",
                entity.get()
            ),
            Self::NotFired { entity, error } => {
                write!(f, "entity {} cannot be fired: {error}", entity.get())
            }
            Self::Playability { error } => write!(f, "the map cannot be checked: {error}"),
            Self::MapEdit(error) => error.fmt(f),
            Self::NoRegion => f.write_str("the region has no volume"),
            Self::NotInRoom { entity } => write!(
                f,
                "entity {} is in no layer or group, so it names no room",
                entity.get()
            ),
            Self::EyeOutsideRegion => {
                f.write_str("the camera is outside the region: play from inside it")
            }
            Self::StartOutsideRegion => f.write_str(
                "the map's player start is outside the region: play from the camera, inside it",
            ),
            Self::NoPlayerStart => f.write_str(
                "the map has no player start: place an info_player_start, or play from the camera",
            ),
            Self::RegionSeal(error) => write!(f, "the region's seal: {error}"),
        }
    }
}

impl Error for WorkbenchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Project(error) => Some(error),
            Self::MapEdit(error) => error.source(),
            Self::Rename(error) => Some(error),
            Self::Save { error, .. } => Some(error),
            Self::Checkpoint(error) => Some(error),
            Self::Map { error, .. } => Some(error),
            Self::NoFile { .. }
            | Self::NoProject
            | Self::NoFix { .. }
            | Self::NoHunk { .. }
            | Self::Pattern { .. }
            | Self::Offset { .. }
            | Self::Reversed { .. }
            | Self::NoMap
            | Self::NotBuilt => None,
            Self::Wad { error, .. } => Some(error),
            Self::WadWrite { error, .. } => Some(error),
            Self::NoMapSource { .. } => None,
            Self::ProfilesRead { error, .. } => Some(error),
            Self::ProfilesParse { error, .. } => Some(error),
            Self::PortalFile { error, .. } => Some(error),
            Self::UnknownProfile { .. } => None,
            Self::MapCompile { error } => Some(error),
            Self::MapLight { error } => Some(error),
            Self::MapLump { error, .. } => Some(error),
            Self::Spawn { error, .. } => Some(error),
            Self::NotFired { error, .. } => Some(error),
            Self::Playability { error } => Some(error),
            Self::NotSpawned { .. } => None,
            Self::NoRegion
            | Self::NotInRoom { .. }
            | Self::EyeOutsideRegion
            | Self::NoPlayerStart
            | Self::StartOutsideRegion => None,
            Self::RegionSeal(error) => Some(error),
        }
    }
}

impl From<MapEditError> for WorkbenchError {
    fn from(error: MapEditError) -> Self {
        Self::MapEdit(error)
    }
}
