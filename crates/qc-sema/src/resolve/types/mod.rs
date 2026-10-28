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
mod layout;
mod structs;

pub(crate) use layout::Layout;

use qc_diag::{Code, Span};
use qc_syntax::c_scalar::{CContext, CScalar, REGISTERED_TYPES};

use crate::cast::Kind;
use crate::check::Shape;
use crate::check::etype::Etype;
use crate::lower::{RawDecl, TypeKind};
use crate::resolve::{MAX_TYPEDEF_DEPTH, Resolver, TABLE_TYPES};
use crate::symbol::Location;
use crate::ty::{FunctionType, SimpleType, Type};

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
                // FTEF-19.68: and a type FTEQCC defines itself, which the typedef must name.
                if let Some(builtin) = builtin_type(&decl.name)
                    && ty != Type::Unknown
                    && !ty.fteqcc_same(&builtin)
                {
                    let message = format!(
                        "Cannot redeclare typedef {} from {} to {}",
                        decl.name,
                        builtin.fteqcc_name(),
                        ty.fteqcc_name()
                    );
                    self.report(Code::E0219, location, message);
                    return;
                }
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
                self.enum_redeclared = other || self.enum_types.contains_key(&decl.name);
                if !self.enum_redeclared {
                    self.enum_types.insert(decl.name.clone(), decl.ty.clone());
                }
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
            || self.enum_types.keys().any(|other| same(other))
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
            .keys()
            .any(same)
            .then(|| format!("enum {name}"))
    }

    /// Whether a bitfield may have type `ty` (`type->type == ev_integer || ev_uint || ev_int64 ||
    /// ev_uint64`, FTEF-6.9): a type the model follows by its kind, a type a program names being
    /// one it does not follow.
    pub(in crate::resolve) fn bitfield_type(&self, ty: &Type) -> bool {
        // An 8- or 16-bit integer is `ev_bitfld` itself, no integer type.
        if matches!(
            ty.fteqcc_scalar(),
            Some(CScalar::Int8 | CScalar::Uint8 | CScalar::Int16 | CScalar::Uint16)
        ) {
            return false;
        }
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
        typedefs: &[(String, Span)],
        at: usize,
        location: Location,
    ) -> bool {
        let Some(name) = self.unknown_type_name(ty, typedefs, at) else {
            return false;
        };
        // FTEF-21.22: a first parameter of a name that is no type makes
        // `QCC_PR_ParseFunctionType` return none, "expected function arg list".
        if self.unknown_first_param(ty, typedefs, at) {
            let message = "expected function arg list".to_owned();
            self.report(Code::E0204, location, message);
            return true;
        }
        let message = format!("\"{name}\" is not a type");
        self.report(Code::E0203, location, message);
        true
    }

    /// Whether the first name in `ty` that is no type is a function type's first parameter.
    fn unknown_first_param(&self, ty: &Type, typedefs: &[(String, Span)], at: usize) -> bool {
        match ty {
            Type::Field(inner) | Type::Pointer(inner) => {
                self.unknown_first_param(inner, typedefs, at)
            }
            Type::Function(func) => {
                if self.unknown_type_name(&func.ret, typedefs, at).is_some() {
                    return self.unknown_first_param(&func.ret, typedefs, at);
                }
                let Some(first) = func.params.first() else {
                    return false;
                };
                let mut base = first;
                while let Type::Pointer(inner) = base {
                    base = inner;
                }
                if matches!(base, Type::Named(_))
                    && self.unknown_type_name(base, typedefs, at).is_some()
                {
                    return true;
                }
                func.params
                    .iter()
                    .find(|p| self.unknown_type_name(p, typedefs, at).is_some())
                    .is_some_and(|p| self.unknown_first_param(p, typedefs, at))
            }
            Type::Named(_) | Type::Simple(_) | Type::Unknown => false,
        }
    }

    /// Whether `name` is a name FTEQCC registers, a type declared before, or one of a body's
    /// `typedefs` before `at` (FTEF-19.20); under `-Fcaseinsens` in any case, `QCC_TypeForName`'s
    /// table searched so (FTEF-19.33).
    pub(in crate::resolve) fn is_type_name(
        &self,
        name: &str,
        typedefs: &[(String, Span)],
        at: usize,
    ) -> bool {
        let exact = REGISTERED_TYPES.contains(&name)
            || self.type_names.contains(name)
            || typedefs
                .iter()
                .any(|(t, seen)| t == name && seen.start < at && at < seen.end);
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
                    || typedefs.iter().any(|(t, seen)| {
                        t.eq_ignore_ascii_case(name) && seen.start < at && at < seen.end
                    })))
    }

    /// The first name in `ty` that names no type: none of the C type words FTEF-2.8 reads, a
    /// name FTEQCC registers, a type declared before, or one of `typedefs` before `at`. A name
    /// of more than one word that is no run of C words (`struct tag`, an inline enum or
    /// struct) is not checked (FTEF-19.63).
    pub(in crate::resolve) fn unknown_type_name<'t>(
        &self,
        ty: &'t Type,
        typedefs: &[(String, Span)],
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
            ) if self.profile.qcfuncs_off()
                && !self.is_type_name(simple.as_str(), typedefs, at) =>
            {
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

impl Resolver<'_> {
    /// What `ty` is at bottom for the check stage (FTEF-23.3): through typedefs, accessors and
    /// strict enums; `None` for a type the model does not follow, on which nothing is checked.
    pub(in crate::resolve) fn shape(&self, ty: &Type) -> Option<Shape> {
        let ty = self.resolved(ty);
        let plain = |etype| Some(Shape { etype, holds: None });
        match &ty {
            Type::Unknown => None,
            Type::Simple(SimpleType::Void) => plain(Etype::Void),
            Type::Simple(SimpleType::Float) => plain(Etype::Float),
            Type::Simple(SimpleType::Vector) => plain(Etype::Vector),
            Type::Simple(SimpleType::String) => plain(Etype::String),
            Type::Simple(SimpleType::Entity) => plain(Etype::Entity),
            Type::Function(_) => plain(Etype::Function),
            Type::Field(inner) => Some(Shape {
                etype: Etype::Field,
                holds: self.shape(inner).map(|s| s.etype),
            }),
            Type::Pointer(inner) => Some(Shape {
                etype: Etype::Pointer,
                holds: self.shape(inner).map(|s| s.etype),
            }),
            Type::Named(name) => self.named_shape(&ty, name),
        }
    }

    /// The shape of the named type `ty`, `name`.
    fn named_shape(&self, ty: &Type, name: &str) -> Option<Shape> {
        let plain = |etype| Some(Shape { etype, holds: None });
        match ty.fteqcc_kind() {
            Kind::Integer => return plain(Etype::Integer),
            Kind::Uint => return plain(Etype::Uint),
            Kind::Int64 => return plain(Etype::Int64),
            Kind::Uint64 => return plain(Etype::Uint64),
            Kind::Double => return plain(Etype::Double),
            Kind::Float => return plain(Etype::Float),
            Kind::String => return plain(Etype::String),
            Kind::Vector => return plain(Etype::Vector),
            Kind::Entity => return plain(Etype::Entity),
            Kind::Void | Kind::Field | Kind::Function | Kind::Pointer | Kind::Opaque => {}
        }
        if matches!(name, "__variant" | "variant") {
            return plain(Etype::Variant);
        }
        // FTEF-2.12: an 8- or 16-bit integer is `ev_bitfld`, read, stored and cast as its
        // `parentclass`, `int` or `uint` (`QCC_TryEvaluateCast`, `QCC_LoadFromPointer`).
        match ty.fteqcc_scalar() {
            Some(CScalar::Int8 | CScalar::Int16) => return plain(Etype::Integer),
            Some(CScalar::Uint8 | CScalar::Uint16) => return plain(Etype::Uint),
            _ => {}
        }
        if let Some(base) = self.named(&self.strict_enums, name) {
            return Some(Shape {
                etype: Etype::Enum,
                holds: self.shape(base).map(|s| s.etype),
            });
        }
        if let Some(base) = self.named(&self.accessor_bases, &self.type_key(name)) {
            return self.shape(base);
        }
        // FTEF-8.6: a named enum's type is of its base type's kind.
        if let Some(base) = self.named(&self.enum_types, name) {
            return self.shape(base);
        }
        if self.has_name_key(&self.class_names, name) {
            return plain(Etype::Entity);
        }
        if let Some(bare) = self.struct_type_name(name) {
            let union = self
                .struct_defs
                .get(&self.type_key(bare))
                .is_some_and(|st| st.union);
            return plain(if union { Etype::Union } else { Etype::Struct });
        }
        None
    }
}

