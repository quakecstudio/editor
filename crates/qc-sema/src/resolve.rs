// SPDX-License-Identifier: GPL-3.0-or-later

//! Module resolution: one symbol table per module (QC-12).
//!
//! `qcc` compiles a module's files in the order of its `progs.src` as one program, each file
//! seeing every definition of the files before it (QC-23.3). This module joins the per-file
//! [`crate::lower::FileDecls`] in that order into one definition list (QC-12.2), resolves
//! every name use against it (QC-12.5), and records the diagnostics the semantic model owns.
//!
//! Implements QC-11 (declarations and redeclaration, the vector components of QC-11.7), QC-12
//! (scopes and lookup) and the state-header name rule of QC-13.6; reports QCD-3.5, QCD-3.6,
//! QCD-3.11 and QCD-7.3. Under an FTEQCC profile it follows FTE-22 instead where FTEQCC
//! differs: locals shadow globals (FTE-22.1), a constant may be initialised twice with the
//! same literal (FTE-22.2), and with autoprototyping every top-level name is visible to every
//! function body (FTE-22.3). The full dialect's classes resolve by FTEF-17: a class's members
//! are found through the class and its parents, inside a member function's body before the
//! globals, and after a `.` through the type of what the access is made on.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use qc_diag::{Code, Diagnostic, Span};
use qc_syntax::Dialect;

use crate::expand::OriginMap;
use crate::lower::{
    FileDecls, LiteralValue, MemberBase, MemberKind, RawBody, RawClass, RawDecl, RawUse, UseKind,
    top_level_kind,
};
use crate::symbol::{DefId, DefKind, Definition, FileId, Location, Reference};
use crate::ty::{FunctionType, SimpleType, Type};

/// A diagnostic and the file it is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiagnostic {
    /// The file the diagnostic's span is in.
    pub file: FileId,
    /// The diagnostic.
    pub diagnostic: Diagnostic,
}

/// Builds a [`FileDiagnostic`] at `location`.
fn diag(code: Code, location: Location, message: String) -> FileDiagnostic {
    FileDiagnostic {
        file: location.file,
        diagnostic: Diagnostic {
            code,
            message,
            span: location.span,
        },
    }
}

/// A module's resolved semantic index: its definitions, references and diagnostics.
///
/// Built by [`resolve_module`] from the module's files in compilation order. Go-to-definition
/// and find-references read it (`crate::nav`).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ModuleIndex {
    /// Every definition, in declaration order (QC-12.2). A [`DefId`] indexes this.
    pub defs: Vec<Definition>,
    /// Every resolved name use, in source order (QC-12.5).
    pub refs: Vec<Reference>,
    /// The semantic diagnostics, in the order found, each with its file.
    pub diagnostics: Vec<FileDiagnostic>,
}

impl ModuleIndex {
    /// The definition a [`DefId`] names.
    #[must_use]
    pub fn def(&self, id: DefId) -> Option<&Definition> {
        self.defs.get(id.0)
    }
}

/// How a module is resolved: the dialect profile and FTEQCC's autoprototyping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResolveOptions {
    /// The dialect profile (PRJ-2.8). An FTEQCC dialect resolves by FTE-22.
    pub dialect: Dialect,
    /// FTEQCC's `-Fautoproto` (FTE-22.3): every top-level declaration of the module is
    /// visible to every function body. Only meaningful with an FTEQCC dialect.
    pub autoproto: bool,
}

/// Resolves a module from its files' lowered declarations, in compilation order (QC-23.3).
///
/// `files` is the module's file graph order; each entry is that file's [`FileDecls`]. The
/// returned [`ModuleIndex`] holds the definition list, the resolved references and the
/// diagnostics.
#[must_use]
pub fn resolve_module(files: &[FileDecls], options: ResolveOptions) -> ModuleIndex {
    resolve_mapped(files, &[], options)
}

/// A function body waiting to be resolved: its file, its function and its class.
type Pending<'a> = (FileId, DefId, Option<DefId>, &'a RawBody);

