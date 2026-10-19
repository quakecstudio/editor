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
//! function body (FTE-22.3).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use qc_diag::{Code, Diagnostic, Span};
use qc_syntax::Dialect;

use crate::expand::OriginMap;
use crate::lower::{FileDecls, LiteralValue, RawBody, RawDecl, RawUse, UseKind, top_level_kind};
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
    };
    if fteqcc && options.autoproto {
        // FTE-22.3: FTEQCC's first pass reads only the definitions, so the second, which
        // compiles the bodies, finds every top-level name already declared.
        let mut bodies = Vec::new();
        for (index, file) in files.iter().enumerate() {
            let file_id = FileId(index);
            for decl in &file.decls {
                let def = r.declare_top_level(file_id, decl);
                r.resolve_uses(file_id, None, &decl.uses);
                if let Some(body) = &decl.body {
                    bodies.push((file_id, def, body));
                }
            }
        }
        for (file_id, def, body) in bodies {
            r.resolve_body(file_id, def, body);
        }
    } else {
        for (index, file) in files.iter().enumerate() {
            let file_id = FileId(index);
            for decl in &file.decls {
                let def = r.declare_top_level(file_id, decl);
                r.resolve_uses(file_id, None, &decl.uses);
                if let Some(body) = &decl.body {
                    r.resolve_body(file_id, def, body);
                }
            }
        }
    }
    r.finish()
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
    /// The field names of every struct the module defines (FTE-14.1).
    members: HashSet<String>,
}

/// The `_x`, `_y` and `_z` suffixes of a vector's component names (QC-11.7).
const COMPONENTS: [&str; 3] = ["_x", "_y", "_z"];

/// The type of each component of a definition of type `ty`: `float` for a `vector`, `.float`
/// for a `.vector` field (QC-11.7); `None` for any other type.
fn component_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::Simple(SimpleType::Vector) => Some(Type::Simple(SimpleType::Float)),
        Type::Field(inner) if **inner == Type::Simple(SimpleType::Vector) => {
            Some(Type::Field(Rc::new(Type::Simple(SimpleType::Float))))
        }
        Type::Unknown | Type::Simple(_) | Type::Field(_) | Type::Function(_) => None,
    }
}

/// The initialiser of a declaration, as redeclaration sees it (QC-11.5, FTE-22.2): `None`
/// when there is none, and otherwise its value when it is a single literal.
type Init<'a> = Option<Option<&'a LiteralValue>>;

impl Resolver<'_> {
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

    /// Adds a definition and returns its id.
    fn push_def(&mut self, def: Definition) -> DefId {
        let id = DefId(self.index.defs.len());
        if def.kind.is_global() {
            self.globals.entry(def.name.clone()).or_insert(id);
        }
        self.index.defs.push(def);
        id
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
        if &def.ty != ty && def.ty != Type::Unknown && *ty != Type::Unknown {
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
    /// and returns the definition it names.
    fn declare_top_level(&mut self, file: FileId, decl: &RawDecl) -> DefId {
        let kind = top_level_kind(&decl.ty);
        let location = self.at(file, decl.span);
        let init = decl.initialized.then_some(decl.literal.as_ref());
        let def = self.declare_global(&decl.name, &decl.ty, kind, location, init);
        if let Some((next_name, next_span)) = &decl.next_think {
            self.declare_next_think(file, next_name, *next_span);
        }
        def
    }

    /// Declares, or checks, the next-think name of a state header (QC-13.6).
    ///
    /// If the name does not exist it is declared as a global `void()`. If it exists with a
    /// different type it is a type mismatch (QCD-3.5).
    fn declare_next_think(&mut self, file: FileId, name: &str, span: Span) {
        let void_fn = Type::Function(Rc::new(FunctionType {
            ret: SimpleType::Void,
            params: Vec::new(),
            variadic: false,
        }));
        let location = self.at(file, span);
        let def = self.declare_global(name, &void_fn, DefKind::Function, location, None);
        // The name in the header is a reference to the think function.
        self.index.refs.push(Reference { location, def });
    }

    /// Resolves one function body: its parameters, locals and name uses (QC-12.1, QC-12.5).
    fn resolve_body(&mut self, file: FileId, owner: DefId, body: &RawBody) {
        // Record where this function's locals start, so lookups in it see only its own.
        let scope = Scope {
            owner,
            locals_start: self.index.defs.len(),
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
    ///
    /// A member use (FTE-9, FTE-13, FTE-14) is a reference when it names a definition of
    /// field type, an entity field; otherwise it is a vector member, `.length` or a struct
    /// field, which name no definition, and a name that is none of these is unknown.
    fn resolve_uses(&mut self, file: FileId, scope: Option<Scope>, uses: &[RawUse]) {
        for use_ in uses {
            let location = self.at(file, use_.span);
            let def = match scope {
                Some(scope) => self.lookup_in_scope(scope, &use_.name),
                None => self.lookup_global(&use_.name),
            };
            let def = match use_.kind {
                UseKind::Value => def,
                UseKind::Member => {
                    let field = def.filter(|d| self.index.def(*d).is_some_and(|d| d.ty.is_field()));
                    if field.is_none() && (def.is_some() || self.is_member_name(&use_.name)) {
                        continue;
                    }
                    field
                }
            };
            match def {
                Some(def) => self.index.refs.push(Reference { location, def }),
                None => self.index.diagnostics.push(diag(
                    Code::E0301,
                    location,
                    format!("Unknown value \"{}\"", use_.name),
                )),
            }
        }
    }

    /// Whether `name` selects a member that is no definition: a vector's `x`, `y` or `z`
    /// (FTE-9.1), an array's `length` (FTE-13.3), or a field of a struct the module defines
    /// (FTE-14.2).
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
            let mismatch = &def.ty != ty && def.ty != Type::Unknown && *ty != Type::Unknown;
            if mismatch {
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
    /// first, since it shadows the global (FTE-22.1). A local of another function, declared
    /// earlier, is invisible (QC-12.4).
    fn lookup_in_scope(&self, scope: Scope, name: &str) -> Option<DefId> {
        if self.fteqcc {
            self.lookup_local(scope, name)
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
            if def.kind == DefKind::Function && !def.initialized {
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

/// The function whose body is being resolved, and where its locals start in the definition
/// list.
#[derive(Clone, Copy)]
struct Scope {
    owner: DefId,
    locals_start: usize,
}