impl Resolver<'_> {
    /// `TypeName`: a type as FTEQCC's messages print it (FTEF-24.7): a class `class NAME`, a
    /// struct or union `struct NAME` or `union NAME`, a strict enum `enum NAME:BASE`, a pointer
    /// `T*`, a field `.T`, and every other type by its name.
    pub(in crate::resolve) fn type_name(&self, ty: &Type) -> String {
        match ty {
            Type::Pointer(inner) => format!("{}*", self.type_name(inner)),
            // `*ret++ = '.'`, which the class branch then overwrites with `class NAME`.
            Type::Field(inner) => match self.class_type_name(inner) {
                Some(class) => class,
                // The field type's own name, `.NAME`, for a struct or union.
                None => match self.resolved(inner) {
                    Type::Named(name) => match self.struct_type_name(&name) {
                        Some(bare) => format!(".{bare}"),
                        None => format!(".{}", self.type_name(inner)),
                    },
                    _ => format!(".{}", self.type_name(inner)),
                },
            },
            Type::Named(name) => {
                if let Some(class) = self.class_type_name(ty) {
                    return class;
                }
                if let Some(base) = self.named(&self.strict_enums, name) {
                    return format!("enum {name}:{}", self.type_name(base));
                }
                // FTEF-24.7: a named union is made `ev_struct` (`QCC_PR_NewType(tname, ev_struct,
                // ...)`) and prints as a struct; one with no tag is `<struct>` or `<union>`.
                if let Some(bare) = self.struct_type_name(name) {
                    if bare.starts_with("{}") {
                        let kind = if name.starts_with("union ") {
                            "union"
                        } else {
                            "struct"
                        };
                        return format!("{kind} <{kind}>");
                    }
                    return format!("struct {bare}");
                }
                let resolved = self.resolved(ty);
                if &resolved == ty {
                    ty.fteqcc_name()
                } else {
                    self.type_name(&resolved)
                }
            }
            Type::Function(_) => ty.fteqcc_name_with(&|inner| self.type_name(inner)),
            Type::Unknown | Type::Simple(_) => ty.fteqcc_name(),
        }
    }

    /// `class NAME` when `ty` names a class.
    fn class_type_name(&self, ty: &Type) -> Option<String> {
        let Type::Named(name) = ty else {
            return None;
        };
        self.has_name_key(&self.class_names, name)
            .then(|| format!("class {name}"))
    }
}

