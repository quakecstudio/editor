// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, compiler_flag, optimisations
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, pr_opcodes, targets, QCC_OPCodeSetTargetName, QCC_OPCodeSetTarget

//! The tables FTEQCC reads its build pragmas against (`docs/spec/dialect-fteqcc.md` FTEF-12.11):
//! the names of its compiler flags and keywords, its optimisations, its opcodes and its targets,
//! as data, written out of `qccmain.c` and `qcc_pr_comp.c` at the pinned commit.

/// One of FTEQCC's compiler flags or keywords (`compiler_flag[]` in `qccmain.c`), named after the
/// variable it sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FteFlag {
    /// `keyword_asm`.
    AsmKeyword,
    /// `keyword_break`.
    BreakKeyword,
    /// `keyword_case`.
    CaseKeyword,
    /// `keyword_class`.
    ClassKeyword,
    /// `keyword_accessor`.
    AccessorKeyword,
    /// `keyword_const`.
    ConstKeyword,
    /// `keyword_continue`.
    ContinueKeyword,
    /// `keyword_default`.
    DefaultKeyword,
    /// `keyword_entity`.
    EntityKeyword,
    /// `keyword_enum`.
    EnumKeyword,
    /// `keyword_enumflags`.
    EnumflagsKeyword,
    /// `keyword_extern`.
    ExternKeyword,
    /// `keyword_float`.
    FloatKeyword,
    /// `keyword_for`.
    ForKeyword,
    /// `keyword_goto`.
    GotoKeyword,
    /// `keyword_int`.
    IntKeyword,
    /// `keyword_integer`.
    IntegerKeyword,
    /// `keyword_double`.
    DoubleKeyword,
    /// `keyword_long`.
    LongKeyword,
    /// `keyword_short`.
    ShortKeyword,
    /// `keyword_char`.
    CharKeyword,
    /// `keyword_signed`.
    SignedKeyword,
    /// `keyword_unsigned`.
    UnsignedKeyword,
    /// `keyword_register`.
    RegisterKeyword,
    /// `keyword_volatile`.
    VolatileKeyword,
    /// `keyword_noref`.
    NorefKeyword,
    /// `keyword_unused`.
    UnusedKeyword,
    /// `keyword_used`.
    UsedKeyword,
    /// `keyword_local`.
    LocalKeyword,
    /// `keyword_static`.
    StaticKeyword,
    /// `keyword_nonstatic`.
    NonstaticKeyword,
    /// `keyword_ignore`.
    IgnoreKeyword,
    /// `keyword_auto`.
    AutoKeyword,
    /// `keyword_nosave`.
    NosaveKeyword,
    /// `keyword_inline`.
    InlineKeyword,
    /// `keyword_strip`.
    StripKeyword,
    /// `keyword_shared`.
    SharedKeyword,
    /// `keyword_state`.
    StateKeyword,
    /// `keyword_optional`.
    OptionalKeyword,
    /// `keyword_inout`.
    InoutKeyword,
    /// `keyword_string`.
    StringKeyword,
    /// `keyword_struct`.
    StructKeyword,
    /// `keyword_switch`.
    SwitchKeyword,
    /// `keyword_thinktime`.
    ThinktimeKeyword,
    /// `keyword_until`.
    UntilKeyword,
    /// `keyword_loop`.
    LoopKeyword,
    /// `keyword_typedef`.
    TypedefKeyword,
    /// `keyword_union`.
    UnionKeyword,
    /// `keyword_var`.
    VarKeyword,
    /// `keyword_vector`.
    VectorKeyword,
    /// `keyword_wrap`.
    WrapKeyword,
    /// `keyword_weak`.
    WeakKeyword,
    /// `keyword_accumulate`.
    AccumulateKeyword,
    /// `keyword_using`.
    UsingKeyword,
    /// `flag_acc`.
    Acc,
    /// `flag_qccx`.
    Qccx,
    /// `keywords_coexist`.
    KeywordsCoexist,
    /// `output_parms`.
    OutputParms,
    /// `autoprototype`.
    Autoprototype,
    /// `writeasm`.
    Writeasm,
    /// `flag_guiannotate`.
    Guiannotate,
    /// `flag_nullemptystr`.
    Nullemptystr,
    /// `flag_ifstring`.
    Ifstring,
    /// `flag_iffloat`.
    Iffloat,
    /// `flag_ifvector`.
    Ifvector,
    /// `flag_vectorlogic`.
    Vectorlogic,
    /// `flag_brokenarrays`.
    Brokenarrays,
    /// `flag_rootconstructor`.
    Rootconstructor,
    /// `flag_caseinsensitive`.
    Caseinsensitive,
    /// `flag_laxcasts`.
    Laxcasts,
    /// `flag_hashonly`.
    Hashonly,
    /// `flag_macroinstrings`.
    Macroinstrings,
    /// `opt_logicops`.
    LogicopsOpt,
    /// `flag_msvcstyle`.
    Msvcstyle,
    /// `flag_debugmacros`.
    Debugmacros,
    /// `flag_filetimes`.
    Filetimes,
    /// `flag_fasttrackarrays`.
    Fasttrackarrays,
    /// `flag_assume_integer`.
    AssumeInteger,
    /// `pr_subscopedlocals`.
    PrSubscopedlocals,
    /// `verbose`.
    Verbose,
    /// `flag_typeexplicit`.
    Typeexplicit,
    /// `flag_boundchecks`.
    Boundchecks,
    /// `flag_attributes`.
    Attributes,
    /// `flag_assumevar`.
    Assumevar,
    /// `flag_dblstarexp`.
    Dblstarexp,
    /// `flag_cpriority`.
    Cpriority,
    /// `flag_assume_double`.
    AssumeDouble,
    /// `flag_qcfuncs`.
    Qcfuncs,
    /// `flag_allowuninit`.
    Allowuninit,
    /// `flag_nopragmafileline`.
    Nopragmafileline,
    /// `flag_utf8strings`.
    Utf8strings,
    /// `flag_reciprocalmaths`.
    Reciprocalmaths,
    /// `flag_ILP32`.
    Ilp32,
    /// `flag_undefwordsize`.
    Undefwordsize,
    /// `flag_pointerrelocs`.
    Pointerrelocs,
    /// `flag_noreflection`.
    Noreflection,
    /// `flag_embedsrc`.
    Embedsrc,
    /// `flag_dumpfilenames`.
    Dumpfilenames,
    /// `flag_dumpfields`.
    Dumpfields,
    /// `flag_dumpsymbols`.
    Dumpsymbols,
    /// `flag_dumpautocvars`.
    Dumpautocvars,
    /// `flag_dumplocalisation`.
    Dumplocalisation,
    /// `flag_dumptags`.
    Dumptags,
    /// `flag_dumpopcodes`.
    Dumpopcodes,
}