/// [`resolve_module`] for files the parser read after preprocessing: `maps` holds, for each
/// entry of `files`, where each span of its rendered text came from (QC-9.7), so every
/// location in the index is a span of a source file. With no maps a span is in the file of
/// its entry.
pub(crate) fn resolve_mapped(
    files: &[FileDecls],
    maps: &[&OriginMap],
    options: ResolveOptions,
) -> ModuleIndex {
    let fteqcc = options.dialect.has_fteqcc_extensions();
    let mut r = Resolver {
        index: ModuleIndex::default(),
        maps,
        fteqcc,
        globals: HashMap::new(),
        literals: HashMap::new(),
        members: files
            .iter()
            .flat_map(|f| f.members.iter().cloned())
            .collect(),
        classes: HashMap::new(),
        class_names: HashMap::new(),
        any_member: HashMap::new(),
    };
    // FTE-22.3: with autoprototyping FTEQCC's first pass reads only the definitions, so the
    // second, which compiles the bodies, finds every top-level name already declared.
    let defer = fteqcc && options.autoproto;
    let mut bodies: Vec<Pending<'_>> = Vec::new();
    for (index, file) in files.iter().enumerate() {
        let file_id = FileId(index);
        for decl in &file.decls {
            let pending = r.declare_top_level(file_id, decl);
            r.resolve_uses(file_id, None, &decl.uses);
            if defer {
                bodies.extend(pending);
            } else {
                for (file, def, class, body) in pending {
                    r.resolve_body(file, def, class, body);
                }
            }
        }
    }
    for (file, def, class, body) in bodies {
        r.resolve_body(file, def, class, body);
    }
    r.finish()
}

/// What the resolver knows of an FTEQCC class (FTEF-9).
#[derive(Debug)]
struct ClassInfo {
    /// The parent class's name, if any.
    parent: Option<String>,
    /// The class's own members by name.
    members: HashMap<String, DefId>,
}

struct Resolver<'a> {
    index: ModuleIndex,
    /// Where the spans of each file's rendered text came from; empty when the files were
    /// parsed as written.
    maps: &'a [&'a OriginMap],
    /// Whether an FTEQCC profile is in force (FTE-22).
    fteqcc: bool,
    /// The first global definition of each name (QC-12.2), so a global lookup is direct.
    globals: HashMap<String, DefId>,
    /// The literal each initialised global was given, for FTE-22.2.
    literals: HashMap<DefId, LiteralValue>,
    /// The field names of every struct and accessor the module defines (FTE-14.1, FTEF-6,
    /// FTEF-10).
    members: HashSet<String>,
    /// Every class the module defines, by its type-name definition (FTEF-9).
    classes: HashMap<DefId, ClassInfo>,
    /// Each class's type-name definition, by its name.
    class_names: HashMap<String, DefId>,
    /// The first member of any class with each name, for an access whose base's class is not
    /// known (FTEF-17.3).
    any_member: HashMap<String, DefId>,
}

/// The `_x`, `_y` and `_z` suffixes of a vector's component names (QC-11.7).
const COMPONENTS: [&str; 3] = ["_x", "_y", "_z"];

/// The deepest chain of parent classes followed; a deeper or circular one stops there.
const MAX_CLASS_DEPTH: usize = 64;

/// The type of each component of a definition of type `ty`: `float` for a `vector`, `.float`
/// for a `.vector` field (QC-11.7); `None` for any other type.
fn component_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::Simple(SimpleType::Vector) => Some(Type::Simple(SimpleType::Float)),
        Type::Field(inner) if **inner == Type::Simple(SimpleType::Vector) => {
            Some(Type::Field(Rc::new(Type::Simple(SimpleType::Float))))
        }
        Type::Unknown
        | Type::Simple(_)
        | Type::Field(_)
        | Type::Function(_)
        | Type::Named(_)
        | Type::Pointer(_) => None,
    }
}

/// Words FTEQCC reads as part of an expression rather than as names a program defines:
/// `this` and `super` in a member function, and the `sizeof` and `_length` intrinsics
/// (FTEF-17.4).
fn is_intrinsic(name: &str) -> bool {
    matches!(name, "this" | "super" | "sizeof" | "_length")
}

