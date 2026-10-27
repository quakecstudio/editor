// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_ParseImmediateStatements, PR_ParseState, PR_ParseValue
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseImmediateStatements, QCC_PR_ParseState, QCC_PR_ParseRefValue

//! Function bodies: parameters, locals, aliases, state headers and the names a body uses
//! (QC-12.5, QC-13, FTEF-19.1, FTEF-19.14, FTEF-20.5).

use qc_diag::{Code, Span};
use qc_preproc::FteFlag;
use std::rc::Rc;

use crate::cast::{Kind, Source, Supply};
use crate::check::{check_body, check_initialiser};
use crate::lower::{
    Qualifier, RawAlias, RawBody, RawDecl, RawLocal, RawUse, StatePart, UseKind, ValueKind,
    top_level_kind,
};
use crate::resolve::check_env::BodyEnv;
use crate::resolve::{
    COMPONENTS, DeclFacts, Found, Holder, Pending, Redeclaration, Resolver, Scope, ScopedFacts,
    component_type,
};
use crate::symbol::{DefId, DefKind, Definition, FileId, Location, Reference};
use crate::ty::{FunctionType, ParamLabels, SimpleType, Type};

impl<'a> Resolver<'a> {
    /// Declares, or checks, the next-think name of a state header (QC-13.6).
    ///
    /// If the name does not exist it is declared as a global `void()`. If it exists with a
    /// different type it is a type mismatch (QCD-3.5).
    pub(in crate::resolve) fn declare_next_think(
        &mut self,
        file: FileId,
        class: Option<DefId>,
        name: &str,
        span: Span,
    ) {
        let void_fn = Type::Function(Rc::new(FunctionType {
            ret: Type::Simple(SimpleType::Void),
            params: Vec::new(),
            variadic: false,
            labels: ParamLabels::default(),
        }));
        let location = self.at(file, span);
        // FTEF-19.42: in a member function the name is a member function of the class or its
        // parents first, which must be a `void()`.
        let earlier = |d: &Definition| {
            d.location.file != location.file || d.location.span.start <= span.start
        };
        if let Some(def) = class
            .and_then(|c| self.member_in(c, name))
            .filter(|def| self.index.def(*def).is_some_and(earlier))
        {
            let member_ty = self.index.def(def).map(|d| d.ty.clone());
            if let Some(member_ty) = member_ty
                && !member_ty.fteqcc_same(&void_fn)
            {
                self.report(
                    Code::E0309,
                    location,
                    format!(
                        "Type mismatch: {}, should be void()",
                        member_ty.fteqcc_name()
                    ),
                );
            }
            self.index.refs.push(Reference { location, def });
            return;
        }
        // FTEF-19.33: a name found in any case under `-Fcaseinsens` is that definition.
        let found = self
            .lookup_global(name)
            .or_else(|| self.lookup_any_case(None, name));
        // FTEF-7.5: a next function declared through a typedef has the typedef's type, which
        // must be a `void()` (FTEF-19.42).
        if let Some(d) = found.and_then(|d| self.index.def(d))
            && matches!(&d.ty, Type::Named(_) | Type::Function(_))
        {
            let resolved = self.resolved(&d.ty);
            if resolved.is_function() && !resolved.fteqcc_same(&void_fn) {
                self.report(
                    Code::E0309,
                    location,
                    format!(
                        "Type mismatch: {}, should be void()",
                        resolved.fteqcc_name()
                    ),
                );
                return;
            }
        }
        let name = found
            .and_then(|d| self.index.def(d))
            .map_or_else(|| name.to_owned(), |d| d.name.clone());
        let name = name.as_str();
        let facts = DeclFacts {
            constant: true,
            ..DeclFacts::default()
        };
        let def =
            self.declare_global_with(name, &void_fn, DefKind::Function, location, None, &facts);
        // The name in the header is a reference to the think function.
        self.index.refs.push(Reference { location, def });
    }

