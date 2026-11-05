// SPDX-License-Identifier: GPL-3.0-or-later

//! Completion: the names visible at the cursor.
//!
//! `qcc` has two scopes, the globals and the function being defined (QC-12.1), and a name
//! resolves at its position in compilation order (QC-12.5), so the names visible in a function
//! body are the globals, functions and fields declared before the cursor, and that function's
//! parameters and the locals declared before it. With FTEQCC's autoprototyping every global is
//! visible (FTE-22.3). After a member access, `.` or `->`, the members of what the access is
//! made on are offered: `x`, `y` and `z` for an FTEQCC vector (FTE-9.1), the members of a
//! class and its parents and then the entity fields for a class (FTEF-17.3), and the fields
//! for an entity or a base whose type is not known, since a field access names a field
//! (QC-16.6). The client filters the list by what has been typed, as the protocol's completion
//! model lets it.

use std::collections::HashSet;
use std::path::Path;

use qc_diag::Span;
use qc_project::Loader;
use qc_sema::{DefId, DefKind, Definition, FileId, ModuleIndex, SimpleType, Type};

use crate::host::AnalysisHost;
use crate::syntax::{enclosing_function, member_access};

/// One name to offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    /// The name.
    pub label: String,
    /// What it is.
    pub kind: DefKind,
    /// Its type, as FTEQCC writes it, such as `float(entity)`.
    pub detail: String,
}

impl<L: Loader> AnalysisHost<L> {
    /// The names visible at byte `offset` of `path`, each once, in declaration order; read in
    /// the first module that compiles the file.
    pub fn completions(&mut self, path: &Path, offset: usize) -> Vec<Completion> {
        let Some(&(module, file)) = self.modules_of(path).first() else {
            return Vec::new();
        };
        let Some((state, data)) = self.module(module) else {
            return Vec::new();
        };
        let Some(parse) = data.parses.get(file.index()) else {
            return Vec::new();
        };
        let index = &data.analysis.index;
        // QC-12.5: a name resolves at its position in compilation order, so only the globals
        // declared before the cursor are visible; with autoprototyping every one is (FTE-22.3).
        let declared = if state.autoproto() {
            None
        } else {
            declared_before(index, file, offset)
        };
        let access = member_access(parse, offset);
        let fields_only = access.is_some();
        let function: Option<DefId> = enclosing_function(parse, offset)
            .and_then(|name| index.definition_at(file, name.span().start))
            .map(|(def, _)| def);
        let base = access
            .flatten()
            .map(|span| base_type(index, file, function, span));
        // FTE-9.1: a vector's members are its three components.
        if state.dialect == qc_syntax::Dialect::Fteqcc
            && base == Some(Type::Simple(SimpleType::Vector))
        {
            return ["x", "y", "z"]
                .into_iter()
                .map(|label| Completion {
                    label: label.to_owned(),
                    kind: DefKind::Field,
                    detail: Type::Simple(SimpleType::Float).fteqcc_name(),
                })
                .collect();
        }
        // FTEF-17.3: a class's members, its own and its parents', come before the fields.
        let members = base
            .as_ref()
            .map(|ty| index.class_members(ty))
            .unwrap_or_default();
        let visible = |id: usize, def: &Definition| -> bool {
            if def.generated || def.member_of.is_some() {
                return false;
            }
            if def.kind.is_global() && declared.is_some_and(|end| id >= end) {
                return false;
            }
            if fields_only {
                return def.kind == DefKind::Field;
            }
            match def.kind {
                DefKind::Global | DefKind::Function | DefKind::Field => true,
                DefKind::Param => function.is_some() && def.owner == function,
                DefKind::Local => {
                    function.is_some()
                        && def.owner == function
                        && def.location.file == file
                        && def.location.span.start < offset
                }
            }
        };
        let mut seen: HashSet<&str> = HashSet::new();
        let mut out = Vec::new();
        for def in members.iter().filter_map(|&id| index.def(id)) {
            if seen.insert(def.name.as_str()) {
                out.push(Completion {
                    label: def.name.clone(),
                    kind: def.kind,
                    detail: def.ty.fteqcc_name(),
                });
            }
        }
        for (id, def) in index.defs().iter().enumerate() {
            if visible(id, def) && seen.insert(def.name.as_str()) {
                out.push(Completion {
                    label: def.name.clone(),
                    kind: def.kind,
                    detail: def.ty.fteqcc_name(),
                });
            }
        }
        out
    }
}

/// How many definitions are declared before byte `offset` of `file`: those before the last
/// definition of the file before `offset`, and it, as definitions are in declaration order
/// (QC-12.2); `None` when the file declares nothing, so where it is compiled is not known.
fn declared_before(index: &ModuleIndex, file: FileId, offset: usize) -> Option<usize> {
    let mut first = None;
    let mut last = None;
    for (id, def) in index.defs().iter().enumerate() {
        if def.location.file != file {
            continue;
        }
        first = first.or(Some(id));
        if def.location.span.start < offset {
            last = Some(id);
        }
    }
    match last {
        Some(id) => id.checked_add(1),
        None => first,
    }
}

/// The type of the expression at `span`, the base of a member access: as the check typed it in
/// the body of `function`, else, for a name, its definition's type; unknown otherwise.
fn base_type(index: &ModuleIndex, file: FileId, function: Option<DefId>, span: Span) -> Type {
    let typed = function
        .and_then(|function| index.body(function))
        .and_then(|body| body.type_at(span))
        .cloned();
    typed
        .or_else(|| {
            index
                .definition_at(file, span.start)
                .and_then(|(def, _)| index.def(def))
                .map(|def| def.ty.clone())
        })
        .unwrap_or(Type::Unknown)
}