/// The initialiser of a declaration, as redeclaration sees it (QC-11.5, FTE-22.2): `None`
/// when there is none, and otherwise its value when it is a single literal.
type Init<'a> = Option<Option<&'a LiteralValue>>;

impl<'a> Resolver<'a> {
    /// The source location of the span `span` of file `file`'s parsed text: through the
    /// file's origin map when it was preprocessed (QC-9.7), else the span itself.
    fn at(&self, file: FileId, span: Span) -> Location {
        self.maps
            .get(file.0)
            .and_then(|map| map.locate(span))
            .unwrap_or(Location { file, span })
    }

    /// Looks up a global name: the first global definition with that name (QC-12.2).
    fn lookup_global(&self, name: &str) -> Option<DefId> {
        self.globals.get(name).copied()
    }

    /// Adds a definition and returns its id. A class member is not a global by its name.
    fn push_def(&mut self, def: Definition) -> DefId {
        let id = DefId(self.index.defs.len());
        if def.kind.is_global() && def.member_of.is_none() {
            self.globals.entry(def.name.clone()).or_insert(id);
        }
        self.index.defs.push(def);
        id
    }

    /// Whether a redeclaration's type `ty` matches the earlier `old` (QC-11.5): equal, or
    /// either unknown; under FTEQCC also when the model does not follow either (FTEF-17.6).
    fn same_type(&self, old: &Type, ty: &Type) -> bool {
        if self.fteqcc {
            old.compatible(ty)
        } else {
            old == ty || *old == Type::Unknown || *ty == Type::Unknown
        }
    }

    /// Declares, or redeclares, a global-scope name (QC-11.5), and the components of a
    /// vector or vector field it declares anew (QC-11.7).
    ///
    /// A name already in scope with the same type refers to the same definition; declaring
    /// it with a different type is QCD-3.5; initialising it a second time is QCD-3.6
    /// (QC-11.5), unless FTE-22.2 accepts it. Returns the [`DefId`] the name resolves to.
    fn declare_global(
        &mut self,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
        init: Init<'_>,
    ) -> DefId {
        if let Some(existing) = self.lookup_global(name) {
            self.redeclare(existing, name, ty, location, init);
            return existing;
        }
        let id = self.push_def(Definition {
            name: name.to_owned(),
            ty: ty.clone(),
            kind,
            location,
            owner: None,
            initialized: init.is_some(),
            member_of: None,
        });
        if let Some(Some(literal)) = init {
            self.literals.insert(id, literal.clone());
        }
        // QC-11.7: `PR_GetDef` makes `name_x`, `name_y` and `name_z` for a new vector, and
        // `.float` fields of the same names for a new `.vector` field.
        if let Some(component) = component_type(ty) {
            let component_kind = top_level_kind(&component);
            let component_init = init.map(|_| None);
            for suffix in COMPONENTS {
                let component_name = format!("{name}{suffix}");
                self.declare_global(
                    &component_name,
                    &component,
                    component_kind,
                    location,
                    component_init,
                );
            }
        }
        id
    }

    /// Handles a redeclaration of an existing definition (QC-11.5).
    fn redeclare(
        &mut self,
        existing: DefId,
        name: &str,
        ty: &Type,
        location: Location,
        init: Init<'_>,
    ) {
        let Some(def) = self.index.defs.get(existing.0) else {
            return;
        };
        // QCD-3.5: a different type is a type mismatch on redeclaration.
        if !self.same_type(&def.ty, ty) {
            self.index.diagnostics.push(diag(
                Code::E0205,
                location,
                format!("Type mismatch on redeclaration of {name}"),
            ));
            return;
        }
        let Some(literal) = init else {
            return;
        };
        if def.initialized {
            // FTE-22.2: FTEQCC compares a second initialiser with the first and accepts it
            // when the two are the same value.
            let same = self.fteqcc && literal.is_some() && literal == self.literals.get(&existing);
            if !same {
                // QCD-3.6: initialising a name that is already initialised.
                self.index.diagnostics.push(diag(
                    Code::E0206,
                    location,
                    format!("{name} redeclared"),
                ));
            }
            return;
        }
        if let Some(def) = self.index.defs.get_mut(existing.0) {
            def.initialized = true;
        }
        if let Some(literal) = literal {
            self.literals.insert(existing, literal.clone());
        }
    }

