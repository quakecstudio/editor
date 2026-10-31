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
//! with the same literal (FTE-22.6), and with autoprototyping every top-level name is visible
//! to every function body (FTE-22.3). The full dialect's classes resolve by FTEF-17: a class's
//! members are found through the class and its parents, inside a member function's body before
//! the globals, and after a `.` through the type of what the access is made on. FTEQCC's
//! definitions follow FTEF-19, ported from `QCC_PR_ParseDefs` and `QCC_PR_GetDef`: casts and
//! constness of initialisers, parameters, array sizes, typedefs, enum values and the
//! redeclaration rules.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use qc_diag::{Code, Diagnostic, Span};
use qc_preproc::{OpcodeTarget, TargetFormat, WarningSettings};
use qc_syntax::{Dialect, Keywords};

use crate::check::PathList;
use crate::check::Temps;
use crate::check::TypedBody;
use crate::check::constant::Constant;
use crate::expand::{OriginMap, ProtoPlan};
use crate::lower::{
    ArrayDim, BuiltinFunction, FileDecls, LiteralValue, RawBody, RawDecl, RawParam, RawParamInfo,
    RawUse, StructMember,
};
use crate::symbol::{DefId, Definition, FileId, Location, Reference};
use crate::ty::{SimpleType, Type};

use profile::Profile;

mod aggregate;
mod body;
mod check_env;
mod global;
mod init;
mod profile;
mod report;
mod scope;
mod types;

pub(crate) use types::Layout;

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
    /// Every global declaration after the first of a name, each naming the definition it
    /// declares again (QC-11.5): a function's body after its prototype, or a second
    /// `float x;`. In declaration order. Find-references lists them with the declaration.
    pub redeclarations: Vec<Reference>,
    /// The semantic diagnostics, in the order found, each with its file.
    pub diagnostics: Vec<FileDiagnostic>,
    /// Each function body's types, by the function's definition (`crate::check`).
    pub bodies: HashMap<DefId, TypedBody>,
    /// What each global whose initialiser is an expression, not a literal, folds to under an
    /// FTEQCC profile (FTEF-34.9), and each enum's item whose value the model computes
    /// (FTEF-19.10), which a compiler writes to the global's words.
    pub initial_values: HashMap<DefId, InitialValue>,
    /// The elements of each array definition whose size the model knows, `arraysize`
    /// (FTEF-19.8): a written number, or the count of an unsized array's initialiser.
    pub arrays: HashMap<DefId, u32>,
    /// The layout of each struct or union type a definition, or a member of another, is
    /// written with, by the type as written (FTEF-6.13), when the model lays it out.
    pub structs: HashMap<Type, StructShape>,
    /// The type each named type a definition is written with stands for, when it is another:
    /// a typedef's, or an enum's base type (FTEF-7.2, FTEF-19.10).
    pub aliases: HashMap<Type, Type>,
    /// The definitions FTEQCC holds constant, `def->constant` (FTEF-19.3): an initialised
    /// global that is no `var`, an enum's item, a function.
    pub constants: HashSet<DefId>,
}

/// A struct's or union's layout as FTEQCC places it (FTEF-6.13): the words it takes and where
/// each member is, its parent's first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructShape {
    /// The words a value takes, `type->size`.
    pub words: u32,
    /// The members that start at a word, in order; a bitfield or a narrow integer is left out.
    pub members: Vec<MemberShape>,
}

/// One member of a [`StructShape`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemberShape {
    /// Its name.
    pub name: String,
    /// Its type as written.
    pub ty: Type,
    /// The word of the struct it starts at.
    pub word: u32,
    /// Its elements, 1 for no array.
    pub count: u32,
}

/// The value a global's initialiser folds to (FTEF-34.9, FTEF-26): the words it holds in the
/// progs' global area, one for a float or an integer and three for a vector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialValue {
    words: Vec<u32>,
}

