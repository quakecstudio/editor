// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_ParseDefs
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseDefs, QCC_PR_GenerateInitializerType, QCC_PR_ParseInitializerType_Internal, QCC_PR_ParseDefaultInitialiser

//! Initialisers: a definition's value, its constness, array initialisers and parameter
//! defaults, cast to the definition's type as FTEQCC casts them (QC-11.3, FTEF-19.2 to
//! FTEF-19.6, FTEF-19.31).

use qc_diag::Code;
use qc_preproc::{CompilerFlags, FteFlag};
use qc_syntax::c_scalar::CScalar;

use crate::cast::{Cast, ConstType, Kind, Source, Supply};
use crate::lower::{ArrayDim, LiteralValue, Qualifier, RawDecl, RawValue, ValueKind};
use crate::resolve::{Facts, Fatal, Holder, MAX_TYPEDEF_DEPTH, REPLACEABLE_BUILTINS, Resolver};
use crate::symbol::{DefId, FileId, Location};
use crate::ty::{SimpleType, Type};

impl Resolver<'_> {
    /// FTEF-19.4, FTEF-19.11: what `QCC_PR_ParseDefs` reports of an initialiser before it reads
    /// it: a `shared` definition given a value; a body for a definition already initialised,
    /// "redeclaration of function body", but for `anglemod` and `crossproduct`, whose builtin
    /// a body replaces with a warning; and a `wrap` that has nothing to wrap.
    pub(in crate::resolve) fn check_initialisation(
        &mut self,
        decl: &RawDecl,
        existing: Option<DefId>,
        location: Location,
    ) -> bool {
        // FTEF-12.19: autoprototyping skips the initialiser before any of this is read.
        if decl.skipped_init {
            return false;
        }
        if !decl.initialized {
            if decl.has(&Qualifier::Accumulate)
                && let Some(existing) = existing
                && self.index.def(existing).is_some_and(|d| d.initialized)
                && !self.facts.get(existing.0).is_some_and(|f| f.accumulate)
            {
                self.report(
                    Code::W0218,
                    location,
                    format!(
                        "{} redeclared to accumulate after initial declaration",
                        decl.name
                    ),
                );
            }
            self.warn_constness_change(decl, existing, location, false);
            return false;
        }
        let flag = |read: fn(&Facts) -> bool| {
            existing.is_some_and(|d| self.facts.get(d.0).is_some_and(read))
        };
        let (shared, accumulating, builtin) = (
            flag(|f| f.shared),
            flag(|f| f.accumulate),
            flag(|f| f.builtin),
        );
        if decl.has(&Qualifier::Shared) || shared {
            self.report(
                Code::E0221,
                location,
                "shared values may not be assigned an initial value".to_owned(),
            );
            return true;
        }
        let initialized =
            existing.is_some_and(|d| self.index.def(d).is_some_and(|d| d.initialized));
        let weak = decl.has(&Qualifier::Weak);
        let wrap = decl.has(&Qualifier::Wrap);
        // FTEF-15.5: a weak definition of an initialised one, or a weak wrap, is skipped.
        if (initialized && weak) || (!initialized && weak && wrap) {
            return false;
        }
        self.warn_constness_change(decl, existing, location, true);
        if decl.has(&Qualifier::Accumulate) || accumulating {
            if initialized && !accumulating && !wrap {
                self.report(
                    Code::W0218,
                    location,
                    format!(
                        "{} redeclared to accumulate after initial declaration",
                        decl.name
                    ),
                );
            }
            return false;
        }
        if decl.body.is_none() {
            return false;
        }
        if wrap {
            if !initialized {
                self.report(
                    Code::E0222,
                    location,
                    "wrapper function does not wrap anything".to_owned(),
                );
                return true;
            } else if decl.body.as_ref().is_some_and(|b| {
                !b.uses
                    .iter()
                    .chain(&b.state_uses)
                    .any(|u| u.name == "prior")
            }) {
                self.report(
                    Code::E0223,
                    location,
                    format!(
                        "Wrapper function \"{}\" does not refer to its prior function",
                        decl.name
                    ),
                );
                return true;
            }
            return false;
        }
        if initialized && existing.is_some_and(|e| !self.redefinable.contains(&e)) {
            if builtin && REPLACEABLE_BUILTINS.contains(&decl.name.as_str()) {
                self.report(
                    Code::W0217,
                    location,
                    format!("{} already declared as a builtin", decl.name),
                );
                if let Some(existing) = existing {
                    self.unused.insert(existing);
                }
            } else {
                self.report(
                    Code::E0216,
                    location,
                    "redeclaration of function body".to_owned(),
                );
            }
        }
        false
    }

    /// FTEF-19.3: a redeclaration that would make a definition constant, or not, is a warning;
    /// the definition keeps the constness its first declaration gave it. A new definition is
    /// compared with the constness it is about to get, which differs only for an initialised
    /// `autocvar_`, made no constant (FTEF-19.16).
    pub(in crate::resolve) fn warn_constness_change(
        &mut self,
        decl: &RawDecl,
        existing: Option<DefId>,
        location: Location,
        initialised: bool,
    ) {
        let was = match existing {
            Some(existing) => {
                if self
                    .index
                    .def(existing)
                    .is_some_and(|d| d.member_of.is_some())
                {
                    return;
                }
                self.facts.get(existing.0).is_some_and(|f| f.constant)
            }
            None => Self::constant_at_file_level(decl),
        };
        let now = Self::constness(decl, initialised);
        if was == now {
            return;
        }
        let message = if initialised {
            format!(
                "Redeclaration of {} would change to {}const.",
                decl.name,
                if now { "" } else { "non-" }
            )
        } else {
            format!("Redeclaration of {} changes const.", decl.name)
        };
        self.report(Code::W0212, location, message);
    }

    /// FTEF-19.31, FTEF-19.3, QC-11.3: the checks of a value initialiser of a definition of
    /// type `ty` named `name`, held as `holder` says, and the value it gives when it names a
    /// constant (FTEF-19.5). Under the vanilla profile a literal of another type than the
    /// definition's is "wrong immediate type" (QCD-3.7); under FTEQCC a value its type
    /// cannot be cast to is "Cannot cast" (QCD-3.13), and a name that is no constant
    /// initialising a global or a static is "is not constant" (QCD-3.14). `Err` is a fatal
    /// error, after which the compilers read nothing more of the declaration.
    pub(in crate::resolve) fn check_value(
        &mut self,
        file: FileId,
        name: &str,
        ty: &Type,
        value: Option<&RawValue>,
        holder: Holder,
    ) -> Result<Option<LiteralValue>, Fatal> {
        let Some(value) = value else {
            return Ok(None);
        };
        let location = self.at(file, value.span);
        if !self.profile.initialisers_cast() {
            let ValueKind::Literal(literal) = &value.kind else {
                return Ok(None);
            };
            let expected = match ty {
                Type::Simple(SimpleType::Float) => Some(ConstType::Float),
                Type::Simple(SimpleType::Vector) => Some(ConstType::Vector),
                Type::Simple(SimpleType::String) => Some(ConstType::String),
                Type::Unknown => return Ok(None),
                Type::Simple(_)
                | Type::Field(_)
                | Type::Function(_)
                | Type::Named(_)
                | Type::Pointer(_) => None,
            };
            if expected != Some(literal.ty) {
                self.report(
                    Code::E0207,
                    location,
                    format!("wrong immediate type for {name}"),
                );
                return Err(Fatal);
            }
            return Ok(None);
        }
        let ty = self.resolved(ty);
        // FTEF-6.12: a struct is initialised by an aggregate only; a struct value is no other
        // type's, and neither is another type's a struct's: "Cannot cast from X to Y".
        if self.profile.type_declarations() {
            let is_struct = |ty: &Type| self.is_struct_type(ty);
            let target_struct = is_struct(&ty);
            let from = match &value.kind {
                ValueKind::Literal(literal) if target_struct => Some(literal.ty.name().to_owned()),
                ValueKind::Name(source_name) => {
                    let found = match holder {
                        Holder::Local(scope) | Holder::Static(scope) | Holder::Const(scope) => {
                            self.lookup_in_scope(scope, source_name)
                        }
                        Holder::Global => self.lookup_used_global(source_name),
                    };
                    found
                        .and_then(|d| self.index.def(d))
                        .map(|d| self.resolved(&d.ty))
                        .filter(|source| source != &ty)
                        .filter(|source| target_struct || is_struct(source))
                        .map(|source| source.fteqcc_name())
                }
                // A cast from a struct is reported as the cast's own use (`UseKind::CastTo`).
                ValueKind::Cast(cast) => {
                    let source = self.resolved(cast);
                    (source != ty
                        && !matches!(source, Type::Pointer(_))
                        && target_struct
                        && !is_struct(&source))
                    .then(|| source.fteqcc_name())
                }
                ValueKind::Literal(_)
                | ValueKind::Translate(_)
                | ValueKind::Indexed(_)
                | ValueKind::Other => None,
            };
            if let Some(from) = from {
                self.report(
                    Code::E0213,
                    location,
                    format!("Cannot cast from {from} to {}", ty.fteqcc_name()),
                );
                return Err(Fatal);
            }
        }
        match &value.kind {
            ValueKind::Literal(literal) => {
                let source = Source::Literal(*literal);
                if self.cast_value(source, &ty, location, value.cast_flags) {
                    return Err(Fatal);
                }
                Ok(None)
            }
            ValueKind::Translate(string) => {
                // FTEF-19.31: `_()` initialising a string takes one string literal.
                if !*string && ty == Type::Simple(SimpleType::String) {
                    self.report(
                        Code::E0232,
                        location,
                        "_() intrinsic accepts only a string immediate".to_owned(),
                    );
                    return Err(Fatal);
                }
                Ok(None)
            }
            ValueKind::Name(source_name) => {
                self.check_named_value(name, &ty, source_name, holder, location, value.cast_flags)
            }
            // FTEF-19.43: `(T)0` is a value of `T`, converted to the definition's type as a
            // name's would be (FTEF-22).
            ValueKind::Cast(cast) => {
                let source = match self.resolved(cast) {
                    // A class is an entity (`QCC_PR_NewType(classname, ev_entity, ...)`).
                    Type::Named(n) if self.has_name_key(&self.class_names, &n) => {
                        Type::Simple(SimpleType::Entity)
                    }
                    other => other,
                };
                // A pointer casts to any type (`QCC_SRef_IsNull`'s 0 keeps the cast's type),
                // which the model does not follow further.
                if source != ty
                    && !matches!(source, Type::Pointer(_))
                    && self.cast_value(Source::Typed(&source), &ty, location, value.cast_flags)
                {
                    return Err(Fatal);
                }
                Ok(None)
            }
            ValueKind::Indexed(array) => {
                // FTEF-19.39: an element of a variable is no constant.
                if matches!(holder, Holder::Global | Holder::Static(_)) {
                    let found = match holder {
                        Holder::Local(scope) | Holder::Static(scope) | Holder::Const(scope) => {
                            self.lookup_in_scope(scope, array)
                        }
                        Holder::Global => self.lookup_used_global(array),
                    };
                    if let Some(source) = found
                        && !self.facts.get(source.0).is_some_and(|f| f.constant)
                    {
                        self.report(
                            Code::E0214,
                            location,
                            format!("initializer for '{name}' is not constant"),
                        );
                    }
                }
                Ok(None)
            }
            ValueKind::Other => Ok(None),
        }
    }

    /// [`Resolver::check_value`] of a value that is the name `source_name`, cast with the
    /// flags `flags` at the token after it (FTEF-22.6).
    pub(in crate::resolve) fn check_named_value(
        &mut self,
        name: &str,
        ty: &Type,
        source_name: &str,
        holder: Holder,
        location: Location,
        flags: CompilerFlags,
    ) -> Result<Option<LiteralValue>, Fatal> {
        let lookup = |name: &str| match holder {
            Holder::Local(scope) | Holder::Static(scope) | Holder::Const(scope) => {
                self.lookup_in_scope(scope, name)
            }
            Holder::Global => self.lookup_used_global(name),
        };
        // `Scope::name`: the definition of that name, or for a scope that is no class the
        // plain name, as a named enum's values are (FTEF-19.10).
        let found = lookup(source_name).or_else(|| {
            let (prefix, bare) = source_name.split_once("::")?;
            (!self.has_name_key(&self.class_names, prefix)
                && !self.struct_defs.contains_key(&self.type_key(prefix)))
            .then(|| lookup(bare))
            .flatten()
        });
        let Some(source) = found else {
            return Ok(None);
        };
        let Some(source_ty) = self.index.def(source).map(|d| d.ty.clone()) else {
            return Ok(None);
        };
        // FTEF-19.31: in a member function a field's name is `this.field`, of the field's type
        // (`QCC_PR_ParseRefValue`, `expandmemberfields`).
        let in_class = match holder {
            Holder::Local(scope) | Holder::Static(scope) | Holder::Const(scope) => {
                scope.class.is_some()
            }
            Holder::Global => false,
        };
        let source_ty = match &source_ty {
            Type::Field(inner) if in_class => (**inner).clone(),
            Type::Unknown
            | Type::Simple(_)
            | Type::Field(_)
            | Type::Function(_)
            | Type::Named(_)
            | Type::Pointer(_) => source_ty,
        };
        let source_ty = self.resolved(&source_ty);
        // FTEF-19.10: a strict enum's value casts to no other type, but under `lax`
        // (FTEF-22.1).
        if let Type::Named(strict) = &source_ty
            && let Some(base) = self.named(&self.strict_enums, strict.as_ref())
            && source_ty != *ty
        {
            let (code, verb) = if flags.has(FteFlag::Laxcasts) {
                (Code::W0229, "Implicit lax cast")
            } else {
                (Code::E0213, "Cannot cast")
            };
            let message = format!(
                "{verb} from enum {strict}:{} to {}",
                base.fteqcc_name(),
                ty.fteqcc_name()
            );
            self.report(code, location, message);
            if code == Code::E0213 {
                return Err(Fatal);
            }
        } else if self.cast_value(Source::Typed(&source_ty), ty, location, flags) {
            return Err(Fatal);
        }
        if matches!(holder, Holder::Local(_)) {
            return Ok(None);
        }
        let constant = self.facts.get(source.0).is_some_and(|f| f.constant);
        if !constant {
            if matches!(holder, Holder::Global | Holder::Static(_)) {
                self.report(
                    Code::E0214,
                    location,
                    format!("initializer for '{name}' is not constant"),
                );
            }
            return Ok(None);
        }
        // FTEF-19.3: a field is initialised behind the scenes, and a function not defined yet
        // is filled in once it is.
        let initialized = self
            .index
            .def(source)
            .is_some_and(|d| d.initialized || d.ty.is_field());
        if !initialized && !ty.is_function() {
            self.report(
                Code::W0219,
                location,
                format!(
                    "initializer for {name} is not initialised yet, {source_name} will be treated \
                     as 0"
                ),
            );
            return Ok(match ty {
                Type::Simple(SimpleType::Float) => Some(LiteralValue::Float(0)),
                Type::Simple(SimpleType::Vector) => Some(LiteralValue::Vector([0; 3])),
                Type::Unknown
                | Type::Simple(_)
                | Type::Field(_)
                | Type::Function(_)
                | Type::Named(_)
                | Type::Pointer(_) => None,
            });
        }
        // FTEF-19.5: a name of the same type gives its value, which a later initialiser is
        // compared with.
        Ok((source_ty == *ty)
            .then(|| self.literals.get(&source).cloned())
            .flatten())
    }

    /// The checks of an array declaration beyond its value (FTEF-19.8, FTE-17.7): each
    /// dimension takes its own braces, `{ { 1, 2 }, { 3, 4 } }`
    /// (`QCC_PR_ParseInitializerType_Internal` recurses with "expected {, found X"), and an
    /// array of functions with no initialiser holds functions with no body. Returns whether the
    /// declaration was aborted.
    pub(in crate::resolve) fn check_array_declaration(
        &mut self,
        file: FileId,
        decl: &RawDecl,
        location: Location,
    ) -> bool {
        if decl.array_dims >= 2
            && let Some((first, span)) = &decl.aggregate_first
            && first != "{"
        {
            let location = self.at(file, *span);
            self.report(Code::E0204, location, format!("expected {{, found {first}"));
            return true;
        }
        // C's `float (*x[2])(float);` is an array of pointers to functions, a `var` (FTEF-21.7).
        if decl.array.is_some()
            && !decl.initialized
            && !decl.pointer_var
            && self.resolved(&decl.ty).is_function()
        {
            self.report(
                Code::E0211,
                location,
                format!("function {} has no body", decl.name),
            );
        }
        false
    }

    /// FTEF-19.8: an array is initialised with `{ ... }`, or an array of `char` with a string;
    /// `QCC_PR_ParseInitializerType_Internal` takes anything else for "expected {, found X",
    /// a fatal error. Returns whether it was reported.
    pub(in crate::resolve) fn check_array_value(
        &mut self,
        file: FileId,
        array: Option<ArrayDim>,
        ty: &Type,
        value: Option<&RawValue>,
    ) -> bool {
        let (Some(_), Some(value)) = (array, value) else {
            return false;
        };
        let string = matches!(
            value.kind,
            ValueKind::Literal(literal) if literal.ty == ConstType::String
        );
        // `def.cast->type == ev_bitfld && def.cast->bits == 8`: an 8-bit integer.
        let chars = matches!(ty.fteqcc_scalar(), Some(CScalar::Int8 | CScalar::Uint8));
        if string && chars {
            if let (Some(ArrayDim::Sized(size)), Some(len)) = (array, value.string_len)
                && len > size
            {
                let location = self.at(file, value.span);
                self.report(
                    Code::E0233,
                    location,
                    "initializer-string too long".to_owned(),
                );
            }
            return false;
        }
        let location = self.at(file, value.span);
        self.report(
            Code::E0204,
            location,
            format!("expected {{, found {}", value.first),
        );
        true
    }

    /// FTEQCC's implicit cast of `source` to `to` with the flags `flags` where it stands
    /// (FTEF-19.31, FTEF-22): "Cannot cast", an error, "Implicit cast" or "Implicit lax cast",
    /// warnings, "pointer<->string casts were disabled.", a non-fatal error, or none. Returns
    /// whether the cast is refused.
    pub(in crate::resolve) fn cast_value(
        &mut self,
        source: Source<'_>,
        to: &Type,
        location: Location,
        flags: CompilerFlags,
    ) -> bool {
        let to = &self.resolved_deep(to, MAX_TYPEDEF_DEPTH);
        let typed;
        let source = match source {
            Source::Typed(ty) => {
                typed = self.resolved_deep(ty, MAX_TYPEDEF_DEPTH);
                Source::Typed(&typed)
            }
            Source::Literal(_) => source,
        };
        let (code, verb) = match source.cast(to, flags) {
            Cast::Silent => return false,
            Cast::Implicit => (Code::W0220, "Implicit cast"),
            Cast::Lax => (Code::W0229, "Implicit lax cast"),
            Cast::Refused => (Code::E0213, "Cannot cast"),
            Cast::StringPointerDisabled => {
                self.report(
                    Code::E0237,
                    location,
                    "pointer<->string casts were disabled.".to_owned(),
                );
                return false;
            }
        };
        self.report(
            code,
            location,
            format!("{verb} from {} to {}", source.name(), to.fteqcc_name()),
        );
        code == Code::E0213
    }

    /// FTEF-19.6: a parameter's default value must be a constant already initialised, and of
    /// a type the parameter's casts from: `QCC_PR_ParseDefaultInitialiser`. With
    /// autoprototyping one not initialised yet is no error.
    pub(in crate::resolve) fn check_defaults(&mut self, file: FileId, decl: &RawDecl) -> bool {
        for param in &decl.params {
            let Some(value) = &param.default else {
                continue;
            };
            let location = self.at(file, value.span);
            match &value.kind {
                ValueKind::Literal(literal) => {
                    let source = Source::Literal(*literal);
                    let ty = self.resolved(&param.ty);
                    if self.cast_value(source, &ty, location, value.cast_flags) {
                        return true;
                    }
                }
                ValueKind::Name(name) => {
                    let Some(source) = self.lookup_global(name) else {
                        continue;
                    };
                    if !self.facts.get(source.0).is_some_and(|f| f.constant) {
                        self.report(
                            Code::E0217,
                            location,
                            "Default value not a constant".to_owned(),
                        );
                        return true;
                    }
                    let initialized = self
                        .index
                        .def(source)
                        .is_some_and(|d| d.initialized || d.ty.is_field());
                    if !initialized && !self.autoproto {
                        self.report(
                            Code::E0218,
                            location,
                            "Default value not initialized yet".to_owned(),
                        );
                        return true;
                    }
                    let source_ty = self
                        .index
                        .def(source)
                        .map_or(Type::Unknown, |d| d.ty.clone());
                    let ty = self.resolved(&param.ty);
                    let source = Source::Typed(&source_ty);
                    if self.cast_value(source, &ty, location, value.cast_flags) {
                        return true;
                    }
                }
                ValueKind::Translate(_)
                | ValueKind::Indexed(_)
                | ValueKind::Cast(_)
                | ValueKind::Other => {}
            }
        }
        false
    }

    /// `QCC_SupplyConversion` of `source`, the value `value`, to a `wanted` (FTEF-22.3), with
    /// the flags at the token after it, and its diagnostic: "Automatic type conversions
    /// disabled." under `typeexplicit`, and "Implicit type mismatch." when nothing converts it,
    /// a warning under `lax`; each names the two kinds by `basictypenames`.
    pub(in crate::resolve) fn supply_value(
        &mut self,
        source: Source<'_>,
        wanted: Kind,
        value: &RawValue,
        location: Location,
    ) -> Supply {
        let supply = source.supply(wanted, value.cast_flags);
        let (code, text) = match supply {
            Supply::Converted => return supply,
            Supply::Explicit => (Code::E0236, "Automatic type conversions disabled."),
            Supply::Lax => (Code::W0230, "Implicit type mismatch."),
            Supply::Mismatch => (Code::E0227, "Implicit type mismatch."),
        };
        let message = format!(
            "{text} Needed {}, got {}.",
            wanted.basic_name(),
            source.kind().basic_name()
        );
        self.report(code, location, message);
        supply
    }
}
