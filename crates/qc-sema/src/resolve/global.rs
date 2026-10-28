// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_GetDef, PR_ParseDefs
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_GetDef, QCC_PR_ParseDefs
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, typecmp, typecmp_lax
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, QCC_PR_FinishCompilation

//! Declaring a module's top-level definitions and their redeclaration (QC-11, FTEF-17,
//! FTEF-19): the definition list, `qcc`'s and FTEQCC's type comparison of a redeclaration,
//! qualifiers, `extern`, and the functions left undefined at the end (QC-11.8).

use qc_diag::Code;
use qc_syntax::c_scalar::CScalar;
use std::collections::HashSet;
use std::rc::Rc;

use crate::lower::{
    ArrayDim, ExternBase, LiteralValue, Qualifier, RawAggregate, RawDecl, RawParamInfo,
    top_level_kind,
};
use crate::resolve::aggregate::{AggregateSite, Initialised};
use crate::resolve::init::string_of_chars;
use crate::resolve::{
    COMPONENTS, DeclFacts, Facts, Fatal, FileDiagnostic, Holder, Init, MAX_TYPEDEF_DEPTH,
    MISDECLARED_BUILTINS, Pending, Redeclaration, Resolver, Special, TypeCheck, component_type,
    defaults_agree, diag,
};
use crate::symbol::{DefId, DefKind, Definition, FileId, Location};
use crate::ty::{FunctionType, Type};

impl<'a> Resolver<'a> {
    pub(in crate::resolve) fn fact_mut(&mut self, def: DefId) -> Option<&mut Facts> {
        self.facts.get_mut(def.0)
    }

    /// Adds a definition and returns its id. A class member is not a global by its name.
    pub(in crate::resolve) fn push_def(&mut self, def: Definition) -> DefId {
        // FTEF-20.4: `QCC_PR_DummyDef` warns of a definition named as a keyword that is on,
        // which keeps its meaning beside it with `kce` on and is turned off with it off.
        // A class's members are made by `QCC_PR_ParseEntClass`, not as definitions named so.
        if self.profile.warns_keyword_names()
            && def.member_of.is_none()
            && self
                .keywords
                .named_definition(&def.name, def.ty.is_function())
                .is_some()
        {
            let what = if self.keywords.coexist() {
                "coexisting"
            } else {
                "disabling"
            };
            let message = format!("\"{}\" keyword used as variable name - {what}", def.name);
            self.report(Code::W0221, def.location, message);
        }
        let id = DefId(self.index.defs.len());
        if def.kind.is_global() && def.member_of.is_none() {
            self.globals.entry(def.name.clone()).or_default().push(id);
        }
        self.index.defs.push(def);
        self.facts.push(Facts::default());
        id
    }

    /// A type with the typedef names it holds resolved to the types they name (FTEF-19.9),
    /// as `QCC_TypeForName` gives a typedef's real type.
    pub(in crate::resolve) fn resolved(&self, ty: &Type) -> Type {
        let mut ty = ty.clone();
        for _ in 0..MAX_TYPEDEF_DEPTH {
            let Type::Named(name) = &ty else {
                return ty;
            };
            match self.named(&self.typedefs, name.as_ref()) {
                Some(aliased) => ty = aliased.clone(),
                None => return ty,
            }
        }
        ty
    }

    /// `ty` with every typedef in it resolved, what a pointer points to, a field holds and a
    /// function takes and returns included, as FTEQCC's types are once `QCC_PR_ParseType` has
    /// read them; `depth` bounds a typedef that names itself through a pointer.
    pub(in crate::resolve) fn resolved_deep(&self, ty: &Type, depth: usize) -> Type {
        let ty = self.resolved(ty);
        let Some(depth) = depth.checked_sub(1) else {
            return ty;
        };
        match ty {
            Type::Pointer(inner) => Type::Pointer(Rc::new(self.resolved_deep(&inner, depth))),
            Type::Field(inner) => Type::Field(Rc::new(self.resolved_deep(&inner, depth))),
            Type::Function(func) => Type::Function(Rc::new(FunctionType {
                ret: self.resolved_deep(&func.ret, depth),
                params: func
                    .params
                    .iter()
                    .map(|p| self.resolved_deep(p, depth))
                    .collect(),
                variadic: func.variadic,
                labels: func.labels.clone(),
            })),
            Type::Unknown | Type::Simple(_) | Type::Named(_) => ty,
        }
    }

    /// Whether a redeclaration's type `ty` matches the earlier `old` (QC-11.5): equal, or
    /// either unknown; under FTEQCC also when the model does not follow either (FTEF-17.13).
    pub(in crate::resolve) fn same_type(&self, old: &Type, ty: &Type) -> bool {
        if self.profile.redeclaration_by_typecmp() {
            self.resolved(old).fteqcc_same(&self.resolved(ty))
        } else {
            // QC-10.10: a variadic type is never the same as another.
            (old == ty && !old.holds_variadic()) || *old == Type::Unknown || *ty == Type::Unknown
        }
    }

