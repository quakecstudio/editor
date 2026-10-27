// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_GetDef
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_GetDef

//! The scopes of a module and the lookup of a name in them (QC-12): the global scope, the
//! function being resolved, and FTEQCC's scoped names, case-insensitive lookup and `for`
//! statement scopes (FTE-22.1, FTEF-16.4, FTEF-19.19, FTEF-19.33).

use qc_diag::Code;
use qc_syntax::Keyword;
use std::collections::{HashMap, HashSet};

use crate::resolve::{Found, Resolver, Scope, component_type, is_intrinsic};
use crate::symbol::{DefId, DefKind};
use crate::ty::{SimpleType, Type};

impl Resolver<'_> {
    /// Looks up a global name: the first global definition with that name (QC-12.2). Under
    /// FTEQCC a `static` global of another unit is not seen (FTEF-19.12), and the next
    /// definition of the name is.
    pub(in crate::resolve) fn lookup_global(&self, name: &str) -> Option<DefId> {
        let defs = self.globals.get(name)?;
        defs.iter().copied().find(|def| {
            !self.fteqcc
                || self
                    .facts
                    .get(def.0)
                    .and_then(|f| f.statics)
                    .is_none_or(|unit| unit == self.unit)
        })
    }

    /// What `table` holds for `name`: the entry written so, else under `-Fcaseinsens` the one
    /// whose name differs only in case, as `Hash_GetInsensitive` finds it (FTEF-19.33).
    pub(in crate::resolve) fn named<'t, V>(
        &self,
        table: &'t HashMap<String, V>,
        name: &str,
    ) -> Option<&'t V> {
        table.get(name).or_else(|| {
            if !self.case_insensitive {
                return None;
            }
            table
                .iter()
                .filter(|(key, _)| key.eq_ignore_ascii_case(name))
                .max_by(|a, b| a.0.cmp(b.0))
                .map(|(_, value)| value)
        })
    }

    /// Whether `table` has an entry for `name` ([`Self::named`]).
    pub(in crate::resolve) fn has_name_key<V>(
        &self,
        table: &HashMap<String, V>,
        name: &str,
    ) -> bool {
        self.named(table, name).is_some()
    }

    /// The type name the program defined that `name` is, in any case under `-Fcaseinsens`
    /// (FTEF-19.33): the one written so, else the greatest of those that differ in case.
    pub(in crate::resolve) fn type_name_spelled(&self, name: &str) -> Option<&str> {
        if let Some(exact) = self.type_names.get(name) {
            return Some(exact.as_str());
        }
        if !self.case_insensitive {
            return None;
        }
        self.type_names
            .iter()
            .filter(|t| t.eq_ignore_ascii_case(name))
            .max()
            .map(String::as_str)
    }

    /// Whether `set` holds `name`, in any case under `-Fcaseinsens` (FTEF-19.33).
    pub(in crate::resolve) fn has_name(&self, set: &HashSet<String>, name: &str) -> bool {
        set.contains(name)
            || (self.case_insensitive && set.iter().any(|t| t.eq_ignore_ascii_case(name)))
    }

    /// Whether the global `def` lies below `before` in `qcc`'s global area (QC-13.5): the
    /// area grows as definitions are made, so a definition made earlier lies lower, but for a
    /// vector and its `_x` component, which share a word.
    pub(in crate::resolve) fn below(&self, def: DefId, before: DefId) -> bool {
        if def >= before {
            return false;
        }
        let x_component = self
            .index
            .def(before)
            .zip(self.index.def(def))
            .is_some_and(|(b, d)| {
                b.name == format!("{}_x", d.name) && component_type(&d.ty).is_some()
            });
        !x_component
    }

    /// Whether a value of type `ty` cannot be called (`not a function`, FTEF-19.40): a float,
    /// vector, string, entity or struct.
    pub(in crate::resolve) fn not_callable(&self, ty: &Type) -> bool {
        match self.resolved(ty) {
            Type::Simple(
                SimpleType::Float | SimpleType::Vector | SimpleType::String | SimpleType::Entity,
            ) => true,
            Type::Named(name) => {
                let bare = name
                    .strip_prefix("struct ")
                    .or_else(|| name.strip_prefix("union "))
                    .unwrap_or(&name);
                self.struct_defs.contains_key(&self.type_key(bare))
            }
            Type::Simple(_)
            | Type::Field(_)
            | Type::Function(_)
            | Type::Pointer(_)
            | Type::Unknown => false,
        }
    }

    /// A plain name used as a value: the function's own, then in a member function the
    /// class's members (FTEF-17.2), then a global (QC-12.2, FTE-22.1); under FTEQCC then an
    /// intrinsic (FTEF-17.4), `this` and `super` only in a member function (FTEF-19.16).
    pub(in crate::resolve) fn resolve_value(
        &self,
        scope: Option<Scope>,
        name: &str,
        type_ok: bool,
    ) -> Found {
        let def = match scope {
            Some(scope) => self.lookup_in_scope(scope, name),
            None => self.lookup_used_global(name),
        };
        if let Some(def) = def {
            // FTEF-19.38: a type's name is no value; `QCC_PR_ParseValue` finds no definition by
            // it.
            if self.fteqcc && !type_ok && self.type_defs.contains(&def) {
                return Found::Unknown;
            }
            return Found::Def(def);
        }
        if !self.fteqcc || !is_intrinsic(name) {
            return Found::Unknown;
        }
        // FTEF-20.11: `keyword_class && !strcmp(name, "this")`, and `super` so.
        if matches!(name, "this" | "super") && !self.keywords.has(Keyword::Class) {
            return Found::Unknown;
        }
        let class = scope.and_then(|s| s.class);
        match name {
            "this" if class.is_none() && scope.is_some() => Found::Error(
                Code::E0310,
                "Cannot use 'this' outside of an OO function".to_owned(),
            ),
            "super" if class.is_none() && scope.is_some() => Found::Error(
                Code::E0311,
                "Cannot use 'super' outside of an OO function".to_owned(),
            ),
            "super" => match class {
                Some(class) if self.classes.get(&class).is_some_and(|c| c.parent.is_none()) => {
                    let class_name = self
                        .index
                        .def(class)
                        .map(|d| d.name.clone())
                        .unwrap_or_default();
                    Found::Error(Code::E0312, format!("class {class_name} has no super"))
                }
                Some(_) | None => Found::Nothing,
            },
            _ => Found::Nothing,
        }
    }

    /// `Class::name` or `super::name` (FTEF-17.4): the member of the class, or of the
    /// function's class's parent; a scope that is no class (an enum's, for one) names a
    /// global.
    pub(in crate::resolve) fn resolve_scoped(
        &self,
        scope: Option<Scope>,
        prefix: &str,
        name: &str,
    ) -> Found {
        let class = if prefix == "super" {
            scope.and_then(|s| s.class).and_then(|c| self.parent_of(c))
        } else {
            self.named(&self.class_names, prefix).copied()
        };
        // FTEF-19.33: the scope of `Scope::name` is read as a type name.
        if prefix != "super"
            && class.is_none()
            && self
                .unknown_type_name(&Type::Named(prefix.into()), &[], 0)
                .is_some()
        {
            return Found::Error(Code::E0203, format!("\"{prefix}\" is not a type"));
        }
        // FTEF-6.11: a struct's members are reached through a value, never as `Struct::name`.
        if class.is_none() && self.struct_defs.contains_key(&self.type_key(prefix)) {
            return Found::Unknown;
        }
        if let Some(class) = class
            && let Some(def) = self.member_in(class, name)
        {
            return Found::Def(def);
        }
        // A definition named `Scope::name`, as a strict enum's value is (FTEF-19.10), comes
        // before the plain name.
        // FTEF-19.33: the type is found in any case, and the value it names is the one written
        // (`strcmp (acc->fieldname, name)`).
        let spelled = self
            .type_name_spelled(prefix)
            .map_or_else(|| prefix.to_owned(), str::to_owned);
        if let Some(def) = self.lookup_global(&format!("{spelled}::{name}")) {
            return Found::Def(def);
        }
        if let Some(def) = self.lookup_global(name) {
            return Found::Def(def);
        }
        if prefix == "super" && class.is_none() {
            // `super::` of a class whose parent is the entity, or that the model did not
            // find: the parent is not known, so the name is not checked.
            return Found::Nothing;
        }
        Found::Unknown
    }

    /// Whether `name` selects a member that is no definition: a vector's `x`, `y` or `z`
    /// (FTE-9.1), an array's `length` (FTE-13.3), or a field of a struct or accessor the
    /// module defines (FTE-14.2, FTEF-6.5, FTEF-10.2).
    pub(in crate::resolve) fn is_member_name(&self, name: &str) -> bool {
        matches!(name, "x" | "y" | "z" | "length") || self.has_name(&self.members, name)
    }

    /// The parameter or local of the function in `scope` named `name` (QC-12.1), but for one
    /// whose `for` statement ended before the lookup (FTEF-19.19), the newest first, as
    /// `localstable` gives them (FTEF-21.12).
    pub(in crate::resolve) fn lookup_local(&self, scope: Scope, name: &str) -> Option<DefId> {
        self.index
            .defs
            .iter()
            .enumerate()
            .skip(scope.locals_start)
            .rev()
            .find(|(index, d)| {
                d.owner == Some(scope.owner) && d.name == name && self.in_scope_at(*index, scope.at)
            })
            .map(|(index, _)| DefId(index))
    }

    /// Whether the definition at `index` is in scope at byte `at`: not a `for` statement's
    /// local whose statement ended before it (FTEF-19.19).
    pub(in crate::resolve) fn in_scope_at(&self, index: usize, at: usize) -> bool {
        let facts = self.facts.get(index);
        facts.and_then(|f| f.scope_end).is_none_or(|end| at < end)
            && facts
                .and_then(|f| f.scope_start)
                .is_none_or(|start| at >= start)
    }

    /// A global name as a use finds it (QC-12.2): as written, then under `-Fcaseinsens` in any
    /// case (FTEF-19.33).
    pub(in crate::resolve) fn lookup_used_global(&self, name: &str) -> Option<DefId> {
        self.lookup_global(name)
            .or_else(|| self.lookup_any_case(None, name))
    }

    /// Looks up a name in a function's scope (QC-12.2). In vanilla the first definition in
    /// the list that is global or the function's own is the global when there is one, since
    /// a local cannot shadow it (QC-12.3); in an FTEQCC profile the function's own comes
    /// first, since it shadows the global (FTE-22.1), then in a member function the class's
    /// members (FTEF-17.2). A local of another function, declared earlier, is invisible
    /// (QC-12.4).
    pub(in crate::resolve) fn lookup_in_scope(&self, scope: Scope, name: &str) -> Option<DefId> {
        self.lookup_in_scope_exactly(scope, name)
            .or_else(|| self.lookup_any_case(Some(scope), name))
    }

    /// FTEF-19.33: under `-Fcaseinsens`, a name no definition has as written is the last local
    /// of the function, then the last global, whose name is it in any ASCII case, as the hash
    /// table's chain gives the newest first:
    /// `QCC_PR_GetDef`'s second pass (`ins == 1`), which a lookup that allocates nothing makes
    /// (`insmax = allocate?1:2;`).
    pub(in crate::resolve) fn lookup_any_case(
        &self,
        scope: Option<Scope>,
        name: &str,
    ) -> Option<DefId> {
        if !self.case_insensitive {
            return None;
        }
        let local = scope.and_then(|scope| {
            self.index
                .defs
                .iter()
                .enumerate()
                .skip(scope.locals_start)
                .rev()
                .find(|(index, d)| {
                    d.owner == Some(scope.owner)
                        && d.name.eq_ignore_ascii_case(name)
                        && self.in_scope_at(*index, scope.at)
                })
                .map(|(index, _)| DefId(index))
        });
        local.or_else(|| {
            self.globals
                .keys()
                .filter(|global| global.eq_ignore_ascii_case(name))
                .filter_map(|global| self.lookup_global(global))
                .max()
        })
    }

    /// [`Resolver::lookup_in_scope`] of the name as written.
    pub(in crate::resolve) fn lookup_in_scope_exactly(
        &self,
        scope: Scope,
        name: &str,
    ) -> Option<DefId> {
        if self.fteqcc {
            // A class's name is the type, not its constructor member (FTEF-17.14).
            // FTEF-17.14: a field named bare is `this.field`, reached through the global
            // `self` (`QCC_PR_GetDef(NULL, "self", NULL, true, 0, false)`), without which
            // FTEQCC ends; a member function is found either way.
            let has_self = self.lookup_global("self").is_some();
            let member = || {
                scope
                    .class
                    .filter(|_| !self.has_name_key(&self.class_names, name))
                    .and_then(|c| self.member_in(c, name))
                    .filter(|id| {
                        has_self
                            || self
                                .index
                                .def(*id)
                                .is_some_and(|d| d.kind == DefKind::Function)
                    })
            };
            self.lookup_local(scope, name)
                .or_else(member)
                .or_else(|| self.lookup_global(name))
        } else {
            self.lookup_global(name)
                .or_else(|| self.lookup_local(scope, name))
        }
    }
}