    /// Resolves a body for its diagnostics alone: what it adds to the index, its locals and
    /// references, is taken out again (FTEF-12.19).
    pub(in crate::resolve) fn resolve_discarded(&mut self, pending: Pending<'a>) {
        let (defs, refs) = (self.index.defs.len(), self.index.refs.len());
        self.resolve_pending(pending);
        self.index.defs.truncate(defs);
        self.facts.truncate(defs);
        self.index.refs.truncate(refs);
        // A body may define a global (an `autocvar_` local, FTEF-19.16), which is cut off too.
        for ids in self.globals.values_mut() {
            ids.retain(|id| id.0 < defs);
        }
    }

    pub(in crate::resolve) fn resolve_pending(
        &mut self,
        (file, def, class, body, decl): Pending<'a>,
    ) {
        // A body is read in its own unit (FTEF-19.12).
        self.unit = file;
        self.body_typedefs.clone_from(&body.typedefs);
        self.resolve_body(file, def, class, body, decl);
        self.body_typedefs.clear();
    }

    /// Resolves one function body: its parameters, its state header, its locals and name
    /// uses (QC-12.1, QC-12.5), in the class `class` when it is a member function's
    /// (FTEF-17.2). `decl` is the declaration the body defines, whose state header names the
    /// next think function.
    pub(in crate::resolve) fn resolve_body(
        &mut self,
        file: FileId,
        owner: DefId,
        class: Option<DefId>,
        body: &RawBody,
        decl: Option<&RawDecl>,
    ) {
        // Record where this function's locals start, so lookups in it see only its own.
        let scope = Scope {
            owner,
            locals_start: self.index.defs.len(),
            class,
            at: 0,
        };
        let reported = self.index.diagnostics.len();
        for d in &body.diagnostics {
            self.settle(d.span.start);
            let location = self.at(file, d.span);
            self.report(d.code, location, d.message.clone());
        }
        if self.profile.body_checks() {
            let function = self
                .index
                .def(owner)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            for (number, span) in &body.unnamed_params {
                self.settle(span.start);
                let location = self.at(file, *span);
                self.report(
                    Code::W0222,
                    location,
                    format!("Parameter {number} of {function} is not named"),
                );
            }
        }
        let mut previous: Option<DefId> = None;
        // FTEF-7.5: a definition of a typedef'd function type has the typedef's parameters.
        let typedef_params = decl
            .filter(|_| body.params.is_empty())
            .and_then(|d| d.ty.type_name())
            .and_then(|n| self.typedef_params.get(&self.type_key(n)).cloned())
            .unwrap_or_default();
        for param in body.params.iter().chain(&typedef_params) {
            self.settle(param.span.start);
            let location = self.at(file, param.span);
            let def = self.declare_scoped(scope, &param.name, &param.ty, DefKind::Param, location);
            // QC-13.5: `PR_ParseImmediateStatements` stops at a parameter whose global lies
            // below the one before it, "bad parm order".
            if self.profile.checks_parameter_order()
                && let (Some(def), Some(before)) = (def, previous)
                && self.below(def, before)
            {
                self.report(Code::E0404, location, "bad parm order".to_owned());
            }
            previous = def;
        }
        // FTEF-19.14: the state header's frame is read before its next function is declared.
        self.resolve_uses(file, Some(scope), &body.state_uses);
        if self.profile.body_checks() {
            self.check_state_literals(file, body);
        }
        if let Some(decl) = decl
            && let Some((next_name, next_span)) = &decl.next_think
        {
            self.declare_next_think(file, scope.class, next_name, *next_span);
        }
        // FTEF-20.5: a `state` statement declares its next think function as a header does.
        for (next_name, next_span) in &body.next_thinks {
            self.settle(next_span.start);
            self.declare_next_think(file, scope.class, next_name, *next_span);
        }
        // QC-11.10, QC-11.9: locals; a local field or function is an error (QCD-3.8 is the
        // parser's; the semantic check is QC-11.9).
        for (ty, span) in &body.type_only {
            let location = self.at(file, *span);
            self.check_type_names(ty, &body.typedefs, span.start, location);
        }
        for local in &body.locals {
            self.settle(local.span.start);
            let location = self.at(file, local.span);
            if local.bad_field_or_function {
                self.report(
                    Code::E0208,
                    location,
                    "Fields and functions must be global".to_owned(),
                );
                continue;
            }
            // FTEF-19.20: a type name no type has is "not a type", and no local is made.
            if self.profile.type_declarations()
                && self.check_type_names(&local.ty, &body.typedefs, local.span.start, location)
            {
                continue;
            }
            let made = self.index.defs.len();
            let at = Scope {
                at: local.span.start,
                ..scope
            };
            self.declare_local(file, at, local, location);
            // FTEF-19.19: what the declaration made new leaves scope with its `for`; a local
            // it found again stays.
            if let Some(end) = local.scope_end {
                for f in self.facts.iter_mut().skip(made) {
                    f.scope_end = Some(end);
                }
            }
        }
        for alias in &body.aliases {
            self.declare_alias(file, scope, alias);
        }
        // QC-12.5: resolve each name use against globals and this function's own locals.
        self.use_defs.clear();
        self.resolve_uses(file, Some(scope), &body.uses);
        if let Some(syntax) = &body.syntax {
            let uses = std::mem::take(&mut self.use_defs);
            let mut env = BodyEnv {
                resolver: self,
                file,
                owner,
                reported,
                in_class: class.is_some(),
                uses: &uses,
                flags: &body.flags,
                lex: &body.lex_flags,
                written: &body.written_types,
                locals: &body.locals,
            };
            let typed = check_body(&mut env, syntax);
            self.index.bodies.insert(owner, typed);
        }
    }

