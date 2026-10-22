// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_CheckKeyword, QCC_PR_CheckName
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_DummyDef

//! FTEQCC's keywords as its parser reads them (`docs/spec/dialect-fteqcc.md` FTEF-20): which of
//! the `keyword_*` flags of `compiler_flag[]` are on at a token ([`Keywords`]), how a word is
//! matched against one (`QCC_PR_CheckKeyword`, [`Keywords::checks`]), and the keywords a
//! definition's name turns off (`QCC_PR_DummyDef`'s `KEYWORD` list,
//! [`Keywords::named_definition`]). The preprocessor marks where a `#pragma keyword` or
//! `#pragma flag` changes them ([`KeywordEdit`], `FileOutput::keywords`), and the parser reads
//! each keyword with the flags in force at its token. Implements FTEF-20.1, FTEF-20.2 and
//! FTEF-20.4. Ported from FTEQCC's behaviour under ADR-0017's clean-port policy.

use crate::fte_tables::FteFlag;

/// One of FTEQCC's `keyword_*` flags (`compiler_flag[]` in `qccmain.c`), in the table's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Keyword {
    /// `keyword_asm`.
    Asm,
    /// `keyword_break`.
    Break,
    /// `keyword_case`.
    Case,
    /// `keyword_class`.
    Class,
    /// `keyword_accessor`.
    Accessor,
    /// `keyword_const`.
    Const,
    /// `keyword_continue`.
    Continue,
    /// `keyword_default`.
    Default,
    /// `keyword_entity`.
    Entity,
    /// `keyword_enum`.
    Enum,
    /// `keyword_enumflags`.
    Enumflags,
    /// `keyword_extern`.
    Extern,
    /// `keyword_float`.
    Float,
    /// `keyword_for`.
    For,
    /// `keyword_goto`.
    Goto,
    /// `keyword_int`.
    Int,
    /// `keyword_integer`.
    Integer,
    /// `keyword_double`.
    Double,
    /// `keyword_long`.
    Long,
    /// `keyword_short`.
    Short,
    /// `keyword_char`.
    Char,
    /// `keyword_signed`.
    Signed,
    /// `keyword_unsigned`.
    Unsigned,
    /// `keyword_register`.
    Register,
    /// `keyword_volatile`.
    Volatile,
    /// `keyword_noref`.
    Noref,
    /// `keyword_unused`.
    Unused,
    /// `keyword_used`.
    Used,
    /// `keyword_local`.
    Local,
    /// `keyword_static`.
    Static,
    /// `keyword_nonstatic`.
    Nonstatic,
    /// `keyword_ignore`.
    Ignore,
    /// `keyword_auto`.
    Auto,
    /// `keyword_nosave`.
    Nosave,
    /// `keyword_inline`.
    Inline,
    /// `keyword_strip`.
    Strip,
    /// `keyword_shared`.
    Shared,
    /// `keyword_state`.
    State,
    /// `keyword_optional`.
    Optional,
    /// `keyword_inout`, which enables `inout`, `in` and `out`.
    Inout,
    /// `keyword_string`.
    String,
    /// `keyword_struct`.
    Struct,
    /// `keyword_switch`.
    Switch,
    /// `keyword_thinktime`.
    Thinktime,
    /// `keyword_until`.
    Until,
    /// `keyword_loop`.
    Loop,
    /// `keyword_typedef`.
    Typedef,
    /// `keyword_union`.
    Union,
    /// `keyword_var`.
    Var,
    /// `keyword_vector`.
    Vector,
    /// `keyword_wrap`.
    Wrap,
    /// `keyword_weak`.
    Weak,
    /// `keyword_accumulate`.
    Accumulate,
    /// `keyword_using`.
    Using,
}

