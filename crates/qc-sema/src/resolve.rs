// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_GetDef, PR_ParseDefs, PR_ParseImmediateStatements, PR_ParseState, PR_ParseValue
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_GetDef, QCC_PR_ParseDefs, QCC_PR_GenerateInitializerType, QCC_PR_ParseInitializerType_Internal, QCC_PR_ParseDefaultInitialiser, QCC_PR_ParseEnum, QCC_PR_ParseImmediateStatements, QCC_PR_ParseState, QCC_PR_ParseRefValue
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, typecmp, typecmp_lax, QCC_TypeForName

//! Module resolution: one symbol table per module (QC-12).
//!
//! `qcc` compiles a module's files in the order of its `progs.src` as one program, each file
//! seeing every definition of the files before it (QC-23.3). This module joins the per-file
//! [`crate::lower::FileDecls`] in that order into one definition list (QC-12.2), resolves
//! every name use against it (QC-12.5), and records the diagnostics the semantic model owns.
//!
//! Implements QC-11 (declarations and redeclaration, the vector components of QC-11.7), QC-12
//! (scopes and lookup) and the state-header name rule of QC-13.6; reports QCD-3.5, QCD-3.6,
//! QCD-3.7, QCD-3.11, QCD-5.4 and QCD-7.3. Under an FTEQCC profile it follows FTE-22 instead
//! where FTEQCC differs: locals shadow globals (FTE-22.1), a constant may be initialised twice
//! with the same literal (FTE-22.5), and with autoprototyping every top-level name is visible
//! to every function body (FTE-22.3). The full dialect's classes resolve by FTEF-17: a class's
//! members are found through the class and its parents, inside a member function's body before
//! the globals, and after a `.` through the type of what the access is made on. FTEQCC's
//! definitions follow FTEF-19, ported from `QCC_PR_ParseDefs` and `QCC_PR_GetDef`: casts and
//! constness of initialisers, parameters, array sizes, typedefs, enum values and the
//! redeclaration rules.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use qc_diag::{Code, Diagnostic, Span};
use qc_syntax::Dialect;

use crate::cast::{ConstType, Source};
use crate::expand::OriginMap;
use crate::lower::{
    ArrayDim, BuiltinFunction, EnumItem, FileDecls, LiteralValue, MemberBase, MemberKind,
    Qualifier, RawBody, RawClass, RawDecl, RawLocal, RawParamInfo, RawUse, RawValue, StatePart,
    TypeKind, UseKind, ValueKind, top_level_kind,
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
    resolve_mapped(files, &Sources::default(), options)
}

/// What [`resolve_mapped`] knows of where the files' declarations came from.
#[derive(Default)]
pub(crate) struct Sources<'a> {
    /// For each entry of the files, where each span of its rendered text came from (QC-9.7).
    /// With none a span is in the file of its entry.
    pub(crate) maps: &'a [&'a OriginMap],
    /// For each entry, the `#pragma noref` changes in its text (FTEF-12.6).
    pub(crate) noref: &'a [&'a [(usize, bool)]],
    /// The source files whose name ends in `extensions.qc`, where FTEQCC keeps a function's
    /// first declaration (FTEF-19.7).
    pub(crate) extensions: HashSet<FileId>,
}

/// A function body waiting to be resolved: its file, its function, its class and the
/// declaration it is the body of.
type Pending<'a> = (
    FileId,
    DefId,
    Option<DefId>,
    &'a RawBody,
    Option<&'a RawDecl>,
);

