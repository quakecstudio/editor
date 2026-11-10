// SPDX-License-Identifier: GPL-3.0-or-later

//! The semantic tools the agent and MCP call as commands (P4-03): `project.symbols`, every
//! global definition of the open project, `engine.builtins`, an engine's builtin table, and
//! `compile`, the project built with a verdict and counts the agent can act on.

use std::path::PathBuf;

use qc_engines::{BuiltinKey, Engine};
use qc_project::ModuleKind;
use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Gravity, HasWorkbench, Location, ModuleBuilt, WorkbenchError};

/// The id of the command that lists the open project's symbols.
pub const PROJECT_SYMBOLS: &str = "project.symbols";
/// The id of the command that lists an engine's builtins.
pub const ENGINE_BUILTINS: &str = "engine.builtins";
/// The id of the command that compiles the open project and says whether it is clean.
pub const COMPILE: &str = "compile";
/// The id of the command that lists the open project's source files.
pub const PROJECT_FILES: &str = "project.files";
/// The id of the command that searches the open project's text.
pub const PROJECT_SEARCH: &str = "project.search";
/// The id of the command that describes the open project's modules.
pub const PROJECT_INFO: &str = "project.info";

/// The most matches `project.search` answers.
pub const SEARCH_LIMIT: usize = 200;

/// The parameters of `project.search`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectSearchParams {
    /// The text to find.
    pub text: String,
    /// Whether case matters; it does not by default.
    #[serde(default)]
    pub case_sensitive: bool,
}

/// One line that holds the text searched for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchMatch {
    /// The file.
    pub path: PathBuf,
    /// The line, from 1.
    pub line: usize,
    /// The line's text, without its line ending.
    pub text: String,
}

/// What `project.search` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchFound {
    /// The lines found, by file and line, at most [`SEARCH_LIMIT`].
    pub matches: Vec<SearchMatch>,
    /// Whether there were more than these.
    pub truncated: bool,
}

/// A module's kind, as `project.info` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum KindName {
    /// Server QuakeC, `progs.dat`.
    Ssqc,
    /// Client QuakeC, `csprogs.dat`.
    Csqc,
    /// Menu QuakeC, `menu.dat`.
    Menuqc,
}

impl From<ModuleKind> for KindName {
    fn from(kind: ModuleKind) -> Self {
        match kind {
            ModuleKind::Ssqc => Self::Ssqc,
            ModuleKind::Csqc => Self::Csqc,
            ModuleKind::Menuqc => Self::Menuqc,
        }
    }
}

/// A module's dialect, as `project.info` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DialectName {
    /// id's QuakeC, as id's `qcc` accepts it.
    Vanilla,
    /// FTEQCC's QuakeC.
    Fteqcc,
    /// GMQCC's QuakeC.
    Gmqcc,
}

impl From<qc_syntax::Dialect> for DialectName {
    fn from(dialect: qc_syntax::Dialect) -> Self {
        match dialect {
            qc_syntax::Dialect::Vanilla => Self::Vanilla,
            qc_syntax::Dialect::Fteqcc => Self::Fteqcc,
            qc_syntax::Dialect::Gmqcc => Self::Gmqcc,
        }
    }
}

/// One module of the open project, as `project.info` describes it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ModuleInfo {
    /// Its name (PRJ-2.4).
    pub name: String,
    /// What it is.
    pub kind: KindName,
    /// The dialect it is read in.
    pub dialect: DialectName,
    /// The id of the engine it targets (ENGT-3.5), which `engine.builtins` takes.
    pub engine: String,
    /// That engine's name.
    pub engine_name: String,
    /// Where its program is written.
    pub output: PathBuf,
}

/// What `project.info` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ProjectInfo {
    /// The project's directory.
    pub root: PathBuf,
    /// Its modules, in the project file's order.
    pub modules: Vec<ModuleInfo>,
}