    /// FTEF-34.9: an initialiser of a global that is an expression, read at global scope,
    /// where every statement it generates is an error, then cast to the global's type, and no
    /// constant "is not constant".
    pub(in crate::resolve) fn check_global_initialiser(&mut self, file: FileId, decl: &RawDecl) {
        if !self.profile.fteqcc_diagnostics() || decl.body.is_some() {
            return;
        }
        let Some(value) = decl.value.as_ref() else {
            return;
        };
        let Some(expr) = value.syntax.clone() else {
            return;
        };
        let written = value.written_types.clone();
        let Some(owner) = self.lookup_global(&decl.name) else {
            return;
        };
        let ty = decl.ty.clone();
        let uses = self.use_defs.clone();
        let flags = [(0, decl.flags)];
        let reported = self.index.diagnostics.len();
        let mut env = BodyEnv {
            resolver: self,
            file,
            owner,
            reported,
            in_class: false,
            uses: &uses,
            flags: &flags,
            lex: &[],
            written: &written,
            locals: &[],
        };
        check_initialiser(&mut env, &expr, &ty, &decl.name);
    }

    /// A definition the attribute `alias(TARGET)` gives (FTEF-21.9): `QCC_PR_GetDef (NULL,
    /// aliasof, externfnc?NULL:pr_scope, false, ...)` finds the definition `TARGET` names, in
    /// `scope` or among the globals, else "TARGET not yet defined, cannot create NAME as an
    /// alias"; it is referenced, and `QCC_PR_DummyDef` makes `name` of type `ty` a new
    /// definition with no lookup, which "alias NAME may not be initialised".
    pub(in crate::resolve) fn declare_alias_def(
        &mut self,
        scope: Option<Scope>,
        name: &str,
        ty: &Type,
        target: &str,
        initialized: bool,
        location: Location,
    ) {
        let found = match scope {
            Some(scope) => self.lookup_in_scope(scope, target),
            None => self.lookup_global(target),
        };
        let Some(found) = found else {
            self.report(
                Code::E0234,
                location,
                format!("{target} not yet defined, cannot create {name} as an alias"),
            );
            return;
        };
        if let Some(f) = self.fact_mut(found) {
            f.referenced = true;
        }
        let kind = if scope.is_some() {
            DefKind::Local
        } else {
            DefKind::Global
        };
        let id = self.push_def(Definition {
            name: name.to_owned(),
            ty: ty.clone(),
            kind,
            location,
            owner: scope.map(|s| s.owner),
            initialized: false,
            member_of: None,
            generated: false,
        });
        if let (Some(scope), Some(f)) = (scope, self.fact_mut(id)) {
            f.scope_start = Some(scope.at);
        }
        if initialized {
            self.report(
                Code::E0235,
                location,
                format!("alias {name} may not be initialised"),
            );
        }
    }