/// [`resolve_module`] for files the parser read after preprocessing, located through
/// `sources`, so every location in the index is a span of a source file.
pub(crate) fn resolve_mapped(
    files: &[FileDecls],
    sources: &Sources<'_>,
    options: ResolveOptions,
) -> ModuleIndex {
    let fteqcc = options.dialect.has_fteqcc_extensions();
    let mut r = Resolver {
        index: ModuleIndex::default(),
        maps: sources.maps,
        extensions: &sources.extensions,
        fteqcc,
        autoproto: fteqcc && options.autoproto,
        unit: FileId(0),
        facts: Vec::new(),
        globals: HashMap::new(),
        literals: HashMap::new(),
        builtin_functions: HashMap::new(),
        members: files
            .iter()
            .flat_map(|f| f.members.iter().cloned())
            .collect(),
        classes: HashMap::new(),
        class_names: HashMap::new(),
        any_member: HashMap::new(),
        typedefs: HashMap::new(),
        structs: HashSet::new(),
        strict_enums: HashMap::new(),
        enum_types: HashSet::new(),
        redefinable: HashSet::new(),
        unused: HashSet::new(),
        bodies: HashSet::new(),
        enum_next: None,
    };
    // FTE-22.3: with autoprototyping FTEQCC's first pass reads only the definitions, so the
    // second, which compiles the bodies, finds every top-level name already declared.
    let defer = r.autoproto;
    let mut bodies: Vec<Pending<'_>> = Vec::new();
    // FTEF-12.6: `#pragma noref` holds from where it stands to the next one, across files.
    let mut noref_on = false;
    for (index, file) in files.iter().enumerate() {
        let file_id = FileId(index);
        r.unit = file_id;
        for d in &file.diagnostics {
            let location = r.at(file_id, d.span);
            r.index
                .diagnostics
                .push(diag(d.code, location, d.message.clone()));
        }
        let events = sources.noref.get(index).copied().unwrap_or_default();
        let start = noref_on;
        if let Some(&(_, last)) = events.last() {
            noref_on = last;
        }
        for decl in &file.decls {
            let pragma = events
                .iter()
                .take_while(|(at, _)| *at <= decl.span.start)
                .last()
                .map_or(start, |&(_, on)| on);
            if decl.enum_item.is_none() {
                r.enum_next = None;
            }
            let pending = r.declare_top_level(file_id, decl, pragma);
            if (decl.has(&Qualifier::Noref) || pragma)
                && let Some(def) = r.lookup_global(&decl.name)
            {
                r.unused.insert(def);
            }
            if decl.enum_item.is_none() {
                r.resolve_uses(file_id, None, &decl.uses);
            }
            if defer {
                bodies.extend(pending);
            } else {
                for pending in pending {
                    r.resolve_pending(pending);
                }
            }
        }
    }
    for pending in bodies {
        r.resolve_pending(pending);
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

/// What the resolver knows of a definition beyond its [`Definition`], as `qcc` and FTEQCC keep
/// it in their `def_t`.
#[derive(Clone, Debug, Default)]
struct Facts {
    /// FTEQCC's `def->constant`, which the first declaration fixes (FTEF-19.3).
    constant: bool,
    /// FTEQCC's `def->arraysize`: `None` for no array (FTEF-19.8).
    array: Option<ArrayDim>,
    /// The parameters of its function type (FTEF-19.6).
    params: Vec<RawParamInfo>,
    /// Under the vanilla profile, a `.vector` field's component, of `qcc`'s `type_floatfield`
    /// (QC-11.12).
    field_component: bool,
    /// `shared` (FTEF-19.11).
    shared: bool,
    /// `static` (FTEF-19.12), and the unit it is static in.
    statics: Option<FileId>,
    /// A `static` local (FTEF-19.1).
    static_local: bool,
    /// The `deprecated` reason (FTEF-19.13).
    deprecated: Option<String>,
    /// FTEQCC's `def->referenced`: a use, or a `noref` mark (FTEF-19.7).
    referenced: bool,
    /// `accumulate` (FTEF-15.5).
    accumulate: bool,
    /// Initialised with a builtin (FTEF-17.9).
    builtin: bool,
}

struct Resolver<'a> {
    index: ModuleIndex,
    /// Where the spans of each file's rendered text came from; empty when the files were
    /// parsed as written.
    maps: &'a [&'a OriginMap],
    /// The source files named `*extensions.qc` (FTEF-19.7).
    extensions: &'a HashSet<FileId>,
    /// Whether an FTEQCC profile is in force (FTE-22).
    fteqcc: bool,
    /// Whether FTEQCC autoprototypes the module (FTE-22.3).
    autoproto: bool,
    /// The compiled file, the unit a `static` is visible in (FTEF-19.12).
    unit: FileId,
    /// For each definition, what [`Facts`] holds.
    facts: Vec<Facts>,
    /// The first global definition of each name (QC-12.2), so a global lookup is direct.
    globals: HashMap<String, Vec<DefId>>,
    /// The literal each initialised global was given, for FTE-22.5.
    literals: HashMap<DefId, LiteralValue>,
    /// For each builtin number and engine name, the definition whose builtin function FTEQCC
    /// made first with them: the one a redeclaration with them reuses (FTE-23.7).
    builtin_functions: HashMap<BuiltinFunction, DefId>,
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
    /// The type each `typedef` names (FTEF-19.9).
    typedefs: HashMap<String, Type>,
    /// The names of the structs and unions (FTEF-19.15).
    structs: HashSet<String>,
    /// The names of the named enums' types (FTEF-19.10).
    enum_types: HashSet<String>,
    /// The base type of each `enum class`, whose values are of its own type (FTEF-19.10).
    strict_enums: HashMap<String, Type>,
    /// The definitions whose last initialiser was `weak` or that are `accumulate`, which the
    /// next may replace (FTEF-15.5): FTEQCC's `def->weak`, which gives the next initialiser
    /// `PIF_STRONGER`, and `def->accumulate`.
    redefinable: HashSet<DefId>,
    /// The definitions marked `noref`, by qualifier or `#pragma noref`, whose initialiser a
    /// later one may replace (FTEF-17.8).
    unused: HashSet<DefId>,
    /// The functions defined with a body, which a later builtin does not replace (FTEF-17.9).
    bodies: HashSet<DefId>,
    /// The value the next name of the enum being read gets when it is given none, and whether
    /// the values double (FTEF-19.10); `None` outside an enum or once a value is unknown.
    enum_next: Option<f64>,
}

/// The `_x`, `_y` and `_z` suffixes of a vector's component names (QC-11.7).
const COMPONENTS: [&str; 3] = ["_x", "_y", "_z"];

/// The deepest chain of parent classes followed; a deeper or circular one stops there.
const MAX_CLASS_DEPTH: usize = 64;

/// The deepest chain of typedefs followed (FTEF-19.9).
const MAX_TYPEDEF_DEPTH: usize = 64;

/// The builtins FTEQCC keeps the first declaration of when a later one gives another type,
/// with a warning (FTEF-19.7): `QCC_PR_GetDef`'s "selfish lazy hack".
const MISDECLARED_BUILTINS: [&str; 3] = ["droptofloor", "callfunction", "trailparticles"];

/// The functions a body may replace a builtin of with a warning, not an error (FTEF-19.4).
const REPLACEABLE_BUILTINS: [&str; 2] = ["anglemod", "crossproduct"];

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

/// Words FTEQCC reads as part of an expression rather than as names a program defines, when
/// no definition has the name: `this` and `super` in a member function, `prior` in a `__wrap`
/// function, and the intrinsics `QCC_PR_ParseRefValue` declares on demand (FTEF-17.4,
/// FTEF-19.16). `sizeof` is parsed as a term of its own (FTEF-18.12), never a name.
fn is_intrinsic(name: &str) -> bool {
    matches!(
        name,
        "this"
            | "super"
            | "prior"
            | "_length"
            | "nil"
            | "randomv"
            | "random"
            | "alloca"
            | "entnum"
            | "autocvar"
            | "used_model"
            | "used_sound"
            | "va_arg"
            | "_"
    )
}

/// Whether a definition named `name` of type `ty` is named as one of the keywords
/// `QCC_PR_DummyDef`'s `KEYWORD` list tests, enabled by default (FTEF-19.16): `break` only for
/// a definition that is no function.
fn is_keyword_name(name: &str, ty: &Type) -> bool {
    matches!(
        name,
        "var"
            | "for"
            | "switch"
            | "case"
            | "local"
            | "default"
            | "goto"
            | "continue"
            | "string"
            | "float"
            | "entity"
            | "vector"
            | "const"
            | "asm"
    ) || (name == "break" && !ty.is_function())
}

/// The initialiser of a declaration, as redeclaration sees it (QC-11.5, FTE-22.5): `None`
/// when there is none, and otherwise its value when it is a single literal.
type Init<'a> = Option<Option<&'a LiteralValue>>;

/// Whether two function types' parameter lists are the same under FTEQCC's `typecmp`, beside
/// the types the model compares: two default values, where both have one, must be equal
/// (FTEF-19.6).
fn defaults_agree(a: &[RawParamInfo], b: &[RawParamInfo]) -> bool {
    a.iter()
        .zip(b)
        .all(|(a, b)| match (&a.default_literal, &b.default_literal) {
            (Some(x), Some(y)) => x == y,
            _ => true,
        })
}

/// A redeclaration of a name already defined: what the earlier definition is, and what the
/// new declaration says.
struct Redeclaration<'r> {
    existing: DefId,
    name: &'r str,
    ty: &'r Type,
    location: Location,
    init: Init<'r>,
    params: &'r [RawParamInfo],
    array: Option<ArrayDim>,
}