impl InitialValue {
    pub(crate) const fn new(words: Vec<u32>) -> Self {
        Self { words }
    }

    /// The words, in order.
    #[must_use]
    pub fn words(&self) -> &[u32] {
        &self.words
    }
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
    /// For each entry, under FTEQCC, the output target and the opcodes it has, and the offsets
    /// they hold from (FTEF-25).
    pub(crate) targets: &'a [&'a [(usize, OpcodeTarget)]],
    /// For each entry, under FTEQCC, the file name and line offset FTEQCC's messages give, from
    /// each offset (FTEF-14.43).
    pub(crate) message_places: &'a [&'a [(usize, String, i64)]],
    /// The source files whose name ends in `extensions.qc`, where FTEQCC keeps a function's
    /// first declaration (FTEF-19.7).
    pub(crate) extensions: HashSet<FileId>,
    /// Each source file's name as a message prints it, and where its lines start (FTEF-31.4).
    pub(crate) places: HashMap<FileId, FilePlace>,
    /// What the first pass prototypes (FTEF-12.18): with a second pass the first pass's
    /// [`Self::first`], else the only pass. `None` when it prototypes nothing.
    pub(crate) plan: Option<&'a ProtoPlan>,
    /// With a second pass, the first pass's files, as it read them. Without it the first pass
    /// is the files themselves, all prototyped.
    pub(crate) first: Option<FirstPass<'a>>,
}

/// An accessor member as the check stage reads it (FTEF-10.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AccessorFacts {
    /// The member's type, `acc->type`: the type of the member declared first.
    pub(crate) ty: Type,
    /// The type its setter stores, its `set` member's.
    pub(crate) set_ty: Type,
    /// Whether it has a `get` member.
    pub(crate) get: bool,
    /// Whether it has a `set` member.
    pub(crate) set: bool,
    /// The type of its index, `indexertype`, when it takes one.
    pub(crate) indexer: Option<Type>,
}

/// A source file's name as a message prints it, `s_filen`, and the offsets its lines start at.
#[derive(Clone, Debug, Default)]
pub(crate) struct FilePlace {
    /// The file's name.
    pub(crate) name: String,
    /// The offset each line starts at, the first 0.
    pub(crate) lines: Vec<usize>,
}

