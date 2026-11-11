// SPDX-License-Identifier: GPL-3.0-or-later

//! The analysis host: a project's modules, each in its own semantic database, and the text of
//! the files an editor has open.
//!
//! rust-analyzer's `AnalysisHost` holds one salsa database for a whole workspace. A QuakeC
//! project is a set of modules (`progs.src` and `csprogs.src`, PRJ-2.4), each compiled on its
//! own in its own order (QC-23.3), and a file such as a shared `defs.qc` may be compiled by
//! several. So the host keeps one [`Db`] per module, keyed by the module's file graph, and a
//! file is in every module whose graph lists it. An open file's text replaces the text on disk
//! in every module that compiles it; a file no module compiles is analysed as a module of its
//! own, so a scratch file still gets answers.

use crate::actions::editor_diagnostics;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use qc_engines::{BuiltinRef, Engine};
use qc_project::{Dialect, Loader, ModuleKind, Project, ProjectError, Target, normalize};
use qc_sema::{Db, FileId, ModuleAnalysis, ModuleInput};
use qc_syntax::Parse;

/// What the editor or the user says about the project where its files say nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// The dialect a project with no `quakec.toml` is read in, vanilla otherwise as
    /// `qc_project::open` gives it, and an open file no module compiles; a project file's
    /// dialect, vanilla when it names none, wins (PRJ-2.8).
    pub dialect: Option<Dialect>,
    /// The engine a project with no `quakec.toml` targets, and an open file no module
    /// compiles: the engine whose builtin table the hover and `W0311` read; a project file's
    /// target wins (PRJ-2.9).
    pub target: Option<Target>,
    /// Whether modules resolve with FTEQCC's autoprototyping, `-Fautoproto` (FTE-22.3), which
    /// a project file cannot say.
    pub autoproto: bool,
}

/// One module: its files, its database and what was last derived from them.
#[derive(Debug)]
pub(crate) struct ModuleState {
    /// The module's kind, which selects the engine table (ENGT-3.11).
    pub(crate) kind: ModuleKind,
    /// The engine it targets.
    pub(crate) engine: Engine,
    /// The dialect it is read in.
    pub(crate) dialect: qc_syntax::Dialect,
    /// Its files, `FileId::new(i)` being `files[i]`, as the file graph orders them.
    pub(crate) files: Vec<PathBuf>,
    /// The files it preprocesses and compiles.
    input: ModuleInput,
    /// Whether it resolves with FTEQCC's autoprototyping (FTE-22.3).
    autoproto: bool,
    /// Where the module comes from.
    origin: Origin,
    /// Its incremental database.
    pub(crate) db: Db,
    /// What was derived from the files' current text, `None` after an edit.
    data: Option<ModuleData>,
    /// The project graph's diagnostics for the module (PRJ-6.5).
    pub(crate) graph_diagnostics: Vec<qc_project::Diagnostic>,
    /// Where a project module is built to, and the names its files are recorded by; `None`
    /// for an open file no module compiles.
    output: Option<Output>,
}

/// Where a project module's program goes (PRJ-3.2), and the directory its files are named from
/// in the progs, its `progs.src`'s (QC-25.4).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Output {
    name: String,
    path: PathBuf,
    base: PathBuf,
}

/// One project module as the build compiles it: its name, where its program goes, its
/// database with the editor's text of each open file, what it compiles, and the name each of
/// its files is recorded by in the progs, as its `progs.src` lists it (QC-25.4).
pub struct ModuleBuild<'a> {
    /// The module's name (PRJ-2.4).
    pub name: &'a str,
    /// Where its program goes (PRJ-3.2).
    pub output: &'a Path,
    /// Its database.
    pub db: &'a mut Db,
    /// The files it preprocesses and compiles.
    pub input: &'a ModuleInput,
    /// The name of each of its files, `names[i]` being `FileId::new(i)`'s.
    pub names: Vec<String>,
    /// The engine its target selects (ENGT-3.5), which its program must load (ENGT-3.12).
    pub engine: Engine,
    /// Its kind: server, client or menu QuakeC.
    pub kind: ModuleKind,
    /// The dialect it is read in.
    pub dialect: qc_syntax::Dialect,
}

/// Where a module comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// A module of the project.
    Project,
    /// An open file no module of the project compiles, analysed on its own.
    Unlisted,
}