    /// One alias of a `using` statement (FTEF-20.5): the definition its target names, found as
    /// a value is, else "NAME is not defined" (`ERR_NOTDEFINED`); the alias a local of its type
    /// that `QCC_PR_DummyDef` makes, with no lookup, until the statement ends.
    pub(in crate::resolve) fn declare_alias(
        &mut self,
        file: FileId,
        scope: Scope,
        alias: &RawAlias,
    ) {
        let (target, target_span) = &alias.target;
        self.settle(target_span.start);
        let target_location = self.at(file, *target_span);
        let at = Scope {
            at: target_span.start,
            ..scope
        };
        let Some(def) = self.lookup_in_scope(at, target) else {
            self.report(
                Code::E0313,
                target_location,
                format!("{target} is not defined"),
            );
            return;
        };
        if let Some(f) = self.fact_mut(def) {
            f.referenced = true;
        }
        self.index.refs.push(Reference {
            location: target_location,
            def,
        });
        let ty = self.index.def(def).map_or(Type::Unknown, |d| d.ty.clone());
        let (name, span) = &alias.alias;
        self.settle(span.start);
        let location = self.at(file, *span);
        let id = self.push_def(Definition {
            name: name.clone(),
            ty,
            kind: DefKind::Local,
            location,
            owner: Some(scope.owner),
            initialized: false,
            member_of: None,
            generated: false,
        });
        if let Some(f) = self.fact_mut(id) {
            f.referenced = true;
            f.scope_start = Some(span.start);
            f.scope_end = alias.scope_end;
        }
    }

    /// FTEF-19.14: FTEQCC converts a state header's frame to a float with
    /// `QCC_SupplyConversion` (FTEF-22.3), and requires its next function to be a `void()`,
    /// "Type mismatch" for a literal but 0. A cycle's frame that `lax` let through unconverted
    /// is "state type mismatch", once both frames are converted. A fatal error ends the header.
    pub(in crate::resolve) fn check_state_literals(&mut self, file: FileId, body: &RawBody) {
        let mut unconverted = None;
        for (part, value) in &body.state_literals {
            let ValueKind::Literal(literal) = &value.kind else {
                continue;
            };
            let location = self.at(file, value.span);
            match part {
                StatePart::Frame | StatePart::CycleFrame => {
                    let source = Source::Literal(*literal);
                    match self.supply_value(source, Kind::Float, value, location) {
                        Supply::Converted => {}
                        Supply::Lax => {
                            if *part == StatePart::CycleFrame {
                                unconverted = Some(location);
                            }
                        }
                        Supply::Explicit | Supply::Mismatch => return,
                    }
                }
                StatePart::Next => {
                    if !literal.null {
                        self.report(
                            Code::E0309,
                            location,
                            format!("Type mismatch: {}, should be void()", literal.ty.name()),
                        );
                    }
                }
            }
        }
        // `if (s1.cast->type != ev_float || def.cast->type != ev_float)`, after both.
        if let Some(location) = unconverted {
            self.report(Code::E0238, location, "state type mismatch".to_owned());
        }
    }