impl FilePlace {
    /// The line of offset `at`, counted from 1.
    pub(crate) fn line(&self, at: usize) -> usize {
        self.lines.partition_point(|&start| start <= at)
    }
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
    let profile = Profile::new(options);
    let two = profile.two_passes();
    let plan = match sources.plan {
        Some(plan) if profile.reads_autoproto_pragmas() => Prototyping::Plan(plan),
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
        temps: Temps::default(),
        maps: sources.maps,
        places: &sources.places,
        warnings: sources.warnings,
        settings: None,
        keyword_marks: sources.keywords,
        keywords: Keywords::default(),
        targets: sources.targets,
        message_places: sources.message_places,
        extensions: &sources.extensions,
        profile,
        use_defs: HashMap::new(),
        autoproto: false,
        externs: HashSet::new(),
        enclosing: HashMap::new(),
        unit: FileId(0),
        facts: Vec::new(),
        globals: HashMap::new(),
        literals: HashMap::new(),
        integer_items: HashMap::new(),
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
        folded_sizes: HashMap::new(),
        paths: HashSet::new(),
        enum_types: HashMap::new(),
        enum_values: HashMap::new(),
        enum_redeclared: false,
        struct_defs: HashMap::new(),
        type_defs: HashSet::new(),
        body_typedefs: Vec::new(),
        var_members: HashSet::new(),
        accessor_bases: HashMap::new(),
        accessors: HashMap::new(),
        typedef_params: HashMap::new(),
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

/// A struct member as `typecmp_strict` compares it on redeclaration (FTEF-6.10): its name,
/// type, array suffixes and bitfield width.
type ComparedMember = (String, Type, Vec<String>, Option<Option<u32>>);

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
    /// Whether it is a union (FTEF-6.7).
    union: bool,
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
    /// FTEQCC's temporaries, across the module's functions (FTEF-27.16).
    temps: Temps,
    /// Where the spans of each file's rendered text came from; empty when the files were
    /// parsed as written.
    maps: &'a [&'a OriginMap],
    /// Each source file's name and lines.
    places: &'a HashMap<FileId, FilePlace>,
    /// For each compiled file, the warning settings and the offsets they hold from
    /// (FTEF-19.22).
    warnings: &'a [&'a [(usize, WarningSettings)]],
    /// Under FTEQCC, the warning settings in force where the resolver reads (FTEF-19.22).
    settings: Option<WarningSettings>,
    /// For each compiled file, the keywords its parser read each token with (FTEF-20.1).
    keyword_marks: &'a [&'a [(usize, Keywords)]],
    /// The keywords in force where the resolver reads (FTEF-20.1).
    keywords: Keywords,
    /// For each compiled file, the output target and the opcodes it has (FTEF-25).
    targets: &'a [&'a [(usize, OpcodeTarget)]],
    /// For each compiled file, the file name and line offset FTEQCC's messages give (FTEF-14.43).
    message_places: &'a [&'a [(usize, String, i64)]],
    /// The source files named `*extensions.qc` (FTEF-19.7).
    extensions: &'a HashSet<FileId>,
    /// The dialect profile and its rules.
    profile: Profile,
    /// The definition each name of the body being resolved resolved to, by where the name
    /// starts in its file's text, for the check stage.
    use_defs: HashMap<usize, DefId>,
    /// Whether FTEQCC's `autoprototype` is on where the pass reads (FTE-22.3, FTEF-12.18).
    autoproto: bool,
    /// The functions an `extern` at file level made, `def->isextern` (FTEF-22.5).
    externs: HashSet<DefId>,
    /// Each nested function's enclosing scope, whose `static` locals it sees (FTEF-18.40).
    enclosing: HashMap<DefId, Enclosing>,
    /// The compiled file, the unit a `static` is visible in (FTEF-19.12).
    unit: FileId,
    /// For each definition, what [`Facts`] holds.
    facts: Vec<Facts>,
    /// The first global definition of each name (QC-12.2), so a global lookup is direct.
    globals: HashMap<String, Vec<DefId>>,
    /// The literal each initialised global was given, for FTE-22.6.
    literals: HashMap<DefId, LiteralValue>,
    /// The value of each item of an enum of an integer base, the constant `QCC_MakeIntConst`
    /// and its kin make of it (FTEF-19.52).
    integer_items: HashMap<DefId, Constant>,
    /// For each builtin number and engine name, the definition whose builtin function FTEQCC
    /// made first with them: the one a redeclaration with them reuses (FTE-23.17).
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
    /// The named enums' types by their names, each its base type (FTEF-19.10, FTEF-8.6).
    enum_types: HashMap<String, Type>,
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
    body_typedefs: Vec<(String, Span)>,
    /// The `var` members of each class, by the class and the member's name (FTEF-19.41).
    var_members: HashSet<(DefId, String)>,
    /// The base type of each accessor (FTEF-10.6).
    accessor_bases: HashMap<String, Type>,
    /// Each accessor's and class's `get` and `set` members, by the owner and the member's name
    /// (FTEF-10.3): the member's type, and whether it has each.
    accessors: HashMap<(String, String), AccessorFacts>,
    /// The named parameters of each typedef of a function type, by the type's key, which a
    /// definition of the type takes as its body's (FTEF-7.5).
    typedef_params: HashMap<String, Vec<RawParam>>,
    /// The base type of each `enum class`, whose values are of its own type (FTEF-19.10).
    strict_enums: HashMap<String, Type>,
    /// The size each global array whose size is an expression folds to, by where its
    /// declaration's name starts (FTEF-19.62).
    folded_sizes: HashMap<usize, u32>,
    /// The paths each list of sounds, models, textures and files holds (FTEF-17.19).
    paths: HashSet<(PathList, String)>,
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
pub(in crate::resolve) const MAX_TYPEDEF_DEPTH: usize = 64;

