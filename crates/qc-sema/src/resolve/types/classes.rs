// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseDefs

//! Classes and accessors: their members, parents, member functions and the
//! member lookup of an access (FTEF-9, FTEF-10, FTEF-17.1 to FTEF-17.3).

use qc_diag::Code;
use std::collections::HashMap;
use std::rc::Rc;

use crate::cast::Kind;
use crate::lower::ArrayDim;
use crate::lower::{MemberBase, MemberKind, RawClass, RawDecl, RawUse, top_level_kind};
use crate::resolve::{ClassInfo, Found, MAX_CLASS_DEPTH, Pending, Resolver, Scope, StructMatch};
use crate::symbol::{DefId, DefKind, Definition, FileId, Location, Reference};
use crate::ty::{FunctionType, SimpleType, Type};

impl<'a> Resolver<'a> {
    /// `accessor Name : Base` (FTEF-10.6): the base is a type and no struct or union, and the
    /// same one each time; a first declaration has one.
    pub(in crate::resolve) fn declare_accessor(&mut self, decl: &RawDecl, location: Location) {
        let Some(base) = &decl.accessor else {
            return;
        };
        let key = self.type_key(&decl.name);
        let known = self.accessor_bases.get(&key).cloned();
        // `if (newt && newt->type != ev_accessor) ... "Type %s cannot be redefined as an
        // accessor"`: the name of a typedef, class, enum, struct or registered type.
        if known.is_none()
            && (self.other_sized_type_named(&decl.name) || self.struct_defs.contains_key(&key))
        {
            let message = format!("Type {} cannot be redefined as an accessor", decl.name);
            self.report(Code::E0258, location, message);
            return;
        }
        if known.is_none()
            && let Some(Type::Named(n)) = base
            && self.type_key(n) == key
        {
            // `QCC_PR_ParseType` finds no type of the name yet: "\"A\" is not a type".
            self.report(Code::E0203, location, format!("\"{n}\" is not a type"));
            return;
        }
        let Some(base) = base else {
            let message = match &known {
                None => format!("Accessor {} cannot be based upon ??", decl.name),
                Some(old) => format!(
                    "Accessor {} basic type mismatch (??, expected {})",
                    decl.name,
                    old.fteqcc_name()
                ),
            };
            self.report(Code::E0258, location, message);
            return;
        };
        if self.check_type_names(base, &[], decl.span.start, location) {
            return;
        }
        let resolved = self.resolved(base);
        // A base written as `struct S` or `union U` is a struct's type whether or not it has
        // its body yet (`QCC_PR_ParseStruct` returns the type of a tag, FTEF-6.8).
        let struct_base = match &resolved {
            Type::Named(n) => {
                n.starts_with("struct ")
                    || n.starts_with("union ")
                    || self.struct_type_name(n).is_some()
            }
            Type::Unknown
            | Type::Simple(_)
            | Type::Field(_)
            | Type::Pointer(_)
            | Type::Function(_) => false,
        };
        match known {
            Some(old) if !old.fteqcc_same(&resolved) => {
                self.report(
                    Code::E0258,
                    location,
                    format!(
                        "Accessor {} basic type mismatch ({}, expected {})",
                        decl.name,
                        resolved.fteqcc_name(),
                        old.fteqcc_name()
                    ),
                );
            }
            Some(_) => {}
            None if struct_base => {
                self.report(
                    Code::E0258,
                    location,
                    format!(
                        "Accessor {} cannot be based upon {}",
                        decl.name,
                        resolved.fteqcc_name()
                    ),
                );
            }
            None => {
                self.accessor_bases.insert(key, resolved);
            }
        }
    }