/// What a redeclaration's type check concluded (FTEF-19.7).
enum TypeCheck {
    /// The types agree, or FTEQCC accepts the difference.
    Same,
    /// FTEQCC keeps the first declaration and ignores this one.
    Ignored,
    /// The earlier definition is replaced by a new one (a `noref` variable).
    Replaced,
    /// A type mismatch, reported.
    Mismatch,
}

impl<'a> Resolver<'a> {
    /// The source location of the span `span` of file `file`'s parsed text: through the
    /// file's origin map when it was preprocessed (QC-9.7), else the span itself.
    fn at(&self, file: FileId, span: Span) -> Location {
        self.maps
            .get(file.0)
            .and_then(|map| map.locate(span))
            .unwrap_or(Location { file, span })
    }

    fn report(&mut self, code: Code, location: Location, message: String) {
        self.index.diagnostics.push(diag(code, location, message));
    }

    /// Looks up a global name: the first global definition with that name (QC-12.2). Under
    /// FTEQCC a `static` global of another unit is not seen (FTEF-19.12), and the next
    /// definition of the name is.
    fn lookup_global(&self, name: &str) -> Option<DefId> {
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

    fn fact(&self, def: DefId) -> Facts {
        self.facts.get(def.0).cloned().unwrap_or_default()
    }

    fn fact_mut(&mut self, def: DefId) -> Option<&mut Facts> {
        self.facts.get_mut(def.0)
    }

    /// Adds a definition and returns its id. A class member is not a global by its name.
    fn push_def(&mut self, def: Definition) -> DefId {
        // FTEF-19.16: `QCC_PR_DummyDef` warns of a definition named as an enabled keyword,
        // which keeps its meaning beside it (`keywords_coexist`).
        if self.fteqcc && is_keyword_name(&def.name, &def.ty) {
            let message = format!(
                "\"{}\" keyword used as variable name - coexisting",
                def.name
            );
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
    fn resolved(&self, ty: &Type) -> Type {
        let mut ty = ty.clone();
        for _ in 0..MAX_TYPEDEF_DEPTH {
            let Type::Named(name) = &ty else {
                return ty;
            };
            match self.typedefs.get(name.as_ref()) {
                Some(aliased) if aliased != &ty => ty = aliased.clone(),
                Some(_) | None => return ty,
            }
        }
        ty
    }

    /// Whether a redeclaration's type `ty` matches the earlier `old` (QC-11.5): equal, or
    /// either unknown; under FTEQCC also when the model does not follow either (FTEF-17.12).
    fn same_type(&self, old: &Type, ty: &Type) -> bool {
        if self.fteqcc {
            self.resolved(old).compatible(&self.resolved(ty))
        } else {
            old == ty || *old == Type::Unknown || *ty == Type::Unknown
        }
    }

    /// FTEQCC's `typecmp_lax` of a function redeclaration (FTEF-19.7): the return types and
    /// the parameters both lists have the same, and every parameter one list has beyond the
    /// other `optional`; whether either is variadic does not matter.
    fn lax_same(
        old: &Type,
        old_params: &[RawParamInfo],
        ty: &Type,
        params: &[RawParamInfo],
    ) -> bool {
        let (Type::Function(a), Type::Function(b)) = (old, ty) else {
            return false;
        };
        if !a.ret.compatible(&b.ret) {
            return false;
        }
        let shared = a.params.len().min(b.params.len());
        let types_agree = a.params.iter().zip(&b.params).all(|(x, y)| x.compatible(y));
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
    /// (QC-11.5), unless FTE-22.5 accepts it. Returns the [`DefId`] the name resolves to.
    fn declare_global(
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
    fn declare_global_with(
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
        let field_component = decl.component && !self.fteqcc && kind == DefKind::Field;
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
        // the vector's initialised state and its words (FTEF-19.5).
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
                let component_init = if self.fteqcc {
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
                if self.fteqcc && self.lookup_global(&component_name).is_some() {
                    // FTEF-19.5: FTEQCC makes the components with `QCC_PR_DummyDef`, which
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
    fn check_redeclared_type(&mut self, r: &Redeclaration<'_>, component: bool) -> TypeCheck {
        let Some(def) = self.index.defs.get(r.existing.0) else {
            return TypeCheck::Mismatch;
        };
        let old = def.ty.clone();
        let facts = self.fact(r.existing);
        // QC-11.12: a vector field's component has `qcc`'s `type_floatfield`, which no `.float`
        // written in the source is, and only another component's lookup has.
        if !self.fteqcc && facts.field_component != component && def.kind == DefKind::Field {
            self.mismatch(r.name, r.location, &old, r.ty);
            return TypeCheck::Mismatch;
        }
        let same =
            self.same_type(&old, r.ty) && (!self.fteqcc || defaults_agree(&facts.params, r.params));
        if same {
            return TypeCheck::Same;
        }
        if !self.fteqcc {
            self.mismatch(r.name, r.location, &old, r.ty);
            return TypeCheck::Mismatch;
        }
        let old = self.resolved(&old);
        let ty = self.resolved(r.ty);
        // FTEF-19.7: at file level a difference only in optional arguments is a warning, and
        // a declaration with more parameters gives the definition its type.
        if Self::lax_same(&old, &facts.params, &ty, r.params) {
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
        if self.unused.contains(&r.existing) && !facts.referenced {
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

    fn mismatch(&mut self, name: &str, location: Location, old: &Type, ty: &Type) {
        // QCD-3.5: `PR_GetDef` finds the earlier definition with another type.
        let message = if self.fteqcc {
            format!(
                "Type mismatch on redeclaration of {name}. {}, should be {}",
                ty.fteqcc_name(),
                old.fteqcc_name()
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
    fn check_array(&mut self, r: &Redeclaration<'_>, local: bool) -> bool {
        if !self.fteqcc {
            return false;
        }
        let size = |dim: Option<ArrayDim>| match dim {
            None => Some(0),
            Some(ArrayDim::Sized(n)) => Some(n),
            Some(ArrayDim::Unsized | ArrayDim::Unknown) => None,
        };
        let old = self.fact(r.existing).array;
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
    fn redeclare(&mut self, r: &Redeclaration<'_>) {
        let Some(def) = self.index.defs.get(r.existing.0) else {
            return;
        };
        let Some(literal) = r.init else {
            return;
        };
        let existing = r.existing;
        let name = r.name;
        if def.initialized {
            // FTE-22.5: FTEQCC compares a second initialiser with the first and accepts it
            // when the two are the same value; FTEF-17.8: it does not compare them when the
            // first is `noref`.
            let same = self.fteqcc
                && (literal.is_some_and(|literal| self.same_value(existing, name, literal))
                    || self.unused.contains(&existing));
            if !same && self.fteqcc && self.parseentitydata(existing, name) {
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
                // FTE-23.7: FTEQCC makes a new builtin function for a builtin it reuses none
                // for.
                if let Some(literal) = literal {
                    self.note_builtin_function(existing, name, literal);
                }
                // QCD-3.6: initialising a name that is already initialised.
                self.report(Code::E0206, r.location, format!("{name} redeclared"));
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

    /// FTEF-19.5: FTEQCC accepts `parseentitydata` given another builtin after `#608` or
    /// `#613`, "dpextensions is WRONG", and makes it `#613`.
    fn parseentitydata(&mut self, existing: DefId, name: &str) -> bool {
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
    /// (FTE-22.5, FTEF-17.10). A builtin is when FTEQCC reuses `existing`'s builtin function
    /// for it (FTE-23.7): the one it made first with the same number and engine name, the
    /// definition's own name standing in for none.
    fn same_value(&self, existing: DefId, name: &str, literal: &LiteralValue) -> bool {
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
    /// one with its number and engine name was made before (FTE-23.7).
    fn note_builtin_function(&mut self, def: DefId, name: &str, literal: &LiteralValue) {
        if let Some(key) = literal.builtin_function(name) {
            self.builtin_functions.entry(key).or_insert(def);
        }
    }

    /// FTEF-19.3: whether FTEQCC makes a new definition of `decl` a constant: `const`, or, at
    /// file level and not `var`, initialised, a function or a field (`GDF_CONST`). An
    /// `autocvar_` never is (FTEF-19.16).
    fn constant_at_file_level(decl: &RawDecl) -> bool {
        if decl.name.starts_with("autocvar_") {
            return false;
        }
        decl.has(&Qualifier::Const)
            || (!decl.has(&Qualifier::Var)
                && (decl.initialized || decl.ty.is_function() || decl.ty.is_field()))
    }

    /// Declares one top-level declaration, and returns the function bodies it brings, to
    /// resolve now or, with autoprototyping, after every declaration (FTE-22.3). `noref` says
    /// a `#pragma noref 1` holds (FTEF-12.6).
    fn declare_top_level(
        &mut self,
        file: FileId,
        decl: &'a RawDecl,
        noref: bool,
    ) -> Vec<Pending<'a>> {
        let location = self.at(file, decl.span);
        if let Some(kind) = decl.type_kind {
            self.declare_type_name(kind, decl, location);
            // FTEF-17.1: a type's name may be declared again, a class first forward and then
            // with its body, and a typedef in more than one header.
            let def =
                self.declare_global(&decl.name, &Type::Unknown, DefKind::Global, location, None);
            if let Some(class) = &decl.class {
                return self.declare_class(file, def, class);
            }
            return Vec::new();
        }
        if self.fteqcc
            && let Some(item) = &decl.enum_item
        {
            self.declare_enum_item(file, decl, item, location);
            return Vec::new();
        }
        let qualifier_fatal = self.fteqcc && self.warn_qualifiers(decl, location);
        if let Some(scope) = &decl.scope
            && self.fteqcc
        {
            return self.define_member(file, scope, decl, location);
        }
        if self.fteqcc {
            self.warn_autocvar(decl, location);
        }
        // FTE-11, FTEF-15.5: a `var` of function type holds a function; it defines none.
        let kind = if decl.has(&Qualifier::Var) && decl.ty.is_function() {
            DefKind::Global
        } else {
            top_level_kind(&decl.ty)
        };
        let pending = |def: DefId| {
            decl.body
                .as_ref()
                .map(|body| vec![(file, def, None, body, Some(decl))])
                .unwrap_or_default()
        };
        // FTEF-19.6: the default values are read with the type, before the name.
        let mut aborted = self.fteqcc && self.check_defaults(file, decl);
        let existing = match self.find_existing(decl, location) {
            Ok(existing) => existing,
            Err(found) => return pending(found),
        };
        aborted |= qualifier_fatal;
        // FTEF-19.12: `static` on a name another file defined is "can't redefine non-static
        // as static".
        if self.fteqcc
            && decl.has(&Qualifier::Static)
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
        if self.fteqcc && !aborted {
            aborted |= self.check_initialisation(decl, existing, location);
        }
        // QC-11.5: id's qcc reports "NAME redeclared" at the `=`, before it reads the value.
        let value = if aborted || (!self.fteqcc && was_initialized && decl.initialized) {
            Ok(None)
        } else {
            self.check_value(
                file,
                &decl.name,
                &decl.ty,
                decl.value.as_ref(),
                Holder::Global,
            )
        };
        let literal = match value {
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
        let external = decl.has(&Qualifier::Extern) && decl.ty.is_function();
        let shared = decl.has(&Qualifier::Shared);
        let initialized = decl.initialized || external || (self.fteqcc && shared);
        let marks_only = (external || shared) && !decl.initialized;
        let init = if aborted || (replaces && existing.is_some()) || marks_only {
            None
        } else {
            initialized.then_some(literal.as_ref())
        };
        // FTEF-17.9: a builtin given for a function that has a body is a warning, and the
        // function keeps its body.
        if self.fteqcc
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
        let statics = (self.fteqcc && decl.has(&Qualifier::Static)).then_some(self.unit);
        let facts = DeclFacts {
            component: false,
            checked: true,
            constant: Self::constant_at_file_level(decl),
            array: decl.array,
            params: &decl.params,
            statics,
            deprecated: decl.deprecated(),
        };
        let def = self.declare_global_with(&decl.name, &decl.ty, kind, location, init, &facts);
        if let Some(f) = self.fact_mut(def) {
            f.shared |= shared;
            f.builtin |= decl.builtin && !aborted;
            f.accumulate |= decl.has(&Qualifier::Accumulate);
            // FTEQCC's `noref` marks a definition referenced too (`def->referenced = true;`).
            f.referenced |= noref || decl.has(&Qualifier::Noref);
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
        } else if decl.initialized && !self.fact(def).accumulate {
            self.redefinable.remove(&def);
        }
        pending(def)
    }

    /// QC-11.5, FTEF-19.7, FTEF-19.8: the definition a top-level declaration redeclares, found
    /// and checked as `PR_GetDef` and `QCC_PR_GetDef` find it, before any initialiser is read;
    /// `None` for a new one, and `Err` with the definition when the declaration is refused.
    fn find_existing(
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
            array: decl.array,
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

    /// FTEF-19.16: an `autocvar_` defined as a constant or static is made neither, with a
    /// warning.
    fn warn_autocvar(&mut self, decl: &RawDecl, location: Location) {
        if !decl.name.starts_with("autocvar_") {
            return;
        }
        let what = if decl.has(&Qualifier::Const) || Self::constant_at_file_level_raw(decl) {
            "constant"
        } else if decl.has(&Qualifier::Static) {
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

    /// The constness test of [`Resolver::constant_at_file_level`] without the `autocvar_`
    /// exception, for its warning (FTEF-19.16).
    fn constant_at_file_level_raw(decl: &RawDecl) -> bool {
        decl.has(&Qualifier::Const)
            || (!decl.has(&Qualifier::Var)
                && (decl.initialized || decl.ty.is_function() || decl.ty.is_field()))
    }

    /// FTEF-19.4, FTEF-19.11: what `QCC_PR_ParseDefs` reports of an initialiser before it reads
    /// it: a `shared` definition given a value; a body for a definition already initialised,
    /// "redeclaration of function body", but for `anglemod` and `crossproduct`, whose builtin
    /// a body replaces with a warning; and a `wrap` that has nothing to wrap.
    fn check_initialisation(
        &mut self,
        decl: &RawDecl,
        existing: Option<DefId>,
        location: Location,
    ) -> bool {
        if !decl.initialized {
            if decl.has(&Qualifier::Accumulate)
                && let Some(existing) = existing
                && self.index.def(existing).is_some_and(|d| d.initialized)
                && !self.fact(existing).accumulate
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
        let facts = existing.map(|d| self.fact(d)).unwrap_or_default();
        if decl.has(&Qualifier::Shared) || facts.shared {
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
        if decl.has(&Qualifier::Accumulate) || facts.accumulate {
            if initialized && !facts.accumulate && !wrap {
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
            if facts.builtin && REPLACEABLE_BUILTINS.contains(&decl.name.as_str()) {
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
    /// the definition keeps the constness its first declaration gave it.
    fn warn_constness_change(
        &mut self,
        decl: &RawDecl,
        existing: Option<DefId>,
        location: Location,
        initialised: bool,
    ) {
        let Some(existing) = existing else {
            return;
        };
        let was = self.fact(existing).constant;
        let now = if initialised {
            decl.has(&Qualifier::Const) || !decl.has(&Qualifier::Var)
        } else {
            decl.has(&Qualifier::Const)
                || (!decl.has(&Qualifier::Var) && (decl.ty.is_function() || decl.ty.is_field()))
        };
        if was == now
            || self
                .index
                .def(existing)
                .is_some_and(|d| d.member_of.is_some())
        {
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

    /// FTEF-19.13 and the warnings of `QCC_PR_ParseDefs` about qualifiers it ignores
    /// (FTEF-19.16): `extern` on what is no function, conflicting storage classes or
    /// constness, a function modifier on what is no function, and `accumulate` on what is no
    /// function, an error.
    fn warn_qualifiers(&mut self, decl: &RawDecl, location: Location) -> bool {
        let function = decl.ty.is_function();
        if decl.has(&Qualifier::Extern) && !function {
            self.report(
                Code::W0214,
                location,
                "extern keyword may only apply to qc functions.".to_owned(),
            );
        }
        let storage = [
            Qualifier::Extern,
            Qualifier::Auto,
            Qualifier::Static,
            Qualifier::Shared,
        ]
        .iter()
        .filter(|q| decl.has(q) && (**q != Qualifier::Extern || function))
        .count();
        if storage > 1 {
            self.report(
                Code::W0214,
                location,
                "Conflicting storage classes.".to_owned(),
            );
        }
        if decl.has(&Qualifier::Const) && decl.has(&Qualifier::Var) {
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
        false
    }

    /// FTEF-19.2, FTEF-19.3, QC-11.3: the checks of a value initialiser of a definition of
    /// type `ty` named `name`, held as `holder` says, and the value it gives when it names a
    /// constant (FTEF-19.5). Under the vanilla profile a literal of another type than the
    /// definition's is "wrong immediate type" (QCD-3.7); under FTEQCC a value its type
    /// cannot be cast to is "Cannot cast" (QCD-3.13), and a name that is no constant
    /// initialising a global or a static is "is not constant" (QCD-3.14). `Err` is a fatal
    /// error, after which the compilers read nothing more of the declaration.
    fn check_value(
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
        if !self.fteqcc {
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
        match &value.kind {
            ValueKind::Literal(literal) => {
                let source = Source::Literal(*literal);
                if self.cast_value(source, &ty, location) {
                    return Err(Fatal);
                }
                Ok(None)
            }
            ValueKind::Translate(string) => {
                // FTEF-19.2: `_()` initialising a string takes one string literal.
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
                self.check_named_value(name, &ty, source_name, holder, location)
            }
            ValueKind::Other => Ok(None),
        }
    }

    /// [`Resolver::check_value`] of a value that is the name `source_name`.
    fn check_named_value(
        &mut self,
        name: &str,
        ty: &Type,
        source_name: &str,
        holder: Holder,
        location: Location,
    ) -> Result<Option<LiteralValue>, Fatal> {
        let lookup = |name: &str| match holder {
            Holder::Local(scope) | Holder::Static(scope) | Holder::Const(scope) => {
                self.lookup_in_scope(scope, name)
            }
            Holder::Global => self.lookup_global(name),
        };
        // `Scope::name`: the definition of that name, or for a scope that is no class the
        // plain name, as a named enum's values are (FTEF-19.10).
        let found = lookup(source_name).or_else(|| {
            let (prefix, bare) = source_name.split_once("::")?;
            (!self.class_names.contains_key(prefix))
                .then(|| lookup(bare))
                .flatten()
        });
        let Some(source) = found else {
            return Ok(None);
        };
        let Some(source_ty) = self.index.def(source).map(|d| d.ty.clone()) else {
            return Ok(None);
        };
        // FTEF-19.2: in a member function a field's name is `this.field`, of the field's type
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
        // FTEF-19.10: a strict enum's value casts to no other type.
        if let Type::Named(strict) = &source_ty
            && let Some(base) = self.strict_enums.get(strict.as_ref())
            && source_ty != *ty
        {
            let message = format!(
                "Cannot cast from enum {strict}:{} to {}",
                base.fteqcc_name(),
                ty.fteqcc_name()
            );
            self.report(Code::E0213, location, message);
            return Err(Fatal);
        }
        if self.cast_value(Source::Typed(&source_ty), ty, location) {
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

    /// FTEQCC's implicit cast of `source` to `to` (FTEF-19.2): "Cannot cast", an error, or
    /// "Implicit cast", a warning, or neither. Returns whether the cast is refused.
    fn cast_value(&mut self, source: Source<'_>, to: &Type, location: Location) -> bool {
        let (code, verb) = if source.refused(to) {
            (Code::E0213, "Cannot cast")
        } else if source.warns(to) {
            (Code::W0220, "Implicit cast")
        } else {
            return false;
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
    fn check_defaults(&mut self, file: FileId, decl: &RawDecl) -> bool {
        for param in &decl.params {
            let Some(value) = &param.default else {
                continue;
            };
            let location = self.at(file, value.span);
            match &value.kind {
                ValueKind::Literal(literal) => {
                    let source = Source::Literal(*literal);
                    let ty = self.resolved(&param.ty);
                    if self.cast_value(source, &ty, location) {
                        return true;
                    }
                }
                ValueKind::Name(name) => {
                    let Some(source) = self.lookup_global(name) else {
                        continue;
                    };
                    if !self.fact(source).constant {
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
                    if self.cast_value(Source::Typed(&source_ty), &ty, location) {
                        return true;
                    }
                }
                ValueKind::Translate(_) | ValueKind::Other => {}
            }
        }
        false
    }

    /// The name of a type: FTEF-19.9 compares a `typedef` with the one of the same name before
    /// it, and records the structs (FTEF-19.15).
    fn declare_type_name(&mut self, kind: TypeKind, decl: &RawDecl, location: Location) {
        if !self.fteqcc {
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
                let ty = self.resolved(ty);
                match self.typedefs.get(&decl.name) {
                    Some(old) if !Self::same_typedef(old, &ty) => {
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
                if !self.enum_types.insert(decl.name.clone()) {
                    self.report(
                        Code::W0224,
                        location,
                        "legacy enum base type. Use \"enum [class] [name_e]:type\" instead"
                            .to_owned(),
                    );
                }
            }
            TypeKind::Class | TypeKind::Accessor => {}
        }
    }

    /// Whether a typedef names again the type `old` an earlier one named (FTEF-19.9): FTEQCC's
    /// `typecmp`, structural, with a C type word compared by the kind it names and any other
    /// type a program names not compared.
    fn same_typedef(old: &Type, ty: &Type) -> bool {
        match (old.fteqcc_kind(), ty.fteqcc_kind()) {
            (Some(a), Some(b)) if a != b => false,
            (Some(_), Some(_)) => {
                old == ty || matches!(old, Type::Named(_)) || matches!(ty, Type::Named(_))
            }
            (None, _) | (_, None) => true,
        }
    }

    /// An enum's name (FTEF-19.10): a constant of the enum's base type whose value is the one
    /// after `=`, a literal or a constant, or else the one before it plus one, doubled for
    /// `enumflags`. `QCC_PR_ParseEnum` makes it with `QCC_PR_DummyDef`, which looks for no
    /// earlier definition (FTEF-17.10).
    fn declare_enum_item(
        &mut self,
        file: FileId,
        decl: &'a RawDecl,
        item: &EnumItem,
        location: Location,
    ) {
        self.resolve_uses(file, None, &decl.uses);
        if item.legacy {
            self.report(
                Code::W0224,
                location,
                "legacy enum base type. Use \"enum [class] [name_e]:type\" instead".to_owned(),
            );
        }
        let base = self.resolved(&item.base);
        let numeric = matches!(
            base.fteqcc_kind(),
            Some("float" | "__int32" | "__uint32" | "__int64" | "__uint64" | "__double")
        );
        let next = self.enum_next.unwrap_or(if item.flags { 1.0 } else { 0.0 });
        let value: Option<f64> = match item.value.as_ref() {
            None => {
                if !numeric && base.fteqcc_kind().is_some() {
                    self.report(
                        Code::E0229,
                        location,
                        "values for enums of this type must be initialised".to_owned(),
                    );
                    None
                } else {
                    Some(next)
                }
            }
            Some(value) => self.enum_value(file, value, item.number, &base),
        };
        if item.flags
            && let Some(v) = value
        {
            self.warn_enumflag(v, &base, location);
        }
        self.enum_next = value.map(|v| {
            if item.flags {
                // FTEF-19.10: `next_i <<= 1;` then `if (!next_i) next_f = next_i = 1;`, where
                // `next_i` holds the value's integer part.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "spec FTEF-19.10: `next_i = next_f` keeps the value's integer part"
                )]
                let whole = v as i64;
                if whole.wrapping_shl(1) == 0 {
                    1.0
                } else {
                    v * 2.0
                }
            } else {
                v + 1.0
            }
        });
        let literal = value
            .filter(|_| base == Type::Simple(SimpleType::Float))
            .map(|v| {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "spec FTEF-19.10: `QCC_MakeFloatConst(next_f)` stores the value as a float"
                )]
                let single = v as f32;
                LiteralValue::Float(single.to_bits())
            });
        // FTEF-17.10: a new definition each time, never a redeclaration; a name defined
        // before keeps its definition for lookups.
        if self.lookup_global(&decl.name).is_some() {
            return;
        }
        if let Type::Named(strict) = &decl.ty
            && decl.ty != base
        {
            self.strict_enums.insert(strict.to_string(), base.clone());
        }
        let id = self.push_def(Definition {
            name: decl.name.clone(),
            ty: decl.ty.clone(),
            kind: DefKind::Global,
            location,
            owner: None,
            initialized: true,
            member_of: None,
            generated: false,
        });
        if let Some(facts) = self.fact_mut(id) {
            facts.constant = true;
        }
        if let Some(literal) = literal {
            self.literals.insert(id, literal);
        }
    }

    /// FTEF-19.10: an `enumflags` value that is no whole number, or has more than one bit set,
    /// is FTEQCC's warning.
    fn warn_enumflag(&mut self, value: f64, base: &Type, location: Location) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec FTEF-19.10: `i = (longlong)next_f;` truncates the value to an integer"
        )]
        let whole = value as i64;
        #[expect(
            clippy::cast_precision_loss,
            clippy::float_cmp,
            reason = "spec FTEF-19.10: `(double)i != next_f` compares the integer back exactly"
        )]
        let fraction = base.fteqcc_kind() != Some("__int32") && whole as f64 != value;
        let message = if fraction {
            format!("enumflags - {value:.6} not an integer value")
        } else if whole.count_ones() > 1 {
            let bits = u64::from_ne_bytes(whole.to_ne_bytes());
            format!("enumflags - {bits:#x}({whole}) has multiple bits set")
        } else {
            return;
        };
        self.report(Code::W0225, location, message);
    }

    /// The value after an enum name's `=` (FTEF-19.10): a number literal's, or a numeric
    /// constant's; a string or vector is "Implicit type mismatch" and a name that is no
    /// constant "is not a compile-time constant".
    fn enum_value(
        &mut self,
        file: FileId,
        value: &RawValue,
        number: Option<u32>,
        base: &Type,
    ) -> Option<f64> {
        let location = self.at(file, value.span);
        let wanted = base.fteqcc_kind().unwrap_or("float");
        match &value.kind {
            ValueKind::Literal(literal) => {
                let got = literal.ty.name();
                let numeric = !matches!(literal.ty, ConstType::String | ConstType::Vector);
                let numeric_base = wanted != "string" && wanted != "vector";
                if numeric != numeric_base || (!numeric && got != wanted) {
                    self.report(
                        Code::E0227,
                        location,
                        format!("Implicit type mismatch. Needed {wanted}, got {got}."),
                    );
                    return None;
                }
                number.map(|bits| f64::from(f32::from_bits(bits)))
            }
            ValueKind::Name(name) => {
                let def = self.lookup_global(name)?;
                let constant =
                    self.fact(def).constant && self.index.def(def).is_some_and(|d| d.initialized);
                if !constant {
                    self.report(
                        Code::E0228,
                        location,
                        format!("enum - {name} is not a compile-time constant"),
                    );
                    return None;
                }
                match self.literals.get(&def) {
                    Some(LiteralValue::Float(bits)) => Some(f64::from(f32::from_bits(*bits))),
                    Some(_) | None => None,
                }
            }
            ValueKind::Translate(_) | ValueKind::Other => None,
        }
    }

    /// Records an FTEQCC class and declares its members (FTEF-17.1, FTEF-17.2). A member is a
    /// definition of the class, not a global by its name.
    fn declare_class(&mut self, file: FileId, def: DefId, class: &'a RawClass) -> Vec<Pending<'a>> {
        let name = self
            .index
            .def(def)
            .map(|d| d.name.clone())
            .unwrap_or_default();
        self.class_names.entry(name.clone()).or_insert(def);
        self.declare_spawnfunc(def, &name);
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
                    generated: false,
                });
                if let Some(info) = self.classes.get_mut(&def) {
                    info.members.insert(member.name.clone(), id);
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

    /// FTEF-9.5: every class defines `spawnfunc_Name`, so maps can spawn it; FTEQCC writes its
    /// body when the program writes none, so it is never undefined.
    fn declare_spawnfunc(&mut self, class: DefId, name: &str) {
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
    fn define_member(
        &mut self,
        file: FileId,
        scope: &str,
        decl: &'a RawDecl,
        location: Location,
    ) -> Vec<Pending<'a>> {
        let Some(class) = self.class_names.get(scope).copied() else {
            let name = format!("{scope}::{}", decl.name);
            if self.structs.contains(scope) {
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
        let facts = DeclFacts {
            constant: true,
            ..DeclFacts::default()
        };
        let def =
            self.declare_global_with(name, &void_fn, DefKind::Function, location, None, &facts);
        // The name in the header is a reference to the think function.
        self.index.refs.push(Reference { location, def });
    }

    fn resolve_pending(&mut self, (file, def, class, body, decl): Pending<'a>) {
        // A body autoprototyping defers is still read in its own unit (FTEF-19.12).
        self.unit = file;
        self.resolve_body(file, def, class, body, decl);
    }

    /// Resolves one function body: its parameters, its state header, its locals and name
    /// uses (QC-12.1, QC-12.5), in the class `class` when it is a member function's
    /// (FTEF-17.2). `decl` is the declaration the body defines, whose state header names the
    /// next think function.
    fn resolve_body(
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
        };
        if self.fteqcc {
            let function = self
                .index
                .def(owner)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            for (number, span) in &body.unnamed_params {
                let location = self.at(file, *span);
                self.report(
                    Code::W0222,
                    location,
                    format!("Parameter {number} of {function} is not named"),
                );
            }
        }
        let mut previous: Option<DefId> = None;
        for param in &body.params {
            let location = self.at(file, param.span);
            let def = self.declare_scoped(scope, &param.name, &param.ty, DefKind::Param, location);
            // QC-13.5: `PR_ParseImmediateStatements` stops at a parameter whose global lies
            // below the one before it, "bad parm order".
            if !self.fteqcc
                && let (Some(def), Some(before)) = (def, previous)
                && self.below(def, before)
            {
                self.report(Code::E0404, location, "bad parm order".to_owned());
            }
            previous = def;
        }
        // FTEF-19.14: the state header's frame is read before its next function is declared.
        self.resolve_uses(file, Some(scope), &body.state_uses);
        if self.fteqcc {
            self.check_state_literals(file, body);
        }
        if let Some(decl) = decl
            && let Some((next_name, next_span)) = &decl.next_think
        {
            self.declare_next_think(file, next_name, *next_span);
        }
        // QC-11.10, QC-11.9: locals; a local field or function is an error (QCD-3.8 is the
        // parser's; the semantic check is QC-11.9).
        for local in &body.locals {
            let location = self.at(file, local.span);
            if local.bad_field_or_function {
                self.report(
                    Code::E0208,
                    location,
                    "Fields and functions must be global".to_owned(),
                );
                continue;
            }
            self.declare_local(file, scope, local, location);
        }
        // QC-12.5: resolve each name use against globals and this function's own locals.
        self.resolve_uses(file, Some(scope), &body.uses);
    }

    /// FTEF-19.14: FTEQCC converts a state header's frame to a float, "Implicit type
    /// mismatch" for a string or vector, and requires its next function to be a `void()`,
    /// "Type mismatch" for a literal but 0.
    fn check_state_literals(&mut self, file: FileId, body: &RawBody) {
        for (part, value) in &body.state_literals {
            let ValueKind::Literal(literal) = &value.kind else {
                continue;
            };
            let location = self.at(file, value.span);
            match part {
                StatePart::Frame => {
                    if matches!(literal.ty, ConstType::String | ConstType::Vector) {
                        self.report(
                            Code::E0227,
                            location,
                            format!(
                                "Implicit type mismatch. Needed float, got {}.",
                                literal.ty.name()
                            ),
                        );
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
    }

    /// Whether the global `def` lies below `before` in `qcc`'s global area (QC-13.5): the
    /// area grows as definitions are made, so a definition made earlier lies lower, but for a
    /// vector and its `_x` component, which share a word.
    fn below(&self, def: DefId, before: DefId) -> bool {
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
                Found::Def(def) => {
                    if let Some(f) = self.fact_mut(def) {
                        f.referenced = true;
                    }
                    if self.fteqcc
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
                    self.index.refs.push(Reference { location, def });
                }
                Found::Nothing => {}
                Found::Error(code, message) => self.report(code, location, message),
                Found::Unknown => {
                    let shown = match &use_.scope {
                        Some(prefix) => format!("{prefix}::{}", use_.name),
                        None => use_.name.clone(),
                    };
                    self.report(Code::E0301, location, format!("Unknown value \"{shown}\""));
                }
            }
        }
    }

    /// A plain name used as a value: the function's own, then in a member function the
    /// class's members (FTEF-17.2), then a global (QC-12.2, FTE-22.1); under FTEQCC then an
    /// intrinsic (FTEF-17.4), `this` and `super` only in a member function (FTEF-19.16).
    fn resolve_value(&self, scope: Option<Scope>, name: &str) -> Found {
        let def = match scope {
            Some(scope) => self.lookup_in_scope(scope, name),
            None => self.lookup_global(name),
        };
        if let Some(def) = def {
            return Found::Def(def);
        }
        if !self.fteqcc || !is_intrinsic(name) {
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

    /// Declares a local of a function body (QC-11.10, FTEF-19.1), with its initialiser's
    /// checks. Under FTEQCC an `autocvar_` local is a global (FTEF-19.16), and a `static` or
    /// `const` local is a constant whose initialiser FTEQCC compares as a global's.
    fn declare_local(&mut self, file: FileId, scope: Scope, local: &RawLocal, location: Location) {
        if self.fteqcc && local.name.starts_with("autocvar_") {
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
        let constant = self.fteqcc && local.has(&Qualifier::Const);
        let compared = constant || (self.fteqcc && local.has(&Qualifier::Static));
        let holder = if !self.fteqcc {
            Holder::Local(scope)
        } else if local.has(&Qualifier::Static) {
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
                statics: local.has(&Qualifier::Static),
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
        if !self.fteqcc && initialised_before {
            self.report(Code::E0206, location, format!("{} redeclared", local.name));
            return;
        }
        let Ok(value) =
            self.check_value(file, &local.name, &local.ty, local.value.as_ref(), holder)
        else {
            return;
        };
        let literal = value.or_else(|| local.literal.clone());
        let tracked = !self.fteqcc || compared;
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
    fn declare_scoped(
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
    fn declare_scoped_with(
        &mut self,
        scope: Scope,
        name: &str,
        ty: &Type,
        kind: DefKind,
        location: Location,
        facts: ScopedFacts,
    ) -> Option<DefId> {
        let existing = self.lookup_local(scope, name).or_else(|| {
            if self.fteqcc {
                None
            } else {
                self.lookup_global(name)
            }
        });
        if let Some(existing) = existing {
            let def = self.index.defs.get(existing.0)?;
            let global = def.kind.is_global();
            if !self.same_type(&def.ty, ty) {
                let old = def.ty.clone();
                self.mismatch(name, location, &old, ty);
                return None;
            }
            if self.fteqcc && !facts.component {
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
                let was_static = self.fact(existing).static_local;
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
            } else if self.fteqcc && kind == DefKind::Param {
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
        if self.fteqcc
            && !facts.component
            && let Some(global) = self.lookup_global(name)
            && self.index.def(global).is_some_and(|g| {
                g.ty.fteqcc_kind() == ty.fteqcc_kind() && ty.fteqcc_kind().is_some()
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
        }
        // QC-11.7: a new vector local or parameter has its components too.
        if facts.array.is_none()
            && let Some(component) = component_type(ty)
        {
            for suffix in COMPONENTS {
                let component_name = format!("{name}{suffix}");
                let component_facts = ScopedFacts {
                    component: true,
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
            // A class's name is the type, not its constructor member (FTEF-17.2).
            let member = || {
                scope
                    .class
                    .filter(|_| !self.class_names.contains_key(name))
                    .and_then(|c| self.member_in(c, name))
            };
            self.lookup_local(scope, name)
                .or_else(member)
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
    /// Under FTEQCC one marked `noref`, by its qualifier or `#pragma noref 1`, that the module
    /// does not refer to is not reported (FTEF-17.11): FTEQCC's check skips a definition `if
    /// (d->unused && !d->used)`.
    fn check_undefined_functions(&mut self) {
        let used: HashSet<DefId> = self.index.refs.iter().map(|r| r.def).collect();
        let mut undefined: Vec<FileDiagnostic> = Vec::new();
        for (index, def) in self.index.defs.iter().enumerate() {
            let id = DefId(index);
            let skipped = self.fteqcc && self.unused.contains(&id) && !used.contains(&id);
            if def.kind == DefKind::Function
                && !def.initialized
                && !def.generated
                && def.member_of.is_none()
                && !skipped
            {
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

/// What FTEQCC's rules read of a declaration when they make or redeclare its definition.
#[derive(Default)]
struct DeclFacts<'d> {
    /// A vector's component (QC-11.7).
    component: bool,
    /// Whether the caller made the type and array checks of a redeclaration already.
    checked: bool,
    /// `GDF_CONST` (FTEF-19.3).
    constant: bool,
    /// The first array dimension (FTEF-19.8).
    array: Option<ArrayDim>,
    /// The function type's parameters (FTEF-19.6).
    params: &'d [RawParamInfo],
    /// The unit of a `static` (FTEF-19.12).
    statics: Option<FileId>,
    /// The `deprecated` reason (FTEF-19.13).
    deprecated: Option<&'d str>,
}

/// What FTEQCC's rules read of a parameter or local.
#[derive(Clone, Copy, Default)]
struct ScopedFacts {
    /// A vector's component (QC-11.7).
    component: bool,
    /// The first array dimension (FTEF-19.8).
    array: Option<ArrayDim>,
    /// `static` (FTEF-19.12).
    statics: bool,
}

/// A fatal error reported: FTEQCC's `QCC_PR_ParseError`, after which it reads nothing more of
/// the declaration.
struct Fatal;

/// What holds a definition an initialiser initialises, which decides FTEF-19.3's constness
/// check.
#[derive(Clone, Copy)]
enum Holder {
    /// A global.
    Global,
    /// A `static` local.
    Static(Scope),
    /// A `const` local.
    Const(Scope),
    /// Any other local, whose initialiser is only cast (FTEF-19.3).
    Local(Scope),
}

/// What a name use resolves to.
enum Found {
    /// A definition: the use is a reference to it.
    Def(DefId),
    /// No definition, and no error: a member that is no definition, or an intrinsic.
    Nothing,
    /// Nothing, with an error of its own (FTEF-19.16).
    Error(Code, String),
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