/// A row of `compiler_flag[]`: the name `#pragma flag` and `-F` take, the flag it sets, whether
/// a pragma may set it (`FLAG_MIDCOMPILE`) and whether it is on before any (`FLAG_ASDEFAULT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FlagRow {
    /// `abbrev`, matched in any case (`QC_strcasecmp`).
    pub(crate) name: &'static str,
    /// The flag it sets.
    pub(crate) flag: FteFlag,
    /// `FLAG_MIDCOMPILE`: a `#pragma flag` may set it.
    pub(crate) mid_compile: bool,
    /// `FLAG_ASDEFAULT`: on unless an option turns it off.
    pub(crate) default: bool,
}

/// `compiler_flag[]`, in its order.
pub(crate) const FLAGS: [FlagRow; 105] = [
    FlagRow {
        name: "asm",
        flag: FteFlag::AsmKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "break",
        flag: FteFlag::BreakKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "case",
        flag: FteFlag::CaseKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "class",
        flag: FteFlag::ClassKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "accessor",
        flag: FteFlag::AccessorKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "const",
        flag: FteFlag::ConstKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "continue",
        flag: FteFlag::ContinueKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "default",
        flag: FteFlag::DefaultKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "entity",
        flag: FteFlag::EntityKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "enum",
        flag: FteFlag::EnumKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "enumflags",
        flag: FteFlag::EnumflagsKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "extern",
        flag: FteFlag::ExternKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "float",
        flag: FteFlag::FloatKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "for",
        flag: FteFlag::ForKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "goto",
        flag: FteFlag::GotoKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "int",
        flag: FteFlag::IntKeyword,
        mid_compile: false,
        default: true,
    },
    FlagRow {
        name: "integer",
        flag: FteFlag::IntegerKeyword,
        mid_compile: false,
        default: true,
    },
    FlagRow {
        name: "double",
        flag: FteFlag::DoubleKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "long",
        flag: FteFlag::LongKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "short",
        flag: FteFlag::ShortKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "char",
        flag: FteFlag::CharKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "signed",
        flag: FteFlag::SignedKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "unsigned",
        flag: FteFlag::UnsignedKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "register",
        flag: FteFlag::RegisterKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "volatile",
        flag: FteFlag::VolatileKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "noref",
        flag: FteFlag::NorefKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "unused",
        flag: FteFlag::UnusedKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "used",
        flag: FteFlag::UsedKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "local",
        flag: FteFlag::LocalKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "static",
        flag: FteFlag::StaticKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "nonstatic",
        flag: FteFlag::NonstaticKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "ignore",
        flag: FteFlag::IgnoreKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "auto",
        flag: FteFlag::AutoKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "nosave",
        flag: FteFlag::NosaveKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "inline",
        flag: FteFlag::InlineKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "strip",
        flag: FteFlag::StripKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "shared",
        flag: FteFlag::SharedKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "state",
        flag: FteFlag::StateKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "optional",
        flag: FteFlag::OptionalKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "inout",
        flag: FteFlag::InoutKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "string",
        flag: FteFlag::StringKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "struct",
        flag: FteFlag::StructKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "switch",
        flag: FteFlag::SwitchKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "thinktime",
        flag: FteFlag::ThinktimeKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "until",
        flag: FteFlag::UntilKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "loop",
        flag: FteFlag::LoopKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "typedef",
        flag: FteFlag::TypedefKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "union",
        flag: FteFlag::UnionKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "var",
        flag: FteFlag::VarKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "vector",
        flag: FteFlag::VectorKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "wrap",
        flag: FteFlag::WrapKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "weak",
        flag: FteFlag::WeakKeyword,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "accumulate",
        flag: FteFlag::AccumulateKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "using",
        flag: FteFlag::UsingKeyword,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "acc",
        flag: FteFlag::Acc,
        mid_compile: false,
        default: false,
    },
    FlagRow {
        name: "qccx",
        flag: FteFlag::Qccx,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "kce",
        flag: FteFlag::KeywordsCoexist,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "parms",
        flag: FteFlag::OutputParms,
        mid_compile: false,
        default: false,
    },
    FlagRow {
        name: "autoproto",
        flag: FteFlag::Autoprototype,
        mid_compile: false,
        default: false,
    },
    FlagRow {
        name: "wasm",
        flag: FteFlag::Writeasm,
        mid_compile: false,
        default: false,
    },
    FlagRow {
        name: "annotate",
        flag: FteFlag::Guiannotate,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "nullemptystr",
        flag: FteFlag::Nullemptystr,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "ifstring",
        flag: FteFlag::Ifstring,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "iffloat",
        flag: FteFlag::Iffloat,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "ifvector",
        flag: FteFlag::Ifvector,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "vectorlogic",
        flag: FteFlag::Vectorlogic,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "brokenarray",
        flag: FteFlag::Brokenarrays,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "rootconstructor",
        flag: FteFlag::Rootconstructor,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "caseinsens",
        flag: FteFlag::Caseinsensitive,
        mid_compile: false,
        default: false,
    },
    FlagRow {
        name: "lax",
        flag: FteFlag::Laxcasts,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "hashonly",
        flag: FteFlag::Hashonly,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "macroinstrings",
        flag: FteFlag::Macroinstrings,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "lo",
        flag: FteFlag::LogicopsOpt,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "msvcstyle",
        flag: FteFlag::Msvcstyle,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "debugmacros",
        flag: FteFlag::Debugmacros,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "filetimes",
        flag: FteFlag::Filetimes,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "fastarrays",
        flag: FteFlag::Fasttrackarrays,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "assumeint",
        flag: FteFlag::AssumeInteger,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "subscope",
        flag: FteFlag::PrSubscopedlocals,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "verbose",
        flag: FteFlag::Verbose,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "typeexplicit",
        flag: FteFlag::Typeexplicit,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "boundchecks",
        flag: FteFlag::Boundchecks,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "attributes",
        flag: FteFlag::Attributes,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "assumevar",
        flag: FteFlag::Assumevar,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "ssp",
        flag: FteFlag::Dblstarexp,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "cpriority",
        flag: FteFlag::Cpriority,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "assumedouble",
        flag: FteFlag::AssumeDouble,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "qcfuncs",
        flag: FteFlag::Qcfuncs,
        mid_compile: true,
        default: true,
    },
    FlagRow {
        name: "allowuninit",
        flag: FteFlag::Allowuninit,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "nofileline",
        flag: FteFlag::Nopragmafileline,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "utf8",
        flag: FteFlag::Utf8strings,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "reciprocal-math",
        flag: FteFlag::Reciprocalmaths,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "ILP32",
        flag: FteFlag::Ilp32,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "undefwordsize",
        flag: FteFlag::Undefwordsize,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "pointerrelocs",
        flag: FteFlag::Pointerrelocs,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "omitinternals",
        flag: FteFlag::Noreflection,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "embedsrc",
        flag: FteFlag::Embedsrc,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "dumpfilenames",
        flag: FteFlag::Dumpfilenames,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "dumpfields",
        flag: FteFlag::Dumpfields,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "dumpsymbols",
        flag: FteFlag::Dumpsymbols,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "dumpautocvars",
        flag: FteFlag::Dumpautocvars,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "dumplocalisation",
        flag: FteFlag::Dumplocalisation,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "dumptags",
        flag: FteFlag::Dumptags,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "tags",
        flag: FteFlag::Dumptags,
        mid_compile: true,
        default: false,
    },
    FlagRow {
        name: "dumpopcodes",
        flag: FteFlag::Dumpopcodes,
        mid_compile: true,
        default: false,
    },
];