impl<L: qc_project::Loader> crate::Workbench<L> {
    /// The open project's directory and modules, `None` with no project.
    pub fn project_info(&mut self) -> Option<ProjectInfo> {
        let root = self.root()?.to_path_buf();
        let modules = self
            .host
            .project_modules()
            .into_iter()
            .map(|module| ModuleInfo {
                name: module.name.to_owned(),
                kind: module.kind.into(),
                dialect: module.dialect.into(),
                engine: module.engine.id().to_owned(),
                engine_name: module.engine.name().to_owned(),
                output: module.output.to_path_buf(),
            })
            .collect();
        Some(ProjectInfo { root, modules })
    }

    /// Every source file the open project compiles, in path order.
    #[must_use]
    pub fn project_files(&self) -> Vec<PathBuf> {
        let mut files = self.host.files();
        files.sort();
        files
    }

    /// Every line of the project's files, their current text, holding `text`: the editor's
    /// unsaved text where it has one. Lines are counted by line feeds.
    pub fn search(&mut self, text: &str, case_sensitive: bool) -> SearchFound {
        let needle = if case_sensitive {
            text.to_owned()
        } else {
            text.to_lowercase()
        };
        let mut matches = Vec::new();
        let mut truncated = false;
        for path in self.project_files() {
            let Some(bytes) = self.host.text(&path) else {
                continue;
            };
            let content = String::from_utf8_lossy(&bytes);
            for (index, line) in content.lines().enumerate() {
                let haystack = if case_sensitive {
                    line.to_owned()
                } else {
                    line.to_lowercase()
                };
                if needle.is_empty() || !haystack.contains(&needle) {
                    continue;
                }
                if matches.len() == SEARCH_LIMIT {
                    truncated = true;
                    break;
                }
                matches.push(SearchMatch {
                    path: path.clone(),
                    line: index.saturating_add(1),
                    text: line.to_owned(),
                });
            }
            if truncated {
                break;
            }
        }
        SearchFound { matches, truncated }
    }
}

/// `project.files`: the open project's files.
struct ProjectFiles;

impl<C: HasWorkbench + 'static> Command<C> for ProjectFiles {
    const ID: &'static str = PROJECT_FILES;
    const SUMMARY: &'static str = "Lists every source file the open project compiles, by path.";
    type Params = qs_commands::NoParams;
    type Output = Vec<PathBuf>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _params: qs_commands::NoParams,
    ) -> Result<Outcome<Vec<PathBuf>, C>, WorkbenchError> {
        let workbench = context.workbench();
        if workbench.root().is_none() {
            return Err(WorkbenchError::NoProject);
        }
        Ok(Outcome::unchanged(workbench.project_files()))
    }
}

/// `project.info`: the open project's modules, with their dialects and engines.
struct ProjectInfoCommand;

impl<C: HasWorkbench + 'static> Command<C> for ProjectInfoCommand {
    const ID: &'static str = PROJECT_INFO;
    const SUMMARY: &'static str = "Describes the open project: its directory and each module's \
        kind, dialect, target engine and output.";
    type Params = qs_commands::NoParams;
    type Output = ProjectInfo;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _params: qs_commands::NoParams,
    ) -> Result<Outcome<ProjectInfo, C>, WorkbenchError> {
        let info = context
            .workbench()
            .project_info()
            .ok_or(WorkbenchError::NoProject)?;
        Ok(Outcome::outside_history(info))
    }
}

/// `project.search`: the lines of the project that hold a text.
struct ProjectSearch;

impl<C: HasWorkbench + 'static> Command<C> for ProjectSearch {
    const ID: &'static str = PROJECT_SEARCH;
    const SUMMARY: &'static str = "Finds every line of the open project's files that holds a \
        text, unsaved edits included, by file and line.";
    type Params = ProjectSearchParams;
    type Output = SearchFound;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: ProjectSearchParams,
    ) -> Result<Outcome<SearchFound, C>, WorkbenchError> {
        let workbench = context.workbench();
        if workbench.root().is_none() {
            return Err(WorkbenchError::NoProject);
        }
        let found = workbench.search(&params.text, params.case_sensitive);
        Ok(Outcome::unchanged(found))
    }
}