/// What the features read of one module, derived once per edit.
#[derive(Debug)]
pub(crate) struct ModuleData {
    /// The module read as the compiler reads it, shared with the database's memo.
    pub(crate) analysis: Rc<ModuleAnalysis>,
    /// Each file's tree as written, `parses[i]` being `FileId::new(i)`'s.
    pub(crate) parses: Vec<Parse>,
    /// The builtin each global name is declared as (ENGT-4.12, ENGT-4.17).
    pub(crate) builtins: HashMap<String, BuiltinRef>,
    /// The `W0311` warnings for the module's target engine (ENGT-4).
    pub(crate) missing: Vec<qc_sema::FileDiagnostic>,
    /// The editor's own diagnostics: unused locals and missing precaches (QCD-7.115,
    /// QCD-7.116).
    pub(crate) editor: Vec<qc_sema::FileDiagnostic>,
}

impl ModuleState {
    /// The id of `path` in this module, if the module has it.
    pub(crate) fn file_id(&self, path: &Path) -> Option<FileId> {
        self.files.iter().position(|p| p == path).map(FileId::new)
    }

    /// The path of file `file`.
    pub(crate) fn path(&self, file: FileId) -> Option<&Path> {
        self.files.get(file.index()).map(PathBuf::as_path)
    }

    /// The derived data, computed if an edit invalidated it.
    fn ensure(&mut self) -> &ModuleData {
        let data = self.data.take().unwrap_or_else(|| {
            let analysis = self.db.analyze_module(&self.input);
            let parses: Vec<Parse> = (0..self.files.len())
                .map(|index| self.db.parse(FileId::new(index)))
                .collect();
            let builtins = qc_engines::declared_builtins(&parses, self.dialect);
            let files: Vec<(FileId, &Parse)> = parses
                .iter()
                .enumerate()
                .map(|(index, parse)| (FileId::new(index), parse))
                .collect();
            let missing = qc_engines::missing_builtins(
                self.engine,
                self.kind,
                &files,
                &analysis.index,
                self.dialect,
            );
            let editor = editor_diagnostics(&analysis.index, &parses);
            ModuleData {
                analysis,
                parses,
                builtins,
                missing,
                editor,
            }
        });
        self.data.insert(data)
    }

    /// The derived data, if computed since the last edit.
    pub(crate) fn data(&self) -> Option<&ModuleData> {
        self.data.as_ref()
    }

    /// The files it preprocesses and compiles.
    pub(crate) const fn input(&self) -> &ModuleInput {
        &self.input
    }

    /// Whether it resolves with FTEQCC's autoprototyping (FTE-22.3).
    pub(crate) const fn autoproto(&self) -> bool {
        self.autoproto
    }

    /// Sets the text of `file`, invalidating what was derived.
    fn set_text(&mut self, file: FileId, text: Vec<u8>) {
        if let Some(path) = self.path(file).map(Path::to_path_buf) {
            self.db.set_file(file, path, text);
            self.data = None;
        }
    }
}

/// What [`AnalysisHost::read_again`] keeps when the project no longer opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Keep {
    /// Nothing: the host has no project modules, as after a bad project file.
    Nothing,
    /// The modules as they were: an edit to a QuakeC file leaves the analysis alone.
    OnError,
}

/// The analysis host. Open a project with [`AnalysisHost::open`], give it the text of the files
/// an editor has open with [`AnalysisHost::set_overlay`], and ask it questions; each feature is
/// a method in its own module of this crate.
#[derive(Debug)]
pub struct AnalysisHost<L> {
    /// Reads files that are not open.
    loader: L,
    /// What the editor says over the project.
    config: Config,
    /// The project directory, once opened.
    root: Option<PathBuf>,
    /// The project's modules, then a module for each open file no module compiles.
    pub(crate) modules: Vec<ModuleState>,
    /// The text of each open file, by normalized path.
    overlays: HashMap<PathBuf, Vec<u8>>,
}

/// A loader that reads an open file's text before the disk.
struct OverlayLoader<'a, L> {
    overlays: &'a HashMap<PathBuf, Vec<u8>>,
    inner: &'a mut L,
}

impl<L: Loader> Loader for OverlayLoader<'_, L> {
    fn load(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        match self.overlays.get(&normalize(path)) {
            Some(text) => Ok(text.clone()),
            None => self.inner.load(path),
        }
    }

    fn stored_name(&mut self, path: &Path) -> Option<std::ffi::OsString> {
        self.inner.stored_name(path)
    }
}

