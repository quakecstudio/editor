// SPDX-License-Identifier: GPL-3.0-or-later

//! QuakeC projects.
//!
//! A project is a set of modules (server, client and menu QuakeC), each a separate program
//! with its own `progs.src`, dialect, target engine and output path. A project is declared in
//! `quakec.toml`, or opened bare from a directory or a `progs.src` (PRJ-1, PRJ-2). Each
//! module's file graph is the ordered list of files it compiles (PRJ-6), read from its
//! `progs.src` in id's list form or FTEQCC's source form (PRJ-4) by following the
//! directives that decide which files are read (PRJ-5).
//!
//! Written from `docs/spec/project-file.md` and QC-23 of `docs/spec/language.md`, not ported.

mod graph;
mod manifest;

#[cfg(test)]
mod tests;

pub use graph::{
    Define, Diagnostic, FileGraph, Form, GraphFile, GraphIndex, PRJ_MAX_DEPTH, PRJ_MAX_FILES, Via,
    build_graph,
};
pub use manifest::ManifestError;
pub use qc_lexer::Code;
pub use qc_preproc::{FsLoader, Loader, MemoryLoader, normalize};

use std::collections::VecDeque;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The project file's name (PRJ-1.1).
pub const PRJ_MANIFEST: &str = "quakec.toml";
/// The server QuakeC output file's name (PRJ-3.2).
pub const PRJ_SSQC_OUTPUT: &str = "progs.dat";
/// The client QuakeC output file's name (PRJ-3.2).
pub const PRJ_CSQC_OUTPUT: &str = "csprogs.dat";
/// The menu QuakeC output file's name (PRJ-3.2).
pub const PRJ_MENUQC_OUTPUT: &str = "menu.dat";
/// The directory, in the project, that build output goes in (PRJ-3.2, ADR-0008).
pub const PRJ_BUILD_DIR: &str = "build";

/// The QuakeC dialect a module is written in (PRJ-2.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    /// id's QuakeC, as id's `qcc` accepts it.
    Vanilla,
    /// FTEQCC's QuakeC.
    Fteqcc,
    /// GMQCC's QuakeC.
    Gmqcc,
}

impl From<Dialect> for qc_lexer::Dialect {
    /// The lexer and parser dialect this project dialect selects. The names match one for
    /// one; the two types are separate only so this crate can deserialise it from a project
    /// file without the lexer depending on `serde`.
    fn from(dialect: Dialect) -> Self {
        match dialect {
            Dialect::Vanilla => Self::Vanilla,
            Dialect::Fteqcc => Self::Fteqcc,
            Dialect::Gmqcc => Self::Gmqcc,
        }
    }
}

/// The engine a module is built for (PRJ-2.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    /// id's engine, which the preview host follows.
    Quake,
    /// QuakeSpasm, written `quakespasm`.
    QuakeSpasm,
    /// Ironwail.
    Ironwail,
    /// vkQuake, written `vkquake`.
    VkQuake,
    /// The 2021 KEX rerelease engine, a compile target only (ENG-8).
    Rerelease,
    /// FTE.
    Fte,
    /// `DarkPlaces`.
    Dp,
}

impl Target {
    /// Every target, in the order PRJ-2.9 lists them.
    pub const ALL: [Self; 7] = [
        Self::Quake,
        Self::QuakeSpasm,
        Self::Ironwail,
        Self::VkQuake,
        Self::Rerelease,
        Self::Fte,
        Self::Dp,
    ];
}

/// What a module is (PRJ-3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModuleKind {
    /// Server QuakeC.
    Ssqc,
    /// Client QuakeC.
    Csqc,
    /// Menu QuakeC.
    Menuqc,
}

impl ModuleKind {
    /// The kind as `quakec.toml` writes it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ssqc => "ssqc",
            Self::Csqc => "csqc",
            Self::Menuqc => "menuqc",
        }
    }

    /// The output file name of this kind (PRJ-3.2).
    #[must_use]
    pub const fn output_file(self) -> &'static str {
        match self {
            Self::Ssqc => PRJ_SSQC_OUTPUT,
            Self::Csqc => PRJ_CSQC_OUTPUT,
            Self::Menuqc => PRJ_MENUQC_OUTPUT,
        }
    }

    /// The kind a bare project gives a module that declares `output` (PRJ-3.3).
    #[must_use]
    pub fn from_declared_output(output: Option<&Path>) -> Self {
        let name = output
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().to_ascii_lowercase());
        match name.as_deref() {
            Some(PRJ_CSQC_OUTPUT) => Self::Csqc,
            Some(PRJ_MENUQC_OUTPUT) => Self::Menuqc,
            _ => Self::Ssqc,
        }
    }
}

