// SPDX-License-Identifier: GPL-3.0-or-later

//! Module resolution: one symbol table per module (QC-12).
//!
//! `qcc` compiles a module's files in the order of its `progs.src` as one program, each file
//! seeing every definition of the files before it (QC-23.3). This module joins the per-file
//! [`crate::lower::FileDecls`] in that order into one definition list (QC-12.2), resolves
//! every name use against it (QC-12.5), and records the diagnostics the semantic model owns.
//!
//! Implements QC-11 (declarations and redeclaration), QC-12 (scopes and lookup) and the
//! state-header name rule of QC-13.6; reports QCD-3.5, QCD-3.6, QCD-3.11 and QCD-7.3.

use std::rc::Rc;

use qc_diag::{Code, Diagnostic, Span};

use crate::lower::{FileDecls, RawBody, RawDecl, top_level_kind};
use crate::symbol::{DefId, DefKind, Definition, FileId, Location, Reference};
use crate::ty::{FunctionType, SimpleType, Type};

/// Builds a [`Diagnostic`] (the struct has public fields and no constructor).
fn diag(code: Code, span: Span, message: String) -> Diagnostic {
    Diagnostic {
        code,
        message,
        span,
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
    /// The semantic diagnostics, in source order.
    pub diagnostics: Vec<Diagnostic>,
}

impl ModuleIndex {
    /// The definition a [`DefId`] names.
    #[must_use]
    pub fn def(&self, id: DefId) -> Option<&Definition> {
        self.defs.get(id.0)
    }
}

/// Resolves a module from its files' lowered declarations, in compilation order (QC-23.3).
///
/// `files` is the module's file graph order; each entry is that file's [`FileDecls`]. The
/// returned [`ModuleIndex`] holds the definition list, the resolved references and the
/// diagnostics.
#[must_use]
pub fn resolve_module(files: &[FileDecls]) -> ModuleIndex {
    let mut r = Resolver::default();
    for (index, file) in files.iter().enumerate() {
        let file_id = FileId(index);
        for decl in &file.decls {
            r.add_top_level(file_id, decl);
        }
    }
    r.finish()
}

#[derive(Default)]
struct Resolver {
    index: ModuleIndex,
}

impl Resolver {
    /// Looks up a global name: the first global definition with that name (QC-12.2).
    fn lookup_global(&self, name: &str) -> Option<DefId> {
        self.index
            .defs
            .iter()
            .position(|d| d.kind.is_global() && d.name == name)
            .map(DefId)
    }

    /// Adds a definition and returns its id.
    fn push_def(&mut self, def: Definition) -> DefId {
        let id = DefId(self.index.defs.len());
        self.index.defs.push(def);
        id
    }

    /// Declares, or redeclares, a global-scope name (QC-11.5).
    ///
    /// A name already in scope with the same type refers to the same definition; declaring
    /// it with a different type is QCD-3.5; initialising it a second time is QCD-3.6
    /// (QC-11.5). Returns the [`DefId`] the name resolves to.
    fn declare_global(
        &mut self,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
        initialized: bool,
    ) -> DefId {
        if let Some(existing) = self.lookup_global(name) {
            self.redeclare(existing, name, ty, location, initialized);
            return existing;
        }
        self.push_def(Definition {
            name: name.to_owned(),
            ty: ty.clone(),
            kind,
            location,
            owner: None,
            initialized,
        })
    }

    /// Handles a redeclaration of an existing definition (QC-11.5).
    fn redeclare(
        &mut self,
        existing: DefId,
        name: &str,
        ty: &Type,
        location: Location,
        initialized: bool,
    ) {
        let Some(def) = self.index.defs.get(existing.0) else {
            return;
        };
        // QCD-3.5: a different type is a type mismatch on redeclaration.
        if &def.ty != ty && def.ty != Type::Unknown && *ty != Type::Unknown {
            self.index.diagnostics.push(diag(
                Code::E0205,
                location.span,
                format!("Type mismatch on redeclaration of {name}"),
            ));
            return;
        }
        // QCD-3.6: initialising a name that is already initialised.
        if initialized && def.initialized {
            self.index.diagnostics.push(diag(
                Code::E0206,
                location.span,
                format!("{name} redeclared"),
            ));
            return;
        }
        if initialized && let Some(def) = self.index.defs.get_mut(existing.0) {
            def.initialized = true;
        }
    }

    /// Adds one top-level declaration and, if it is a function body, its scope (QC-13).
    fn add_top_level(&mut self, file: FileId, decl: &RawDecl) {
        let kind = top_level_kind(&decl.ty);
        let location = Location {
            file,
            span: decl.span,
        };
        let def = self.declare_global(&decl.name, &decl.ty, kind, location, decl.initialized);
        // QC-13.6: the next-think name is declared as a global void() if it is new.
        if let Some((next_name, next_span)) = &decl.next_think {
            self.declare_next_think(file, next_name, *next_span);
        }
        if let Some(body) = &decl.body {
            self.resolve_body(file, def, body);
        }
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
        let location = Location { file, span };
        let def = self.declare_global(name, &void_fn, DefKind::Function, location, false);
        // The name in the header is a reference to the think function.
        self.index.refs.push(Reference { location, def });
    }

    /// Resolves one function body: its parameters, locals and name uses (QC-12.1, QC-12.5).
    fn resolve_body(&mut self, file: FileId, owner: DefId, body: &RawBody) {
        // Record where this function's locals start, so lookups in it see only its own.
        let locals_start = self.index.defs.len();
        // QC-12.3: a parameter that names an existing global reuses the global, it does not
        // shadow it; otherwise it is a new parameter in the function's scope.
        for param in &body.params {
            let location = Location {
                file,
                span: param.span,
            };
            self.declare_scoped(owner, &param.name, &param.ty, DefKind::Param, location);
        }
        // QC-11.10, QC-11.9: locals; a local field or function is an error (QCD-3.8 is the
        // parser's; the semantic check is QC-11.9).
        for local in &body.locals {
            let location = Location {
                file,
                span: local.span,
            };
            if local.bad_field_or_function {
                self.index.diagnostics.push(diag(
                    Code::E0208,
                    local.span,
                    "Fields and functions must be global".to_owned(),
                ));
                continue;
            }
            self.declare_scoped(owner, &local.name, &local.ty, DefKind::Local, location);
        }
        // QC-12.5: resolve each name use against globals and this function's own locals.
        for use_ in &body.uses {
            let location = Location {
                file,
                span: use_.span,
            };
            match self.lookup_in_scope(owner, &use_.name, locals_start) {
                Some(def) => self.index.refs.push(Reference { location, def }),
                None => self.index.diagnostics.push(diag(
                    Code::E0301,
                    use_.span,
                    format!("Unknown value \"{}\"", use_.name),
                )),
            }
        }
    }

    /// Declares a parameter or local, honouring QC-12.3's no-shadow rule.
    fn declare_scoped(
        &mut self,
        owner: DefId,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
    ) {
        if let Some(global) = self.lookup_global(name) {
            // QC-12.3: the local or parameter reuses the global declared before it.
            if let Some(def) = self.index.defs.get(global.0)
                && &def.ty != ty
                && def.ty != Type::Unknown
                && *ty != Type::Unknown
            {
                self.index.diagnostics.push(diag(
                    Code::E0205,
                    location.span,
                    format!("Type mismatch on redeclaration of {name}"),
                ));
                return;
            }
            // QCD-7.3: warn that the local name is the global.
            self.index.diagnostics.push(diag(
                Code::W0201,
                location.span,
                format!("local {name} is the global {name}"),
            ));
            // The declaration is a reference to the global it reuses.
            self.index.refs.push(Reference {
                location,
                def: global,
            });
            return;
        }
        self.push_def(Definition {
            name: name.to_owned(),
            ty: ty.clone(),
            kind,
            location,
            owner: Some(owner),
            initialized: false,
        });
    }

    /// Looks up a name in a function's scope: its own locals and parameters first in
    /// declaration order, then globals (QC-12.2).
    ///
    /// `locals_start` is the index in `defs` where this function's locals begin, so a local
    /// of another function (declared earlier) is invisible (QC-12.4).
    fn lookup_in_scope(&self, owner: DefId, name: &str, locals_start: usize) -> Option<DefId> {
        // QC-12.2: walk the single list from the start, taking the first definition that is
        // global or belongs to this function.
        self.index
            .defs
            .iter()
            .enumerate()
            .find(|(index, d)| {
                d.name == name
                    && (d.kind.is_global() || (d.owner == Some(owner) && *index >= locals_start))
            })
            .map(|(index, _)| DefId(index))
    }

    fn finish(mut self) -> ModuleIndex {
        self.check_undefined_functions();
        self.index
    }

    /// QC-11.8, QCD-3.11: every global function must be defined; report those that are not.
    fn check_undefined_functions(&mut self) {
        let mut undefined: Vec<Diagnostic> = Vec::new();
        for def in &self.index.defs {
            if def.kind == DefKind::Function && !def.initialized {
                undefined.push(diag(
                    Code::E0211,
                    def.location.span,
                    format!("function {} was not defined", def.name),
                ));
            }
        }
        self.index.diagnostics.extend(undefined);
    }
}