    /// Records an FTEQCC class and declares its members (FTEF-17.1, FTEF-17.2). A member is a
    /// definition of the class, not a global by its name.
    pub(in crate::resolve) fn declare_class(
        &mut self,
        file: FileId,
        def: DefId,
        class: &'a RawClass,
    ) -> Vec<Pending<'a>> {
        let name = self
            .index
            .def(def)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        // FTEF-9.12: a class given a body whose name is another kind of type with a size is
        // "Parent class changed on redeclaration".
        if self.named(&self.class_names, &name).is_none() && self.sized_type_named(&name) {
            if class.body
                && let Some(location) = self.index.def(def).map(|d| d.location)
            {
                self.report(
                    Code::E0242,
                    location,
                    format!("Parent class changed on redeclaration of {name}"),
                );
            }
            // `class float x;` is a `float` named `x` (FTEF-9.8): the name stays that type.
            return Vec::new();
        }
        let reported = self.index.diagnostics.len();
        self.check_parent(def, &name, class);
        let parent_reported = self.index.diagnostics.len() != reported;
        // The class is the one its name finds, in any case under `-Fcaseinsens` (FTEF-19.33).
        let class_def = self.named(&self.class_names, &name).copied().unwrap_or(def);
        self.class_names.entry(name.clone()).or_insert(def);
        self.declare_spawnfunc(def, &name);
        let info = self.classes.entry(def).or_insert_with(|| ClassInfo {
            parent: None,
            members: HashMap::new(),
            shape: None,
        });
        if class.parent.is_some() {
            info.parent.clone_from(&class.parent);
        }
        let mut pending = Vec::new();
        // FTEF-9.13: a class given its body again is compared with the first, member by
        // member (`typecmp(newt->params[i].type, parms[i].type) ||
        // strcmp(newt->params[i].paramname, parms[i].paramname)`), and in number.
        // A `nonvirtual` or `static` member is a global (`continue` before `parms[numparms]`),
        // not among the members compared.
        let shape: Vec<(String, Type, Option<String>)> = class
            .members
            .iter()
            .filter(|m| m.kind != MemberKind::Method && !m.is_global)
            .map(|m| (m.name.clone(), self.resolved(&m.ty), m.array.clone()))
            .collect();
        if class.body
            && !parent_reported
            && let Some(old) = self.classes.get(&class_def).and_then(|c| c.shape.clone())
            && let Some(location) = self.index.def(def).map(|d| d.location)
        {
            let differing = old
                .iter()
                .zip(&shape)
                .find(|(a, b)| a.0 != b.0 || !a.1.fteqcc_same(&b.1) || a.2 != b.2)
                .map(|(_, b)| b.0.clone());
            match differing {
                Some(member) => {
                    let message =
                        format!("Incompatible redeclaration of class {name}. {member} differs.");
                    self.report(Code::E0260, location, message);
                }
                None if old.len() != shape.len() => {
                    let message = format!("Incompatible redeclaration of class {name}.");
                    self.report(Code::E0260, location, message);
                }
                None => {}
            }
        }
        if class.body
            && let Some(info) = self.classes.get_mut(&class_def)
        {
            info.shape.get_or_insert(shape);
        }
        for member in &class.members {
            let location = self.at(file, member.span);
            // FTEF-9.9: `basetype = QCC_PR_ParseType(false, false, false)`: a member's type
            // names a type; FTEF-9.11: a struct or union named by its name is one too.
            if self.check_type_names(&member.ty, &[], member.span.start, location) {
                continue;
            }
            if self.is_struct_type(&self.resolved(&member.ty)) {
                let message = format!("Struct or union in class {name}");
                self.report(Code::E0254, location, message);
                continue;
            }
            if member.is_var {
                self.var_members.insert((def, member.name.clone()));
            }
            // FTEF-7.5: a member of a typedef'd function type is a member function.
            let typedef_function =
                member.kind == MemberKind::Field && self.resolved(&member.ty).is_function();
            let member_kind = if typedef_function {
                if member.is_virtual {
                    MemberKind::Virtual
                } else {
                    MemberKind::Method
                }
            } else {
                member.kind
            };
            if member.bad_init && !typedef_function {
                self.report(
                    Code::E0257,
                    location,
                    "Can only initialise member functions".to_owned(),
                );
            }
            let kind = match member_kind {
                MemberKind::Field | MemberKind::Virtual => DefKind::Field,
                MemberKind::Method => DefKind::Function,
            };
            let existing = self
                .classes
                .get(&def)
                .and_then(|c| c.members.get(&member.name))
                .copied();
            let id = if let Some(existing) = existing {
                if member.initialized
                    && let Some(d) = self.index.defs.get_mut(existing.0)
                {
                    d.initialized = true;
                }
                existing
            } else {
                let id = self.push_def(Definition {
                    name: member.name.clone(),
                    ty: member.ty.clone(),
                    kind,
                    location,
                    owner: None,
                    initialized: member.initialized,
                    member_of: Some(def),
                    generated: false,
                });
                if let Some(info) = self.classes.get_mut(&def) {
                    info.members.insert(member.name.clone(), id);
                }
                // FTEF-31.2: a member array is an array of fields, indexed by its length.
                if let Some(size) = &member.array
                    && let Some(facts) = self.facts.get_mut(id.0)
                {
                    facts.array = Some(
                        size.trim()
                            .parse()
                            .map_or(ArrayDim::Unknown, ArrayDim::Sized),
                    );
                }
                self.any_member.entry(member.name.clone()).or_insert(id);
                id
            };
            if let Some(body) = &member.body {
                pending.push((file, id, Some(def), body, None));
            }
        }
        pending
    }

    /// FTEF-9.7: the parent of a class is a type `QCC_TypeForName` finds, a class or an entity,
    /// and the same one each time the class is given its members.
    pub(in crate::resolve) fn check_parent(&mut self, def: DefId, name: &str, class: &RawClass) {
        let Some(parent) = &class.parent else {
            return;
        };
        let Some(location) = self.index.def(def).map(|d| d.location) else {
            return;
        };
        let found = self.named(&self.class_names, parent).copied();
        let entity = parent == "__entity" || (parent == "entity" && !self.profile.qcfuncs_off());
        if found.is_none() && !entity {
            let parent_type = Type::Named(parent.as_str().into());
            if self.unknown_type_name(&parent_type, &[], 0).is_some()
                || (name == parent && !self.class_names.contains_key(name))
            {
                self.report(
                    Code::E0240,
                    location,
                    format!("Parent class {parent} was not yet defined"),
                );
                return;
            }
            let kind = self.resolved(&parent_type).fteqcc_kind();
            if !matches!(kind, Kind::Entity | Kind::Opaque)
                || self.has_name(&self.structs, parent)
                || self.has_name(&self.enum_types, parent)
            {
                self.report(
                    Code::E0241,
                    location,
                    format!("Parent type {parent} is not a class/entity"),
                );
                return;
            }
        }
        // The class is the one its name finds, in any case under `-Fcaseinsens`.
        let class_def = self.named(&self.class_names, name).copied().unwrap_or(def);
        let redeclared = self
            .classes
            .get(&class_def)
            .is_some_and(|info| !info.members.is_empty());
        let before = self
            .classes
            .get(&class_def)
            .and_then(|info| info.parent.as_ref())
            .and_then(|p| self.named(&self.class_names, p).copied());
        if redeclared && before != found {
            self.report(
                Code::E0242,
                location,
                format!("Parent class changed on redeclaration of {name}"),
            );
        }
    }

    /// FTEF-9.5: every class defines `spawnfunc_Name`, so maps can spawn it; FTEQCC writes its
    /// body when the program writes none, so it is never undefined.
    pub(in crate::resolve) fn declare_spawnfunc(&mut self, class: DefId, name: &str) {
        let spawnfunc = format!("spawnfunc_{name}");
        if self.lookup_global(&spawnfunc).is_some() {
            return;
        }
        let Some(location) = self.index.def(class).map(|d| d.location) else {
            return;
        };
        let ty = Type::Function(Rc::new(FunctionType {
            ret: Type::Simple(SimpleType::Void),
            params: Vec::new(),
            variadic: false,
        }));
        let id = self.push_def(Definition {
            name: spawnfunc,
            ty,
            kind: DefKind::Function,
            location,
            owner: None,
            initialized: false,
            member_of: None,
            generated: true,
        });
        if let Some(facts) = self.fact_mut(id) {
            facts.constant = true;
        }
    }

    /// An FTEQCC `Class::member` definition (FTEF-17.2): it defines the member the class or
    /// a parent declares, or a new member of the class; its name is a reference to the
    /// member's declaration, and its body is resolved in the class. A scope that is no class
    /// or struct is "is not a class" (FTEF-19.15); a member a struct does not have is an
    /// error, and one a class does not have a warning.
    pub(in crate::resolve) fn define_member(
        &mut self,
        file: FileId,
        scope: &str,
        decl: &'a RawDecl,
        location: Location,
    ) -> Vec<Pending<'a>> {
        let Some(class) = self.named(&self.class_names, scope).copied() else {
            let name = format!("{scope}::{}", decl.name);
            if self.has_name(&self.structs, scope) {
                if self.lookup_global(&name).is_none() {
                    self.report(
                        Code::E0226,
                        location,
                        format!("{name} is not part of class {scope}"),
                    );
                }
            } else {
                self.report(Code::E0225, location, format!("{scope} is not a class"));
            }
            // A global named `Scope::name`, as FTEQCC names it.
            let kind = top_level_kind(&decl.ty);
            let init = decl.initialized.then_some(decl.literal.as_ref());
            let def = self.declare_global(&name, &decl.ty, kind, location, init);
            return decl
                .body
                .as_ref()
                .map(|body| vec![(file, def, None, body, Some(decl))])
                .unwrap_or_default();
        };
        let member = match self.member_in(class, &decl.name) {
            Some(existing) => {
                // FTEF-19.4: a body for a member already initialised, as for any definition.
                if decl.body.is_some()
                    && !decl.redefinable()
                    && !self.redefinable.contains(&existing)
                    && self.index.def(existing).is_some_and(|d| d.initialized)
                {
                    self.report(
                        Code::E0216,
                        location,
                        "redeclaration of function body".to_owned(),
                    );
                }
                if decl.initialized
                    && let Some(d) = self.index.defs.get_mut(existing.0)
                {
                    d.initialized = true;
                }
                self.index.refs.push(Reference {
                    location,
                    def: existing,
                });
                existing
            }
            None => {
                self.report(
                    Code::W0216,
                    location,
                    format!("{scope}::{} is not part of class {scope}", decl.name),
                );
                let kind = if decl.ty.is_function() {
                    DefKind::Function
                } else {
                    DefKind::Field
                };
                let id = self.push_def(Definition {
                    name: decl.name.clone(),
                    ty: decl.ty.clone(),
                    kind,
                    location,
                    owner: None,
                    initialized: decl.initialized,
                    member_of: Some(class),
                    generated: false,
                });
                if let Some(info) = self.classes.get_mut(&class) {
                    info.members.insert(decl.name.clone(), id);
                }
                self.any_member.entry(decl.name.clone()).or_insert(id);
                id
            }
        };
        decl.body
            .as_ref()
            .map(|body| vec![(file, member, Some(class), body, Some(decl))])
            .unwrap_or_default()
    }

    /// The member `name` of class `class` or its parents (FTEF-17.2).
    pub(in crate::resolve) fn member_in(&self, class: DefId, name: &str) -> Option<DefId> {
        let mut current = Some(class);
        for _ in 0..MAX_CLASS_DEPTH {
            let info = self.classes.get(&current?)?;
            if let Some(id) = self.named(&info.members, name) {
                return Some(*id);
            }
            current = info
                .parent
                .as_ref()
                .and_then(|p| self.named(&self.class_names, p).copied());
        }
        None
    }

    /// The parent of class `class`.
    pub(in crate::resolve) fn parent_of(&self, class: DefId) -> Option<DefId> {
        self.classes
            .get(&class)?
            .parent
            .as_ref()
            .and_then(|p| self.named(&self.class_names, p).copied())
    }

    /// The class a type names, if it names one (FTEF-17.3).
    pub(in crate::resolve) fn class_of(&self, ty: &Type) -> Option<DefId> {
        self.named(&self.class_names, ty.type_name()?).copied()
    }

    /// The selector of a member access (FTE-9, FTE-13, FTE-14, FTEF-17.3).
    ///
    /// A member of the class of what the access is made on, when that is known; else an
    /// entity field the name defines; else a member of any class with the name. A vector
    /// member, `.length`, a struct or accessor field, or a name that is a value but no field
    /// names no definition; a name that is none of these is unknown.
    pub(in crate::resolve) fn resolve_member(&self, scope: Option<Scope>, use_: &RawUse) -> Found {
        let name = use_.name.as_str();
        // FTE-13: an array takes `.length` and no other member (`QCC_PR_Expect("length")`
        // after an array's `.`: "expected length, found NAME").
        if self.profile.name_use_checks()
            && name != "length"
            && let MemberBase::Name(base) = &use_.base
            && let Some(def) = match scope {
                Some(scope) => self.lookup_in_scope(scope, base),
                None => self.lookup_used_global(base),
            }
            && self.facts.get(def.0).is_some_and(|f| f.array.is_some())
        {
            return Found::Error(Code::E0204, format!("expected length, found {name}"));
        }
        match self.struct_matches(scope, &use_.base, name) {
            StructMatch::Several(owner) => {
                return Found::Error(
                    Code::E0252,
                    format!("multiple members found matching {owner}.{name}"),
                );
            }
            // FTEF-6.11: a member no struct of the chain has, and no anonymous member may
            // hold, is "NAME is not a member of STRUCT".
            StructMatch::None(owner) => {
                return Found::Error(Code::E0261, format!("{name} is not a member of {owner}"));
            }
            StructMatch::One | StructMatch::NotStruct => {}
        }
        if let Some(class) = self.base_class(scope, &use_.base)
            && let Some(def) = self.member_in(class, name)
        {
            return Found::Def(def);
        }
        let def = match scope {
            Some(scope) => self.lookup_in_scope(scope, name),
            None => self.lookup_used_global(name),
        };
        let field = def.filter(|d| self.index.def(*d).is_some_and(|d| d.ty.is_field()));
        if let Some(field) = field {
            return Found::Def(field);
        }
        if let Some(member) = self.named(&self.any_member, name) {
            return Found::Def(*member);
        }
        if def.is_some() || self.is_member_name(name) {
            return Found::Nothing;
        }
        Found::Unknown
    }

    /// The class of what a member access is made on (FTEF-17.3): `this`, a name's type, a
    /// cast's type or a call's return type, when it names a class.
    pub(in crate::resolve) fn base_class(
        &self,
        scope: Option<Scope>,
        base: &MemberBase,
    ) -> Option<DefId> {
        if !self.profile.type_declarations() {
            return None;
        }
        match base {
            MemberBase::Name(name) if name == "this" => scope.and_then(|s| s.class),
            MemberBase::Name(name) => {
                let def = match scope {
                    Some(scope) => self.lookup_in_scope(scope, name),
                    None => self.lookup_used_global(name),
                }?;
                self.class_of(&self.index.def(def)?.ty)
            }
            MemberBase::Type(ty) => self.class_of(ty),
            MemberBase::Call(name) => {
                let def = match scope {
                    Some(scope) => self.lookup_in_scope(scope, name),
                    None => self.lookup_used_global(name),
                }?;
                match &self.index.def(def)?.ty {
                    Type::Function(func) => self.class_of(&func.ret),
                    _ => None,
                }
            }
            MemberBase::Unknown => None,
        }
    }
}

impl Resolver<'_> {
    /// The member `name` of the class `ty` names, in it or its parents (FTEF-24.6).
    pub(in crate::resolve) fn class_member(&self, ty: &Type, name: &str) -> Option<DefId> {
        let mut class = self.class_of(ty);
        for _ in 0..MAX_CLASS_DEPTH {
            let current = class?;
            if let Some(member) = self.member_in(current, name) {
                return Some(member);
            }
            class = self.parent_of(current);
        }
        None
    }
}