/// One program of a project (PRJ-3.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    /// Its name, unique in the project (PRJ-2.4, PRJ-1.4).
    pub name: String,
    /// What it is.
    pub kind: ModuleKind,
    /// Its `progs.src` or root source file.
    pub src: PathBuf,
    /// Its dialect.
    pub dialect: Dialect,
    /// Its target engine.
    pub target: Target,
    /// Where the IDE writes it (PRJ-3.2).
    pub output: PathBuf,
    /// Macros defined before its first file.
    pub defines: Vec<Define>,
    /// Directories searched for included files, after the including file's own.
    pub include_dirs: Vec<PathBuf>,
    /// The files it compiles, in order.
    pub graph: FileGraph,
}

/// An opened project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// The project directory.
    pub root: PathBuf,
    /// Its `quakec.toml`, or `None` for a bare project.
    pub manifest: Option<PathBuf>,
    /// Its name.
    pub name: String,
    /// The single directory name its build output goes in (PRJ-2.3, ADR-0008).
    pub mod_name: String,
    /// The project's dialect; a module may override it.
    pub dialect: Dialect,
    /// The project's target engine; a module may override it.
    pub target: Target,
    /// Its modules, in the order declared or found.
    pub modules: Vec<Module>,
}

/// Why a project did not open.
#[derive(Debug)]
pub enum ProjectError {
    /// A file could not be read.
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        source: io::Error,
    },
    /// `quakec.toml` is not valid (PRJ-2.1).
    Manifest {
        /// The file.
        path: PathBuf,
        /// What is wrong.
        error: ManifestError,
    },
    /// A directory with neither `quakec.toml` nor `progs.src` (PRJ-1.2).
    NoProject {
        /// The directory.
        dir: PathBuf,
    },
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Manifest { path, error } => write!(f, "{}: {error}", path.display()),
            Self::NoProject { dir } => {
                write!(f, "{}: no {PRJ_MANIFEST} and no progs.src", dir.display())
            }
        }
    }
}

impl std::error::Error for ProjectError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Manifest { error, .. } => Some(error),
            Self::NoProject { .. } => None,
        }
    }
}

/// Opens the project at `path`, a directory or a file, from the file system (PRJ-1).
///
/// # Errors
///
/// [`ProjectError`] when the project cannot be opened. Problems in the file graphs are
/// diagnostics in each module's graph, not errors.
pub fn open(path: &Path) -> Result<Project, ProjectError> {
    let path = std::path::absolute(path).map_err(|source| ProjectError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let path = normalize(&path);
    if path.is_dir() {
        open_dir(&mut FsLoader, &path)
    } else {
        open_file(&mut FsLoader, &path)
    }
}

/// Opens the project in directory `dir` (PRJ-1.1, PRJ-1.2).
///
/// # Errors
///
/// [`ProjectError`] when `quakec.toml` cannot be read or is invalid, or when the directory has
/// neither it nor `progs.src`.
pub fn open_dir<L: Loader>(loader: &mut L, dir: &Path) -> Result<Project, ProjectError> {
    let manifest = dir.join(PRJ_MANIFEST);
    match loader.load(&manifest) {
        Ok(bytes) => from_manifest(loader, dir, &manifest, &bytes),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            let progs = dir.join("progs.src");
            if let Err(err) = loader.load(&progs) {
                return Err(if err.kind() == io::ErrorKind::NotFound {
                    ProjectError::NoProject {
                        dir: dir.to_path_buf(),
                    }
                } else {
                    ProjectError::Read {
                        path: progs,
                        source: err,
                    }
                });
            }
            let mut sources = vec![progs];
            let csprogs = dir.join("csprogs.src");
            if loader.load(&csprogs).is_ok() {
                sources.push(csprogs);
            }
            Ok(bare(loader, dir, sources))
        }
        Err(source) => Err(ProjectError::Read {
            path: manifest,
            source,
        }),
    }
}

/// Opens the project of file `file`: a `quakec.toml`, or any other file as a bare project
/// (PRJ-1.1, PRJ-1.3).
///
/// # Errors
///
/// [`ProjectError`] when `file` is a `quakec.toml` that cannot be read or is invalid. Any
/// other file opens, and a source that cannot be read is a diagnostic (PRJ-4.5).
pub fn open_file<L: Loader>(loader: &mut L, file: &Path) -> Result<Project, ProjectError> {
    let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
    if file.file_name().is_some_and(|n| n == PRJ_MANIFEST) {
        let bytes = loader.load(file).map_err(|source| ProjectError::Read {
            path: file.to_path_buf(),
            source,
        })?;
        return from_manifest(loader, &dir, file, &bytes);
    }
    Ok(bare(loader, &dir, vec![file.to_path_buf()]))
}