impl<L: Loader> AnalysisHost<L> {
    /// A host with no project, reading files through `loader`.
    pub fn new(loader: L, config: Config) -> Self {
        Self {
            loader,
            config,
            root: None,
            modules: Vec::new(),
            overlays: HashMap::new(),
        }
    }

    /// Replaces what the editor says over the project; it takes effect when the project is
    /// next opened or read again.
    pub fn set_config(&mut self, config: Config) {
        self.config = config;
    }

    /// Opens the project in directory `root` (PRJ-1.1, PRJ-1.2): its `quakec.toml`, or a bare
    /// `progs.src` and `csprogs.src`. A directory that holds neither is no error: the host then
    /// analyses each open file on its own.
    ///
    /// # Errors
    ///
    /// [`ProjectError`] when the project file cannot be read or is invalid. The host then has
    /// no project modules.
    pub fn open(&mut self, root: &Path) -> Result<(), ProjectError> {
        self.root = Some(normalize(root));
        self.reload()
    }

    /// Reads the project again, as after a project file, a `progs.src` or an `#include` line
    /// changed, keeping the open files' text.
    ///
    /// # Errors
    ///
    /// As [`AnalysisHost::open`].
    ///
    /// A module whose files and settings are unchanged keeps its database, with the text of each
    /// file that is not open read again, so only what changed on disk is analysed again.
    pub fn reload(&mut self) -> Result<(), ProjectError> {
        self.read_again(Keep::Nothing)
    }

