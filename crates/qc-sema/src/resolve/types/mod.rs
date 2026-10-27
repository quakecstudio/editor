// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseDefs
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_TypeForName

//! The type names a module declares and their checks (FTEF-19.20, FTEF-19.38): typedefs,
//! the names of structs, unions, classes, enums and accessors, and whether a written type
//! names one declared before it.

mod classes;
mod enums;
mod structs;

use qc_diag::Code;
use qc_syntax::c_scalar::{CScalar, REGISTERED_TYPES};

use crate::cast::Kind;
use crate::lower::{RawDecl, TypeKind};
use crate::resolve::{Resolver, TABLE_TYPES};
use crate::symbol::Location;
use crate::ty::{SimpleType, Type};

impl Resolver<'_> {
    /// The name of a type: FTEF-19.9 compares a `typedef` with the one of the same name before
    /// it, and records the structs (FTEF-19.15).
    pub(in crate::resolve) fn declare_type_name(
        &mut self,
        kind: TypeKind,
        decl: &RawDecl,
        location: Location,
    ) {
        if !self.profile.type_declarations() {
            return;
        }
        match kind {
            TypeKind::Struct => {
                self.structs.insert(decl.name.clone());
            }
            TypeKind::Typedef => {
                let Some(ty) = &decl.typedef else {
                    return;
                };
                // FTEF-19.20: the type a typedef names is read as any declaration's type is.
                if self.check_type_names(ty, &[], decl.span.start, location) {
                    return;
                }
                let ty = self.resolved(ty);
                // `QCC_TypeForName` finds any type by the name, in any case under
                // `-Fcaseinsens` (FTEF-19.33): a struct, class, accessor or enum is one
                // "Cannot redeclare typedef" names, unless the typedef names it itself.
                let itself = ty.type_name().is_some_and(|n| {
                    let bare = n
                        .strip_prefix("struct ")
                        .or_else(|| n.strip_prefix("union "))
                        .unwrap_or(n);
                    self.type_key(bare) == self.type_key(&decl.name)
                });
                if !itself
                    && ty != Type::Unknown
                    && let Some(other) = self.other_type_named(&decl.name)
                {
                    let message = format!(
                        "Cannot redeclare typedef {} from {other} to {}",
                        decl.name,
                        ty.fteqcc_name()
                    );
                    self.report(Code::E0219, location, message);
                    return;
                }
                // FTEF-7.5: the parameters a definition of the type takes, the typedef's own
                // or those of the typedef it names.
                let params = if decl.typedef_params.is_empty() {
                    decl.typedef
                        .as_ref()
                        .and_then(Type::type_name)
                        .and_then(|n| self.typedef_params.get(&self.type_key(n)).cloned())
                        .unwrap_or_default()
                } else {
                    decl.typedef_params.clone()
                };
                let key = self.type_key(&decl.name);
                self.typedef_params.entry(key).or_insert(params);
                match self.named(&self.typedefs, &decl.name) {
                    Some(old) if !old.fteqcc_same(&ty) => {
                        let message = format!(
                            "Cannot redeclare typedef {} from {} to {}",
                            decl.name,
                            old.fteqcc_name(),
                            ty.fteqcc_name()
                        );
                        self.report(Code::E0219, location, message);
                    }
                    Some(_) => {}
                    None => {
                        self.typedefs.insert(decl.name.clone(), ty);
                    }
                }
            }
            TypeKind::Enum => {
                // FTEF-19.10: `enum Name {` of a name already an enum's type reads the name as
                // the base type, the legacy form.
                // FTEF-19.10: `type = QCC_PR_ParseType(false, true, false)` finds any type's
                // name, a class's or struct's among them (FTEF-19.44).
                let other = self.other_sized_type_named(&decl.name)
                    || self.struct_defs.contains_key(&self.type_key(&decl.name));
                self.enum_redeclared = other || !self.enum_types.insert(decl.name.clone());
                if self.enum_redeclared {
                    self.report(
                        Code::W0224,
                        location,
                        "legacy enum base type. Use \"enum [class] [name_e]:type\" instead"
                            .to_owned(),
                    );
                }
            }
            TypeKind::Accessor => self.declare_accessor(decl, location),
            TypeKind::Class => {}
        }
    }

    /// The key `name` is found by in the type tables: itself, or in lower case under
    /// `-Fcaseinsens` (FTEF-19.33).
    pub(in crate::resolve) fn type_key(&self, name: &str) -> String {
        if self.profile.case_insensitive() {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        }
    }

    /// Whether a type named `name` has a size already, so a `struct` of that name given a body
    /// is compared with it (`if (newt->size)`, FTEF-6.10): a type FTEQCC registers, a class, an
    /// enum or a typedef.
    pub(in crate::resolve) fn sized_type_named(&self, name: &str) -> bool {
        let key = self.type_key(name);
        if let Some(def) = self.struct_defs.get(&key) {
            return def.defined;
        }
        self.other_sized_type_named(name)
    }

    /// Whether `name` is a type that is no struct and has a size: one FTEQCC registers, a
    /// class, an enum or a typedef.
    pub(in crate::resolve) fn other_sized_type_named(&self, name: &str) -> bool {
        let same = |other: &str| {
            if self.profile.case_insensitive() {
                other.eq_ignore_ascii_case(name)
            } else {
                other == name
            }
        };
        REGISTERED_TYPES.iter().any(|other| same(other))
            || CScalar::named(name).is_some()
            || self.class_names.keys().any(|other| same(other))
            || self.enum_types.iter().any(|other| same(other))
            || self.typedefs.keys().any(|other| same(other))
    }

    /// What another kind of type named `name` is, for "Cannot redeclare typedef" (FTEF-19.9): a
    /// struct, a class, an accessor or an enum, found in any case under `-Fcaseinsens`.
    pub(in crate::resolve) fn other_type_named(&self, name: &str) -> Option<String> {
        let key = self.type_key(name);
        if self.struct_defs.contains_key(&key) {
            return Some(format!("struct {name}"));
        }
        if self.has_name_key(&self.class_names, name) {
            return Some(format!("class {name}"));
        }
        if self.accessor_bases.contains_key(&key) {
            return Some(name.to_owned());
        }
        let same = |other: &String| {
            if self.profile.case_insensitive() {
                other.eq_ignore_ascii_case(name)
            } else {
                other == name
            }
        };
        self.enum_types
            .iter()
            .any(same)
            .then(|| format!("enum {name}"))
    }

    /// Whether a bitfield may have type `ty` (`type->type == ev_integer || ev_uint || ev_int64 ||
    /// ev_uint64`, FTEF-6.9): a type the model follows by its kind, a type a program names being
    /// one it does not follow.
    pub(in crate::resolve) fn bitfield_type(&self, ty: &Type) -> bool {
        match ty.fteqcc_kind() {
            Kind::Integer | Kind::Uint | Kind::Int64 | Kind::Uint64 => true,
            Kind::Opaque => ty.fteqcc_scalar().is_none() && matches!(ty, Type::Named(_)),
            Kind::Void
            | Kind::String
            | Kind::Float
            | Kind::Vector
            | Kind::Entity
            | Kind::Field
            | Kind::Function
            | Kind::Pointer
            | Kind::Double => false,
        }
    }

    /// FTEF-19.20: reports the first name in `ty` that is no type at `at`, "\"NAME\" is not a
    /// type", which ends the declaration; `typedefs` are a body's, each with where it stands.
    /// Returns whether one was reported.
    pub(in crate::resolve) fn check_type_names(
        &mut self,
        ty: &Type,
        typedefs: &[(String, usize)],
        at: usize,
        location: Location,
    ) -> bool {
        let Some(name) = self.unknown_type_name(ty, typedefs, at) else {
            return false;
        };
        let message = format!("\"{name}\" is not a type");
        self.report(Code::E0203, location, message);
        true
    }

    /// Whether `name` is a name FTEQCC registers, a type declared before, or one of a body's
    /// `typedefs` before `at` (FTEF-19.20); under `-Fcaseinsens` in any case, `QCC_TypeForName`'s
    /// table searched so (FTEF-19.33).
    pub(in crate::resolve) fn is_type_name(
        &self,
        name: &str,
        typedefs: &[(String, usize)],
        at: usize,
    ) -> bool {
        let exact = REGISTERED_TYPES.contains(&name)
            || self.type_names.contains(name)
            || typedefs.iter().any(|(t, start)| t == name && *start < at);
        exact
            || (self.profile.case_insensitive()
                && (REGISTERED_TYPES
                    .iter()
                    .chain(&TABLE_TYPES)
                    .any(|t| t.eq_ignore_ascii_case(name))
                    || (!self.profile.qcfuncs_off()
                        && ["string", "vector", "entity"]
                            .iter()
                            .any(|t| t.eq_ignore_ascii_case(name)))
                    || self.type_names.iter().any(|t| t.eq_ignore_ascii_case(name))
                    || typedefs
                        .iter()
                        .any(|(t, start)| t.eq_ignore_ascii_case(name) && *start < at)))
    }

    /// The first name in `ty` that names no type: none of the C type words FTEF-2.8 reads, a
    /// name FTEQCC registers, a type declared before, or one of `typedefs` before `at`. A name
    /// of more than one word that is no run of C words (`struct tag`, an inline enum or
    /// struct) is not checked (FTEF-19.32).
    pub(in crate::resolve) fn unknown_type_name<'t>(
        &self,
        ty: &'t Type,
        typedefs: &[(String, usize)],
        at: usize,
    ) -> Option<&'t str> {
        match ty {
            Type::Named(name) => {
                let known = name.contains(char::is_whitespace)
                    || ty.fteqcc_scalar().is_some()
                    || self.is_type_name(name, typedefs, at);
                (!known).then_some(name.as_ref())
            }
            // FTEF-19.28: `type_string = QCC_PR_NewType(flag_qcfuncs?"string":"__string",
            // ...)`, and so `vector` and `entity`.
            Type::Simple(
                simple @ (SimpleType::String | SimpleType::Vector | SimpleType::Entity),
            ) if self.profile.qcfuncs_off() && !self.is_type_name(simple.as_str(), typedefs, at) => {
                Some(simple.as_str())
            }
            Type::Field(inner) | Type::Pointer(inner) => {
                self.unknown_type_name(inner, typedefs, at)
            }
            Type::Function(func) => self.unknown_type_name(&func.ret, typedefs, at).or_else(|| {
                func.params
                    .iter()
                    .find_map(|p| self.unknown_type_name(p, typedefs, at))
            }),
            Type::Simple(_) | Type::Unknown => None,
        }
    }
}