    /// Resolves name uses in a function's scope, or with no scope in the global scope (an
    /// initialiser or a default value, FTE-10.2, FTE-18.2), as references; a value that
    /// resolves to nothing is "Unknown value" (QCD-4.1).
    pub(in crate::resolve) fn resolve_uses(
        &mut self,
        file: FileId,
        scope: Option<Scope>,
        uses: &[RawUse],
    ) {
        for use_ in uses {
            self.settle(use_.span.start);
            let location = self.at(file, use_.span);
            let scope = scope.map(|s| Scope {
                at: use_.span.start,
                ..s
            });
            let found = match (&use_.scope, use_.kind) {
                (Some(prefix), UseKind::Value | UseKind::Member | UseKind::TypeOrValue)
                    if self.profile.name_use_checks() =>
                {
                    self.resolve_scoped(scope, prefix, &use_.name)
                }
                (_, UseKind::Value) => self.resolve_value(scope, &use_.name, false),
                (_, UseKind::TypeOrValue) => self.resolve_value(scope, &use_.name, true),
                (_, UseKind::CastTo { from }) => {
                    // FTEF-6.12: in a body the check stage refuses the cast as it reads it.
                    if scope.is_none() && self.struct_type_name(&use_.name).is_some() {
                        let message = format!("Cannot cast from {from} to struct {}", use_.name);
                        self.report(Code::E0213, location, message);
                    }
                    Found::Nothing
                }
                (_, UseKind::TypeName) => {
                    let ty = Type::Named(use_.name.as_str().into());
                    let typedefs = self.body_typedefs.clone();
                    self.check_type_names(&ty, &typedefs, use_.span.start, location);
                    Found::Nothing
                }
                (_, UseKind::Member) => self.resolve_member(scope, use_),
            };
            // FTEF-24.1: in a body, what a member names when it is no field is the check
            // stage's to judge from the base's type, as is a member no definition has
            // (`QCC_PR_ParseField`); the definition is passed on, with no reference made.
            let found = match (found, scope) {
                (Found::Nothing | Found::Unknown, Some(at))
                    if use_.kind == UseKind::Member && self.profile.name_use_checks() =>
                {
                    if let Some(def) = self.lookup_in_scope(at, &use_.name) {
                        self.use_defs.insert(use_.span.start, def);
                    }
                    Found::Nothing
                }
                (found, _) => found,
            };
            match found {
                Found::Def(def) => {
                    if let Some(f) = self.fact_mut(def) {
                        f.referenced = true;
                    }
                    if self.profile.name_use_checks()
                        && let Some(reason) =
                            self.facts.get(def.0).and_then(|f| f.deprecated.clone())
                    {
                        let name = &use_.name;
                        let message = if reason.is_empty() {
                            format!("Variable \"{name}\" is deprecated")
                        } else {
                            format!("Variable \"{name}\" is deprecated: {reason}")
                        };
                        self.report(Code::W0314, location, message);
                    }
                    self.use_defs.insert(use_.span.start, def);
                    self.index.refs.push(Reference { location, def });
                }
                Found::Nothing => {}
                Found::Error(code, message) => self.report(code, location, message),
                Found::Unknown => {
                    // FTEF-19.45: FTEQCC makes an unknown name a `variant` and reports it with a
                    // full stop, `QCC_PR_ParseWarning (ERR_UNKNOWNVALUE, "Unknown value
                    // \"%s\".", name)`; a scoped name has none.
                    let message = match &use_.scope {
                        Some(prefix) => format!("Unknown value \"{prefix}::{}\"", use_.name),
                        None if self.profile.fteqcc_diagnostics() => {
                            // The `variant` is made by `QCC_PR_GetDef`, which warns of an
                            // `autocvar_` name in a body (FTEF-19.16) and of a keyword's name
                            // (FTEF-20.4) first.
                            if scope.is_some() && use_.name.starts_with("autocvar_") {
                                let message = format!(
                                    "Autocvar \"{}\" defined with local scope. promoting to \
                                     global.",
                                    use_.name
                                );
                                self.report(Code::W0213, location, message);
                            }
                            if self.profile.warns_keyword_names()
                                && self.keywords.named_definition(&use_.name, false).is_some()
                            {
                                let what = if self.keywords.coexist() {
                                    "coexisting"
                                } else {
                                    "disabling"
                                };
                                let message = format!(
                                    "\"{}\" keyword used as variable name - {what}",
                                    use_.name
                                );
                                self.report(Code::W0221, location, message);
                            }
                            format!("Unknown value \"{}\".", use_.name)
                        }
                        None => format!("Unknown value \"{}\"", use_.name),
                    };
                    self.report(Code::E0301, location, message);
                }
            }
        }
    }