    /// [`AnalysisHost::reload`]; with [`Keep::OnError`] a project that no longer opens leaves the
    /// modules as they were, so an edit to a QuakeC file never loses the analysis.
    fn read_again(&mut self, keep: Keep) -> Result<(), ProjectError> {
        let opened = match self.root.clone() {
            Some(root) => {
                let mut loader = OverlayLoader {
                    overlays: &self.overlays,
                    inner: &mut self.loader,
                };
                match qc_project::open_dir(&mut loader, &root) {
                    Ok(project) => Ok(Some(project)),
                    Err(ProjectError::NoProject { .. }) => Ok(None),
                    Err(err) => Err(err),
                }
            }
            None => Ok(None),
        };
        if keep == Keep::OnError && opened.is_err() {
            return Ok(());
        }
        let mut old = std::mem::take(&mut self.modules);
        let result = match opened {
            Ok(Some(project)) => {
                self.add_project(&project, &mut old);
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(err) => Err(err),
        };
        let orphans: Vec<PathBuf> = self
            .overlays
            .keys()
            .filter(|path| !is_project_file(path) && !self.is_compiled(path))
            .cloned()
            .collect();
        for path in orphans {
            self.add_orphan(path);
        }
        result
    }

    /// Adds a module for each of `project`'s modules, taking from `old` the state of a module
    /// that is the same.
    fn add_project(&mut self, project: &Project, old: &mut Vec<ModuleState>) {
        // A project file says the dialect and the target, its defaults when it names neither;
        // the editor's say them only for a project with none.
        let bare = project.manifest.is_none();
        for module in &project.modules {
            let dialect = match self.config.dialect {
                Some(dialect) if bare => dialect,
                Some(_) | None => module.dialect,
            };
            let target = match self.config.target {
                Some(target) if bare => target,
                Some(_) | None => module.target,
            };
            let files: Vec<PathBuf> = module
                .graph
                .files
                .iter()
                .map(|f| normalize(&f.path))
                .collect();
            let input = ModuleInput::from_module(module);
            let engine = Engine::from_target(target);
            let same = old.iter().position(|m| {
                m.origin == Origin::Project
                    && m.kind == module.kind
                    && m.engine == engine
                    && m.dialect == qc_syntax::Dialect::from(dialect)
                    && m.autoproto == self.config.autoproto
                    && m.files == files
                    && m.input == input
            });
            let mut state = match same {
                Some(at) => {
                    let mut state = old.swap_remove(at);
                    self.refresh(&mut state);
                    state
                }
                None => {
                    self.module_state(module.kind, dialect, target, files, input, Origin::Project)
                }
            };
            state
                .graph_diagnostics
                .clone_from(&module.graph.diagnostics);
            state.output = Some(Output {
                name: module.name.clone(),
                path: module.output.clone(),
                base: module.src.parent().map_or_else(PathBuf::new, normalize),
            });
            self.modules.push(state);
        }
    }

    /// Adds the one-file module of `path`, a file no project module compiles.
    fn add_orphan(&mut self, path: PathBuf) {
        let dialect = self.config.dialect.unwrap_or(Dialect::Vanilla);
        let target = self.config.target.unwrap_or(Target::Quake);
        let input = ModuleInput {
            compiled: vec![FileId::new(0)],
            ..ModuleInput::default()
        };
        let state = self.module_state(
            ModuleKind::Ssqc,
            dialect,
            target,
            vec![path],
            input,
            Origin::Unlisted,
        );
        self.modules.push(state);
    }

    /// Reads again the text of each of a kept module's files that is not open, setting only
    /// what changed.
    fn refresh(&mut self, state: &mut ModuleState) {
        for index in 0..state.files.len() {
            let file = FileId::new(index);
            let Some(path) = state.path(file).map(Path::to_path_buf) else {
                continue;
            };
            let text = match self.overlays.get(&path) {
                Some(text) => text.clone(),
                None => self.loader.load(&path).unwrap_or_default(),
            };
            if state.db.file_text(file) != text.as_slice() {
                state.set_text(file, text);
            }
        }
    }

    /// A module's state with every file's text set, from an overlay or the loader.
    fn module_state(
        &mut self,
        kind: ModuleKind,
        dialect: Dialect,
        target: Target,
        files: Vec<PathBuf>,
        input: ModuleInput,
        origin: Origin,
    ) -> ModuleState {
        let mut db = Db::new();
        db.set_dialect(dialect.into());
        db.set_autoproto(self.config.autoproto);
        for (index, path) in files.iter().enumerate() {
            let text = match self.overlays.get(path) {
                Some(text) => Some(text.clone()),
                None => self.loader.load(path).ok(),
            };
            if let Some(text) = text {
                db.set_file(FileId::new(index), path.clone(), text);
            }
        }
        ModuleState {
            kind,
            engine: Engine::from_target(target),
            dialect: dialect.into(),
            files,
            input,
            autoproto: self.config.autoproto,
            origin,
            db,
            data: None,
            graph_diagnostics: Vec::new(),
            output: None,
        }
    }

    /// Whether a project module compiles `path`.
    fn is_compiled(&self, path: &Path) -> bool {
        self.modules
            .iter()
            .any(|m| m.origin == Origin::Project && m.file_id(path).is_some())
    }

    /// Sets the text of the open file `path`, which replaces its text on disk. Editing a
    /// project file or a `progs.src`, or a line of a QuakeC file that may be a directive
    /// (PRJ-5.1), such as an `#include`, reads the project again, so the module's file graph
    /// follows the text being typed.
    ///
    /// # Errors
    ///
    /// As [`AnalysisHost::reload`], when `path` is a project file whose new text does not
    /// open.
    pub fn set_overlay(&mut self, path: &Path, text: Vec<u8>) -> Result<(), ProjectError> {
        let path = normalize(path);
        let before = self.overlays.insert(path.clone(), text.clone());
        if is_project_file(&path) {
            return self.reload();
        }
        let before = before.or_else(|| self.loader.load(&path).ok());
        if before.is_some_and(|before| directive_lines(&before) != directive_lines(&text)) {
            return self.read_again(Keep::OnError);
        }
        let mut found = false;
        for module in &mut self.modules {
            if let Some(file) = module.file_id(&path) {
                module.set_text(file, text.clone());
                found = true;
            }
        }
        if !found {
            self.add_orphan(path);
        }
        Ok(())
    }

    /// Forgets the open text of `path`: the file is read from disk again, and a file no module
    /// compiles is no longer analysed.
    ///
    /// # Errors
    ///
    /// As [`AnalysisHost::reload`], when `path` is a project file whose text on disk does not
    /// open.
    pub fn remove_overlay(&mut self, path: &Path) -> Result<(), ProjectError> {
        let path = normalize(path);
        if self.overlays.remove(&path).is_none() {
            return Ok(());
        }
        self.modules
            .retain(|m| !(m.origin == Origin::Unlisted && m.files.first() == Some(&path)));
        if is_project_file(&path) {
            return self.reload();
        }
        let text = self.loader.load(&path).unwrap_or_default();
        // The open text the modules still hold, now the overlay is gone.
        let open = self.text(&path);
        if open.is_some_and(|open| directive_lines(&open) != directive_lines(&text)) {
            return self.read_again(Keep::OnError);
        }
        for module in &mut self.modules {
            if let Some(file) = module.file_id(&path) {
                module.set_text(file, text.clone());
            }
        }
        Ok(())
    }

    /// The current text of `path`: its open text, else its text in a module, else what the
    /// loader reads; `None` when none of them has it.
    pub fn text(&mut self, path: &Path) -> Option<Vec<u8>> {
        let path = normalize(path);
        if let Some(text) = self.overlays.get(&path) {
            return Some(text.clone());
        }
        for module in &self.modules {
            if let Some(file) = module.file_id(&path) {
                return Some(module.db.file_text(file).to_vec());
            }
        }
        self.loader.load(&path).ok()
    }

    /// The project's modules, in the order declared or found, each as the build compiles it.
    pub fn project_modules(&mut self) -> Vec<ModuleBuild<'_>> {
        let mut builds = Vec::new();
        for module in &mut self.modules {
            let Some(output) = &module.output else {
                continue;
            };
            let names = module
                .files
                .iter()
                .map(|path| {
                    path.strip_prefix(&output.base)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            builds.push(ModuleBuild {
                name: &output.name,
                output: &output.path,
                db: &mut module.db,
                input: &module.input,
                names,
                engine: module.engine,
                kind: module.kind,
                dialect: module.dialect,
            });
        }
        builds
    }

    /// Every file some module reads, once each, in module and then graph order.
    #[must_use]
    pub fn files(&self) -> Vec<PathBuf> {
        let mut out: Vec<PathBuf> = Vec::new();
        for module in &self.modules {
            for path in &module.files {
                if !out.contains(path) {
                    out.push(path.clone());
                }
            }
        }
        out
    }

    /// The modules that read `path`, each with the file's id there; every module's data is
    /// computed.
    pub(crate) fn modules_of(&mut self, path: &Path) -> Vec<(usize, FileId)> {
        let path = normalize(path);
        let mut out = Vec::new();
        for (index, module) in self.modules.iter_mut().enumerate() {
            if let Some(file) = module.file_id(&path) {
                module.ensure();
                out.push((index, file));
            }
        }
        out
    }

    /// Computes every module's data.
    pub(crate) fn ensure_all(&mut self) {
        for module in &mut self.modules {
            module.ensure();
        }
    }

    /// Module `index` and its data, once computed.
    pub(crate) fn module(&self, index: usize) -> Option<(&ModuleState, &ModuleData)> {
        let module = self.modules.get(index)?;
        Some((module, module.data()?))
    }
}

/// Whether editing `path` changes the project's modules or their files: a `quakec.toml` or a
/// `.src` file (PRJ-1.1, PRJ-4).
fn is_project_file(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == "quakec.toml")
        || path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("src"))
}