    /// FTEQCC's `typecmp_lax` of a function redeclaration (FTEF-19.7): the return types and
    /// the parameters both lists have the same, and every parameter one list has beyond the
    /// other `optional`; whether either is variadic does not matter.
    pub(in crate::resolve) fn lax_same(
        old: &Type,
        old_params: &[RawParamInfo],
        ty: &Type,
        params: &[RawParamInfo],
    ) -> bool {
        let (Type::Function(a), Type::Function(b)) = (old, ty) else {
            return false;
        };
        if !a.ret.fteqcc_same(&b.ret) {
            return false;
        }
        let shared = a.params.len().min(b.params.len());
        let types_agree = a
            .params
            .iter()
            .zip(&b.params)
            .all(|(x, y)| x.fteqcc_same(y));
        let outs_agree = old_params.iter().zip(params).all(|(x, y)| x.out == y.out);
        let optional_beyond = |list: &[Type], info: &[RawParamInfo]| {
            (shared..list.len()).all(|i| info.get(i).is_some_and(|p| p.optional))
        };
        types_agree
            && outs_agree
            && defaults_agree(old_params, params)
            && optional_beyond(&a.params, old_params)
            && optional_beyond(&b.params, params)
    }

    /// Declares, or redeclares, a global-scope name (QC-11.5), and the components of a
    /// vector or vector field it declares anew (QC-11.7).
    ///
    /// A name already in scope with the same type refers to the same definition; declaring
    /// it with a different type is QCD-3.5; initialising it a second time is QCD-3.6
    /// (QC-11.5), unless FTE-22.6 accepts it. Returns the [`DefId`] the name resolves to.
    pub(in crate::resolve) fn declare_global(
        &mut self,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
        init: Init<'_>,
    ) -> DefId {
        self.declare_global_with(name, ty, kind, location, init, &DeclFacts::default())
    }

    /// [`Resolver::declare_global`] with what FTEQCC's redeclaration rules read of the
    /// declaration (FTEF-19).
    pub(in crate::resolve) fn declare_global_with(
        &mut self,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
        init: Init<'_>,
        decl: &DeclFacts<'_>,
    ) -> DefId {
        if let Some(existing) = self.lookup_global(name) {
            let redeclaration = Redeclaration {
                existing,
                name,
                ty,
                location,
                init,
                params: decl.params,
                array: decl.array,
            };
            if decl.checked {
                self.redeclare(&redeclaration);
                return existing;
            }
            match self.check_redeclared_type(&redeclaration, decl.component) {
                TypeCheck::Same => {
                    if !self.check_array(&redeclaration, false) {
                        self.redeclare(&redeclaration);
                    }
                    return existing;
                }
                TypeCheck::Ignored => {
                    self.redeclare(&redeclaration);
                    return existing;
                }
                TypeCheck::Mismatch => return existing,
                TypeCheck::Replaced => {
                    if let Some(defs) = self.globals.get_mut(name) {
                        defs.retain(|d| *d != existing);
                    }
                }
            }
        }
        let id = self.push_def(Definition {
            name: name.to_owned(),
            ty: ty.clone(),
            kind,
            location,
            owner: None,
            initialized: init.is_some(),
            member_of: None,
            generated: false,
        });
        let constant = decl.constant;
        let statics = decl.statics;
        let field_component =
            decl.component && self.profile.field_components_typed_apart() && kind == DefKind::Field;
        if let Some(facts) = self.fact_mut(id) {
            facts.constant = constant;
            facts.array = decl.array;
            facts.params = decl.params.to_vec();
            facts.field_component = field_component;
            facts.statics = statics;
            facts.deprecated = decl.deprecated.map(str::to_owned);
        }
        if let Some(Some(literal)) = init {
            self.note_builtin_function(id, name, literal);
            self.literals.insert(id, literal.clone());
        }
        // QC-11.7: `PR_GetDef` makes `name_x`, `name_y` and `name_z` for a new vector, and
        // `.float` fields of the same names for a new `.vector` field. QC-11.11: the vector's
        // initialiser does not initialise them under the vanilla profile; FTEQCC gives them
        // the vector's initialised state and its words (FTEF-19.58).
        if decl.array.is_none()
            && let Some(component) = component_type(ty)
        {
            let component_kind = top_level_kind(&component);
            let words = match init {
                Some(Some(LiteralValue::Vector(words))) => Some(*words),
                _ => None,
            };
            for (index, suffix) in COMPONENTS.iter().enumerate() {
                let component_name = format!("{name}{suffix}");
                let word = words
                    .and_then(|w| w.get(index).copied())
                    .map(LiteralValue::Float);
                let component_init = if self.profile.components_initialised() {
                    init.map(|_| word.as_ref())
                } else {
                    None
                };
                let facts = DeclFacts {
                    component: true,
                    constant,
                    statics,
                    ..DeclFacts::default()
                };
                if self.profile.components_initialised()
                    && self.lookup_global(&component_name).is_some()
                {
                    // FTEF-19.58: FTEQCC makes the components with `QCC_PR_DummyDef`, which
                    // looks for no earlier definition, so an earlier one of the name stays.
                    continue;
                }
                self.declare_global_with(
                    &component_name,
                    &component,
                    component_kind,
                    location,
                    component_init,
                    &facts,
                );
            }
        }
        id
    }