/// What `compile` answers: whether the compile is clean, its counts, and each module.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Compiled {
    /// Every module built, with no error (AGT-5's clean compile).
    pub clean: bool,
    /// The errors of every module.
    pub errors: usize,
    /// The warnings of every module.
    pub warnings: usize,
    /// Each module: where its program was written, whether it was, and its problems.
    #[schemars(skip)]
    pub modules: Vec<ModuleBuilt>,
}

impl Compiled {
    /// The verdict and counts of `modules`.
    fn of(modules: Vec<ModuleBuilt>) -> Self {
        let problems = || modules.iter().flat_map(|m| &m.problems);
        let errors = problems().filter(|p| p.gravity == Gravity::Error).count();
        let warnings = problems().count().saturating_sub(errors);
        Self {
            clean: errors == 0 && modules.iter().all(|m| m.built),
            errors,
            warnings,
            modules,
        }
    }
}

/// `compile`: builds the open project, as `project.build` does, and says whether it is clean.
struct CompileCommand;

impl<C: HasWorkbench + 'static> Command<C> for CompileCommand {
    const ID: &'static str = COMPILE;
    const SUMMARY: &'static str =
        "Compiles the open project and says whether it is clean, with its error counts.";
    type Params = qs_commands::NoParams;
    type Output = Compiled;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _params: qs_commands::NoParams,
    ) -> Result<Outcome<Compiled, C>, WorkbenchError> {
        let modules = context.workbench().build()?;
        Ok(Outcome::outside_history(Compiled::of(modules)))
    }
}

/// The parameters of `project.symbols`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectSymbolsParams {
    /// Only the symbols whose name contains this, without case; every one when absent.
    #[serde(default)]
    pub query: Option<String>,
}

/// One global definition of the project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SymbolFound {
    /// Its name.
    pub name: String,
    /// What it is: a function, a global, a field and so on.
    pub kind: String,
    /// The file its name is first written in.
    pub path: String,
    /// Its line, from 1.
    pub line: u32,
    /// Its column, from 1.
    pub column: u32,
}

/// `project.symbols`: the open project's global definitions.
struct ProjectSymbols;

impl<C: HasWorkbench + 'static> Command<C> for ProjectSymbols {
    const ID: &'static str = PROJECT_SYMBOLS;
    const SUMMARY: &'static str = "Lists the open project's global definitions, functions, \
        globals and fields, each with where its name is first written; a query keeps those \
        whose name contains it.";
    type Params = ProjectSymbolsParams;
    type Output = Vec<SymbolFound>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: ProjectSymbolsParams,
    ) -> Result<Outcome<Vec<SymbolFound>, C>, WorkbenchError> {
        let workbench = context.workbench();
        if workbench.root().is_none() {
            return Err(WorkbenchError::NoProject);
        }
        let query = params.query.map(|q| q.to_lowercase());
        let symbols: Vec<_> = workbench
            .project_symbols()
            .iter()
            .filter(|s| {
                query
                    .as_ref()
                    .is_none_or(|q| s.name.to_lowercase().contains(q.as_str()))
            })
            .cloned()
            .collect();
        let found = symbols
            .into_iter()
            .map(|symbol| {
                let text = workbench.text(&symbol.range.path).unwrap_or_default();
                let at = Location::of(&text, symbol.range.span.start);
                SymbolFound {
                    name: symbol.name,
                    kind: format!("{:?}", symbol.kind).to_lowercase(),
                    path: symbol.range.path.display().to_string(),
                    line: at.line(),
                    column: at.column(),
                }
            })
            .collect();
        Ok(Outcome::outside_history(found))
    }
}

/// The parameters of `engine.builtins`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EngineBuiltinsParams {
    /// The engine's id, such as `ironwail` (ENGT-3.4); the open project's when absent.
    #[serde(default)]
    pub engine: Option<String>,
    /// The module kind: `ssqc`, `csqc` or `menuqc`; `ssqc` when absent.
    #[serde(default)]
    pub kind: Option<String>,
}

