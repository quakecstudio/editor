// SPDX-License-Identifier: GPL-3.0-or-later

//! Completion: the names visible at the cursor.
//!
//! `qcc` has two scopes, the globals and the function being defined (QC-12.1), and a name
//! resolves at its position in compilation order (QC-12.5), so the names visible in a function
//! body are the globals, functions and fields declared before the cursor, and that function's
//! parameters and the locals declared before it. With FTEQCC's autoprototyping every global is
//! visible (FTE-22.3). After a member access, `.` or `->`, the members of what the access is
//! made on are offered: `x`, `y` and `z` after `.` on a vector outside vanilla QuakeC (FTE-9.1),
//! the members of a class and its parents and then the entity fields for a class
//! (FTEF-17.3), a struct's members, and the fields for an entity or a base whose type is not
//! known, since a field access names a field (QC-16.6). The base's type is read from the
//! names in it, as written: a name's definition, `this` (FTEF-17.14), and each field or member
//! of a chain. The client filters the list by what has been typed, as the protocol's
//! completion model lets it.

use std::collections::HashSet;
use std::path::Path;

use qc_project::Loader;
use qc_sema::{DefId, DefKind, Definition, FileId, ModuleIndex, SimpleType, Type};
use qc_syntax::SyntaxKind;
use qc_syntax::ast;

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
        let base = access.as_ref().map(|access| {
            let ty = access
                .base
                .as_ref()
                .map_or(Type::Unknown, |base| expr_type(index, file, function, base));
            resolved(index, ty, access.operator)
        });
        let mut seen: HashSet<&str> = HashSet::new();
        let mut out = Vec::new();
        match &base {
            // FTE-9.1: a vector's members are its three components; vanilla QuakeC has none.
            Some(Type::Simple(SimpleType::Vector))
                if state.dialect != qc_syntax::Dialect::Vanilla
                    && access
                        .as_ref()
                        .is_some_and(|a| a.operator == SyntaxKind::Dot) =>
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
            // A struct's or union's members (FTEF-6.13), and nothing else.
            Some(ty) if index.struct_shape(ty).is_some() => {
                return index
                    .struct_shape(ty)
                    .map(|shape| {
                        shape
                            .members
                            .iter()
                            .map(|member| Completion {
                                label: member.name.clone(),
                                kind: DefKind::Field,
                                detail: member.ty.fteqcc_name(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
            }
            // A value of another simple type has no members.
            Some(Type::Simple(simple)) if *simple != SimpleType::Entity => return Vec::new(),
            // FTEF-17.3: a class's members, its own and its parents', come before the fields.
            Some(ty) => {
                for def in index
                    .class_members(ty)
                    .into_iter()
                    .filter_map(|id| index.def(id))
                {
                    if seen.insert(def.name.as_str()) {
                        out.push(Completion {
                            label: def.name.clone(),
                            kind: def.kind,
                            detail: def.ty.fteqcc_name(),
                        });
                    }
                }
            }
            None => {}
        }
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

/// The type of `expr`, the base of a member access, as its names tell it in the file as
/// written: a name's definition, or for `this` in a member function its class
/// (FTEF-17.14); for `(e)` the type of `e`; for `a.b` the type `b` holds as a field or a member.
/// Unknown for anything else.
fn expr_type(index: &ModuleIndex, file: FileId, function: Option<DefId>, expr: &ast::Expr) -> Type {
    match expr {
        ast::Expr::Name(name) => {
            let Some(token) = name.name() else {
                return Type::Unknown;
            };
            if let Some(def) = index
                .definition_at(file, token.span().start)
                .and_then(|(def, _)| index.def(def))
            {
                return def.ty.clone();
            }
            // FTEF-17.14: `this` is the function's class, which names no definition of its own.
            if token.text() == b"this" {
                return function
                    .and_then(|function| index.def(function))
                    .and_then(|function| function.member_of)
                    .and_then(|class| index.def(class))
                    .map_or(Type::Unknown, |class| {
                        Type::Named(class.name.as_str().into())
                    });
            }
            Type::Unknown
        }
        ast::Expr::Paren(paren) => paren.expr().map_or(Type::Unknown, |inner| {
            expr_type(index, file, function, &inner)
        }),
        ast::Expr::Field(field) => {
            let Some(ast::Expr::Name(member)) = field.field() else {
                return Type::Unknown;
            };
            member
                .name()
                .and_then(|token| index.definition_at(file, token.span().start))
                .and_then(|(def, _)| index.def(def))
                .map_or(Type::Unknown, |def| match &def.ty {
                    // An entity field holds its type; a class's member is its type.
                    Type::Field(held) => held.as_ref().clone(),
                    other => other.clone(),
                })
        }
        _ => Type::Unknown,
    }
}

/// `ty` as a member access reads it: a typedef's type for its name, and after `->` what a
/// pointer points to (FTEF-4.1).
fn resolved(index: &ModuleIndex, ty: Type, operator: SyntaxKind) -> Type {
    let mut ty = index.alias(&ty).cloned().unwrap_or(ty);
    if operator == SyntaxKind::Arrow
        && let Type::Pointer(target) = &ty
    {
        ty = target.as_ref().clone();
        ty = index.alias(&ty).cloned().unwrap_or(ty);
    }
    ty
}
