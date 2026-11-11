// SPDX-License-Identifier: GPL-3.0-or-later

//! `quakec.toml` (PRJ-2).

use std::fmt;
use std::path::{Component, Path};

use serde::Deserialize;

use crate::{Define, Dialect, ModuleKind, Target};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectTable {
    pub(crate) name: Option<String>,
    #[serde(rename = "mod")]
    pub(crate) mod_name: Option<String>,
    pub(crate) dialect: Option<Dialect>,
    pub(crate) target: Option<Target>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModuleTable {
    pub(crate) kind: ModuleKind,
    pub(crate) src: String,
    pub(crate) name: Option<String>,
    pub(crate) dialect: Option<Dialect>,
    pub(crate) target: Option<Target>,
    pub(crate) output: Option<String>,
    #[serde(default)]
    pub(crate) defines: Vec<String>,
    #[serde(default)]
    pub(crate) include_dirs: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TaskTable {
    pub(crate) name: String,
    pub(crate) command: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    #[serde(default)]
    pub(crate) project: ProjectTable,
    #[serde(default, rename = "module")]
    pub(crate) modules: Vec<ModuleTable>,
    #[serde(default, rename = "task")]
    pub(crate) tasks: Vec<TaskTable>,
}

/// A named command the project runs in its directory (PRJ-2.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// Its name, unique in the project.
    pub name: String,
    /// The command line the user's shell runs.
    pub command: String,
}

/// What [`parse`] reads: the `[project]` table, the modules and the tasks.
pub(crate) struct Parsed {
    pub(crate) project: ProjectTable,
    pub(crate) modules: Vec<CheckedModule>,
    pub(crate) tasks: Vec<Task>,
}

/// One module of the manifest, checked.
pub(crate) struct CheckedModule {
    pub(crate) name: String,
    pub(crate) kind: ModuleKind,
    pub(crate) src: String,
    pub(crate) dialect: Option<Dialect>,
    pub(crate) target: Option<Target>,
    pub(crate) output: Option<String>,
    pub(crate) defines: Vec<Define>,
    pub(crate) include_dirs: Vec<String>,
}

/// What is wrong with a `quakec.toml` (PRJ-2.1 to PRJ-2.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// The file is not TOML of the manifest's shape (PRJ-2.1): TOML's own message, at its
    /// 1-based line.
    Toml {
        /// The line.
        line: usize,
        /// TOML's message.
        message: String,
    },
    /// No `[[module]]` is declared (PRJ-2.1).
    NoModule,
    /// The mod name is not one directory name (PRJ-2.3).
    BadModName(String),
    /// Two modules have this name (PRJ-2.4).
    DuplicateModule(String),
    /// The module's `src` has a root (PRJ-2.5).
    RootedSrc(String),
    /// The module's `output` has a root or leaves the project (PRJ-2.5).
    OutputOutside(String),
    /// The module's include directory has a root (PRJ-2.5).
    RootedIncludeDir {
        /// The module.
        module: String,
        /// The directory.
        dir: String,
    },
    /// The module's `defines` entry is not `NAME` or `NAME=VALUE` (PRJ-2.6).
    BadDefine {
        /// The module.
        module: String,
        /// The entry.
        entry: String,
    },
    /// Two tasks have this name, or a task's name is empty (PRJ-2.10).
    BadTaskName(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Toml { line, message } => write!(f, "line {line}: {message}"),
            Self::NoModule => f.write_str("no [[module]] is declared"),
            Self::BadModName(name) => {
                write!(f, "mod name {name:?} is not a single directory name")
            }
            Self::DuplicateModule(name) => write!(f, "module name {name:?} is used twice"),
            Self::RootedSrc(module) => write!(f, "module {module}: src must be a relative path"),
            Self::OutputOutside(module) => write!(
                f,
                "module {module}: output must be a relative path inside the project"
            ),
            Self::RootedIncludeDir { module, dir } => write!(
                f,
                "module {module}: include directory {dir:?} must be a relative path"
            ),
            Self::BadDefine { module, entry } => {
                write!(f, "module {module}: {entry:?} is not NAME or NAME=VALUE")
            }
            Self::BadTaskName(name) if name.is_empty() => f.write_str("a task has no name"),
            Self::BadTaskName(name) => write!(f, "task name {name:?} is used twice"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// The tasks of the project file `text` (PRJ-2.10), the whole file checked.
///
/// # Errors
///
/// The file is not a valid project file (PRJ-2.1 to PRJ-2.10).
pub fn tasks(text: &str) -> Result<Vec<Task>, ManifestError> {
    parse(text).map(|parsed| parsed.tasks)
}

/// Parses and checks `quakec.toml` (PRJ-2.1 to PRJ-2.6).
pub(crate) fn parse(text: &str) -> Result<Parsed, ManifestError> {
    let manifest: Manifest = toml::from_str(text).map_err(|e| {
        let line = e.span().map_or(1, |span| {
            let before = text.get(..span.start).unwrap_or_default();
            before
                .bytes()
                .filter(|b| *b == b'\n')
                .count()
                .saturating_add(1)
        });
        ManifestError::Toml {
            line,
            message: e.message().to_owned(),
        }
    })?;
    if manifest.modules.is_empty() {
        return Err(ManifestError::NoModule);
    }
    if let Some(name) = &manifest.project.mod_name
        && !valid_mod_name(name)
    {
        return Err(ManifestError::BadModName(name.clone()));
    }
    let mut modules: Vec<CheckedModule> = Vec::new();
    for module in manifest.modules {
        let name = module
            .name
            .unwrap_or_else(|| module.kind.as_str().to_owned());
        if modules.iter().any(|m| m.name == name) {
            return Err(ManifestError::DuplicateModule(name));
        }
        if rooted(Path::new(&module.src)) {
            return Err(ManifestError::RootedSrc(name));
        }
        if let Some(output) = &module.output {
            let path = Path::new(output);
            let escapes = path.components().any(|c| c == Component::ParentDir);
            if rooted(path) || escapes {
                return Err(ManifestError::OutputOutside(name));
            }
        }
        if let Some(dir) = module.include_dirs.iter().find(|d| rooted(Path::new(d))) {
            return Err(ManifestError::RootedIncludeDir {
                module: name,
                dir: dir.clone(),
            });
        }
        let mut defines = Vec::new();
        for entry in &module.defines {
            let Some(define) = Define::parse(entry) else {
                return Err(ManifestError::BadDefine {
                    module: name,
                    entry: entry.clone(),
                });
            };
            defines.push(define);
        }
        modules.push(CheckedModule {
            name,
            kind: module.kind,
            src: module.src,
            dialect: module.dialect,
            target: module.target,
            output: module.output,
            defines,
            include_dirs: module.include_dirs,
        });
    }
    let mut tasks: Vec<Task> = Vec::new();
    for task in manifest.tasks {
        if task.name.is_empty() || tasks.iter().any(|t| t.name == task.name) {
            return Err(ManifestError::BadTaskName(task.name));
        }
        tasks.push(Task {
            name: task.name,
            command: task.command,
        });
    }
    Ok(Parsed {
        project: manifest.project,
        modules,
        tasks,
    })
}

/// Whether `path` has a root (`/x`) or, on Windows, a drive (`C:`) (PRJ-2.5).
fn rooted(path: &Path) -> bool {
    path.has_root() || matches!(path.components().next(), Some(Component::Prefix(_)))
}

/// Whether `name` is a single directory name (PRJ-2.3).
pub(crate) fn valid_mod_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', ':'])
}