    /// The type check of a redeclaration (QC-11.5, FTEF-19.7), with FTEQCC's rules for the
    /// differences it accepts. `component` is a vector's component being declared.
    pub(in crate::resolve) fn check_redeclared_type(
        &mut self,
        r: &Redeclaration<'_>,
        component: bool,
    ) -> TypeCheck {
        let Some(def) = self.index.defs.get(r.existing.0) else {
            return TypeCheck::Mismatch;
        };
        let old = def.ty.clone();
        let kind = def.kind;
        let facts = self.facts.get(r.existing.0);
        let field_component = facts.is_some_and(|f| f.field_component);
        let referenced = facts.is_some_and(|f| f.referenced);
        let params = facts.map_or(&[][..], |f| f.params.as_slice());
        let defaults_ok = defaults_agree(params, r.params);
        let resolved_old = self.resolved(&old);
        let resolved_ty = self.resolved(r.ty);
        let lax = Self::lax_same(&resolved_old, params, &resolved_ty, r.params);
        // QC-11.12: a vector field's component has `qcc`'s `type_floatfield`, which no `.float`
        // written in the source is, and only another component's lookup has.
        if self.profile.field_components_typed_apart()
            && field_component != component
            && kind == DefKind::Field
        {
            self.mismatch(r.name, r.location, &old, r.ty);
            return TypeCheck::Mismatch;
        }
        let same =
            self.same_type(&old, r.ty) && (!self.profile.redeclaration_by_typecmp() || defaults_ok);
        if same {
            return TypeCheck::Same;
        }
        if !self.profile.redeclaration_by_typecmp() {
            self.mismatch(r.name, r.location, &old, r.ty);
            return TypeCheck::Mismatch;
        }
        let (old, ty) = (resolved_old, resolved_ty);
        // FTEF-19.7: at file level a difference only in optional arguments is a warning, and
        // a declaration with more parameters gives the definition its type.
        if lax {
            let (Type::Function(a), Type::Function(b)) = (&old, &ty) else {
                return TypeCheck::Same;
            };
            let silent = a.params.len() == b.params.len() && a.variadic && !b.variadic;
            if !silent {
                self.report(
                    Code::W0209,
                    r.location,
                    format!(
                        "Optional arguments differ on redeclaration of {}. {}, should be {}",
                        r.name,
                        ty.fteqcc_name(),
                        old.fteqcc_name()
                    ),
                );
            }
            if b.params.len() > a.params.len() {
                let new_ty = r.ty.clone();
                if let Some(def) = self.index.defs.get_mut(r.existing.0) {
                    def.ty = new_ty;
                }
                let params = r.params.to_vec();
                if let Some(facts) = self.fact_mut(r.existing) {
                    facts.params = params;
                }
            }
            return TypeCheck::Same;
        }
        if MISDECLARED_BUILTINS.contains(&r.name) {
            self.report(
                Code::W0210,
                r.location,
                format!(
                    "{} builtin was wrongly redefined as {}. ignoring later definition",
                    r.name,
                    ty.fteqcc_name()
                ),
            );
            return TypeCheck::Ignored;
        }
        if self.extensions.contains(&r.location.file) && old.is_function() {
            self.report(
                Code::W0210,
                r.location,
                format!(
                    "{} builtin was redefined as {}. ignoring alternative definition",
                    r.name,
                    ty.fteqcc_name()
                ),
            );
            return TypeCheck::Ignored;
        }
        if self.unused.contains(&r.existing) && !referenced {
            self.report(
                Code::W0211,
                r.location,
                format!(
                    "Type redeclaration of {} {} replaces existing variable",
                    ty.fteqcc_name(),
                    r.name
                ),
            );
            return TypeCheck::Replaced;
        }
        // FTEF-19.7: "c89-style dumb redeclarations": `T(...)` of a function already declared.
        if let (Type::Function(_), Type::Function(b)) = (&old, &ty)
            && b.variadic
            && b.params.is_empty()
        {
            return TypeCheck::Ignored;
        }
        self.mismatch(r.name, r.location, &old, r.ty);
        TypeCheck::Mismatch
    }

    /// The checks of a global's initialiser before its value is read: what `QCC_PR_ParseDefs`
    /// reports at the `=` (FTEF-19.57), an array's value and dimensions (FTEF-19.8) and a braced
    /// initialiser read against the type (FTEF-19.47). Returns whether the declaration was
    /// aborted.
    fn check_initialiser_shape(
        &mut self,
        file: FileId,
        decl: &RawDecl,
        existing: Option<DefId>,
        location: Location,
    ) -> bool {
        if self.profile.definition_checks()
            && (self.check_initialisation(decl, existing, location)
                || self.check_array_value(file, self.array_of(decl), &decl.ty, decl.value.as_ref())
                || self.check_array_declaration(file, decl, location))
        {
            return true;
        }
        let Some(aggregate) = decl
            .aggregate
            .as_ref()
            .filter(|_| self.profile.fteqcc_diagnostics())
        else {
            return false;
        };
        let def = Initialised {
            name: &decl.name,
            holder: Holder::Global,
        };
        // FTEF-19.62: a folded size bounds the initialiser as a written one does.
        let sized;
        let aggregate = match (self.array_of(decl), aggregate.sizes.split_first()) {
            (Some(ArrayDim::Sized(size)), Some((None, rest))) => {
                let mut sizes = vec![Some(size)];
                sizes.extend_from_slice(rest);
                sized = RawAggregate {
                    sizes,
                    ..aggregate.clone()
                };
                &sized
            }
            _ => aggregate,
        };
        self.check_aggregate(file, aggregate, &decl.ty, def, AggregateSite::Global)
            .is_err()
    }