    /// Declares one top-level declaration and its state header's next-think name (QC-13.6),
    /// and returns the function bodies it brings, to resolve now or, with autoprototyping,
    /// after every declaration (FTE-22.3).
    fn declare_top_level(&mut self, file: FileId, decl: &'a RawDecl) -> Vec<Pending<'a>> {
        let location = self.at(file, decl.span);
        if decl.type_name {
            // FTEF-17.1: a type's name may be declared again, a class first forward and then
            // with its body, and a typedef in more than one header.
            let def =
                self.declare_global(&decl.name, &Type::Unknown, DefKind::Global, location, None);
            if let Some(class) = &decl.class {
                return self.declare_class(file, def, class);
            }
            return Vec::new();
        }
        if let Some(scope) = &decl.scope
            && self.fteqcc
        {
            return self.define_member(file, scope, decl, location);
        }
        let kind = top_level_kind(&decl.ty);
        let init = decl.initialized.then_some(decl.literal.as_ref());
        let def = self.declare_global(&decl.name, &decl.ty, kind, location, init);
        if let Some((next_name, next_span)) = &decl.next_think {
            self.declare_next_think(file, next_name, *next_span);
        }
        decl.body
            .as_ref()
            .map(|body| vec![(file, def, None, body)])
            .unwrap_or_default()
    }

    /// Records an FTEQCC class and declares its members (FTEF-17.1, FTEF-17.2). A member is a
    /// definition of the class, not a global by its name.
    fn declare_class(&mut self, file: FileId, def: DefId, class: &'a RawClass) -> Vec<Pending<'a>> {
        let name = self
            .index
            .def(def)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        self.class_names.entry(name).or_insert(def);
        let info = self.classes.entry(def).or_insert_with(|| ClassInfo {
            parent: None,
            members: HashMap::new(),
        });
        if class.parent.is_some() {
            info.parent.clone_from(&class.parent);
        }
        let mut pending = Vec::new();
        for member in &class.members {
            let location = self.at(file, member.span);
            let kind = match member.kind {
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
                });
                if let Some(info) = self.classes.get_mut(&def) {
                    info.members.insert(member.name.clone(), id);
                }
                self.any_member.entry(member.name.clone()).or_insert(id);
                id
            };
            if let Some(body) = &member.body {
                pending.push((file, id, Some(def), body));
            }
        }
        pending
    }