/// `optimisations[]`: each `abbrev` and `fullname`, which `#pragma optimise` matches in any case.
pub(crate) const OPTIMISATIONS: [(&str, &str); 20] = [
    ("t", "assignments"),
    ("i", "shortenifs"),
    ("p", "nonvec_parms"),
    ("c", "constant_names"),
    ("cs", "constant_names_strings"),
    ("d", "dupconstdefs"),
    ("s", "noduplicatestrings"),
    ("l", "locals"),
    ("n", "function_names"),
    ("f", "filenames"),
    ("r", "overlaptemps"),
    ("a", "constantarithmatic"),
    ("pf", "precache_file"),
    ("ro", "return_only"),
    ("cj", "compound_jumps"),
    ("sf", "strip_functions"),
    ("lo", "locals_overlapping"),
    ("vc", "vectorcalls"),
    ("cf", "class_fields"),
    ("uf", "strip_unused_fields"),
];

/// The `opname` of each row of `pr_opcodes[]`, in its order, which `#pragma opcode` matches in any
/// case.
pub(crate) const OPCODES: [&str; 413] = [
    "DONE",
    "MUL_F",
    "MUL_V",
    "MUL_FV",
    "MUL_VF",
    "DIV_F",
    "ADD_F",
    "ADD_V",
    "SUB_F",
    "SUB_V",
    "EQ_F",
    "EQ_V",
    "EQ_S",
    "EQ_E",
    "EQ_FNC",
    "NE_F",
    "NE_V",
    "NE_S",
    "NE_E",
    "NE_FNC",
    "LE_F",
    "GE_F",
    "LT_F",
    "GT_F",
    "LOADF_F",
    "LOADF_V",
    "LOADF_S",
    "LOADF_ENT",
    "LOADF_FLD",
    "LOADF_FNC",
    "FLDADDRESS",
    "STORE_F",
    "STORE_V",
    "STORE_S",
    "STORE_ENT",
    "STORE_FLD",
    "STORE_FNC",
    "STOREP_F",
    "STOREP_V",
    "STOREP_S",
    "STOREP_ENT",
    "STOREP_FLD",
    "STOREP_FNC",
    "RETURN",
    "NOT_F",
    "NOT_V",
    "NOT_S",
    "NOT_ENT",
    "NOT_FNC",
    "IF",
    "IFNOT",
    "CALL0",
    "CALL1",
    "CALL2",
    "CALL3",
    "CALL4",
    "CALL5",
    "CALL6",
    "CALL7",
    "CALL8",
    "STATE",
    "GOTO",
    "AND_F",
    "OR_F",
    "BITAND",
    "BITOR",
    "MULSTORE_F",
    "MULSTORE_VF",
    "MULSTOREP_F",
    "MULSTOREP_VF",
    "DIVSTORE_F",
    "DIVSTOREP_F",
    "ADDSTORE_F",
    "ADDSTORE_V",
    "ADDSTOREP_F",
    "ADDSTOREP_V",
    "SUBSTORE_F",
    "SUBSTORE_V",
    "SUBSTOREP_F",
    "SUBSTOREP_V",
    "FETCH_GBL_F",
    "FETCH_GBL_V",
    "FETCH_GBL_S",
    "FETCH_GBL_E",
    "FETCH_GBL_FNC",
    "CSTATE",
    "CWSTATE",
    "THINKTIME",
    "BITSETSTORE_F",
    "BITSETSTOREP_F",
    "BITCLRSTORE_F",
    "BITCLRSTOREP_F",
    "RAND0",
    "RAND1",
    "RAND2",
    "RANDV0",
    "RANDV1",
    "RANDV2",
    "SWITCH_F",
    "SWITCH_V",
    "SWITCH_S",
    "SWITCH_E",
    "SWITCH_FNC",
    "CASE",
    "CASERANGE",
    "CALL1H",
    "CALL2H",
    "CALL3H",
    "CALL4H",
    "CALL5H",
    "CALL6H",
    "CALL7H",
    "CALL8H",
    "STORE_I",
    "STORE_IF",
    "STORE_FI",
    "ADD_I",
    "ADD_FI",
    "ADD_IF",
    "SUB_I",
    "SUB_FI",
    "SUB_IF",
    "CONV_IF",
    "CONV_FI",
    "CONVP_IF",
    "CONVP_FI",
    "LOADF_I",
    "STOREP_I",
    "STOREP_IF",
    "STOREP_FI",
    "BITAND_I",
    "BITOR_I",
    "MUL_I",
    "DIV_I",
    "EQ_I",
    "NE_I",
    "IFNOTS",
    "IFS",
    "NOT_I",
    "DIV_VF",
    "BITXOR_I",
    "RSHIFT_I",
    "LSHIFT_I",
    "GLOBALADDRESS",
    "ADD_PIW",
    "LOADA_F",
    "LOADA_V",
    "LOADA_S",
    "LOADA_ENT",
    "LOADA_FLD",
    "LOADA_FNC",
    "LOADA_I",
    "STORE_P",
    "LOADF_P",
    "LOADP_F",
    "LOADP_V",
    "LOADP_S",
    "LOADP_ENT",
    "LOADP_FLD",
    "LOADP_FNC",
    "LOADP_I",
    "LE_I",
    "GE_I",
    "LT_I",
    "GT_I",
    "LE_IF",
    "GE_IF",
    "LT_IF",
    "GT_IF",
    "LE_FI",
    "GE_FI",
    "LT_FI",
    "GT_FI",
    "EQ_IF",
    "EQ_FI",
    "ADD_SF",
    "SUB_S",
    "STOREP_C",
    "LOADP_C",
    "MUL_IF",
    "MUL_FI",
    "MUL_VI",
    "MUL_IV",
    "DIV_IF",
    "DIV_FI",
    "BITAND_IF",
    "BITOR_IF",
    "BITAND_FI",
    "BITOR_FI",
    "AND_I",
    "OR_I",
    "AND_IF",
    "OR_IF",
    "AND_FI",
    "OR_FI",
    "NE_IF",
    "NE_FI",
    "GSTOREP_I",
    "GSTOREP_F",
    "GSTOREP_ENT",
    "GSTOREP_FLD",
    "GSTOREP_S",
    "GSTOREP_FNC",
    "GSTOREP_V",
    "GADDRESS",
    "GLOAD_I",
    "GLOAD_F",
    "GLOAD_FLD",
    "GLOAD_ENT",
    "GLOAD_S",
    "GLOAD_FNC",
    "BOUNDCHECK",
    "UNUSED",
    "PUSH",
    "POP",
    "SWITCH_I",
    "GLOAD_V",
    "IF_F",
    "IFNOT_F",
    "STOREF_V",
    "STOREF_F",
    "STOREF_S",
    "STOREF_I",
    "STOREP_I8",
    "LOADP_U8",
    "LE_U",
    "LT_U",
    "DIV_U",
    "RSHIFT_U",
    "ADD_I64",
    "SUB_I64",
    "MUL_I64",
    "DIV_I64",
    "BITAND_I64",
    "BITOR_I64",
    "BITXOR_I64",
    "LSHIFT_I64I",
    "RSHIFT_I64I",
    "LE_I64",
    "LT_I64",
    "EQ_I64",
    "NE_I64",
    "LE_U64",
    "LT_U64",
    "DIV_U64",
    "RSHIFT_U64I",
    "STORE_I64",
    "STOREP_I64",
    "STOREF_I64",
    "LOADF_I64",
    "LOADA_I64",
    "LOADP_I64",
    "CONV_UI64",
    "CONV_II64",
    "CONV_I64I",
    "CONV_FD",
    "CONV_DF",
    "CONV_I64F",
    "CONV_FI64",
    "CONV_I64D",
    "CONV_DI64",
    "ADD_D",
    "SUB_D",
    "MUL_D",
    "DIV_D",
    "LE_D",
    "LT_D",
    "EQ_D",
    "NE_D",
    "STOREP_I16",
    "LOADP_I16",
    "LOADP_U16",
    "LOADP_I8",
    "BITEXTEND_I",
    "BITEXTEND_U",
    "BITCOPY_I",
    "CONV_UF",
    "CONV_FU",
    "CONV_U64D",
    "CONV_DU64",
    "CONV_U64F",
    "CONV_FU64",
    "OP_EMULATED",
    "BITSET_I",
    "BITSETP_I",
    "BITCLR_I",
    "MULSTORE_I",
    "DIVSTORE_I",
    "ADDSTORE_I",
    "SUBSTORE_I",
    "MULSTOREP_I",
    "DIVSTOREP_I",
    "ADDSTOREP_I",
    "SUBSTOREP_I",
    "MULSTORE_IF",
    "MULSTOREP_IF",
    "DIVSTORE_IF",
    "DIVSTOREP_IF",
    "ADDSTORE_IF",
    "ADDSTOREP_IF",
    "SUBSTORE_IF",
    "SUBSTOREP_IF",
    "MULSTORE_FI",
    "MULSTOREP_FI",
    "DIVSTORE_FI",
    "DIVSTOREP_FI",
    "ADDSTORE_FI",
    "ADDSTOREP_FI",
    "SUBSTORE_FI",
    "SUBSTOREP_FI",
    "MULSTORE_VI",
    "MULSTOREP_VI",
    "LOADA_STRUCT",
    "LOADP_P",
    "STOREP_P",
    "BITNOT_F",
    "BITNOT_I",
    "EQ_P",
    "NE_P",
    "LE_P",
    "GE_P",
    "LT_P",
    "GT_P",
    "ANDSTORE_F",
    "BITCLR_F",
    "BITCLR_I",
    "BITCLR_V",
    "ADD_SI",
    "ADD_IS",
    "ADD_PF",
    "ADD_FP",
    "ADD_PI",
    "ADD_IP",
    "ADD_PU",
    "ADD_UP",
    "SUB_SI",
    "SUB_PF",
    "SUB_PI",
    "SUB_PU",
    "SUB_PP",
    "MOD_F",
    "MOD_I",
    "MOD_FI",
    "MOD_IF",
    "MOD_V",
    "BITXOR_F",
    "RSHIFT_F",
    "LSHIFT_F",
    "RSHIFT_IF",
    "LSHIFT_IF",
    "RSHIFT_FI",
    "LSHIFT_FI",
    "AND_ANY",
    "OR_ANY",
    "ADD_EI",
    "ADD_EF",
    "SUB_EI",
    "SUB_EF",
    "BITAND_V",
    "BITOR_V",
    "BITNOT_V",
    "BITXOR_V",
    "POW_F",
    "POW_I",
    "POW_FI",
    "POW_IF",
    "CROSS_V",
    "EQ_FLD",
    "NE_FLD",
    "SPACESHIP_F",
    "SPACESHIP_S",
    "ADD_U",
    "SUB_U",
    "MUL_U",
    "MOD_U",
    "BITAND_U",
    "BITOR_U",
    "BITXOR_U",
    "BITNOT_U",
    "BITCLR_U",
    "LSHIFT_U",
    "GE_U",
    "GT_U",
    "EQ_U",
    "NE_U",
    "BITNOT_I64",
    "BITCLR_I64",
    "GE_I64",
    "GT_I64",
    "ADD_U64",
    "SUB_U64",
    "MUL_U64",
    "MOD_U64",
    "BITAND_U64",
    "BITOR_U64",
    "BITXOR_U64",
    "BITNOT_U64",
    "BITCLR_U64",
    "LSHIFT_U64I",
    "GE_U64",
    "GT_U64",
    "EQ_U64",
    "NE_U64",
    "BITAND_D",
    "BITOR_D",
    "BITXOR_D",
    "BITNOT_D",
    "BITCLR_D",
    "LSHIFT_DI",
    "RSHIFT_DI",
    "GE_D",
    "GT_D",
    "WSTATE",
];