    pub(in crate::resolve) fn mismatch(
        &mut self,
        name: &str,
        location: Location,
        old: &Type,
        ty: &Type,
    ) {
        // QCD-3.5: `PR_GetDef` finds the earlier definition with another type.
        // FTEF-23.8: `TypeName` prints `type_bint` by its own name, which a cast does not.
        let shown = |ty: &Type| match ty.fteqcc_scalar() {
            Some(CScalar::Bool) => "__bint".to_owned(),
            _ => self.type_name(ty),
        };
        let message = if self.profile.fteqcc_diagnostics() {
            format!(
                "Type mismatch on redeclaration of {name}. {}, should be {}",
                shown(ty),
                shown(old)
            )
        } else {
            format!("Type mismatch on redeclaration of {name}")
        };
        self.report(Code::E0205, location, message);
    }

    /// FTEF-19.8: under FTEQCC a redeclaration gives the same array size as the definition,
    /// where it gives a size: `QCC_PR_GetDef` compares `def->arraysize != arraysize &&
    /// arraysize>=0`. An unsized definition takes the size. `local` spaces its message as
    /// FTEQCC's local branch does. Returns whether the sizes differ, a fatal error.
    pub(in crate::resolve) fn check_array(&mut self, r: &Redeclaration<'_>, local: bool) -> bool {
        if !self.profile.definition_checks() {
            return false;
        }
        let size = |dim: Option<ArrayDim>| match dim {
            None => Some(0),
            Some(ArrayDim::Sized(n)) => Some(n),
            Some(ArrayDim::Unsized | ArrayDim::Unknown) => None,
        };
        let old = self.facts.get(r.existing.0).and_then(|f| f.array);
        let (Some(new), Some(was)) = (size(r.array), size(old)) else {
            if matches!(old, Some(ArrayDim::Unsized))
                && let Some(facts) = self.fact_mut(r.existing)
            {
                facts.array = r.array;
            }
            return false;
        };
        if new != was {
            let message = if local {
                format!(
                    "Array sizes for redecleration of {} do not match ({was} -> {new})",
                    r.name
                )
            } else {
                format!(
                    "Array sizes for redecleration of {} do not match ({was}->{new})",
                    r.name
                )
            };
            self.report(Code::E0220, r.location, message);
            return true;
        }
        false
    }

    /// Handles the initialiser of a redeclaration of an existing definition (QC-11.5).
    pub(in crate::resolve) fn redeclare(&mut self, r: &Redeclaration<'_>) {
        let Some(def) = self.index.defs.get(r.existing.0) else {
            return;
        };
        let Some(literal) = r.init else {
            return;
        };
        let existing = r.existing;
        let name = r.name;
        if def.initialized {
            // FTE-22.6: FTEQCC compares a second initialiser with the first and accepts it
            // when the two are the same value; FTEF-17.8: it does not compare them when the
            // first is `noref`.
            let same = self.profile.reinitialise_with_same_value()
                && (literal.is_some_and(|literal| self.same_value(existing, name, literal))
                    || self.unused.contains(&existing));
            if !same
                && self.profile.reinitialise_with_same_value()
                && self.parseentitydata(existing, name)
            {
                self.report(
                    Code::W0223,
                    r.location,
                    "incompatible redeclaration. Please validate builtin numbers. \
                     parseentitydata is #613"
                        .to_owned(),
                );
                return;
            }
            if !same {
                // FTE-23.17: FTEQCC makes a new builtin function for a builtin it reuses none
                // for.
                if let Some(literal) = literal {
                    self.note_builtin_function(existing, name, literal);
                }
                if !self.profile.reinitialise_with_same_value() {
                    // QCD-3.6: initialising a name that is already initialised.
                    self.report(Code::E0206, r.location, format!("{name} redeclared"));
                } else if literal.is_some() || matches!(r.ty, Type::Function(_)) {
                    // FTE-22.6: a constant's other words are "incompatible redeclaration"; a
                    // value that is no constant was "not constant" and is not compared.
                    let message = "incompatible redeclaration".to_owned();
                    self.report(Code::E0363, r.location, message);
                }
            }
            return;
        }
        if let Some(def) = self.index.defs.get_mut(existing.0) {
            def.initialized = true;
        }
        if let Some(literal) = literal {
            self.note_builtin_function(existing, name, literal);
            self.literals.insert(existing, literal.clone());
        }
    }

