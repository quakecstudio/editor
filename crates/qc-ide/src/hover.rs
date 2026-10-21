// SPDX-License-Identifier: GPL-3.0-or-later

//! Hover: what the name under the cursor is, how it was declared, its comment, and for a
//! builtin which engines have it.
//!
//! A builtin is bound to the engine by its number or engine name alone (QC-13.3, ENGT-2.4),
//! and engines differ in which they have, so the hover of a builtin names the module's target
//! engine's entry for it and every engine whose table for the module's kind has it or lacks it
//! (ENGT-3.9, ENGT-3.11), from the engine-target database (`qc-engines`, LANG-6).

use std::path::Path;

use qc_engines::{Builtin, BuiltinRef, Engine};
use qc_project::{Loader, ModuleKind};
use qc_sema::{DefKind, Definition};

use crate::FileRange;
use crate::host::{AnalysisHost, ModuleData, ModuleState};
use crate::syntax::{declaration_of, declared_as, doc_comment, name_at, name_token_near, token_at};

/// What the hover of a name shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    /// The name under the cursor.
    pub range: FileRange,
    /// The definition's name.
    pub name: String,
    /// What it is.
    pub kind: DefKind,
    /// How it was declared, such as `float(entity targ) T_Heal`.
    pub declaration: String,
    /// The comment that documents its declaration, if any.
    pub docs: Option<String>,
    /// For a builtin, the engines that have it.
    pub builtin: Option<BuiltinInfo>,
}

/// Which engines have a builtin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuiltinInfo {
    /// How the module names it: `#N` or an engine name.
    pub key: BuiltinRef,
    /// The module's kind, which selects each engine's table (ENGT-3.11).
    pub kind: ModuleKind,
    /// The engine the module targets.
    pub target: Engine,
    /// Each engine with a table for the module's kind, in `Engine::ALL` order, with its entry
    /// for the builtin when it has one (ENGT-3.9).
    pub engines: Vec<(Engine, Option<Builtin>)>,
}

impl BuiltinInfo {
    /// The target engine's entry, `None` when it lacks the builtin or has no table for the
    /// module's kind.
    #[must_use]
    pub fn on_target(&self) -> Option<Builtin> {
        self.engines
            .iter()
            .find(|(engine, _)| *engine == self.target)
            .and_then(|(_, builtin)| *builtin)
    }
}

impl<L: Loader> AnalysisHost<L> {
    /// The hover of the name at byte `offset` of `path`, or of the name ending there; `None`
    /// when no resolved name is there.
    pub fn hover(&mut self, path: &Path, offset: usize) -> Option<Hover> {
        let target = self.def_at(path, offset)?;
        let (state, data) = self.module(target.module)?;
        let def = data.analysis.index.def(target.def)?;
        let file = state.file_id(&qc_project::normalize(path))?;
        let parse = data.parses.get(file.0)?;
        let under = name_token_near(parse, offset)?;
        Some(Hover {
            range: FileRange {
                path: state.path(file)?.to_path_buf(),
                span: under.span(),
            },
            name: def.name.clone(),
            kind: def.kind,
            declaration: declaration(state, data, def),
            docs: docs(state, data, def),
            builtin: builtin_info(state, data, def),
        })
    }
}

/// How `def` was declared: its declaration's text when its name is written there, else its
/// type and name.
fn declaration(state: &ModuleState, data: &ModuleData, def: &Definition) -> String {
    declaration_node(state, data, def)
        .and_then(|(decl, token, _)| declared_as(&decl, &token))
        .unwrap_or_else(|| format!("{} {}", def.ty.fteqcc_name(), def.name))
}

/// The comment of `def`'s declaration.
fn docs(state: &ModuleState, data: &ModuleData, def: &Definition) -> Option<String> {
    let (decl, _, file) = declaration_node(state, data, def)?;
    doc_comment(data.parses.get(file.0)?, &decl)
}

/// The declaration node, name token and file of `def`'s first declaration, when its name is
/// written there.
fn declaration_node(
    state: &ModuleState,
    data: &ModuleData,
    def: &Definition,
) -> Option<(
    qc_syntax::SyntaxNode,
    qc_syntax::SyntaxToken,
    qc_sema::FileId,
)> {
    let location = def.location;
    if !name_at(state.db.file_text(location.file), location.span, &def.name) {
        return None;
    }
    let token = token_at(data.parses.get(location.file.0)?, location.span)?;
    let decl = declaration_of(&token)?;
    Some((decl, token, location.file))
}

/// The engines that have `def`, when it is a builtin.
fn builtin_info(state: &ModuleState, data: &ModuleData, def: &Definition) -> Option<BuiltinInfo> {
    if def.kind != DefKind::Function {
        return None;
    }
    let key = data.builtins.get(&def.name)?.clone();
    let engines = Engine::ALL
        .into_iter()
        .filter(|engine| engine.has_table(state.kind))
        .map(|engine| (engine, key.lookup(engine, state.kind)))
        .collect();
    Some(BuiltinInfo {
        key,
        kind: state.kind,
        target: state.engine,
        engines,
    })
}