    /// Declares a local of a function body (QC-11.10, FTEF-19.1), with its initialiser's
    /// checks. Under FTEQCC an `autocvar_` local is a global (FTEF-19.16), and a `static` or
    /// `const` local is a constant whose initialiser FTEQCC compares as a global's.
    pub(in crate::resolve) fn declare_local(
        &mut self,
        file: FileId,
        scope: Scope,
        local: &RawLocal,
        location: Location,
    ) {
        if self.profile.qualifiers() && local.name.starts_with("autocvar_") {
            self.report(
                Code::W0213,
                location,
                format!(
                    "Autocvar \"{}\" defined with local scope. promoting to global.",
                    local.name
                ),
            );
            let kind = top_level_kind(&local.ty);
            let init = local.initialized.then_some(local.literal.as_ref());
            self.declare_global(&local.name, &local.ty, kind, location, init);
            return;
        }
        if self.profile.body_checks() && local.global_function {
            // FTEF-21.10: a C function declared in a body is a global constant, its body
            // elsewhere.
            self.declare_global(&local.name, &local.ty, DefKind::Function, location, None);
            return;
        }
        if self.profile.qualifiers()
            && let Some(target) = local.qualifiers.iter().find_map(|q| match q {
                Qualifier::Alias(name) => Some(name.as_str()),
                _ => None,
            })
        {
            self.declare_alias_def(
                Some(scope),
                &local.name,
                &local.ty,
                target,
                local.initialized,
                location,
            );
            return;
        }
        let statics = self.profile.qualifiers() && local.is_static();
        let constant = self.profile.qualifiers() && local.has(&Qualifier::Const);
        let compared = constant || statics;
        let holder = if !self.profile.qualifiers() {
            Holder::Local(scope)
        } else if statics {
            Holder::Static(scope)
        } else if constant {
            Holder::Const(scope)
        } else {
            Holder::Local(scope)
        };
        // `PR_GetDef` and `QCC_PR_GetDef` find or make the definition before the initialiser
        // is read, and a type mismatch ends the declaration.
        let def = self.declare_scoped_with(
            scope,
            &local.name,
            &local.ty,
            DefKind::Local,
            location,
            ScopedFacts {
                component: false,
                array: local.array,
                statics,
                subscope: self.profile.body_checks() && local.flags.has(FteFlag::PrSubscopedlocals),
            },
        );
        let Some(def) = def else {
            return;
        };
        if let Some(f) = self.fact_mut(def) {
            f.constant |= constant;
        }
        if !local.initialized {
            return;
        }
        // QC-11.13: under the vanilla profile a local, or the global it reuses (QC-12.3), may
        // be initialised once, which `PR_ParseDefs` checks at the `=`; FTEQCC compares a
        // constant's or static's second initialiser as a global's (FTE-22.5), and keeps no
        // other local initialised.
        let initialised_before = self.index.def(def).is_some_and(|d| d.initialized);
        if !self.profile.reinitialise_with_same_value() && initialised_before {
            self.report(Code::E0206, location, format!("{} redeclared", local.name));
            return;
        }
        if self.profile.definition_checks()
            && self.check_array_value(file, local.array, &local.ty, local.value.as_ref())
        {
            return;
        }
        let Ok(value) =
            self.check_value(file, &local.name, &local.ty, local.value.as_ref(), holder)
        else {
            return;
        };
        let literal = value.or_else(|| local.literal.clone());
        let tracked = !self.profile.reinitialise_with_same_value() || compared;
        if !tracked {
            return;
        }
        if initialised_before {
            let same = literal
                .as_ref()
                .is_some_and(|l| self.same_value(def, &local.name, l));
            if !same {
                self.report(Code::E0206, location, format!("{} redeclared", local.name));
            }
            return;
        }
        if let Some(d) = self.index.defs.get_mut(def.0) {
            d.initialized = true;
        }
        if let Some(literal) = literal {
            self.literals.insert(def, literal);
        }
    }

    /// Declares a parameter or local, and the components of a new vector (QC-11.7); returns
    /// the definition its name is, if any.
    pub(in crate::resolve) fn declare_scoped(
        &mut self,
        scope: Scope,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
    ) -> Option<DefId> {
        self.declare_scoped_with(scope, name, ty, kind, location, ScopedFacts::default())
    }