impl Resolver<'_> {
    /// `typecmp_lax` (`qcc_pr_lex.c`), whether `a` and `b` are one type to a conditional's arms
    /// and a store (FTEF-30.3, FTEF-29.7): of one basic type and size, or one a `variant` with
    /// nothing held, and what a field, a pointer or a function holds the same in turn; a
    /// function's parameters of one basic type pairwise, a class or struct parameter by name,
    /// as many on each side, and both variadic or neither. Class names are not compared; a
    /// class's members are, as a function's parameters (FTEF-9.14), and an entity has none, so
    /// two classes with the same members are one. A type the model does not follow is any.
    pub(in crate::resolve) fn typecmp_lax(&self, a: &Type, b: &Type) -> bool {
        self.typecmp_lax_within(a, b, MAX_TYPEDEF_DEPTH)
    }

    /// [`Self::typecmp_lax`] of what `a` and `b` hold to `depth` levels: one type is itself
    /// (`if (a == b) return 0;`), which ends a class holding its own type, and two types
    /// compared no deeper are one.
    fn typecmp_lax_within(&self, a: &Type, b: &Type, depth: usize) -> bool {
        let (a, b) = (self.resolved(a), self.resolved(b));
        let Some(depth) = depth.checked_sub(1) else {
            return true;
        };
        if a == b {
            return true;
        }
        // FTEF-23.8: `__bint` and `__bfloat` are `ev_boolean`, which no other type is.
        let boolean =
            |ty: &Type| matches!(ty.fteqcc_scalar(), Some(CScalar::Bool | CScalar::BFloat));
        if boolean(&a) || boolean(&b) {
            return boolean(&a) && boolean(&b);
        }
        let (Some(sa), Some(sb)) = (self.shape(&a), self.shape(&b)) else {
            return true;
        };
        let variant = sa.etype == Etype::Variant || sb.etype == Etype::Variant;
        if sa.etype != sb.etype && !variant {
            return false;
        }
        if sa.etype == sb.etype && a.word_size() != b.word_size() {
            return false;
        }
        match (&a, &b) {
            (Type::Field(x), Type::Field(y)) | (Type::Pointer(x), Type::Pointer(y)) => {
                self.typecmp_lax_within(x, y, depth)
            }
            // `typecmp_lax` compares the parameters both have (`minargs`), any more one has
            // being `optional`, and not whether either takes `...` (FTEF-30.14).
            (Type::Function(x), Type::Function(y)) => {
                let both = x.params.len().min(y.params.len());
                let optional_rest = |f: &FunctionType| {
                    (both..f.params.len())
                        .all(|at| f.labels.0.get(at).is_some_and(|label| label.optional))
                };
                self.typecmp_lax_within(&x.ret, &y.ret, depth)
                    && x.params
                        .iter()
                        .zip(&y.params)
                        .all(|(p, q)| self.same_param(p, q, depth))
                    && optional_rest(x)
                    && optional_rest(y)
            }
            // What one holds against nothing the other holds.
            (Type::Field(_) | Type::Pointer(_) | Type::Function(_), _)
            | (_, Type::Field(_) | Type::Pointer(_) | Type::Function(_)) => false,
            _ if sa.etype == Etype::Entity && sb.etype == Etype::Entity => {
                let (x, y) = (self.compared_members(&a), self.compared_members(&b));
                x.len() == y.len() && x.iter().zip(&y).all(|(p, q)| self.same_param(p, q, depth))
            }
            _ => true,
        }
    }

    /// Whether `ty` names an accessor, `ev_accessor` (FTEF-10.8).
    pub(in crate::resolve) fn is_accessor_type(&self, ty: &Type) -> bool {
        match self.resolved(ty) {
            Type::Named(name) => self
                .named(&self.accessor_bases, &self.type_key(&name))
                .is_some(),
            _ => false,
        }
    }

    /// `ty` with its typedefs resolved, and a named enum that is no `enum class` its base type,
    /// the type `QCC_PR_ParseEnum` gives its name (FTEF-19.10).
    fn plain_enum_base(&self, ty: &Type) -> Type {
        let ty = self.resolved(ty);
        match &ty {
            Type::Named(name) if self.named(&self.strict_enums, name).is_none() => self
                .named(&self.enum_types, name)
                .map_or(ty.clone(), |base| self.resolved(base)),
            _ => ty,
        }
    }

    /// `typecmp` of two values' types (FTEF-30.3): the same type object, so one C type word or
    /// typedef of it, two classes or structs by name, and what fields, pointers and functions
    /// hold in turn. A type the model does not follow is any.
    pub(in crate::resolve) fn typecmp(&self, a: &Type, b: &Type) -> bool {
        let (a, b) = (self.plain_enum_base(a), self.plain_enum_base(b));
        if a == b {
            return true;
        }
        // FTEF-23.8: `__bint` and `__bfloat` are both `ev_boolean`, one type to `typecmp`, and
        // no other type is.
        let boolean =
            |ty: &Type| matches!(ty.fteqcc_scalar(), Some(CScalar::Bool | CScalar::BFloat));
        if boolean(&a) || boolean(&b) {
            return boolean(&a) && boolean(&b);
        }
        match (&a, &b) {
            (Type::Field(x), Type::Field(y)) | (Type::Pointer(x), Type::Pointer(y)) => {
                self.typecmp(x, y)
            }
            (Type::Function(x), Type::Function(y)) => {
                x.variadic == y.variadic
                    && x.params.len() == y.params.len()
                    && self.typecmp(&x.ret, &y.ret)
                    && x.params
                        .iter()
                        .zip(&y.params)
                        .all(|(p, q)| self.typecmp(p, q))
            }
            (Type::Unknown, _) | (_, Type::Unknown) => true,
            // `typecmp` compares two structs or unions by their size and their name, which is
            // `<struct>` or `<union>` for every one with no tag (FTEF-6.12).
            _ if self.is_struct_type(&a) && self.is_struct_type(&b) => {
                self.type_name(&a) == self.type_name(&b)
                    && self.layout(&a).map(|l| l.words) == self.layout(&b).map(|l| l.words)
            }
            _ => match (a.fteqcc_kind(), b.fteqcc_kind()) {
                (Kind::Opaque, Kind::Opaque) => a.fteqcc_name() == b.fteqcc_name(),
                (x, y) => x == y,
            },
        }
    }

    /// The types of the members a class compares (FTEF-9.13, FTEF-9.14), its `params`: none for
    /// an entity or a class given no body.
    fn compared_members(&self, ty: &Type) -> Vec<Type> {
        self.class_of(ty)
            .and_then(|class| self.classes.get(&class)?.shape.as_ref())
            .map(|shape| shape.iter().map(|(_, ty, _)| ty.clone()).collect())
            .unwrap_or_default()
    }

    /// `typecmp_lax`'s test of a pair of parameters: one basic type, and an entity, struct or
    /// union by its name, anything else as [`Self::typecmp_lax`] says.
    pub(in crate::resolve) fn same_param(&self, p: &Type, q: &Type, depth: usize) -> bool {
        let (p, q) = (self.resolved(p), self.resolved(q));
        // `a->params[t].type->type != b->params[t].type->type`: an accessor is `ev_accessor`,
        // no other type's kind.
        if self.is_accessor_type(&p) != self.is_accessor_type(&q) {
            return false;
        }
        let (Some(sp), Some(sq)) = (self.shape(&p), self.shape(&q)) else {
            return true;
        };
        if sp.etype != sq.etype {
            return false;
        }
        match sp.etype {
            Etype::Entity | Etype::Struct | Etype::Union => p.fteqcc_name() == q.fteqcc_name(),
            _ => self.typecmp_lax_within(&p, &q, depth),
        }
    }
}

/// The type FTEQCC defines itself by the name `name`, which `QCC_TypeForName` finds as any
/// other (FTEF-19.68): the simple types and the C scalars.
fn builtin_type(name: &str) -> Option<Type> {
    let simple = match name {
        "void" => SimpleType::Void,
        "float" => SimpleType::Float,
        "vector" => SimpleType::Vector,
        "string" => SimpleType::String,
        "entity" => SimpleType::Entity,
        _ => return CScalar::named(name).map(|_| Type::named(name, CContext::default())),
    };
    Some(Type::Simple(simple))
}
