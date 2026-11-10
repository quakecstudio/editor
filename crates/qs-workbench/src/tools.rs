// SPDX-License-Identifier: GPL-3.0-or-later

//! The semantic tools the agent and MCP call as commands (P4-03): `project.symbols`, every
//! global definition of the open project, and `engine.builtins`, an engine's builtin table.

use qc_engines::{BuiltinKey, Engine};
use qc_project::ModuleKind;
use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HasWorkbench, Location, WorkbenchError};

/// The id of the command that lists the open project's symbols.
pub const PROJECT_SYMBOLS: &str = "project.symbols";
/// The id of the command that lists an engine's builtins.
pub const ENGINE_BUILTINS: &str = "engine.builtins";

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
    registry.register(EngineBuiltinsCommand)
}

#[cfg(test)]
mod tests;