/// One builtin of an engine's table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct BuiltinFound {
    /// Its number, when the engine looks it up by number.
    pub number: Option<u32>,
    /// Its engine name, when the engine looks it up by name (`#0:NAME`).
    pub engine_name: Option<String>,
    /// The QuakeC name its source declares it with.
    pub name: String,
    /// Its function type, such as `float(string s)`; empty when its source gives none.
    pub signature: String,
    /// The extensions it belongs to; empty for id's builtins.
    pub extensions: Vec<String>,
}

/// What `engine.builtins` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct EngineBuiltins {
    /// The engine's id.
    pub engine: String,
    /// Its builtins for the kind, in table order (ENGT-3.9, ENGT-3.11).
    pub builtins: Vec<BuiltinFound>,
}

/// Why `engine.builtins` could not answer.
#[derive(Debug)]
pub enum EngineBuiltinsError {
    /// No engine was named and no project is open.
    NoEngine,
    /// No engine has the id.
    UnknownEngine(String),
    /// The kind is not `ssqc`, `csqc` or `menuqc`.
    UnknownKind(String),
}

impl std::fmt::Display for EngineBuiltinsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEngine => f.write_str("name an engine, or open a project"),
            Self::UnknownEngine(id) => write!(f, "no engine has the id {id}"),
            Self::UnknownKind(kind) => write!(f, "{kind} is not ssqc, csqc or menuqc"),
        }
    }
}

impl std::error::Error for EngineBuiltinsError {}

/// `engine.builtins`: an engine's builtin table for a module kind.
struct EngineBuiltinsCommand;

impl<C: HasWorkbench + 'static> Command<C> for EngineBuiltinsCommand {
    const ID: &'static str = ENGINE_BUILTINS;
    const SUMMARY: &'static str = "Lists an engine's builtins for a module kind, each with its \
        number or engine name, QuakeC name, signature and extensions; the open project's \
        engine and server QuakeC when none is named.";
    type Params = EngineBuiltinsParams;
    type Output = EngineBuiltins;
    type Error = EngineBuiltinsError;

    fn run(
        &self,
        context: &mut C,
        params: EngineBuiltinsParams,
    ) -> Result<Outcome<EngineBuiltins, C>, EngineBuiltinsError> {
        let engine = match params.engine {
            Some(id) => Engine::ALL
                .into_iter()
                .find(|engine| engine.id() == id)
                .ok_or(EngineBuiltinsError::UnknownEngine(id))?,
            None => context
                .workbench()
                .engine()
                .ok_or(EngineBuiltinsError::NoEngine)?,
        };
        let kind = match params.kind.as_deref() {
            None | Some("ssqc") => ModuleKind::Ssqc,
            Some("csqc") => ModuleKind::Csqc,
            Some("menuqc") => ModuleKind::Menuqc,
            Some(other) => return Err(EngineBuiltinsError::UnknownKind(other.to_owned())),
        };
        Ok(Outcome::outside_history(builtins_of(engine, kind)))
    }
}

/// `engine`'s builtins for `kind`, as `engine.builtins` answers them.
fn builtins_of(engine: Engine, kind: ModuleKind) -> EngineBuiltins {
    let builtins = engine
        .builtins(kind)
        .into_iter()
        .map(|builtin| {
            let (number, engine_name) = match builtin.key {
                BuiltinKey::Number(n) => (Some(n), None),
                BuiltinKey::Name(name) => (None, Some(name.to_owned())),
            };
            BuiltinFound {
                number,
                engine_name,
                name: builtin.name.to_owned(),
                signature: builtin.signature.to_owned(),
                extensions: builtin.extensions.iter().map(|e| (*e).to_owned()).collect(),
            }
        })
        .collect();
    EngineBuiltins {
        engine: engine.id().to_owned(),
        builtins,
    }
}

/// Registers `project.symbols` and `engine.builtins`.
///
/// # Errors
///
/// [`RegisterError`] when one of their ids is taken.
pub fn register_tools<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(ProjectSymbols)?;
    registry.register(EngineBuiltinsCommand)?;
    registry.register(CompileCommand)?;
    registry.register(ProjectFiles)?;
    registry.register(ProjectInfoCommand)?;
    registry.register(ProjectSearch)
}

#[cfg(test)]
mod tests;