/// The UTF-8 byte order mark a file may start with, which the graph skips (PRJ-5.1).
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// The lines of `text` that may choose a module's files (PRJ-5.1, PRJ-5.3): each logical line,
/// a `\` at a line's end joining the next, whose first byte that is not white space is `#`,
/// after a byte order mark at the start; and every line between an `#includelist` and its
/// `#endlist`, each a list of files. An `#include`, a list's entries and the `#ifdef` and
/// `#define` lines that choose between them are all among them.
pub(crate) fn directive_lines(text: &[u8]) -> Vec<Vec<u8>> {
    let text = text.strip_prefix(UTF8_BOM).unwrap_or(text);
    let mut lines = Vec::new();
    let mut logical: Vec<u8> = Vec::new();
    let mut in_list = false;
    for raw in text.split(|&b| b == b'\n') {
        let raw = raw.trim_ascii_end();
        if let Some(head) = raw.strip_suffix(b"\\") {
            logical.extend_from_slice(head);
            continue;
        }
        logical.extend_from_slice(raw);
        let line = core::mem::take(&mut logical);
        let word = line.trim_ascii_start();
        if word.starts_with(b"#includelist") {
            in_list = true;
        } else if word.starts_with(b"#endlist") {
            in_list = false;
        }
        if in_list || word.first() == Some(&b'#') {
            lines.push(line);
        }
    }
    lines.push(logical);
    lines
}