    /// [`Resolver::declare_scoped`]. A name the function already declared is that definition
    /// again, or a type mismatch (QC-11.5), and under FTEQCC a parameter again is "Duplicate
    /// definition" and a local again a warning (FTEF-19.1). In vanilla a name that is a global
    /// reuses the global (QC-12.3); in an FTEQCC profile the local is new and shadows it
    /// (FTE-22.1), with a warning when the global's type is of the same kind (FTEF-19.1). A
    /// `component` (QC-11.7) that reuses a global is not warned about, since the source never
    /// names it.
    pub(in crate::resolve) fn declare_scoped_with(
        &mut self,
        scope: Scope,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
        facts: ScopedFacts,
    ) -> Option<DefId> {
        let existing = self.lookup_local(scope, name).or_else(|| {
            if self.profile.locals_shadow_globals() {
                None
            } else {
                self.lookup_global(name)
            }
        });
        // FTEF-21.12: under `subscope` a local found again is passed over, `assume it was
        // defined in a different subscope`, for a type mismatch, and for any other but a
        // `static` (`subscopes mean that the later one replaces the first, hopefully.`).
        let existing = existing.filter(|&found| {
            let replaced = facts.subscope
                && kind == DefKind::Local
                && (!facts.statics
                    || self
                        .index
                        .def(found)
                        .is_some_and(|d| !self.same_type(&d.ty, ty)));
            !replaced
        });
        if let Some(existing) = existing {
            let def = self.index.defs.get(existing.0)?;
            let global = def.kind.is_global();
            if !self.same_type(&def.ty, ty) {
                let old = def.ty.clone();
                self.mismatch(name, location, &old, ty);
                return None;
            }
            if self.profile.redeclaration_by_typecmp() && !facts.component {
                let redeclaration = Redeclaration {
                    existing,
                    name,
                    ty,
                    location,
                    init: None,
                    params: &[],
                    array: facts.array,
                };
                if self.check_array(&redeclaration, true) {
                    return Some(existing);
                }
                let was_static = self.facts.get(existing.0).is_some_and(|f| f.static_local);
                if kind == DefKind::Param {
                    self.report(
                        Code::E0215,
                        location,
                        format!("Duplicate definition of {name}."),
                    );
                } else if !facts.statics {
                    let message = if was_static {
                        format!("nonstatic redeclaration of {name} ignored")
                    } else {
                        format!("{name} duplicate definition ignored")
                    };
                    self.report(Code::W0207, location, message);
                }
            } else if self.profile.components_initialised() && kind == DefKind::Param {
                // A parameter's vector components are made with no lookup (FTEF-19.1).
                return Some(existing);
            }
            if global && !facts.component {
                // QCD-7.3: warn that the local name is the global.
                self.report(
                    Code::W0201,
                    location,
                    format!("local {name} is the global {name}"),
                );
            }
            // The declaration is a reference to the definition it reuses.
            self.index.refs.push(Reference {
                location,
                def: existing,
            });
            return Some(existing);
        }
        if self.profile.locals_shadow_globals()
            && !facts.component
            && let Some(global) = self.lookup_global(name)
            && self.index.def(global).is_some_and(|g| {
                g.ty.fteqcc_kind() == ty.fteqcc_kind() && ty.fteqcc_kind() != Kind::Opaque
            })
        {
            // FTEF-19.1: WARN_SAMENAMEASGLOBAL, for a global of the same kind of type only.
            self.report(
                Code::W0208,
                location,
                format!("Local \"{name}\" hides global with same name and type"),
            );
        }
        let id = self.push_def(Definition {
            name: name.to_owned(),
            ty: ty.clone(),
            kind,
            location,
            owner: Some(scope.owner),
            initialized: false,
            member_of: None,
            generated: false,
        });
        if let Some(f) = self.fact_mut(id) {
            f.array = facts.array;
            f.static_local = facts.statics;
            // FTEF-21.12: a local is found from where it is made, so one made beside another of
            // its name hides it from there on, as `QCC_PR_GetDef` makes each as it reads it.
            f.scope_start = Some(scope.at);
        }
        // QC-11.7: a new vector local or parameter has its components too.
        if facts.array.is_none()
            && let Some(component) = component_type(ty)
        {
            for suffix in COMPONENTS {
                let component_name = format!("{name}{suffix}");
                let component_facts = ScopedFacts {
                    component: true,
                    subscope: facts.subscope,
                    ..ScopedFacts::default()
                };
                self.declare_scoped_with(
                    scope,
                    &component_name,
                    &component,
                    kind,
                    location,
                    component_facts,
                );
            }
        }
        Some(id)
    }
}