/// `OP_NUMREALOPS`: the rows of [`OPCODES`] from here on are internal.
pub(crate) const REAL_OPCODES: usize = 282;

/// An output format of `targets[]` (`qcc_targetformat_t`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetFormat {
    /// `QCF_STANDARD`.
    Standard,
    /// `QCF_QSS`.
    Qss,
    /// `QCF_HEXEN2`.
    Hexen2,
    /// `QCF_UHEXEN2`.
    Uhexen2,
    /// `QCF_KK7`.
    Kk7,
    /// `QCF_FTE`.
    Fte,
    /// `QCF_FTEH2`.
    Fteh2,
    /// `QCF_FTEDEBUG`.
    FteDebug,
    /// `QCF_DARKPLACES`.
    Darkplaces,
    /// `QCF_QTEST`.
    Qtest,
}

/// `targets[]`: each name `#pragma TARGET` matches in any case, its format and its default
/// revision.
pub(crate) const TARGETS: [(&str, TargetFormat, u32); 21] = [
    ("standard", TargetFormat::Standard, 0),
    ("vanilla", TargetFormat::Standard, 0),
    ("q1", TargetFormat::Standard, 0),
    ("id", TargetFormat::Standard, 0),
    ("quakec", TargetFormat::Standard, 0),
    ("qs", TargetFormat::Standard, 0),
    ("qss", TargetFormat::Qss, 0),
    ("hexen2", TargetFormat::Hexen2, 0),
    ("h2", TargetFormat::Hexen2, 0),
    ("uhexen2", TargetFormat::Uhexen2, 0),
    ("kkqwsv", TargetFormat::Kk7, 0),
    ("kk7", TargetFormat::Kk7, 0),
    ("version7", TargetFormat::Kk7, 0),
    ("fte", TargetFormat::Fte, 5768),
    ("fteh2", TargetFormat::Fteh2, 5768),
    ("ftedebug", TargetFormat::FteDebug, 5768),
    ("debug", TargetFormat::FteDebug, 5768),
    ("quake2c", TargetFormat::Fte, 5744),
    ("darkplaces", TargetFormat::Darkplaces, 12901),
    ("dp", TargetFormat::Darkplaces, 12901),
    ("qtest", TargetFormat::Qtest, 0),
];

/// `QCTARGVER_FTE_MAX` (`QCTARGVER_FTE_PRELOCS`), `QCTARGVER_DP_MAX` and `QCTARGVER_QSS_MAX`: the
/// highest revision of each format; any other format has only revision 0.
pub(crate) const FTE_MAX_REVISION: u32 = 6614;
/// See [`FTE_MAX_REVISION`].
pub(crate) const DP_MAX_REVISION: u32 = 20_250_104;
/// See [`FTE_MAX_REVISION`].
pub(crate) const QSS_MAX_REVISION: u32 = 0;
