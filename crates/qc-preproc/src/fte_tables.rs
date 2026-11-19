// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, compiler_flag, optimisations
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, pr_opcodes, targets, QCC_OPCodeSetTargetName, QCC_OPCodeSetTarget

//! The tables FTEQCC reads its build pragmas against (`docs/spec/dialect-fteqcc.md` FTEF-12.11):
//! the names of its compiler flags and keywords, its optimisations, its opcodes and its targets,
//! as data, written out of `qccmain.c` and `qcc_pr_comp.c` at the pinned commit.

use qc_progs::Op;

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

/// Whether `name` is the `opname` of a row of `pr_opcodes[]`, compared exactly (`STRCMP`), as
/// `QCC_PR_ParseAsm` reads an `asm` instruction's first token (FTEF-20.5).
#[must_use]
pub fn is_opcode_name(name: &str) -> bool {
    Op::named(name).is_some()
}