    /// An FTEQCC `Class::member` definition (FTEF-17.2): it defines the member the class or
    /// a parent declares, or a new member of the class; its name is a reference to the
    /// member's declaration, and its body is resolved in the class.
    fn define_member(
        &mut self,
        file: FileId,
        scope: &str,
        decl: &'a RawDecl,
        location: Location,
    ) -> Vec<Pending<'a>> {
        let Some(class) = self.class_names.get(scope).copied() else {
            // Not a class: a global named `Scope::name`, as FTEQCC names it.
            let name = format!("{scope}::{}", decl.name);
            let kind = top_level_kind(&decl.ty);
            let init = decl.initialized.then_some(decl.literal.as_ref());
            let def = self.declare_global(&name, &decl.ty, kind, location, init);
            return decl
                .body
                .as_ref()
                .map(|body| vec![(file, def, None, body)])
                .unwrap_or_default();
        };
        let member = match self.member_in(class, &decl.name) {
            Some(existing) => {
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
            .map(|body| vec![(file, member, Some(class), body)])
            .unwrap_or_default()
    }

    /// The member `name` of class `class` or its parents (FTEF-17.2).
    fn member_in(&self, class: DefId, name: &str) -> Option<DefId> {
        let mut current = Some(class);
        for _ in 0..MAX_CLASS_DEPTH {
            let info = self.classes.get(&current?)?;
            if let Some(id) = info.members.get(name) {
                return Some(*id);
            }
            current = info
                .parent
                .as_ref()
                .and_then(|p| self.class_names.get(p).copied());
        }
        None
    }

    /// The parent of class `class`.
    fn parent_of(&self, class: DefId) -> Option<DefId> {
        self.classes
            .get(&class)?
            .parent
            .as_ref()
            .and_then(|p| self.class_names.get(p).copied())
    }

    /// The class a type names, if it names one (FTEF-17.3).
    fn class_of(&self, ty: &Type) -> Option<DefId> {
        self.class_names.get(ty.type_name()?).copied()
    }

    /// Declares, or checks, the next-think name of a state header (QC-13.6).
    ///
    /// If the name does not exist it is declared as a global `void()`. If it exists with a
    /// different type it is a type mismatch (QCD-3.5).
    fn declare_next_think(&mut self, file: FileId, name: &str, span: Span) {
        let void_fn = Type::Function(Rc::new(FunctionType {
            ret: Type::Simple(SimpleType::Void),
            params: Vec::new(),
            variadic: false,
        }));
        let location = self.at(file, span);
        let def = self.declare_global(name, &void_fn, DefKind::Function, location, None);
        // The name in the header is a reference to the think function.
        self.index.refs.push(Reference { location, def });
    }

    /// Resolves one function body: its parameters, locals and name uses (QC-12.1, QC-12.5),
    /// in the class `class` when it is a member function's (FTEF-17.2).
    fn resolve_body(&mut self, file: FileId, owner: DefId, class: Option<DefId>, body: &RawBody) {
        // Record where this function's locals start, so lookups in it see only its own.
        let scope = Scope {
            owner,
            locals_start: self.index.defs.len(),
            class,
        };
        for param in &body.params {
            let location = self.at(file, param.span);
            self.declare_scoped(scope, &param.name, &param.ty, DefKind::Param, location);
        }
        // QC-11.10, QC-11.9: locals; a local field or function is an error (QCD-3.8 is the
        // parser's; the semantic check is QC-11.9).
        for local in &body.locals {
            let location = self.at(file, local.span);
            if local.bad_field_or_function {
                self.index.diagnostics.push(diag(
                    Code::E0208,
                    location,
                    "Fields and functions must be global".to_owned(),
                ));
                continue;
            }
            self.declare_scoped(scope, &local.name, &local.ty, DefKind::Local, location);
        }
        // QC-12.5: resolve each name use against globals and this function's own locals.
        self.resolve_uses(file, Some(scope), &body.uses);
    }

    /// Resolves name uses in a function's scope, or with no scope in the global scope (an
    /// initialiser or a default value, FTE-10.2, FTE-18.2), as references; a value that
    /// resolves to nothing is "Unknown value" (QCD-4.1).
    fn resolve_uses(&mut self, file: FileId, scope: Option<Scope>, uses: &[RawUse]) {
        for use_ in uses {
            let location = self.at(file, use_.span);
            let found = match (&use_.scope, use_.kind) {
                (Some(prefix), _) if self.fteqcc => self.resolve_scoped(scope, prefix, &use_.name),
                (_, UseKind::Value) => self.resolve_value(scope, &use_.name),
                (_, UseKind::Member) => self.resolve_member(scope, use_),
            };
            match found {
                Found::Def(def) => self.index.refs.push(Reference { location, def }),
                Found::Nothing => {}
                Found::Unknown => {
                    let shown = match &use_.scope {
                        Some(prefix) => format!("{prefix}::{}", use_.name),
                        None => use_.name.clone(),
                    };
                    self.index.diagnostics.push(diag(
                        Code::E0301,
                        location,
                        format!("Unknown value \"{shown}\""),
                    ));
                }
            }
        }
    }

    /// A plain name used as a value: the function's own, then in a member function the
    /// class's members (FTEF-17.2), then a global (QC-12.2, FTE-22.1).
    fn resolve_value(&self, scope: Option<Scope>, name: &str) -> Found {
        if self.fteqcc && is_intrinsic(name) {
            return Found::Nothing;
        }
        let def = match scope {
            Some(scope) => self.lookup_in_scope(scope, name),
            None => self.lookup_global(name),
        };
        def.map_or(Found::Unknown, Found::Def)
    }

    /// `Class::name` or `super::name` (FTEF-17.4): the member of the class, or of the
    /// function's class's parent; a scope that is no class (an enum's, for one) names a
    /// global.
    fn resolve_scoped(&self, scope: Option<Scope>, prefix: &str, name: &str) -> Found {
        let class = if prefix == "super" {
            scope.and_then(|s| s.class).and_then(|c| self.parent_of(c))
        } else {
            self.class_names.get(prefix).copied()
        };
        if let Some(class) = class
            && let Some(def) = self.member_in(class, name)
        {
            return Found::Def(def);
        }
        if let Some(def) = self.lookup_global(name) {
            return Found::Def(def);
        }
        if let Some(def) = self.lookup_global(&format!("{prefix}::{name}")) {
            return Found::Def(def);
        }
        if prefix == "super" && class.is_none() {
            // `super::` of a class whose parent is the entity, or that the model did not
            // find: the parent is not known, so the name is not checked.
            return Found::Nothing;
        }
        Found::Unknown
    }

    /// The selector of a member access (FTE-9, FTE-13, FTE-14, FTEF-17.3).
    ///
    /// A member of the class of what the access is made on, when that is known; else an
    /// entity field the name defines; else a member of any class with the name. A vector
    /// member, `.length`, a struct or accessor field, or a name that is a value but no field
    /// names no definition; a name that is none of these is unknown.
    fn resolve_member(&self, scope: Option<Scope>, use_: &RawUse) -> Found {
        let name = use_.name.as_str();
        if let Some(class) = self.base_class(scope, &use_.base)
            && let Some(def) = self.member_in(class, name)
        {
            return Found::Def(def);
        }
        let def = match scope {
            Some(scope) => self.lookup_in_scope(scope, name),
            None => self.lookup_global(name),
        };
        let field = def.filter(|d| self.index.def(*d).is_some_and(|d| d.ty.is_field()));
        if let Some(field) = field {
            return Found::Def(field);
        }
        if let Some(member) = self.any_member.get(name) {
            return Found::Def(*member);
        }
        if def.is_some() || self.is_member_name(name) {
            return Found::Nothing;
        }
        Found::Unknown
    }

    /// The class of what a member access is made on (FTEF-17.3): `this`, a name's type, a
    /// cast's type or a call's return type, when it names a class.
    fn base_class(&self, scope: Option<Scope>, base: &MemberBase) -> Option<DefId> {
        if !self.fteqcc {
            return None;
        }
        match base {
            MemberBase::Name(name) if name == "this" => scope.and_then(|s| s.class),
            MemberBase::Name(name) => {
                let def = match scope {
                    Some(scope) => self.lookup_in_scope(scope, name),
                    None => self.lookup_global(name),
                }?;
                self.class_of(&self.index.def(def)?.ty)
            }
            MemberBase::Type(ty) => self.class_of(ty),
            MemberBase::Call(name) => {
                let def = match scope {
                    Some(scope) => self.lookup_in_scope(scope, name),
                    None => self.lookup_global(name),
                }?;
                match &self.index.def(def)?.ty {
                    Type::Function(func) => self.class_of(&func.ret),
                    _ => None,
                }
            }
            MemberBase::Unknown => None,
        }
    }

    /// Whether `name` selects a member that is no definition: a vector's `x`, `y` or `z`
    /// (FTE-9.1), an array's `length` (FTE-13.3), or a field of a struct or accessor the
    /// module defines (FTE-14.2, FTEF-6.5, FTEF-10.2).
    fn is_member_name(&self, name: &str) -> bool {
        matches!(name, "x" | "y" | "z" | "length") || self.members.contains(name)
    }

    /// Declares a parameter or local, and the components of a new vector (QC-11.7).
    ///
    /// A name the function already declared is that definition again, or a type mismatch
    /// (QC-11.5). In vanilla a name that is a global reuses the global (QC-12.3); in an
    /// FTEQCC profile the local is new and shadows it (FTE-22.1).
    fn declare_scoped(
        &mut self,
        scope: Scope,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
    ) {
        self.declare_scoped_inner(scope, name, ty, kind, location, false);
    }

    /// [`Resolver::declare_scoped`]; a `component` (QC-11.7) that reuses a global is not
    /// warned about, since the source never names it.
    fn declare_scoped_inner(
        &mut self,
        scope: Scope,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
        component: bool,
    ) {
        let existing = self.lookup_local(scope, name).or_else(|| {
            if self.fteqcc {
                None
            } else {
                self.lookup_global(name)
            }
        });
        if let Some(existing) = existing {
            let Some(def) = self.index.defs.get(existing.0) else {
                return;
            };
            let global = def.kind.is_global();
            if !self.same_type(&def.ty, ty) {
                // QCD-3.5: `PR_GetDef` finds the earlier definition with another type.
                self.index.diagnostics.push(diag(
                    Code::E0205,
                    location,
                    format!("Type mismatch on redeclaration of {name}"),
                ));
                return;
            }
            if global && !component {
                // QCD-7.3: warn that the local name is the global.
                self.index.diagnostics.push(diag(
                    Code::W0201,
                    location,
                    format!("local {name} is the global {name}"),
                ));
            }
            // The declaration is a reference to the definition it reuses.
            self.index.refs.push(Reference {
                location,
                def: existing,
            });
            return;
        }
        self.push_def(Definition {
            name: name.to_owned(),
            ty: ty.clone(),
            kind,
            location,
            owner: Some(scope.owner),
            initialized: false,
            member_of: None,
        });
        // QC-11.7: a new vector local or parameter has its components too.
        if let Some(component) = component_type(ty) {
            for suffix in COMPONENTS {
                let component_name = format!("{name}{suffix}");
                self.declare_scoped_inner(scope, &component_name, &component, kind, location, true);
            }
        }
    }

    /// The parameter or local of the function in `scope` named `name` (QC-12.1).
    fn lookup_local(&self, scope: Scope, name: &str) -> Option<DefId> {
        self.index
            .defs
            .iter()
            .enumerate()
            .skip(scope.locals_start)
            .find(|(_, d)| d.owner == Some(scope.owner) && d.name == name)
            .map(|(index, _)| DefId(index))
    }

    /// Looks up a name in a function's scope (QC-12.2). In vanilla the first definition in
    /// the list that is global or the function's own is the global when there is one, since
    /// a local cannot shadow it (QC-12.3); in an FTEQCC profile the function's own comes
    /// first, since it shadows the global (FTE-22.1), then in a member function the class's
    /// members (FTEF-17.2). A local of another function, declared earlier, is invisible
    /// (QC-12.4).
    fn lookup_in_scope(&self, scope: Scope, name: &str) -> Option<DefId> {
        if self.fteqcc {
            self.lookup_local(scope, name)
                .or_else(|| scope.class.and_then(|c| self.member_in(c, name)))
                .or_else(|| self.lookup_global(name))
        } else {
            self.lookup_global(name)
                .or_else(|| self.lookup_local(scope, name))
        }
    }

    fn finish(mut self) -> ModuleIndex {
        self.check_undefined_functions();
        self.index
    }

    /// QC-11.8, QCD-3.11: every global function must be defined; report those that are not.
    fn check_undefined_functions(&mut self) {
        let mut undefined: Vec<FileDiagnostic> = Vec::new();
        for def in &self.index.defs {
            if def.kind == DefKind::Function && !def.initialized && def.member_of.is_none() {
                undefined.push(diag(
                    Code::E0211,
                    def.location,
                    format!("function {} was not defined", def.name),
                ));
            }
        }
        self.index.diagnostics.extend(undefined);
    }
}

/// What a name use resolves to.
enum Found {
    /// A definition: the use is a reference to it.
    Def(DefId),
    /// No definition, and no error: a member that is no definition, or an intrinsic.
    Nothing,
    /// Nothing: "Unknown value" (QCD-4.1).
    Unknown,
}

/// The function whose body is being resolved, where its locals start in the definition
/// list, and the class it is a member function of.
#[derive(Clone, Copy)]
struct Scope {
    owner: DefId,
    locals_start: usize,
    class: Option<DefId>,
}