impl Keyword {
    /// Every keyword flag, in `compiler_flag[]`'s order.
    pub const ALL: [Self; 54] = [
        Self::Asm,
        Self::Break,
        Self::Case,
        Self::Class,
        Self::Accessor,
        Self::Const,
        Self::Continue,
        Self::Default,
        Self::Entity,
        Self::Enum,
        Self::Enumflags,
        Self::Extern,
        Self::Float,
        Self::For,
        Self::Goto,
        Self::Int,
        Self::Integer,
        Self::Double,
        Self::Long,
        Self::Short,
        Self::Char,
        Self::Signed,
        Self::Unsigned,
        Self::Register,
        Self::Volatile,
        Self::Noref,
        Self::Unused,
        Self::Used,
        Self::Local,
        Self::Static,
        Self::Nonstatic,
        Self::Ignore,
        Self::Auto,
        Self::Nosave,
        Self::Inline,
        Self::Strip,
        Self::Shared,
        Self::State,
        Self::Optional,
        Self::Inout,
        Self::String,
        Self::Struct,
        Self::Switch,
        Self::Thinktime,
        Self::Until,
        Self::Loop,
        Self::Typedef,
        Self::Union,
        Self::Var,
        Self::Vector,
        Self::Wrap,
        Self::Weak,
        Self::Accumulate,
        Self::Using,
    ];

    /// The row of `compiler_flag[]` that sets it.
    #[must_use]
    pub const fn flag(self) -> FteFlag {
        match self {
            Self::Asm => FteFlag::AsmKeyword,
            Self::Break => FteFlag::BreakKeyword,
            Self::Case => FteFlag::CaseKeyword,
            Self::Class => FteFlag::ClassKeyword,
            Self::Accessor => FteFlag::AccessorKeyword,
            Self::Const => FteFlag::ConstKeyword,
            Self::Continue => FteFlag::ContinueKeyword,
            Self::Default => FteFlag::DefaultKeyword,
            Self::Entity => FteFlag::EntityKeyword,
            Self::Enum => FteFlag::EnumKeyword,
            Self::Enumflags => FteFlag::EnumflagsKeyword,
            Self::Extern => FteFlag::ExternKeyword,
            Self::Float => FteFlag::FloatKeyword,
            Self::For => FteFlag::ForKeyword,
            Self::Goto => FteFlag::GotoKeyword,
            Self::Int => FteFlag::IntKeyword,
            Self::Integer => FteFlag::IntegerKeyword,
            Self::Double => FteFlag::DoubleKeyword,
            Self::Long => FteFlag::LongKeyword,
            Self::Short => FteFlag::ShortKeyword,
            Self::Char => FteFlag::CharKeyword,
            Self::Signed => FteFlag::SignedKeyword,
            Self::Unsigned => FteFlag::UnsignedKeyword,
            Self::Register => FteFlag::RegisterKeyword,
            Self::Volatile => FteFlag::VolatileKeyword,
            Self::Noref => FteFlag::NorefKeyword,
            Self::Unused => FteFlag::UnusedKeyword,
            Self::Used => FteFlag::UsedKeyword,
            Self::Local => FteFlag::LocalKeyword,
            Self::Static => FteFlag::StaticKeyword,
            Self::Nonstatic => FteFlag::NonstaticKeyword,
            Self::Ignore => FteFlag::IgnoreKeyword,
            Self::Auto => FteFlag::AutoKeyword,
            Self::Nosave => FteFlag::NosaveKeyword,
            Self::Inline => FteFlag::InlineKeyword,
            Self::Strip => FteFlag::StripKeyword,
            Self::Shared => FteFlag::SharedKeyword,
            Self::State => FteFlag::StateKeyword,
            Self::Optional => FteFlag::OptionalKeyword,
            Self::Inout => FteFlag::InoutKeyword,
            Self::String => FteFlag::StringKeyword,
            Self::Struct => FteFlag::StructKeyword,
            Self::Switch => FteFlag::SwitchKeyword,
            Self::Thinktime => FteFlag::ThinktimeKeyword,
            Self::Until => FteFlag::UntilKeyword,
            Self::Loop => FteFlag::LoopKeyword,
            Self::Typedef => FteFlag::TypedefKeyword,
            Self::Union => FteFlag::UnionKeyword,
            Self::Var => FteFlag::VarKeyword,
            Self::Vector => FteFlag::VectorKeyword,
            Self::Wrap => FteFlag::WrapKeyword,
            Self::Weak => FteFlag::WeakKeyword,
            Self::Accumulate => FteFlag::AccumulateKeyword,
            Self::Using => FteFlag::UsingKeyword,
        }
    }

