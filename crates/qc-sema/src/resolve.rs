// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, PR_GetDef, PR_ParseDefs, PR_ParseImmediateStatements, PR_ParseState, PR_ParseValue
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_GetDef, QCC_PR_ParseDefs, QCC_PR_GenerateInitializerType, QCC_PR_ParseInitializerType_Internal, QCC_PR_ParseDefaultInitialiser, QCC_PR_ParseEnum, QCC_PR_ParseImmediateStatements, QCC_PR_ParseState, QCC_PR_ParseRefValue
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, typecmp, typecmp_lax, QCC_TypeForName
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, QCC_PR_FinishCompilation

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
use qc_preproc::{CompilerFlags, FteFlag, TargetFormat, WarningSettings};
use qc_syntax::c_scalar::{CScalar, REGISTERED_TYPES};
use qc_syntax::{Dialect, Keyword, Keywords};

use crate::cast::{Cast, ConstType, Kind, Source, Supply};
use crate::expand::{OriginMap, ProtoPlan, settings_at};
use crate::lower::{
    ArrayDim, BuiltinFunction, EnumItem, ExternBase, FileDecls, LiteralValue, MemberBase,
    MemberKind, Qualifier, RawAlias, RawBody, RawClass, RawDecl, RawLocal, RawParamInfo, RawStruct,
    RawUse, RawValue, StatePart, StructMember, TypeKind, UseKind, ValueKind, top_level_kind,
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
    /// FTEQCC's `-Fcaseinsens` (FTEF-19.33): a name no definition has is looked up again in
    /// any case. Only meaningful with an FTEQCC dialect.
    pub case_insensitive: bool,
    /// Whether `qcfuncs` was off when FTEQCC named its core types, by the command line's
    /// `-Fno-qcfuncs` (FTEF-19.28): then `string`, `vector` and `entity` are no type names.
    pub qcfuncs_off: bool,
    /// FTEQCC's output format once every file is read, which `QCC_PR_FinishCompilation` reads
    /// for `extern` functions (FTEF-22.5); `None` when the module was not preprocessed, which
    /// checks none.
    pub target: Option<TargetFormat>,
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
    /// For each entry, under FTEQCC, the warning settings and the offsets they hold from
    /// (FTEF-19.22).
    pub(crate) warnings: &'a [&'a [(usize, WarningSettings)]],
    /// For each entry, the keywords its parser read each token with, from each offset
    /// (FTEF-20.1).
    pub(crate) keywords: &'a [&'a [(usize, Keywords)]],
    /// The source files whose name ends in `extensions.qc`, where FTEQCC keeps a function's
    /// first declaration (FTEF-19.7).
    pub(crate) extensions: HashSet<FileId>,
    /// What the first pass prototypes (FTEF-12.18): with a second pass the first pass's
    /// [`Self::first`], else the only pass. `None` when it prototypes nothing.
    pub(crate) plan: Option<&'a ProtoPlan>,
    /// With a second pass, the first pass's files, as it read them. Without it the first pass
    /// is the files themselves, all prototyped.
    pub(crate) first: Option<FirstPass<'a>>,
}

/// The first pass of a two-pass compile, read with the flags, warning settings and keywords
/// the first pass had (FTEF-12.11, FTEF-14.72).
pub(crate) struct FirstPass<'a> {
    /// The lowered declarations of each compiled file.
    pub(crate) files: &'a [FileDecls],
    /// Where each span of each file's rendered text came from.
    pub(crate) maps: &'a [&'a OriginMap],
    /// The warning settings of each file.
    pub(crate) warnings: &'a [&'a [(usize, WarningSettings)]],
    /// The keywords of each file.
    pub(crate) keywords: &'a [&'a [(usize, Keywords)]],
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

/// Whether FTEQCC's `autoprototype` is on where a pass reads a declaration (FTEF-12.18).
#[derive(Clone, Copy)]
enum Prototyping<'a> {
    /// Never: the second pass, or a module without autoprototyping.
    Off,
    /// Always: the first pass of a module that autoprototypes from its first token.
    On,
    /// As the pragmas of the first pass set it.
    Plan(&'a ProtoPlan),
}

impl Prototyping<'_> {
    /// Whether it is on at the rendered offset `at` of compiled file `file`.
    fn at(self, file: usize, at: usize) -> bool {
        match self {
            Self::Off => false,
            Self::On => true,
            Self::Plan(plan) => plan.proto_at(file, at),
        }
    }
}

/// [`resolve_module`] for files the parser read after preprocessing, located through
/// `sources`, so every location in the index is a span of a source file.
///
/// With autoprototyping FTEQCC reads the module twice (FTE-22.3, FTEF-12.18): the first pass,
/// where `autoprototype` is on, only declares what it reads, and the second compiles it all
/// with every name declared. A `#pragma autoproto` turns the flag on part way through the first
/// pass, so what it reads before is compiled in full, and again in the second pass.
pub(crate) fn resolve_mapped(
    files: &[FileDecls],
    sources: &Sources<'_>,
    options: ResolveOptions,
) -> ModuleIndex {
    let fteqcc = options.dialect.has_fteqcc_extensions();
    let two = fteqcc && options.autoproto;
    let plan = match sources.plan {
        Some(plan) if fteqcc => Prototyping::Plan(plan),
        Some(_) | None => Prototyping::Off,
    };
    // The first pass of two is the files themselves, all prototyped, unless the preprocessor
    // kept it.
    let first_files = sources.first.as_ref().map_or(files, |first| first.files);
    let first_plan = if two && sources.first.is_none() {
        Prototyping::On
    } else {
        plan
    };
    let prototyped = if two || matches!(plan, Prototyping::Plan(_)) {
        prototyped(first_files, first_plan)
    } else {
        Vec::new()
    };
    let mut r = Resolver {
        index: ModuleIndex::default(),
        maps: sources.maps,
        warnings: sources.warnings,
        settings: None,
        keyword_marks: sources.keywords,
        keywords: Keywords::default(),
        extensions: &sources.extensions,
        fteqcc,
        autoproto: false,
        case_insensitive: fteqcc && options.case_insensitive,
        qcfuncs_off: fteqcc && options.qcfuncs_off,
        target: options.target.filter(|_| fteqcc),
        externs: HashSet::new(),
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
        enum_values: HashMap::new(),
        enum_redeclared: false,
        struct_defs: HashMap::new(),
        type_defs: HashSet::new(),
        body_typedefs: Vec::new(),
        var_members: HashSet::new(),
        accessor_bases: HashMap::new(),
        redefinable: HashSet::new(),
        unused: HashSet::new(),
        bodies: HashSet::new(),
        enum_next: EnumCursor::Start,
        type_names: HashSet::new(),
    };
    if two {
        // FTE-22.3: the first pass reads only the definitions, so the second, which compiles
        // the bodies, finds every top-level name already declared.
        match &sources.first {
            Some(first) => {
                r.maps = first.maps;
                r.warnings = first.warnings;
                r.keyword_marks = first.keywords;
                r.pass(first.files, &prototyped, false);
                r.maps = sources.maps;
                r.warnings = sources.warnings;
                r.keyword_marks = sources.keywords;
            }
            None => r.pass(files, &prototyped, false),
        }
        let first_pass = (r.index.diagnostics.len(), r.index.refs.len());
        r.pass(files, &[], true);
        r.drop_repeats(first_pass.0);
        r.drop_repeated_refs(first_pass.1);
    } else {
        r.pass(files, &prototyped, true);
    }
    r.finish()
}