/// The builtins FTEQCC keeps the first declaration of when a later one gives another type,
/// with a warning (FTEF-19.7): `QCC_PR_GetDef`'s "selfish lazy hack".
const MISDECLARED_BUILTINS: [&str; 3] = ["droptofloor", "callfunction", "trailparticles"];

/// The functions a body may replace a builtin of with a warning, not an error (FTEF-19.57).
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

/// The initialiser of a declaration, as redeclaration sees it (QC-11.5, FTE-22.6): `None`
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
            for accessor in &file.accessors {
                let facts = self
                    .accessors
                    .entry((accessor.owner.clone(), accessor.name.clone()))
                    .or_insert_with(|| AccessorFacts {
                        ty: accessor.ty.clone(),
                        set_ty: accessor.ty.clone(),
                        get: false,
                        set: false,
                        indexer: accessor.indexer.clone(),
                    });
                if facts.indexer.is_none() {
                    facts.indexer.clone_from(&accessor.indexer);
                }
                if accessor.set {
                    facts.set = true;
                    facts.set_ty = accessor.ty.clone();
                } else {
                    facts.get = true;
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
                self.declare_structs_before(file_id, &mut structs, decl.span.end);
                self.settle(decl.span.start);
                // FTEF-12.6: `noref` marks a definition found again unused before its
                // initialiser is read (`if (noref) def->unused = true;`), so no initialiser it had
                // is compared with the new one.
                if decl.is_noref()
                    && let Some(def) = self.lookup_global(&decl.name)
                {
                    self.unused.insert(def);
                }
                // FTEF-19.62: the size is read, and folded, before the definition is made.
                let (size, uses) = if last {
                    self.fold_array_length(file_id, decl)
                } else {
                    (None, decl.uses.iter().collect())
                };
                if let Some(size) = size {
                    self.folded_sizes.insert(decl.span.start, size);
                }
                let pending = self.declare_top_level(file_id, decl);
                // FTEF-12.6: `#pragma noref` holds from where it stands to the next one,
                // across files, as the definition's flags hold it.
                if decl.is_noref()
                    && let Some(def) = self.lookup_global(&decl.name)
                {
                    self.unused.insert(def);
                }
                if decl.enum_item.is_none() {
                    let uses: Vec<RawUse> = uses.into_iter().cloned().collect();
                    self.resolve_uses(file_id, None, &uses);
                    if last {
                        self.check_global_initialiser(file_id, decl);
                    }
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
            self.declare_structs_before(file_id, &mut structs, usize::MAX);
        }
        self.autoproto = false;
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

    fn finish(mut self) -> ModuleIndex {
        self.check_undefined_functions();
        self.record_shapes();
        self.index
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
    /// `extern`, a lookup of a definition made elsewhere rather than a new one (FTEF-19.59).
    external: bool,
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

/// Where a nested function is declared in the function around it (FTEF-18.40).
#[derive(Clone, Copy)]
struct Enclosing {
    /// The scope of the function around it, at the nested function's declaration.
    scope: Scope,
    /// The definitions made before it, beyond which the outer function's locals are not yet
    /// declared.
    end: usize,
}

/// The function whose body is being resolved, where its locals start in the definition
/// list, and the class it is a member function of.
#[derive(Clone, Copy)]
struct Scope {
    owner: DefId,
    /// The compiled file the body is in, whose text [`Self::at`] is an offset of.
    file: FileId,
    locals_start: usize,
    class: Option<DefId>,
    /// The byte offset the lookup is made at, which a `for` statement's locals are found
    /// before the end of (FTEF-19.19).
    at: usize,
    /// Whether the body is a member function's written in its class, compiled where it
    /// stands (FTEF-17.16).
    inline_member: bool,
}