    /// The keyword `flag` is, if it is one.
    #[must_use]
    pub fn of_flag(flag: FteFlag) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.flag() == flag)
    }

    /// Its bit in [`Keywords`]: its place in [`Self::ALL`].
    const fn bit(self) -> u64 {
        let at: u32 = match self {
            Self::Asm => 0,
            Self::Break => 1,
            Self::Case => 2,
            Self::Class => 3,
            Self::Accessor => 4,
            Self::Const => 5,
            Self::Continue => 6,
            Self::Default => 7,
            Self::Entity => 8,
            Self::Enum => 9,
            Self::Enumflags => 10,
            Self::Extern => 11,
            Self::Float => 12,
            Self::For => 13,
            Self::Goto => 14,
            Self::Int => 15,
            Self::Integer => 16,
            Self::Double => 17,
            Self::Long => 18,
            Self::Short => 19,
            Self::Char => 20,
            Self::Signed => 21,
            Self::Unsigned => 22,
            Self::Register => 23,
            Self::Volatile => 24,
            Self::Noref => 25,
            Self::Unused => 26,
            Self::Used => 27,
            Self::Local => 28,
            Self::Static => 29,
            Self::Nonstatic => 30,
            Self::Ignore => 31,
            Self::Auto => 32,
            Self::Nosave => 33,
            Self::Inline => 34,
            Self::Strip => 35,
            Self::Shared => 36,
            Self::State => 37,
            Self::Optional => 38,
            Self::Inout => 39,
            Self::String => 40,
            Self::Struct => 41,
            Self::Switch => 42,
            Self::Thinktime => 43,
            Self::Until => 44,
            Self::Loop => 45,
            Self::Typedef => 46,
            Self::Union => 47,
            Self::Var => 48,
            Self::Vector => 49,
            Self::Wrap => 50,
            Self::Weak => 51,
            Self::Accumulate => 52,
            Self::Using => 53,
        };
        1 << at
    }
}

/// How `QCC_PR_CheckKeyword` is told whether a keyword is on: by its flag, or always
/// (`QCC_PR_CheckKeyword(1, ...)`, `true`) or never (`false`), when only `__WORD` takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enabled {
    /// The keyword flag's setting.
    By(Keyword),
    /// Always on: `return`, `if`, `else`, `while`, `do` and `not`, which are `#define`d as 1,
    /// `_Bool`, `sizeof`, `_length`, and a class member's qualifiers.
    Always,
    /// Never on, so only `__WORD` takes it: `deprecated`.
    Never,
}

/// `keywords_coexist`, the `kce` flag: a definition named as a keyword leaves it on.
const COEXIST: u64 = 1 << 60;
/// `flag_caseinsensitive`, the `caseinsens` flag: a keyword is matched in any case.
const CASE_INSENSITIVE: u64 = 1 << 61;
/// The target is Hexen II's (`QCF_HEXEN2` or `QCF_UHEXEN2`), under which a definition named
/// `float` does not turn the keyword off ("hmm... hexen2 requires this...").
const HEXEN2: u64 = 1 << 62;

/// Which of FTEQCC's keywords are on at a token, with the flags `QCC_PR_CheckKeyword` and
/// `QCC_PR_DummyDef` read beside them: `kce`, `caseinsens` and whether the target is Hexen II's
/// (FTEF-20.1). [`Default`] is FTEQCC's default: `compiler_flag[]`'s `FLAG_ASDEFAULT` rows on,
/// `kce` among them, and the standard target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Keywords {
    bits: u64,
}