/// Each declaration of `files` as FTEQCC reads it with `autoprototype` as `proto` says: only the
/// ones it is on for, prototyped (FTEF-12.18), the others `None`.
fn prototyped(files: &[FileDecls], proto: Prototyping<'_>) -> Vec<Vec<Option<RawDecl>>> {
    files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            file.decls
                .iter()
                .map(|decl| proto.at(index, decl.span.start).then(|| decl.prototyped()))
                .collect()
        })
        .collect()
}

/// The names `QCC_main` registers with `QCC_PR_NewType` that are in the table `QCC_TypeForName`
/// searches and are no `QCC_PR_ParseType` C word (FTEF-19.20, FTEF-19.33): found in any case under
/// `-Fcaseinsens`, as `Hash_GetInsensitive` finds them. `string`, `vector` and `entity` are there
/// only while `qcfuncs` is on when they are named (FTEF-19.28).
const TABLE_TYPES: [&str; 17] = [
    "void",
    "float",
    "__double",
    "__int32",
    "__uint32",
    "__int64",
    "__uint64",
    "__variant",
    "__int8",
    "__uint8",
    "__int16",
    "__uint16",
    "__bfloat",
    "__bint",
    "variant",
    "__int",
    "__uint",
];

/// What the resolver knows of an FTEQCC class (FTEF-9).
#[derive(Debug)]
struct ClassInfo {
    /// The parent class's name, if any.
    parent: Option<String>,
    /// The class's own members by name.
    members: HashMap<String, DefId>,
    /// The members of the body given, in order, each with its type and array suffix, once a
    /// body was (FTEF-9.13).
    shape: Option<Vec<(String, Type, Option<String>)>>,
}

/// What `QCC_PR_FindStructMember` finds for a member access on a struct value (FTEF-6.11).
#[derive(Clone, Debug, PartialEq, Eq)]
enum StructMatch {
    /// The access is made on no struct value the model knows.
    NotStruct,
    /// One member, in the struct or a parent.
    One,
    /// More than one member of the named struct matches.
    Several(String),
    /// No member of the named struct or its parents.
    None(String),
}

/// A struct or union `QCC_PR_ParseStruct` made (FTEF-6.10).
#[derive(Clone, Debug)]
struct StructDef {
    /// The tag as the body wrote it, which `typecmp_strict` compares with `STRCMP` (FTEF-6.10).
    name: String,
    /// The own members of its body.
    members: Vec<StructMember>,
    /// The parent's name, as the type table keys it.
    parent: Option<String>,
    /// Whether a body defined it, else a tag named it.
    defined: bool,
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
    /// For a local a `for` statement defines, the byte offset where the statement ends and
    /// the local leaves `localstable` (FTEF-19.19).
    scope_end: Option<usize>,
    /// For a `using` alias, the byte offset of its name, before which it is not found
    /// (FTEF-20.5).
    scope_start: Option<usize>,
}