    /// FTEF-19.58: FTEQCC accepts `parseentitydata` given another builtin after `#608` or
    /// `#613`, "dpextensions is WRONG", and makes it `#613`.
    pub(in crate::resolve) fn parseentitydata(&mut self, existing: DefId, name: &str) -> bool {
        if name != "parseentitydata" {
            return false;
        }
        let Some(LiteralValue::Builtin { number, .. }) = self.literals.get_mut(&existing) else {
            return false;
        };
        if !matches!(*number, 608 | 613) {
            return false;
        }
        *number = 613;
        true
    }

    /// Whether `literal`, initialising `existing` (named `name`) again, is the value it has
    /// (FTE-22.6, FTEF-17.10). A builtin is when FTEQCC reuses `existing`'s builtin function
    /// for it (FTE-23.17): the one it made first with the same number and engine name, the
    /// definition's own name standing in for none.
    pub(in crate::resolve) fn same_value(
        &self,
        existing: DefId,
        name: &str,
        literal: &LiteralValue,
    ) -> bool {
        let Some(first) = self.literals.get(&existing) else {
            return false;
        };
        match (first.builtin_function(name), literal.builtin_function(name)) {
            (Some(first), Some(key)) => {
                first == key && self.builtin_functions.get(&key) == Some(&existing)
            }
            _ => first == literal,
        }
    }

    /// Records the builtin function FTEQCC makes for `def` when `literal` is a builtin, unless
    /// one with its number and engine name was made before (FTE-23.17).
    pub(in crate::resolve) fn note_builtin_function(
        &mut self,
        def: DefId,
        name: &str,
        literal: &LiteralValue,
    ) {
        if let Some(key) = literal.builtin_function(name) {
            self.builtin_functions.entry(key).or_insert(def);
        }
    }

    /// FTEF-19.3: whether FTEQCC makes a new definition of `decl` a constant: `const`, or, at
    /// file level and not `var`, initialised, a function or a field (`GDF_CONST`). An
    /// `autocvar_` never is (FTEF-19.16).
    pub(in crate::resolve) fn constant_at_file_level(decl: &RawDecl) -> bool {
        !decl.name.starts_with("autocvar_")
            && Self::constness(decl, decl.initialized || decl.skipped_init)
    }

    /// FTEF-19.3: the constness `QCC_PR_ParseDefs` gives a file-level declaration, initialised
    /// or not: `const`, or not `var` and initialised, a function or a field; FTEF-21.11: an
    /// initialised one not under `assumevar`; FTEF-21.7: C's function pointer is a `var`.
    pub(in crate::resolve) fn constness(decl: &RawDecl, initialised: bool) -> bool {
        decl.has(&Qualifier::Const)
            || (!decl.has(&Qualifier::Var)
                && !decl.pointer_var
                && ((initialised && !decl.assumevar)
                    || decl.ty.is_function()
                    || decl.ty.is_field()))
    }