/// The keywords `compiler_flag[]` marks `FLAG_ASDEFAULT` (`defaultkeyword` and `typekeyword`),
/// which a test checks against [`crate::FlagSettings`]'s table.
const DEFAULT_ON: [Keyword; 36] = [
    Keyword::Asm,
    Keyword::Break,
    Keyword::Case,
    Keyword::Class,
    Keyword::Accessor,
    Keyword::Const,
    Keyword::Continue,
    Keyword::Default,
    Keyword::Entity,
    Keyword::Enum,
    Keyword::Enumflags,
    Keyword::Extern,
    Keyword::Float,
    Keyword::For,
    Keyword::Goto,
    Keyword::Int,
    Keyword::Integer,
    Keyword::Unsigned,
    Keyword::Noref,
    Keyword::Local,
    Keyword::Static,
    Keyword::Nonstatic,
    Keyword::Nosave,
    Keyword::Inline,
    Keyword::Strip,
    Keyword::Shared,
    Keyword::Optional,
    Keyword::String,
    Keyword::Struct,
    Keyword::Switch,
    Keyword::Typedef,
    Keyword::Union,
    Keyword::Var,
    Keyword::Vector,
    Keyword::Wrap,
    Keyword::Weak,
];

impl Default for Keywords {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Keywords {
    /// No keyword on, `kce` and `caseinsens` off and the standard target.
    pub const NONE: Self = Self { bits: 0 };

    /// FTEQCC's defaults: [`DEFAULT_ON`] and `kce` on, the standard target.
    const DEFAULT: Self = {
        let mut bits = COEXIST;
        let mut rest: &[Keyword] = &DEFAULT_ON;
        while let Some((first, tail)) = rest.split_first() {
            bits |= first.bit();
            rest = tail;
        }
        Self { bits }
    };

    /// Whether `keyword` is on.
    #[must_use]
    pub fn has(self, keyword: Keyword) -> bool {
        self.bits & keyword.bit() != 0
    }

    /// These keywords with `keyword` turned on or off.
    #[must_use]
    pub fn with(self, keyword: Keyword, on: bool) -> Self {
        self.set(keyword.bit(), on)
    }

    /// Whether `kce` is on, under which a definition named as a keyword leaves it on
    /// (FTEF-20.4).
    #[must_use]
    pub const fn coexist(self) -> bool {
        self.bits & COEXIST != 0
    }

    /// Whether `caseinsens` is on, under which a bare keyword is matched in any case
    /// (FTEF-20.2).
    #[must_use]
    pub const fn case_insensitive(self) -> bool {
        self.bits & CASE_INSENSITIVE != 0
    }

    /// Whether the target is Hexen II's, `QCF_HEXEN2` or `QCF_UHEXEN2` (FTEF-20.4).
    #[must_use]
    pub const fn hexen2_target(self) -> bool {
        self.bits & HEXEN2 != 0
    }

    /// These keywords with `kce` set to `on`.
    #[must_use]
    pub const fn with_coexist(self, on: bool) -> Self {
        self.set(COEXIST, on)
    }

    /// These keywords with `caseinsens` set to `on`.
    #[must_use]
    pub const fn with_case_insensitive(self, on: bool) -> Self {
        self.set(CASE_INSENSITIVE, on)
    }

    /// These keywords under a Hexen II target or not.
    #[must_use]
    pub const fn with_hexen2_target(self, on: bool) -> Self {
        self.set(HEXEN2, on)
    }

    /// These keywords with the bit `bit` set to `on`.
    const fn set(self, bit: u64, on: bool) -> Self {
        Self {
            bits: if on {
                self.bits | bit
            } else {
                self.bits & !bit
            },
        }
    }

    /// The keywords in force at byte `at` of a text whose keywords change at `marks`, each the
    /// byte it holds from and the keywords, in order: the last mark at or before `at`, else the
    /// defaults (FTEF-20.1).
    #[must_use]
    pub fn at(marks: &[(usize, Self)], at: usize) -> Self {
        marks
            .iter()
            .take_while(|(from, _)| *from <= at)
            .last()
            .map_or_else(Self::default, |(_, keywords)| *keywords)
    }

    /// `QCC_PR_CheckKeyword(enabled, word)` of the name `token` (FTEF-20.2): a token that starts
    /// with `__` is the keyword when the rest is `word` in any case, whether or not it is on
    /// (`if (pr_token[0] == '_' && pr_token[1] == '_') { if (stricmp(string, pr_token+2)) return
    /// false; ...`); any other only when it is on, and is `word` in any case under `caseinsens`
    /// and exactly otherwise.
    #[must_use]
    pub fn checks(self, enabled: Enabled, word: &str, token: &str) -> bool {
        if let Some(rest) = token.strip_prefix("__") {
            return rest.eq_ignore_ascii_case(word);
        }
        let on = match enabled {
            Enabled::By(keyword) => self.has(keyword),
            Enabled::Always => true,
            Enabled::Never => false,
        };
        on && self.names(word, token)
    }