struct Resolver<'a> {
    index: ModuleIndex,
    /// Where the spans of each file's rendered text came from; empty when the files were
    /// parsed as written.
    maps: &'a [&'a OriginMap],
    /// For each compiled file, the warning settings and the offsets they hold from
    /// (FTEF-19.22).
    warnings: &'a [&'a [(usize, WarningSettings)]],
    /// Under FTEQCC, the warning settings in force where the resolver reads (FTEF-19.22).
    settings: Option<WarningSettings>,
    /// For each compiled file, the keywords its parser read each token with (FTEF-20.1).
    keyword_marks: &'a [&'a [(usize, Keywords)]],
    /// The keywords in force where the resolver reads (FTEF-20.1).
    keywords: Keywords,
    /// The source files named `*extensions.qc` (FTEF-19.7).
    extensions: &'a HashSet<FileId>,
    /// Whether an FTEQCC profile is in force (FTE-22).
    fteqcc: bool,
    /// Whether FTEQCC autoprototypes the module (FTE-22.3).
    autoproto: bool,
    /// Whether FTEQCC compiles without case sensitivity (FTEF-19.33).
    case_insensitive: bool,
    /// Whether FTEQCC named its core types with `qcfuncs` off (FTEF-19.28).
    qcfuncs_off: bool,
    /// Under FTEQCC, the output format `QCC_PR_FinishCompilation` reads (FTEF-22.5).
    target: Option<TargetFormat>,
    /// The functions an `extern` at file level made, `def->isextern` (FTEF-22.5).
    externs: HashSet<DefId>,
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
    /// The value of each name of a named enum, by the enum's name and the name's (FTEF-19.36).
    enum_values: HashMap<(String, String), f64>,
    /// Whether the enum being declared names a type already, so `QCC_PR_ParseEnum` reads its
    /// name as the base type and keeps no names under it (FTEF-19.10).
    enum_redeclared: bool,
    /// The structs and unions defined so far, by name (FTEF-6.10).
    struct_defs: HashMap<String, StructDef>,
    /// The definitions that stand for a type's name, which are no values (FTEF-19.38).
    type_defs: HashSet<DefId>,
    /// The `typedef`s of the body being resolved, each with where it stands (FTEF-19.20).
    body_typedefs: Vec<(String, usize)>,
    /// The `var` members of each class, by the class and the member's name (FTEF-19.41).
    var_members: HashSet<(DefId, String)>,
    /// The base type of each accessor (FTEF-10.6).
    accessor_bases: HashMap<String, Type>,
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
    /// The value the next name of the enum being read gets when it is given none
    /// (FTEF-19.10).
    enum_next: EnumCursor,
    /// The names declared as types so far: classes, structs, unions, typedefs, enums and
    /// accessors (FTEF-19.20).
    type_names: HashSet<String>,
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

    /// Reports `code` at `location`, as the warning settings in force have it (FTEF-19.22): a
    /// warning they ignore is not reported, and one they make an error is `E0125`.
    fn report(&mut self, code: Code, location: Location, message: String) {
        let code = match self.settings {
            Some(settings) if self.fteqcc => match settings.reported_code(code) {
                Some(code) => code,
                None => return,
            },
            Some(_) | None => code,
        };
        self.index.diagnostics.push(diag(code, location, message));
    }

    /// FTEF-19.22: takes the warning settings in force at `at` in the compiled file
    /// `self.unit`, as FTEQCC's `qccwarningaction` stands when it reads there.
    fn settle(&mut self, at: usize) {
        if let Some(events) = self.warnings.get(self.unit.0) {
            self.settings = settings_at(events, at).or(self.settings);
        }
        // FTEF-20.1: and the keywords the parser read there.
        if let Some(marks) = self.keyword_marks.get(self.unit.0) {
            self.keywords = Keywords::at(marks, at);
        }
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

    fn fact_mut(&mut self, def: DefId) -> Option<&mut Facts> {
        self.facts.get_mut(def.0)
    }

    /// Adds a definition and returns its id. A class member is not a global by its name.
    fn push_def(&mut self, def: Definition) -> DefId {
        // FTEF-20.4: `QCC_PR_DummyDef` warns of a definition named as a keyword that is on,
        // which keeps its meaning beside it with `kce` on and is turned off with it off.
        // A class's members are made by `QCC_PR_ParseEntClass`, not as definitions named so.
        if self.fteqcc
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

    /// What `table` holds for `name`: the entry written so, else under `-Fcaseinsens` the one
    /// whose name differs only in case, as `Hash_GetInsensitive` finds it (FTEF-19.33).
    fn named<'t, V>(&self, table: &'t HashMap<String, V>, name: &str) -> Option<&'t V> {
        table.get(name).or_else(|| {
            if !self.case_insensitive {
                return None;
            }
            table
                .iter()
                .filter(|(key, _)| key.eq_ignore_ascii_case(name))
                .max_by(|a, b| a.0.cmp(b.0))
                .map(|(_, value)| value)
        })
    }

    /// Whether `table` has an entry for `name` ([`Self::named`]).
    fn has_name_key<V>(&self, table: &HashMap<String, V>, name: &str) -> bool {
        self.named(table, name).is_some()
    }

    /// The type name the program defined that `name` is, in any case under `-Fcaseinsens`
    /// (FTEF-19.33): the one written so, else the greatest of those that differ in case.
    fn type_name_spelled(&self, name: &str) -> Option<&str> {
        if let Some(exact) = self.type_names.get(name) {
            return Some(exact.as_str());
        }
        if !self.case_insensitive {
            return None;
        }
        self.type_names
            .iter()
            .filter(|t| t.eq_ignore_ascii_case(name))
            .max()
            .map(String::as_str)
    }

    /// Whether `set` holds `name`, in any case under `-Fcaseinsens` (FTEF-19.33).
    fn has_name(&self, set: &HashSet<String>, name: &str) -> bool {
        set.contains(name)
            || (self.case_insensitive && set.iter().any(|t| t.eq_ignore_ascii_case(name)))
    }

    /// A type with the typedef names it holds resolved to the types they name (FTEF-19.9),
    /// as `QCC_TypeForName` gives a typedef's real type.
    fn resolved(&self, ty: &Type) -> Type {
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
    fn resolved_deep(&self, ty: &Type, depth: usize) -> Type {
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
            })),
            Type::Unknown | Type::Simple(_) | Type::Named(_) => ty,
        }
    }

    /// Whether a redeclaration's type `ty` matches the earlier `old` (QC-11.5): equal, or
    /// either unknown; under FTEQCC also when the model does not follow either (FTEF-17.13).
    fn same_type(&self, old: &Type, ty: &Type) -> bool {
        if self.fteqcc {
            self.resolved(old).fteqcc_same(&self.resolved(ty))
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
        if !self.fteqcc && field_component != component && kind == DefKind::Field {
            self.mismatch(r.name, r.location, &old, r.ty);
            return TypeCheck::Mismatch;
        }
        let same = self.same_type(&old, r.ty) && (!self.fteqcc || defaults_ok);
        if same {
            return TypeCheck::Same;
        }
        if !self.fteqcc {
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
        !decl.name.starts_with("autocvar_")
            && Self::constness(decl, decl.initialized || decl.skipped_init)
    }

    /// FTEF-19.3: the constness `QCC_PR_ParseDefs` gives a file-level declaration, initialised
    /// or not: `const`, or not `var` and initialised, a function or a field; FTEF-21.11: an
    /// initialised one not under `assumevar`; FTEF-21.7: C's function pointer is a `var`.
    fn constness(decl: &RawDecl, initialised: bool) -> bool {
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
    fn declare_special(
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
        if self.fteqcc
            && let Some(item) = &decl.enum_item
        {
            self.declare_enum_item(file, decl, item, location);
            return Special::Declared(Vec::new());
        }
        if self.fteqcc && self.check_type_names(&decl.ty, &[], decl.span.start, location) {
            return Special::Declared(Vec::new());
        }
        let qualifier_fatal = self.fteqcc && self.warn_qualifiers(decl, location);
        if let Some(scope) = &decl.scope
            && self.fteqcc
        {
            return Special::Declared(self.define_member(file, scope, decl, location));
        }
        if self.fteqcc
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
    fn declare_top_level(&mut self, file: FileId, decl: &'a RawDecl) -> Vec<Pending<'a>> {
        let location = self.at(file, decl.span);
        let qualifier_fatal = match self.declare_special(file, decl, location) {
            Special::Declared(pending) => return pending,
            Special::Plain { qualifier_fatal } => qualifier_fatal,
        };
        if self.fteqcc {
            self.warn_autocvar(decl, location);
        }
        // FTEF-6.10: a field whose type is a struct with no body so far reads the struct's size
        // that is not there ("type NAME not fully defined yet").
        if self.fteqcc
            && let Type::Field(inner) = &decl.ty
            && self.incomplete_struct(inner, None)
        {
            let message = format!("type {} not fully defined yet", inner.fteqcc_name());
            self.report(Code::E0255, location, message);
            return Vec::new();
        }
        // FTE-11, FTEF-15.5: a `var` of function type holds a function; it defines none.
        let kind = if (decl.has(&Qualifier::Var) || decl.pointer_var) && decl.ty.is_function() {
            DefKind::Global
        } else {
            top_level_kind(&self.resolved(&decl.ty))
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
        if self.fteqcc && !aborted {
            aborted |= self.check_initialisation(decl, existing, location);
        }
        if self.fteqcc && !aborted {
            aborted |= self.check_array_value(file, decl.array, &decl.ty, decl.value.as_ref());
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
        let external = self.is_extern(decl) && self.resolved(&decl.ty).is_function();
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
        let statics = (self.fteqcc && decl.is_static()).then_some(self.unit);
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
        // FTEF-22.5: `if (externfnc && !pr_scope) { ... def->isextern = true; }`.
        // `QCC_PR_FinishCompilation` checks the constant ones, every function but a `var` one.
        let constant = !decl.has(&Qualifier::Var) && !decl.pointer_var;
        if self.fteqcc && external && constant && !aborted {
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
    fn warn_constness_change(
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

    /// Whether `QCC_PR_ParseDefs` keeps the `extern` of a declaration whose base type is `base`
    /// (FTEF-21.10): when the type, its typedefs read, is a function's, or one the model does not
    /// know.
    fn extern_kept(&self, base: &ExternBase) -> bool {
        let ty = self.resolved(&base.ty);
        ty.is_function() || ty == Type::Unknown
    }

    /// Whether `decl` is `extern` once `QCC_PR_ParseDefs` has tested it against its base type
    /// (FTEF-21.10).
    fn is_extern(&self, decl: &RawDecl) -> bool {
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
    fn warn_qualifiers(&mut self, decl: &RawDecl, location: Location) -> bool {
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

    /// FTEF-19.31, FTEF-19.3, QC-11.3: the checks of a value initialiser of a definition of
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
        // FTEF-6.12: a struct is initialised by an aggregate only; a struct value is no other
        // type's, and neither is another type's a struct's: "Cannot cast from X to Y".
        if self.fteqcc {
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
    fn check_named_value(
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

    /// FTEF-19.8: an array is initialised with `{ ... }`, or an array of `char` with a string;
    /// `QCC_PR_ParseInitializerType_Internal` takes anything else for "expected {, found X",
    /// a fatal error. Returns whether it was reported.
    fn check_array_value(
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
    fn cast_value(
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

    /// `accessor Name : Base` (FTEF-10.6): the base is a type and no struct or union, and the
    /// same one each time; a first declaration has one.
    fn declare_accessor(&mut self, decl: &RawDecl, location: Location) {
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
        // FTEF-19.37: the base type after `:` is a type.
        if item.first && self.check_type_names(&item.base, &[], decl.span.start, location) {
            return;
        }
        if item.legacy {
            self.report(
                Code::W0224,
                location,
                "legacy enum base type. Use \"enum [class] [name_e]:type\" instead".to_owned(),
            );
        }
        let base = self.resolved(&item.base);
        // FTEF-19.44: a base that is a class, a struct or a union, as the legacy form reads a
        // class's name, is no number: "values for enums of this type must be initialised".
        let class_base = self.enum_redeclared
            && item.enum_name.as_ref().is_some_and(|n| {
                self.has_name_key(&self.class_names, n)
                    || self.struct_defs.contains_key(&self.type_key(n))
            })
            || matches!(&base, Type::Named(n) if self.has_name_key(&self.class_names, n))
            || self.is_struct_type(&base);
        if class_base {
            self.report(
                Code::E0229,
                location,
                "values for enums of this type must be initialised".to_owned(),
            );
            return;
        }
        let numeric = base.fteqcc_kind().is_numeric();
        if item.first {
            self.enum_next = EnumCursor::Start;
        }
        let next = match self.enum_next {
            EnumCursor::Start => Some(if item.flags { 1.0 } else { 0.0 }),
            EnumCursor::Known(next) => Some(next),
            EnumCursor::Unknown => None,
        };
        let value: Option<f64> = match item.value.as_ref() {
            None => {
                if !numeric && base.fteqcc_kind() != Kind::Opaque {
                    self.report(
                        Code::E0229,
                        location,
                        "values for enums of this type must be initialised".to_owned(),
                    );
                    None
                } else {
                    next
                }
            }
            Some(value) => self.enum_value(file, value, item.number, next, &base),
        };
        if item.flags
            && let Some(v) = value
        {
            self.warn_enumflag(v, &base, location);
        }
        // FTEF-19.36: a name an enum gave again with another value is "NAME::ITEM already
        // declared".
        if let (Some(enum_name), Some(v), false) = (&item.enum_name, value, self.enum_redeclared) {
            let key = (
                if self.case_insensitive {
                    enum_name.to_ascii_lowercase()
                } else {
                    enum_name.clone()
                },
                decl.name.clone(),
            );
            #[expect(
                clippy::float_cmp,
                reason = "spec FTEF-19.36: FTEQCC compares the constants' values exactly"
            )]
            let differs = self.enum_values.get(&key).is_some_and(|old| *old != v);
            if differs {
                let message = format!("{enum_name}::{} already declared", decl.name);
                self.report(Code::E0251, location, message);
            }
            self.enum_values.entry(key).or_insert(v);
        }
        // A value the model does not compute leaves every later implicit one unknown.
        self.enum_next = value.map_or(EnumCursor::Unknown, |v| {
            if item.flags {
                // FTEF-19.10: `next_i <<= 1;` then `if (!next_i) next_f = next_i = 1;`, where
                // `next_i` holds the value's integer part.
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "spec FTEF-19.10: `next_i = next_f` keeps the value's integer part"
                )]
                let whole = v as i64;
                EnumCursor::Known(if whole.wrapping_shl(1) == 0 {
                    1.0
                } else {
                    v * 2.0
                })
            } else {
                EnumCursor::Known(v + 1.0)
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
        // The same declaration read by the other pass is one definition for navigation.
        if self
            .globals
            .get(&decl.name)
            .into_iter()
            .flatten()
            .any(|id| {
                self.index
                    .def(*id)
                    .is_some_and(|d| d.location == location && d.ty == decl.ty)
            })
        {
            return;
        }
        // FTEF-17.10: a new definition each time, never a redeclaration, which the name's
        // lookups find before the older ones (`Hash_Add`, FTEF-19.33).
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
        if let Some(newer) = self.globals.get_mut(&decl.name)
            && let Some(last) = newer.pop()
        {
            newer.insert(0, last);
        }
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
        let fraction = base.fteqcc_kind() != Kind::Integer && whole as f64 != value;
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

    /// The value after an enum name's `=` (FTEF-19.10): converted to the base type by
    /// `QCC_SupplyConversion` (FTEF-22.3), then a number literal's value or a numeric
    /// constant's, and a name that is no constant "is not a compile-time constant". A value
    /// `lax` let through unconverted leaves the name the value it would have had with none,
    /// `next`, as no branch of `QCC_PR_ParseEnum` reads its type.
    fn enum_value(
        &mut self,
        file: FileId,
        value: &RawValue,
        number: Option<u32>,
        next: Option<f64>,
        base: &Type,
    ) -> Option<f64> {
        let location = self.at(file, value.span);
        let kind = base.fteqcc_kind();
        let wanted = if kind == Kind::Opaque {
            Kind::Float
        } else {
            kind
        };
        match &value.kind {
            ValueKind::Literal(literal) => {
                match self.supply_value(Source::Literal(*literal), wanted, value, location) {
                    Supply::Converted => number.map(|bits| f64::from(f32::from_bits(bits))),
                    Supply::Lax => next,
                    Supply::Explicit | Supply::Mismatch => None,
                }
            }
            ValueKind::Name(name) => {
                let def = self.lookup_global(name)?;
                let ty = self.resolved(&self.index.def(def)?.ty);
                let supply = self.supply_value(Source::Typed(&ty), wanted, value, location);
                if matches!(supply, Supply::Explicit | Supply::Mismatch) {
                    return None;
                }
                let constant = self.facts.get(def.0).is_some_and(|f| f.constant)
                    && self.index.def(def).is_some_and(|d| d.initialized);
                if !constant {
                    self.report(
                        Code::E0228,
                        location,
                        format!("enum - {name} is not a compile-time constant"),
                    );
                    return None;
                }
                if supply == Supply::Lax {
                    return next;
                }
                match self.literals.get(&def) {
                    Some(LiteralValue::Float(bits)) => Some(f64::from(f32::from_bits(*bits))),
                    Some(_) | None => None,
                }
            }
            ValueKind::Translate(_)
            | ValueKind::Indexed(_)
            | ValueKind::Cast(_)
            | ValueKind::Other => None,
        }
    }

    /// `QCC_SupplyConversion` of `source`, the value `value`, to a `wanted` (FTEF-22.3), with
    /// the flags at the token after it, and its diagnostic: "Automatic type conversions
    /// disabled." under `typeexplicit`, and "Implicit type mismatch." when nothing converts it,
    /// a warning under `lax`; each names the two kinds by `basictypenames`.
    fn supply_value(
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

    /// Records an FTEQCC class and declares its members (FTEF-17.1, FTEF-17.2). A member is a
    /// definition of the class, not a global by its name.
    fn declare_class(&mut self, file: FileId, def: DefId, class: &'a RawClass) -> Vec<Pending<'a>> {
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
            if member.bad_init {
                self.report(
                    Code::E0257,
                    location,
                    "Can only initialise member functions".to_owned(),
                );
            }
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

    /// The key `name` is found by in the type tables: itself, or in lower case under
    /// `-Fcaseinsens` (FTEF-19.33).
    fn type_key(&self, name: &str) -> String {
        if self.case_insensitive {
            name.to_ascii_lowercase()
        } else {
            name.to_owned()
        }
    }

    /// Whether a type named `name` has a size already, so a `struct` of that name given a body
    /// is compared with it (`if (newt->size)`, FTEF-6.10): a type FTEQCC registers, a class, an
    /// enum or a typedef.
    fn sized_type_named(&self, name: &str) -> bool {
        let key = self.type_key(name);
        if let Some(def) = self.struct_defs.get(&key) {
            return def.defined;
        }
        self.other_sized_type_named(name)
    }

    /// Whether `name` is a type that is no struct and has a size: one FTEQCC registers, a
    /// class, an enum or a typedef.
    fn other_sized_type_named(&self, name: &str) -> bool {
        let same = |other: &str| {
            if self.case_insensitive {
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

    /// `QCC_PR_ParseStruct` (FTEF-6.7 to FTEF-6.10): a struct's parent, its redefinition and its
    /// members. A struct a function's body writes lasts to the block's end and is not followed
    /// here.
    fn declare_struct(&mut self, file: FileId, st: &RawStruct) {
        if !self.fteqcc {
            return;
        }
        let location = self.at(file, st.span);
        let mut parent_key = None;
        if let Some((parent, _)) = &st.parent {
            // `QCC_TypeForName(parentname)` follows a typedef to the struct it names.
            let parent = match self.resolved(&Type::Named(parent.as_str().into())) {
                Type::Named(n) => n
                    .strip_prefix("struct ")
                    .or_else(|| n.strip_prefix("union "))
                    .unwrap_or(&n)
                    .to_owned(),
                _ => parent.clone(),
            };
            let parent = &parent;
            let key = self.type_key(parent);
            // The struct's own name is no type yet (`newt = QCC_TypeForName(tname)` comes after
            // the parent): `struct S : S` is "not yet defined".
            let is_struct = self.struct_defs.contains_key(&key)
                || (st.local
                    && self.type_names.contains(parent.as_str())
                    && !self.other_sized_type_named(parent));
            let own = st.name.as_deref().is_some_and(|n| self.type_key(n) == key);
            let known = is_struct
                || matches!(
                    parent.as_str(),
                    "void" | "float" | "vector" | "string" | "entity"
                )
                || (!own && self.is_type_name(parent, &[], st.at));
            if !known {
                self.report(
                    Code::E0247,
                    location,
                    format!("Parent type {parent} was not yet defined"),
                );
                return;
            }
            if !is_struct {
                self.report(
                    Code::E0248,
                    location,
                    format!("Parent type {parent} is not a struct"),
                );
                return;
            }
            parent_key = Some(key);
        }
        let name = st.name.as_deref();
        let key = name.map(|n| self.type_key(n));
        let known = key.as_ref().and_then(|k| self.struct_defs.get(k)).cloned();
        let sized = name.is_some_and(|n| self.sized_type_named(n));
        if let Some(def) = &known
            && (def.defined || def.parent.is_some())
            && let Some(parent_key) = &parent_key
            && def.parent.as_ref() != Some(parent_key)
        {
            self.report(
                Code::E0249,
                location,
                "Redeclaration of struct with different parent type".to_owned(),
            );
            return;
        }
        if !st.body {
            if let (Some(key), None) = (&key, &known) {
                self.struct_defs.insert(
                    key.clone(),
                    StructDef {
                        name: name.unwrap_or_default().to_owned(),
                        members: Vec::new(),
                        parent: parent_key,
                        defined: false,
                    },
                );
            }
            return;
        }
        let members: Vec<&StructMember> = st.members.iter().collect();
        self.check_struct_members(file, location, st, &members);
        // `oldparm = QCC_PR_FindStructMember(newt, parmname, &ofs, &bitofs)`: a member the
        // parent has with the name and type is the parent's, not counted, so a body of such
        // members alone "has no members" (FTEF-6.7).
        if let Some(parent_key) = &parent_key
            && !st.members.is_empty()
            && st
                .members
                .iter()
                .all(|m| self.parent_has_member(parent_key, m))
        {
            let kind = if st.union { "union" } else { "struct" };
            let message = format!("{kind} {} has no members", name.unwrap_or_default());
            self.report(Code::E0243, location, message);
            return;
        }
        let owned: Vec<StructMember> = st.members.clone();
        // `if (STRCMP(a->name, b->name)) return 1;`: the tags are compared exactly, so under
        // `-Fcaseinsens` a body given again under another spelling of the tag differs.
        let same = |old: &StructDef| {
            old.name == name.unwrap_or_default()
                && old.members.len() == owned.len()
                && old.members.iter().zip(&owned).all(|(a, b)| {
                    (&a.name, &a.ty, &a.array, a.bits, a.is_static)
                        == (&b.name, &b.ty, &b.array, b.bits, b.is_static)
                })
        };
        if sized
            && let Some(old) = known.as_ref()
            && !same(old)
        {
            let kind = if st.union { "union" } else { "struct" };
            self.report(
                Code::E0246,
                location,
                format!("{kind} {} redeclared differently", name.unwrap_or_default()),
            );
            return;
        }
        if sized && known.is_none() {
            // A type of another kind has the name, which no body of ours equals.
            let kind = if st.union { "union" } else { "struct" };
            self.report(
                Code::E0246,
                location,
                format!("{kind} {} redeclared differently", name.unwrap_or_default()),
            );
            return;
        }
        if let Some(key) = key {
            self.struct_defs.insert(
                key,
                StructDef {
                    name: name.unwrap_or_default().to_owned(),
                    members: owned,
                    parent: parent_key,
                    defined: true,
                },
            );
        }
    }

    /// Whether the struct of `parent_key`, or one of its parents, has a member named and typed
    /// as `member` is (`QCC_PR_FindStructMember`, FTEF-6.11).
    fn parent_has_member(&self, parent_key: &str, member: &StructMember) -> bool {
        let mut current = Some(parent_key.to_owned());
        for _ in 0..MAX_CLASS_DEPTH {
            let Some(key) = current else {
                return false;
            };
            let Some(def) = self.struct_defs.get(&key) else {
                return false;
            };
            if def.members.iter().any(|m| {
                let same_name = if self.case_insensitive {
                    m.name.eq_ignore_ascii_case(&member.name)
                } else {
                    m.name == member.name
                };
                same_name && m.ty == member.ty && m.array == member.array
            }) {
                return true;
            }
            current = def.parent.clone();
        }
        false
    }

    /// The checks of a struct's members (FTEF-6.7): each member's type is one, and a bitfield's
    /// is an integer's.
    fn check_struct_members(
        &mut self,
        file: FileId,
        location: Location,
        st: &RawStruct,
        members: &[&StructMember],
    ) {
        for member in members {
            if self.check_type_names(&member.ty, &[], st.at, location) {
                return;
            }
            // FTEF-6.9: the width is `QCC_PR_IntConstExpr()`'s, whose names are values and
            // whose literal is an integer ("Value is not an integer constant").
            self.resolve_uses(file, None, &member.width_uses);
            if !member.width_integer {
                self.report(
                    Code::E0262,
                    location,
                    "Value is not an integer constant".to_owned(),
                );
                return;
            }
            // FTEF-19.41: a `virtual` function member is `Struct::member`, a function with no
            // body unless one follows.
            if member.is_virtual && member.ty.is_function() && !member.initialized {
                let owner = st.name.as_deref().unwrap_or("<struct>");
                let message = format!("function {owner}::{} has no body", member.name);
                self.report(Code::E0211, location, message);
                return;
            }
            if member
                .bits
                .is_some_and(|width| width.is_some_and(|w| w > 64))
            {
                self.report(Code::E0250, location, "too many bits".to_owned());
                return;
            }
            if self.incomplete_struct(&member.ty, st.name.as_deref()) {
                let message = format!("type {} not fully defined yet", member.ty.fteqcc_name());
                self.report(Code::E0255, location, message);
                return;
            }
            if member.bits.is_some() && !self.bitfield_type(&member.ty) {
                self.report(
                    Code::E0250,
                    location,
                    "bitfields must be integer types".to_owned(),
                );
                return;
            }
        }
    }

    /// Whether `ty` is a struct or union a member cannot hold yet, one with no body so far or
    /// the one being defined (`type == newt || ((type->type == ev_struct || ev_union) &&
    /// !type->size)`, FTEF-6.7).
    fn incomplete_struct(&self, ty: &Type, own: Option<&str>) -> bool {
        let Type::Named(name) = ty else {
            return false;
        };
        let tagged = name
            .strip_prefix("struct ")
            .or_else(|| name.strip_prefix("union "))
            .map(str::trim);
        let bare = tagged.unwrap_or(name);
        if bare == "{}" {
            // A body with no tag, `struct { ... }`, has its size.
            return false;
        }
        let key = self.type_key(bare);
        if own.is_some_and(|own| self.type_key(own) == key) {
            return true;
        }
        match self.struct_defs.get(&key) {
            Some(def) => !def.defined,
            None => tagged.is_some() && !self.other_sized_type_named(bare),
        }
    }

    /// The struct or union a type name names, `struct S` or `S`, when it is one: written with
    /// its keyword, with or without a body (`QCC_PR_ParseStruct` returns the type of a tag,
    /// FTEF-6.8), or the name of one declared.
    fn struct_type_name<'n>(&self, name: &'n str) -> Option<&'n str> {
        let tagged = name
            .strip_prefix("struct ")
            .or_else(|| name.strip_prefix("union "));
        let bare = tagged.unwrap_or(name);
        if tagged.is_some() {
            // `QCC_TypeForName(tname)`: a name that is another sized type is that type, so
            // `struct float` is `float`.
            return (bare.starts_with('{') || !self.other_sized_type_named(bare)).then_some(bare);
        }
        self.struct_defs
            .contains_key(&self.type_key(bare))
            .then_some(bare)
    }

    /// Whether `ty` is a struct or union (FTEF-9.11, FTEF-6.12).
    fn is_struct_type(&self, ty: &Type) -> bool {
        matches!(ty, Type::Named(n) if self.struct_type_name(n).is_some())
    }

    /// What another kind of type named `name` is, for "Cannot redeclare typedef" (FTEF-19.9): a
    /// struct, a class, an accessor or an enum, found in any case under `-Fcaseinsens`.
    fn other_type_named(&self, name: &str) -> Option<String> {
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
            if self.case_insensitive {
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
    fn bitfield_type(&self, ty: &Type) -> bool {
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

    /// FTEF-9.7: the parent of a class is a type `QCC_TypeForName` finds, a class or an entity,
    /// and the same one each time the class is given its members.
    fn check_parent(&mut self, def: DefId, name: &str, class: &RawClass) {
        let Some(parent) = &class.parent else {
            return;
        };
        let Some(location) = self.index.def(def).map(|d| d.location) else {
            return;
        };
        let found = self.named(&self.class_names, parent).copied();
        let entity = parent == "__entity" || (parent == "entity" && !self.qcfuncs_off);
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
    fn member_in(&self, class: DefId, name: &str) -> Option<DefId> {
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
    fn parent_of(&self, class: DefId) -> Option<DefId> {
        self.classes
            .get(&class)?
            .parent
            .as_ref()
            .and_then(|p| self.named(&self.class_names, p).copied())
    }

    /// The class a type names, if it names one (FTEF-17.3).
    fn class_of(&self, ty: &Type) -> Option<DefId> {
        self.named(&self.class_names, ty.type_name()?).copied()
    }

    /// Declares, or checks, the next-think name of a state header (QC-13.6).
    ///
    /// If the name does not exist it is declared as a global `void()`. If it exists with a
    /// different type it is a type mismatch (QCD-3.5).
    fn declare_next_think(&mut self, file: FileId, class: Option<DefId>, name: &str, span: Span) {
        let void_fn = Type::Function(Rc::new(FunctionType {
            ret: Type::Simple(SimpleType::Void),
            params: Vec::new(),
            variadic: false,
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

    /// One FTEQCC pass over `files`, whose declarations `prototyped` replaces where it holds
    /// one (FTEF-12.18). The last pass is the one the module is compiled by: its bodies stay in
    /// the index and its lowering diagnostics are reported. An earlier pass leaves in the index
    /// only the declarations (FTEF-12.19), and the diagnostics it found, which the last pass
    /// finds again and [`Self::drop_repeats`] removes.
    fn pass(&mut self, files: &'a [FileDecls], prototyped: &'a [Vec<Option<RawDecl>>], last: bool) {
        self.enum_next = EnumCursor::Start;
        for (index, file) in files.iter().enumerate() {
            let file_id = FileId(index);
            self.unit = file_id;
            if last {
                for d in &file.diagnostics {
                    self.settle(d.span.start);
                    let location = self.at(file_id, d.span);
                    self.report(d.code, location, d.message.clone());
                }
            }
            let protos = prototyped.get(index);
            let mut tags = file.struct_tags.iter().peekable();
            let mut structs = file.structs.iter().peekable();
            for (at, written) in file.decls.iter().enumerate() {
                let proto = protos.and_then(|p| p.get(at)).and_then(Option::as_ref);
                let decl = proto.unwrap_or(written);
                self.autoproto = proto.is_some();
                // FTEF-19.20: a struct's tag is a type from where it is written.
                while let Some((tag, _)) = tags.next_if(|(_, from)| *from < decl.span.end) {
                    self.type_names.insert(tag.clone());
                }
                while let Some(st) = structs.next_if(|st| st.end <= decl.span.end) {
                    self.declare_struct(file_id, st);
                }
                self.settle(decl.span.start);
                let pending = self.declare_top_level(file_id, decl);
                // FTEF-12.6: `#pragma noref` holds from where it stands to the next one,
                // across files, as the definition's flags hold it.
                if decl.is_noref()
                    && let Some(def) = self.lookup_global(&decl.name)
                {
                    self.unused.insert(def);
                }
                if decl.enum_item.is_none() {
                    self.resolve_uses(file_id, None, &decl.uses);
                }
                for pending in pending {
                    if last {
                        self.resolve_pending(pending);
                    } else {
                        self.resolve_discarded(pending);
                    }
                }
            }
            // The structs a last body writes, and any after the last declaration.
            for st in structs {
                self.declare_struct(file_id, st);
            }
        }
        self.autoproto = false;
    }

    /// Resolves a body for its diagnostics alone: what it adds to the index, its locals and
    /// references, is taken out again (FTEF-12.19).
    fn resolve_discarded(&mut self, pending: Pending<'a>) {
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

    /// Removes the references after the first `earlier` that are among them, which the last
    /// pass made again.
    fn drop_repeated_refs(&mut self, earlier: usize) {
        let later = self.index.refs.split_off(earlier);
        let found = self.index.refs.clone();
        self.index
            .refs
            .extend(later.into_iter().filter(|r| !found.contains(r)));
    }

    /// Removes the diagnostics after the first `earlier` that are among them, which the last
    /// pass found again.
    fn drop_repeats(&mut self, earlier: usize) {
        let later = self.index.diagnostics.split_off(earlier);
        let found = self.index.diagnostics.clone();
        self.index
            .diagnostics
            .extend(later.into_iter().filter(|d| !found.contains(d)));
    }

    fn resolve_pending(&mut self, (file, def, class, body, decl): Pending<'a>) {
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
            at: 0,
        };
        for d in &body.diagnostics {
            self.settle(d.span.start);
            let location = self.at(file, d.span);
            self.report(d.code, location, d.message.clone());
        }
        if self.fteqcc {
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
        for param in &body.params {
            self.settle(param.span.start);
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
            if self.fteqcc
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
        self.resolve_uses(file, Some(scope), &body.uses);
    }

    /// A definition the attribute `alias(TARGET)` gives (FTEF-21.9): `QCC_PR_GetDef (NULL,
    /// aliasof, externfnc?NULL:pr_scope, false, ...)` finds the definition `TARGET` names, in
    /// `scope` or among the globals, else "TARGET not yet defined, cannot create NAME as an
    /// alias"; it is referenced, and `QCC_PR_DummyDef` makes `name` of type `ty` a new
    /// definition with no lookup, which "alias NAME may not be initialised".
    fn declare_alias_def(
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
    fn declare_alias(&mut self, file: FileId, scope: Scope, alias: &RawAlias) {
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

    /// FTEF-19.20: reports the first name in `ty` that is no type at `at`, "\"NAME\" is not a
    /// type", which ends the declaration; `typedefs` are a body's, each with where it stands.
    /// Returns whether one was reported.
    fn check_type_names(
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
    fn is_type_name(&self, name: &str, typedefs: &[(String, usize)], at: usize) -> bool {
        let exact = REGISTERED_TYPES.contains(&name)
            || self.type_names.contains(name)
            || typedefs.iter().any(|(t, start)| t == name && *start < at);
        exact
            || (self.case_insensitive
                && (REGISTERED_TYPES
                    .iter()
                    .chain(&TABLE_TYPES)
                    .any(|t| t.eq_ignore_ascii_case(name))
                    || (!self.qcfuncs_off
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
    fn unknown_type_name<'t>(
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
            ) if self.qcfuncs_off && !self.is_type_name(simple.as_str(), typedefs, at) => {
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

    /// FTEF-19.14: FTEQCC converts a state header's frame to a float with
    /// `QCC_SupplyConversion` (FTEF-22.3), and requires its next function to be a `void()`,
    /// "Type mismatch" for a literal but 0. A cycle's frame that `lax` let through unconverted
    /// is "state type mismatch", once both frames are converted. A fatal error ends the header.
    fn check_state_literals(&mut self, file: FileId, body: &RawBody) {
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
            self.settle(use_.span.start);
            let location = self.at(file, use_.span);
            let scope = scope.map(|s| Scope {
                at: use_.span.start,
                ..s
            });
            let found = match (&use_.scope, use_.kind) {
                (Some(prefix), UseKind::Value | UseKind::Member | UseKind::TypeOrValue)
                    if self.fteqcc =>
                {
                    self.resolve_scoped(scope, prefix, &use_.name)
                }
                (_, UseKind::Value) => self.resolve_value(scope, &use_.name, false),
                (_, UseKind::TypeOrValue) => self.resolve_value(scope, &use_.name, true),
                (_, UseKind::CastTo) => {
                    if self.struct_type_name(&use_.name).is_some() {
                        let from = use_.scope.clone().unwrap_or_default();
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
            match found {
                Found::Def(def) => {
                    if let Some(f) = self.fact_mut(def) {
                        f.referenced = true;
                    }
                    // FTEF-19.40: a call of a name whose type is a value's, not a function's.
                    if self.fteqcc
                        && use_.called
                        && self
                            .index
                            .def(def)
                            .is_some_and(|d| self.not_callable(&d.ty))
                    {
                        self.report(Code::E0256, location, "not a function".to_owned());
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

    /// Whether a value of type `ty` cannot be called (`not a function`, FTEF-19.40): a float,
    /// vector, string, entity or struct.
    fn not_callable(&self, ty: &Type) -> bool {
        match self.resolved(ty) {
            Type::Simple(
                SimpleType::Float | SimpleType::Vector | SimpleType::String | SimpleType::Entity,
            ) => true,
            Type::Named(name) => {
                let bare = name
                    .strip_prefix("struct ")
                    .or_else(|| name.strip_prefix("union "))
                    .unwrap_or(&name);
                self.struct_defs.contains_key(&self.type_key(bare))
            }
            Type::Simple(_)
            | Type::Field(_)
            | Type::Function(_)
            | Type::Pointer(_)
            | Type::Unknown => false,
        }
    }

    /// A plain name used as a value: the function's own, then in a member function the
    /// class's members (FTEF-17.2), then a global (QC-12.2, FTE-22.1); under FTEQCC then an
    /// intrinsic (FTEF-17.4), `this` and `super` only in a member function (FTEF-19.16).
    fn resolve_value(&self, scope: Option<Scope>, name: &str, type_ok: bool) -> Found {
        let def = match scope {
            Some(scope) => self.lookup_in_scope(scope, name),
            None => self.lookup_used_global(name),
        };
        if let Some(def) = def {
            // FTEF-19.38: a type's name is no value; `QCC_PR_ParseValue` finds no definition by
            // it.
            if self.fteqcc && !type_ok && self.type_defs.contains(&def) {
                return Found::Unknown;
            }
            return Found::Def(def);
        }
        if !self.fteqcc || !is_intrinsic(name) {
            return Found::Unknown;
        }
        // FTEF-20.11: `keyword_class && !strcmp(name, "this")`, and `super` so.
        if matches!(name, "this" | "super") && !self.keywords.has(Keyword::Class) {
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
            self.named(&self.class_names, prefix).copied()
        };
        // FTEF-19.33: the scope of `Scope::name` is read as a type name.
        if prefix != "super"
            && class.is_none()
            && self
                .unknown_type_name(&Type::Named(prefix.into()), &[], 0)
                .is_some()
        {
            return Found::Error(Code::E0203, format!("\"{prefix}\" is not a type"));
        }
        // FTEF-6.11: a struct's members are reached through a value, never as `Struct::name`.
        if class.is_none() && self.struct_defs.contains_key(&self.type_key(prefix)) {
            return Found::Unknown;
        }
        if let Some(class) = class
            && let Some(def) = self.member_in(class, name)
        {
            return Found::Def(def);
        }
        // A definition named `Scope::name`, as a strict enum's value is (FTEF-19.10), comes
        // before the plain name.
        // FTEF-19.33: the type is found in any case, and the value it names is the one written
        // (`strcmp (acc->fieldname, name)`).
        let spelled = self
            .type_name_spelled(prefix)
            .map_or_else(|| prefix.to_owned(), str::to_owned);
        if let Some(def) = self.lookup_global(&format!("{spelled}::{name}")) {
            return Found::Def(def);
        }
        if let Some(def) = self.lookup_global(name) {
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
        // FTE-13: an array takes `.length` and no other member (`QCC_PR_Expect("length")`
        // after an array's `.`: "expected length, found NAME").
        if self.fteqcc
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

    /// The struct a member access is made on, when a name's type is one (FTEF-6.5), and whether
    /// more than one of its own members (the nearest struct with one) is named `name`, or by it
    /// in any case under `-Fcaseinsens`: `QCC_PR_FindStructMember`'s "multiple members found
    /// matching" (FTEF-6.11).
    fn struct_matches(&self, scope: Option<Scope>, base: &MemberBase, name: &str) -> StructMatch {
        if !self.fteqcc {
            return StructMatch::NotStruct;
        }
        let MemberBase::Name(base) = base else {
            return StructMatch::NotStruct;
        };
        let Some(def) = (match scope {
            Some(scope) => self.lookup_in_scope(scope, base),
            None => self.lookup_used_global(base),
        }) else {
            return StructMatch::NotStruct;
        };
        let Some(Type::Named(type_name)) = self.index.def(def).map(|d| self.resolved(&d.ty)) else {
            return StructMatch::NotStruct;
        };
        let bare = type_name
            .strip_prefix("struct ")
            .or_else(|| type_name.strip_prefix("union "))
            .unwrap_or(&type_name);
        let mut current = Some(self.type_key(bare));
        let mut anonymous = false;
        for _ in 0..MAX_CLASS_DEPTH {
            let Some(key) = current else {
                break;
            };
            let Some(def) = self.struct_defs.get(&key) else {
                return StructMatch::NotStruct;
            };
            anonymous |= def.members.iter().any(|m| m.name.is_empty());
            // A member declared again with its name, type and size is one
            // (`oldparm && oldparm->arraysize == arraysize && !typecmp_lax`).
            let mut distinct: Vec<(&str, &Type, &[String])> = Vec::new();
            for m in &def.members {
                let same_name = if self.case_insensitive {
                    m.name.eq_ignore_ascii_case(name)
                } else {
                    m.name == name
                };
                let entry = (m.name.as_str(), &m.ty, m.array.as_slice());
                if same_name && !distinct.contains(&entry) {
                    distinct.push(entry);
                }
            }
            match distinct.len() {
                0 => {}
                1 => return StructMatch::One,
                _ => return StructMatch::Several(bare.to_owned()),
            }
            current = def.parent.clone();
        }
        if anonymous
            || !self
                .struct_defs
                .get(&self.type_key(bare))
                .is_some_and(|d| d.defined)
        {
            // An anonymous member's names are not followed (FTEF-6.11), and a struct with no
            // body has no members to miss.
            return StructMatch::One;
        }
        StructMatch::None(bare.to_owned())
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

    /// Whether `name` selects a member that is no definition: a vector's `x`, `y` or `z`
    /// (FTE-9.1), an array's `length` (FTE-13.3), or a field of a struct or accessor the
    /// module defines (FTE-14.2, FTEF-6.5, FTEF-10.2).
    fn is_member_name(&self, name: &str) -> bool {
        matches!(name, "x" | "y" | "z" | "length") || self.has_name(&self.members, name)
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
        if self.fteqcc && local.global_function {
            // FTEF-21.10: a C function declared in a body is a global constant, its body
            // elsewhere.
            self.declare_global(&local.name, &local.ty, DefKind::Function, location, None);
            return;
        }
        if self.fteqcc
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
        let statics = self.fteqcc && local.is_static();
        let constant = self.fteqcc && local.has(&Qualifier::Const);
        let compared = constant || statics;
        let holder = if !self.fteqcc {
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
                subscope: self.fteqcc && local.flags.has(FteFlag::PrSubscopedlocals),
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
        if self.fteqcc && self.check_array_value(file, local.array, &local.ty, local.value.as_ref())
        {
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

    /// The parameter or local of the function in `scope` named `name` (QC-12.1), but for one
    /// whose `for` statement ended before the lookup (FTEF-19.19), the newest first, as
    /// `localstable` gives them (FTEF-21.12).
    fn lookup_local(&self, scope: Scope, name: &str) -> Option<DefId> {
        self.index
            .defs
            .iter()
            .enumerate()
            .skip(scope.locals_start)
            .rev()
            .find(|(index, d)| {
                d.owner == Some(scope.owner) && d.name == name && self.in_scope_at(*index, scope.at)
            })
            .map(|(index, _)| DefId(index))
    }

    /// Whether the definition at `index` is in scope at byte `at`: not a `for` statement's
    /// local whose statement ended before it (FTEF-19.19).
    fn in_scope_at(&self, index: usize, at: usize) -> bool {
        let facts = self.facts.get(index);
        facts.and_then(|f| f.scope_end).is_none_or(|end| at < end)
            && facts
                .and_then(|f| f.scope_start)
                .is_none_or(|start| at >= start)
    }

    /// A global name as a use finds it (QC-12.2): as written, then under `-Fcaseinsens` in any
    /// case (FTEF-19.33).
    fn lookup_used_global(&self, name: &str) -> Option<DefId> {
        self.lookup_global(name)
            .or_else(|| self.lookup_any_case(None, name))
    }

    /// Looks up a name in a function's scope (QC-12.2). In vanilla the first definition in
    /// the list that is global or the function's own is the global when there is one, since
    /// a local cannot shadow it (QC-12.3); in an FTEQCC profile the function's own comes
    /// first, since it shadows the global (FTE-22.1), then in a member function the class's
    /// members (FTEF-17.2). A local of another function, declared earlier, is invisible
    /// (QC-12.4).
    fn lookup_in_scope(&self, scope: Scope, name: &str) -> Option<DefId> {
        self.lookup_in_scope_exactly(scope, name)
            .or_else(|| self.lookup_any_case(Some(scope), name))
    }

    /// FTEF-19.33: under `-Fcaseinsens`, a name no definition has as written is the last local
    /// of the function, then the last global, whose name is it in any ASCII case, as the hash
    /// table's chain gives the newest first:
    /// `QCC_PR_GetDef`'s second pass (`ins == 1`), which a lookup that allocates nothing makes
    /// (`insmax = allocate?1:2;`).
    fn lookup_any_case(&self, scope: Option<Scope>, name: &str) -> Option<DefId> {
        if !self.case_insensitive {
            return None;
        }
        let local = scope.and_then(|scope| {
            self.index
                .defs
                .iter()
                .enumerate()
                .skip(scope.locals_start)
                .rev()
                .find(|(index, d)| {
                    d.owner == Some(scope.owner)
                        && d.name.eq_ignore_ascii_case(name)
                        && self.in_scope_at(*index, scope.at)
                })
                .map(|(index, _)| DefId(index))
        });
        local.or_else(|| {
            self.globals
                .keys()
                .filter(|global| global.eq_ignore_ascii_case(name))
                .filter_map(|global| self.lookup_global(global))
                .max()
        })
    }

    /// [`Resolver::lookup_in_scope`] of the name as written.
    fn lookup_in_scope_exactly(&self, scope: Scope, name: &str) -> Option<DefId> {
        if self.fteqcc {
            // A class's name is the type, not its constructor member (FTEF-17.14).
            // FTEF-17.14: a field named bare is `this.field`, reached through the global
            // `self` (`QCC_PR_GetDef(NULL, "self", NULL, true, 0, false)`), without which
            // FTEQCC ends; a member function is found either way.
            let has_self = self.lookup_global("self").is_some();
            let member = || {
                scope
                    .class
                    .filter(|_| !self.has_name_key(&self.class_names, name))
                    .and_then(|c| self.member_in(c, name))
                    .filter(|id| {
                        has_self
                            || self
                                .index
                                .def(*id)
                                .is_some_and(|d| d.kind == DefKind::Function)
                    })
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
    ///
    /// FTEF-22.5: in the same pass, definition by definition, an `extern` function is an error
    /// on a target with no `extern`. Like the undefined functions, and unlike FTEQCC's check,
    /// which a module with errors never reaches, it is reported whatever else was (QCD-1.4).
    fn check_undefined_functions(&mut self) {
        let used: HashSet<DefId> = self.index.refs.iter().map(|r| r.def).collect();
        let externs_refused = self.target.is_some_and(|target| !target.supports_extern());
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
            let skipped = self.fteqcc && self.unused.contains(&id) && !used.contains(&id);
            // FTEF-19.41: a member function a class declares and nothing defines has no body
            // either, a virtual one among them, which is a field of function type.
            let var_member = def
                .member_of
                .is_some_and(|c| self.var_members.contains(&(c, def.name.clone())));
            let member_function = def.member_of.is_some()
                && !var_member
                && (def.kind == DefKind::Function
                    || (def.kind == DefKind::Field && self.resolved(&def.ty).is_function()));
            let function =
                (def.kind == DefKind::Function && def.member_of.is_none()) || member_function;
            if function && !def.initialized && !def.generated && !skipped {
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
    /// Whether `subscope` is on where it is declared, so a local of the name found again is
    /// a new one (FTEF-21.12).
    subscope: bool,
}

/// What [`Resolver::declare_special`] did with a top-level declaration.
enum Special<'a> {
    /// It declared it, with the function bodies it brings.
    Declared(Vec<Pending<'a>>),
    /// It is a plain definition, whose qualifiers' warnings ended it when `qualifier_fatal`.
    Plain { qualifier_fatal: bool },
}

/// Where the count of the enum being read is (FTEF-19.10).
#[derive(Clone, Copy, Debug)]
enum EnumCursor {
    /// At its first name: 0, or 1 for `enumflags`.
    Start,
    /// At this value.
    Known(f64),
    /// After a value the model does not compute.
    Unknown,
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
    /// The byte offset the lookup is made at, which a `for` statement's locals are found
    /// before the end of (FTEF-19.19).
    at: usize,
}