/// The directory's own name, if it has one.
fn dir_name(dir: &Path) -> Option<String> {
    dir.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// The mod name of a project directory with no `mod` given (PRJ-1.4).
fn default_mod_name(dir: &Path) -> String {
    dir_name(dir)
        .filter(|n| manifest::valid_mod_name(n))
        .unwrap_or_else(|| "mod".to_owned())
}

fn from_manifest<L: Loader>(
    loader: &mut L,
    root: &Path,
    path: &Path,
    bytes: &[u8],
) -> Result<Project, ProjectError> {
    let text = String::from_utf8_lossy(bytes);
    let (project, modules) = manifest::parse(&text).map_err(|error| ProjectError::Manifest {
        path: path.to_path_buf(),
        error,
    })?;
    let mod_name = project.mod_name.unwrap_or_else(|| default_mod_name(root));
    let dialect = project.dialect.unwrap_or(Dialect::Vanilla);
    let target = project.target.unwrap_or(Target::Quake);
    let mut out = Vec::new();
    for module in modules {
        let src = normalize(&root.join(&module.src));
        let include_dirs: Vec<PathBuf> = module
            .include_dirs
            .iter()
            .map(|d| normalize(&root.join(d)))
            .collect();
        let output = module.output.map_or_else(
            || default_output(root, &mod_name, module.kind),
            |o| normalize(&root.join(o)),
        );
        let module_dialect = module.dialect.unwrap_or(dialect);
        let graph = build_graph(
            loader,
            &src,
            &module.defines,
            &include_dirs,
            module_dialect.into(),
        );
        out.push(Module {
            name: module.name,
            kind: module.kind,
            src,
            dialect: module_dialect,
            target: module.target.unwrap_or(target),
            output,
            defines: module.defines,
            include_dirs,
            graph,
        });
    }
    Ok(Project {
        root: root.to_path_buf(),
        manifest: Some(path.to_path_buf()),
        name: project
            .name
            .or_else(|| dir_name(root))
            .unwrap_or_else(|| "project".to_owned()),
        mod_name,
        dialect,
        target,
        modules: out,
    })
}

/// `build/<mod>/<file of the kind>` in the project (PRJ-3.2).
fn default_output(root: &Path, mod_name: &str, kind: ModuleKind) -> PathBuf {
    root.join(PRJ_BUILD_DIR)
        .join(mod_name)
        .join(kind.output_file())
}

/// A bare project's name for the module whose source is `src` (PRJ-1.4).
fn bare_module_name(root: &Path, src: &Path) -> String {
    let relative = src.strip_prefix(root).unwrap_or(src);
    let is_src = relative
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("src"));
    let relative = if is_src {
        relative.with_extension("")
    } else {
        relative.to_path_buf()
    };
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// A bare project of `sources` and every source file they register (PRJ-1.2 to PRJ-1.5).
fn bare<L: Loader>(loader: &mut L, root: &Path, sources: Vec<PathBuf>) -> Project {
    let mod_name = default_mod_name(root);
    let mut queue: VecDeque<PathBuf> = sources.iter().map(|s| normalize(s)).collect();
    let mut seen: Vec<PathBuf> = queue.iter().cloned().collect();
    let mut modules: Vec<Module> = Vec::new();
    while let Some(src) = queue.pop_front() {
        let graph = build_graph(loader, &src, &[], &[], qc_lexer::Dialect::Vanilla);
        for registered in &graph.sourcefiles {
            if !seen.contains(registered) {
                seen.push(registered.clone());
                queue.push_back(registered.clone());
            }
        }
        if graph.is_program_list() {
            continue;
        }
        let kind = ModuleKind::from_declared_output(graph.declared_output.as_deref());
        let mut name = bare_module_name(root, &src);
        if modules.iter().any(|m| m.name == name) {
            name = src.to_string_lossy().into_owned();
        }
        modules.push(Module {
            name,
            kind,
            output: default_output(root, &mod_name, kind),
            src,
            dialect: Dialect::Vanilla,
            target: Target::Quake,
            defines: Vec::new(),
            include_dirs: Vec::new(),
            graph,
        });
    }
    Project {
        root: root.to_path_buf(),
        manifest: None,
        name: dir_name(root).unwrap_or_else(|| "project".to_owned()),
        mod_name,
        dialect: Dialect::Vanilla,
        target: Target::Quake,
        modules,
    }
}