    /// `QCC_PR_CheckName(word)` of the name `token` (FTEF-20.2): `word` in any case under
    /// `caseinsens`, and exactly otherwise.
    #[must_use]
    pub fn names(self, word: &str, token: &str) -> bool {
        if self.case_insensitive() {
            token.eq_ignore_ascii_case(word)
        } else {
            token == word
        }
    }

    /// The keyword a new definition named `name` names, which `QCC_PR_DummyDef` warns of and,
    /// with `kce` off, turns off (FTEF-20.4): the name is exactly one of `var`, `thinktime`,
    /// `for`, `switch`, `case`, `local`, `default`, `goto`, `break` when the definition is no
    /// function, `continue`, `state`, `string`, `float` when the target is not Hexen II's,
    /// `entity`, `vector`, `const` or `asm`, and that keyword is on (`#define KEYWORD(x) if
    /// (!STRCMP(name, #x) && keyword_##x) ...`).
    #[must_use]
    pub fn named_definition(self, name: &str, function: bool) -> Option<Keyword> {
        let keyword = match name {
            "var" => Keyword::Var,
            "thinktime" => Keyword::Thinktime,
            "for" => Keyword::For,
            "switch" => Keyword::Switch,
            "case" => Keyword::Case,
            "local" => Keyword::Local,
            "default" => Keyword::Default,
            "goto" => Keyword::Goto,
            "break" if !function => Keyword::Break,
            "continue" => Keyword::Continue,
            "state" => Keyword::State,
            "string" => Keyword::String,
            "float" if !self.hexen2_target() => Keyword::Float,
            "entity" => Keyword::Entity,
            "vector" => Keyword::Vector,
            "const" => Keyword::Const,
            "asm" => Keyword::Asm,
            _ => return None,
        };
        self.has(keyword).then_some(keyword)
    }

    /// These keywords after a definition that names `keyword`: `keyword_##x=keywords_coexist;`
    /// (FTEF-20.4).
    #[must_use]
    pub fn after_definition(self, keyword: Keyword) -> Self {
        self.with(keyword, self.coexist())
    }
}

/// A change to [`Keywords`] at a token: the bits it sets and their values. The first holds the
/// whole state; a `#pragma keyword` or `#pragma flag` sets only the keywords it names, so a
/// keyword a definition turned off since stays off unless the pragma names it (FTEF-20.1,
/// FTEF-20.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeywordEdit {
    mask: u64,
    bits: u64,
}

impl KeywordEdit {
    /// The edit that sets every bit to `keywords`'.
    #[must_use]
    pub const fn whole(keywords: Keywords) -> Self {
        Self {
            mask: u64::MAX,
            bits: keywords.bits,
        }
    }

    /// The edit that sets the bits of the flags `set` to their values in `now`: each keyword,
    /// `kce` and `caseinsens` among them; any other flag sets none.
    pub(crate) fn of_flags(set: &[FteFlag], now: Keywords) -> Self {
        let mask = set.iter().fold(0_u64, |mask, &flag| {
            mask | match flag {
                FteFlag::KeywordsCoexist => COEXIST,
                FteFlag::Caseinsensitive => CASE_INSENSITIVE,
                flag => Keyword::of_flag(flag).map_or(0, Keyword::bit),
            }
        });
        Self {
            mask,
            bits: now.bits & mask,
        }
    }

    /// The edit that sets whether the target is Hexen II's.
    pub(crate) const fn hexen2_target(on: bool) -> Self {
        Self {
            mask: HEXEN2,
            bits: if on { HEXEN2 } else { 0 },
        }
    }

    /// Whether it changes nothing.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.mask == 0
    }

    /// `keywords` after this edit.
    #[must_use]
    pub const fn apply(self, keywords: Keywords) -> Keywords {
        Keywords {
            bits: (keywords.bits & !self.mask) | (self.bits & self.mask),
        }
    }
}

#[cfg(test)]
mod tests;