    /// What [`Resolver::declare_top_level`] declares before a plain definition: a type's
    /// name, an enum's item, a `Class::member` and a definition `alias` gives (FTEF-21.9), each
    /// of its own; a declaration whose type names no type, nothing; and the warnings of the
    /// qualifiers.
    pub(in crate::resolve) fn declare_special(
        &mut self,
        file: FileId,
        decl: &'a RawDecl,
        location: Location,
    ) -> Special<'a> {
        if let Some(kind) = decl.type_kind {
            self.type_names.insert(decl.name.clone());
            self.declare_type_name(kind, decl, location);
            // FTEF-17.1: a type's name may be declared again, a class first forward and then
            // with its body, and a typedef in more than one header.
            let def =
                self.declare_global(&decl.name, &Type::Unknown, DefKind::Global, location, None);
            self.type_defs.insert(def);
            if let Some(class) = &decl.class {
                return Special::Declared(self.declare_class(file, def, class));
            }
            return Special::Declared(Vec::new());
        }
        if decl.type_only {
            self.check_type_names(&decl.ty, &[], decl.span.start, location);
            return Special::Declared(Vec::new());
        }
        if self.profile.type_declarations()
            && let Some(item) = &decl.enum_item
        {
            self.declare_enum_item(file, decl, item, location);
            return Special::Declared(Vec::new());
        }
        if self.profile.type_declarations()
            && self.check_type_names(&decl.ty, &[], decl.span.start, location)
        {
            return Special::Declared(Vec::new());
        }
        let qualifier_fatal = self.profile.qualifiers() && self.warn_qualifiers(decl, location);
        if let Some(scope) = &decl.scope
            && self.profile.type_declarations()
        {
            return Special::Declared(self.define_member(file, scope, decl, location));
        }
        if self.profile.qualifiers()
            && let Some(target) = decl.alias()
        {
            self.declare_alias_def(
                None,
                &decl.name,
                &decl.ty,
                target,
                decl.initialized,
                location,
            );
            return Special::Declared(Vec::new());
        }
        Special::Plain { qualifier_fatal }
    }

    /// Declares one top-level declaration, and returns the function bodies it brings, to
    /// resolve now or, with autoprototyping, after every declaration (FTE-22.3).
    pub(in crate::resolve) fn declare_top_level(
        &mut self,
        file: FileId,
        decl: &'a RawDecl,
    ) -> Vec<Pending<'a>> {
        let location = self.at(file, decl.span);
        let qualifier_fatal = match self.declare_special(file, decl, location) {
            Special::Declared(pending) => return pending,
            Special::Plain { qualifier_fatal } => qualifier_fatal,
        };
        if self.profile.qualifiers() {
            self.warn_autocvar(decl, location);
        }
        // FTEF-6.10: a field whose type is a struct with no body so far reads the struct's size
        // that is not there ("type NAME not fully defined yet").
        if self.profile.type_declarations()
            && let Type::Field(inner) = &decl.ty
            && self.incomplete_struct(inner, None)
        {
            let message = format!("type {} not fully defined yet", inner.fteqcc_name());
            self.report(Code::E0255, location, message);
            return Vec::new();
        }
        // FTE-11, FTEF-15.5: a `var` of function type holds a function; it defines none.
        let resolved_ty = self.resolved(&decl.ty);
        // FTE-17.7: an array of functions is a global holding functions, no function.
        // A `const` C function pointer is a constant, the function itself (FTEF-21.7).
        let pointer_var = decl.pointer_var && !decl.has(&Qualifier::Const);
        let kind = if (decl.has(&Qualifier::Var) || pointer_var || decl.array.is_some())
            && resolved_ty.is_function()
        {
            DefKind::Global
        } else {
            top_level_kind(&resolved_ty)
        };
        let pending = |def: DefId| {
            decl.body
                .as_ref()
                .map(|body| vec![(file, def, None, body, Some(decl))])
                .unwrap_or_default()
        };
        // FTEF-19.6: the default values are read with the type, before the name, so a fatal
        // error in one ends the declaration before it defines or checks the name.
        if self.profile.definition_checks() && self.check_defaults(file, decl) {
            return Vec::new();
        }
        let mut aborted = false;
        let existing = match self.find_existing(decl, location) {
            Ok(existing) => existing,
            Err(found) => return pending(found),
        };
        aborted |= qualifier_fatal;
        // FTEF-19.12: `static` on a name another file defined is "can't redefine non-static
        // as static".
        if self.profile.qualifiers()
            && decl.is_static()
            && let Some(existing) = existing
            && self
                .index
                .def(existing)
                .is_some_and(|d| d.location.file != location.file)
        {
            self.report(
                Code::E0231,
                location,
                "can't redefine non-static as static".to_owned(),
            );
            aborted = true;
        }
        let was_initialized =
            existing.is_some_and(|d| self.index.def(d).is_some_and(|d| d.initialized));
        aborted = aborted || self.check_initialiser_shape(file, decl, existing, location);
        let reported = self.index.diagnostics.len();
        // QC-11.5: id's qcc reports "NAME redeclared" at the `=`, before it reads the value.
        let value = if aborted
            || (!self.profile.reinitialise_with_same_value() && was_initialized && decl.initialized)
        {
            Ok(None)
        } else {
            let value = decl
                .value
                .as_ref()
                .filter(|value| !string_of_chars(self.array_of(decl), &decl.ty, value));
            self.check_value(file, &decl.name, &decl.ty, value, Holder::Global)
        };
        // FTE-22.6: a value FTEQCC found no constant is not compared with the first.
        let not_constant = self
            .index
            .diagnostics
            .get(reported..)
            .is_some_and(|new| new.iter().any(|d| d.diagnostic.code == Code::E0214));
        let literal = match value {
            Ok(_) if not_constant && self.profile.reinitialise_with_same_value() => None,
            Ok(value) => value.or_else(|| decl.literal.clone()),
            Err(Fatal) => {
                aborted = true;
                None
            }
        };
        // FTEF-15.5: a definition marked `wrap`, `weak` or `accumulate`, or one after such a
        // definition, is given again without "NAME redeclared"; an `extern` one has its body
        // elsewhere.
        let replaces =
            decl.redefinable() || existing.is_some_and(|d| self.redefinable.contains(&d));
        let external = self.is_extern(decl) && self.resolved(&decl.ty).is_function();
        let shared = decl.has(&Qualifier::Shared);
        let initialized = decl.initialized || external || (self.profile.qualifiers() && shared);
        let marks_only = (external || shared) && !decl.initialized;
        let init = if aborted || (replaces && existing.is_some()) || marks_only {
            None
        } else {
            initialized.then_some(literal.as_ref())
        };
        // FTEF-17.9: a builtin given for a function that has a body is a warning, and the
        // function keeps its body.
        if self.profile.definition_checks()
            && !aborted
            && decl.builtin
            && let Some(existing) = existing
            && self.bodies.contains(&existing)
            && !self.unused.contains(&existing)
        {
            self.report(
                Code::W0206,
                location,
                "incompatible redeclaration. Ignoring replacement of qc function with builtin."
                    .to_owned(),
            );
            return Vec::new();
        }
        let statics = (self.profile.qualifiers() && decl.is_static()).then_some(self.unit);
        let facts = DeclFacts {
            component: false,
            checked: true,
            constant: Self::constant_at_file_level(decl),
            array: self.array_of(decl),
            params: &decl.params,
            statics,
            deprecated: decl.deprecated(),
        };
        let def = self.declare_global_with(&decl.name, &decl.ty, kind, location, init, &facts);
        // FTEF-22.5: `if (externfnc && !pr_scope) { ... def->isextern = true; }`.
        // `QCC_PR_FinishCompilation` checks the constant ones, every function but a `var` one.
        let constant = !decl.has(&Qualifier::Var) && !decl.pointer_var;
        if self.profile.qualifiers() && external && constant && !aborted {
            self.externs.insert(def);
        }
        if let Some(f) = self.fact_mut(def) {
            f.shared |= shared;
            f.builtin |= decl.builtin && !aborted;
            f.accumulate |= decl.has(&Qualifier::Accumulate);
            // FTEQCC's `noref` marks a definition referenced too (`def->referenced = true;`).
            f.referenced |= decl.is_noref();
        }
        if aborted {
            return pending(def);
        }
        if decl.body.is_some() {
            self.bodies.insert(def);
        }
        if ((replaces && initialized) || marks_only)
            && let Some(d) = self.index.defs.get_mut(def.0)
        {
            d.initialized = true;
        }
        if decl.has(&Qualifier::Weak) || decl.has(&Qualifier::Accumulate) {
            self.redefinable.insert(def);
        } else if decl.initialized && !self.facts.get(def.0).is_some_and(|f| f.accumulate) {
            self.redefinable.remove(&def);
        }
        pending(def)
    }

    /// QC-11.5, FTEF-19.7, FTEF-19.8: the definition a top-level declaration redeclares, found
    /// and checked as `PR_GetDef` and `QCC_PR_GetDef` find it, before any initialiser is read;
    /// `None` for a new one, and `Err` with the definition when the declaration is refused.
    pub(in crate::resolve) fn find_existing(
        &mut self,
        decl: &RawDecl,
        location: Location,
    ) -> Result<Option<DefId>, DefId> {
        let Some(found) = self.lookup_global(&decl.name) else {
            return Ok(None);
        };
        let redeclaration = Redeclaration {
            existing: found,
            name: &decl.name,
            ty: &decl.ty,
            location,
            init: None,
            params: &decl.params,
            array: self.array_of(decl),
        };
        match self.check_redeclared_type(&redeclaration, false) {
            TypeCheck::Mismatch => Err(found),
            TypeCheck::Replaced => {
                if let Some(defs) = self.globals.get_mut(&decl.name) {
                    defs.retain(|d| *d != found);
                }
                Ok(None)
            }
            TypeCheck::Same | TypeCheck::Ignored => {
                if self.check_array(&redeclaration, false) {
                    Err(found)
                } else {
                    Ok(Some(found))
                }
            }
        }
    }

    /// The first array dimension of `decl`, its size folded when it is an expression
    /// (FTEF-19.62).
    pub(in crate::resolve) fn array_of(&self, decl: &RawDecl) -> Option<ArrayDim> {
        match (decl.array, self.folded_sizes.get(&decl.span.start)) {
            (Some(ArrayDim::Unknown), Some(&size)) => Some(ArrayDim::Sized(size)),
            (array, _) => array,
        }
    }

    /// FTEF-19.16: an `autocvar_` defined as a constant or static is made neither, with a
    /// warning.
    pub(in crate::resolve) fn warn_autocvar(&mut self, decl: &RawDecl, location: Location) {
        if !decl.name.starts_with("autocvar_") {
            return;
        }
        let what = if Self::constness(decl, decl.initialized || decl.skipped_init) {
            "constant"
        } else if decl.is_static() {
            "static"
        } else {
            return;
        };
        self.report(
            Code::W0213,
            location,
            format!(
                "Autocvar \"{}\" defined as {what}. attempting to correct that for you.",
                decl.name
            ),
        );
    }

    /// Whether `QCC_PR_ParseDefs` keeps the `extern` of a declaration whose base type is `base`
    /// (FTEF-21.10): when the type, its typedefs read, is a function's, or one the model does not
    /// know.
    pub(in crate::resolve) fn extern_kept(&self, base: &ExternBase) -> bool {
        let ty = self.resolved(&base.ty);
        ty.is_function() || ty == Type::Unknown
    }

    /// Whether `decl` is `extern` once `QCC_PR_ParseDefs` has tested it against its base type
    /// (FTEF-21.10).
    pub(in crate::resolve) fn is_extern(&self, decl: &RawDecl) -> bool {
        decl.has(&Qualifier::Extern)
            && decl
                .extern_base
                .as_ref()
                .is_none_or(|base| self.extern_kept(base))
    }

    /// FTEF-19.13 and the warnings of `QCC_PR_ParseDefs` about qualifiers it ignores
    /// (FTEF-19.16): `extern` on what is no function, conflicting storage classes or
    /// constness, a function modifier on what is no function, and `accumulate` on what is no
    /// function, an error.
    pub(in crate::resolve) fn warn_qualifiers(
        &mut self,
        decl: &RawDecl,
        location: Location,
    ) -> bool {
        let function = decl.ty.is_function();
        // FTEF-21.10: an `extern` on a base type that is no function's is dropped, with its
        // warning when `qcfuncs` is on.
        if decl
            .extern_base
            .as_ref()
            .is_some_and(|base| base.qcfuncs && !self.extern_kept(base))
        {
            self.report(
                Code::W0214,
                location,
                "extern keyword may only apply to qc functions.".to_owned(),
            );
        }
        // `istypedef+externfnc+isauto+isstatic+shared+(aliasof!=NULL)>1`, `isstatic` with its
        // `#pragma defaultstatic` default (FTEF-21.14).
        let storage = [Qualifier::Auto, Qualifier::Shared]
            .iter()
            .filter(|q| decl.has(q))
            .count()
            .saturating_add(usize::from(self.is_extern(decl)))
            .saturating_add(usize::from(decl.is_static()))
            .saturating_add(usize::from(decl.alias().is_some()));
        if storage > 1 {
            self.report(
                Code::W0214,
                location,
                "Conflicting storage classes.".to_owned(),
            );
        }
        // `isconstant+isvar>1`, C's function pointer a `var` (`isvar |= isptr>0`, FTEF-21.7).
        if decl.has(&Qualifier::Const) && (decl.has(&Qualifier::Var) || decl.pointer_var) {
            self.report(Code::W0214, location, "Conflicting constness.".to_owned());
        }
        let modifier = [
            Qualifier::Inline,
            Qualifier::Wrap,
            Qualifier::Weak,
            Qualifier::Accumulate,
        ]
        .iter()
        .any(|q| decl.has(q));
        if modifier && !function {
            self.report(
                Code::W0214,
                location,
                "function modifier on non-function.".to_owned(),
            );
        }
        if decl.has(&Qualifier::Accumulate) && !function {
            self.report(
                Code::E0230,
                location,
                format!("accumulate applies only to functions, not {}", decl.name),
            );
            return true;
        }
        // FTEF-19.61: a global `__auto` is allocated by `memalloc`, or `malloc`, which
        // `QCC_PR_EmulationFunc` finds by name.
        if decl.has(&Qualifier::Auto)
            && self.lookup_global("memalloc").is_none()
            && self.lookup_global("malloc").is_none()
        {
            let message = format!(
                "memalloc not yet defined, cannot define __auto {} at global scope",
                decl.name
            );
            self.report(Code::E0365, location, message);
            return true;
        }
        false
    }

    /// QC-11.8, QCD-3.11: every global function must be defined; report those that are not.
    /// Under FTEQCC one marked `noref`, by its qualifier or `#pragma noref 1`, that the module
    /// does not refer to is not reported (FTEF-17.11): FTEQCC's check skips a definition `if
    /// (d->unused && !d->used)`.
    ///
    /// FTEF-22.5: in the same pass, definition by definition, an `extern` function is an error
    /// on a target with no `extern`. Like the undefined functions, and unlike FTEQCC's check,
    /// which a module with errors never reaches, it is reported whatever else was (QCD-1.4).
    pub(in crate::resolve) fn check_undefined_functions(&mut self) {
        let used: HashSet<DefId> = self.index.refs.iter().map(|r| r.def).collect();
        let externs_refused = self
            .profile
            .target()
            .is_some_and(|target| !target.supports_extern());
        let mut undefined: Vec<FileDiagnostic> = Vec::new();
        for (index, def) in self.index.defs.iter().enumerate() {
            let id = DefId(index);
            if externs_refused && self.externs.contains(&id) && self.resolved(&def.ty).is_function()
            {
                undefined.push(diag(
                    Code::E0239,
                    def.location,
                    "extern is not supported with this target format".to_owned(),
                ));
            }
            let skipped =
                self.profile.qualifiers() && self.unused.contains(&id) && !used.contains(&id);
            // FTEF-19.41: a member function a class declares and nothing defines has no body
            // either, a virtual one among them, which is a field of function type.
            let var_member = def
                .member_of
                .is_some_and(|c| self.var_members.contains(&(c, def.name.clone())));
            // An array of member functions is an array of fields, which no body defines.
            let member_array = self.facts.get(id.0).is_some_and(|f| f.array.is_some());
            let member_function = def.member_of.is_some()
                && !var_member
                && !member_array
                && (def.kind == DefKind::Function
                    || (def.kind == DefKind::Field && self.resolved(&def.ty).is_function()));
            let function =
                (def.kind == DefKind::Function && def.member_of.is_none()) || member_function;
            if function && !def.initialized && !def.generated && !skipped {
                // FTEF-17.11: FTEQCC's `QCC_PR_Warning(ERR_NOFUNC, ..., "function %s has no
                // body", d->name)`.
                let message = if self.profile.fteqcc_diagnostics() {
                    // A member function is named `CLASS::NAME`, as FTEQCC defines it.
                    let class = def.member_of.and_then(|c| self.index.defs.get(c.0));
                    match class {
                        Some(class) => format!("function {}::{} has no body", class.name, def.name),
                        None => format!("function {} has no body", def.name),
                    }
                } else {
                    format!("function {} was not defined", def.name)
                };
                undefined.push(diag(Code::E0211, def.location, message));
            }
        }
        self.index.diagnostics.extend(undefined);
    }
}
