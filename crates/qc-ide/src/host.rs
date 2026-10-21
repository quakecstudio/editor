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

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use qc_engines::{BuiltinRef, Engine};
use qc_project::{Dialect, Loader, ModuleKind, Project, ProjectError, Target, normalize};
use qc_sema::{Db, FileId, ModuleAnalysis, ModuleInput};
use qc_syntax::Parse;

/// What the editor or the user says about the project, over what its files say.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config {
    /// The dialect every module is read in, over the project file's (PRJ-2.8). A project with
    /// no `quakec.toml` is vanilla otherwise, as `qc_project::open` gives it.
    pub dialect: Option<Dialect>,
    /// The engine every module targets, over the project file's (PRJ-2.9): the engine whose
    /// builtin table the hover and `W0311` read.
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
    /// Its files, `FileId(i)` being `files[i]`, as the file graph orders them.
    pub(crate) files: Vec<PathBuf>,
    /// The files it preprocesses and compiles.
    input: ModuleInput,
    /// Whether it resolves with FTEQCC's autoprototyping (FTE-22.3).
    autoproto: bool,
    /// Whether the module is one file no module of the project compiles.
    orphan: bool,
    /// Its incremental database.
    pub(crate) db: Db,
    /// What was derived from the files' current text, `None` after an edit.
    data: Option<ModuleData>,
    /// The project graph's diagnostics for the module (PRJ-6.5).
    pub(crate) graph_diagnostics: Vec<qc_project::Diagnostic>,
}

/// What the features read of one module, derived once per edit.
#[derive(Debug)]
pub(crate) struct ModuleData {
    /// The module read as the compiler reads it.
    pub(crate) analysis: ModuleAnalysis,
    /// Each file's tree as written, `parses[i]` being `FileId(i)`'s.
    pub(crate) parses: Vec<Parse>,
    /// The builtin each global name is declared as (ENGT-4.12, ENGT-4.17).
    pub(crate) builtins: HashMap<String, BuiltinRef>,
    /// The `W0311` warnings for the module's target engine (ENGT-4).
    pub(crate) missing: Vec<qc_engines::FileDiagnostic>,
}

impl ModuleState {
    /// The id of `path` in this module, if the module has it.
    pub(crate) fn file_id(&self, path: &Path) -> Option<FileId> {
        self.files.iter().position(|p| p == path).map(FileId)
    }

    /// The path of file `file`.
    pub(crate) fn path(&self, file: FileId) -> Option<&Path> {
        self.files.get(file.0).map(PathBuf::as_path)
    }

    /// The derived data, computed if an edit invalidated it.
    fn ensure(&mut self) -> &ModuleData {
        let data = self.data.take().unwrap_or_else(|| {
            let analysis = self.db.analyze_module(&self.input);
            let parses: Vec<Parse> = (0..self.files.len())
                .map(|index| self.db.parse(FileId(index)))
                .collect();
            let builtins = qc_engines::declared_builtins(&parses, self.dialect);
            let missing = qc_engines::missing_builtins(
                self.engine,
                self.kind,
                &parses,
                &analysis.index,
                self.dialect,
            );
            ModuleData {
                analysis,
                parses,
                builtins,
                missing,
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
    pub fn reload(&mut self) -> Result<(), ProjectError> {
        self.modules.clear();
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
        let result = match opened {
            Ok(Some(project)) => {
                self.add_project(&project);
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

    /// Adds a module for each of `project`'s modules.
    fn add_project(&mut self, project: &Project) {
        for module in &project.modules {
            let dialect = self.config.dialect.unwrap_or(module.dialect);
            let target = self.config.target.unwrap_or(module.target);
            let files: Vec<PathBuf> = module
                .graph
                .files
                .iter()
                .map(|f| normalize(&f.path))
                .collect();
            let mut state = self.module_state(
                module.kind,
                dialect,
                target,
                files,
                ModuleInput::from_module(module),
                false,
            );
            state
                .graph_diagnostics
                .clone_from(&module.graph.diagnostics);
            self.modules.push(state);
        }
    }

    /// Adds the one-file module of `path`, a file no project module compiles.
    fn add_orphan(&mut self, path: PathBuf) {
        let dialect = self.config.dialect.unwrap_or(Dialect::Vanilla);
        let target = self.config.target.unwrap_or(Target::Quake);
        let input = ModuleInput {
            compiled: vec![FileId(0)],
            ..ModuleInput::default()
        };
        let state = self.module_state(ModuleKind::Ssqc, dialect, target, vec![path], input, true);
        self.modules.push(state);
    }

    /// A module's state with every file's text set, from an overlay or the loader.
    fn module_state(
        &mut self,
        kind: ModuleKind,
        dialect: Dialect,
        target: Target,
        files: Vec<PathBuf>,
        input: ModuleInput,
        orphan: bool,
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
                db.set_file(FileId(index), path.clone(), text);
            }
        }
        ModuleState {
            kind,
            engine: Engine::from_target(target),
            dialect: dialect.into(),
            files,
            input,
            autoproto: self.config.autoproto,
            orphan,
            db,
            data: None,
            graph_diagnostics: Vec::new(),
        }
    }

    /// Whether a project module compiles `path`.
    fn is_compiled(&self, path: &Path) -> bool {
        self.modules
            .iter()
            .any(|m| !m.orphan && m.file_id(path).is_some())
    }

    /// Sets the text of the open file `path`, which replaces its text on disk. Editing a
    /// project file or a `progs.src` reads the project again.
    ///
    /// # Errors
    ///
    /// As [`AnalysisHost::reload`], when `path` is a project file whose new text does not
    /// open.
    pub fn set_overlay(&mut self, path: &Path, text: Vec<u8>) -> Result<(), ProjectError> {
        let path = normalize(path);
        self.overlays.insert(path.clone(), text.clone());
        if is_project_file(&path) {
            return self.reload();
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
            .retain(|m| !(m.orphan && m.files.first() == Some(&path)));
        if is_project_file(&path) {
            return self.reload();
        }
        let text = self.loader.load(&path).unwrap_or_default();
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
