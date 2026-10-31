// SPDX-License-Identifier: GPL-3.0-or-later

//! The FTEQCC compiler flags the lexer, the parser and the semantic model read where a token
//! stands (`docs/spec/dialect-fteqcc.md` FTEF-12.12, FTEF-21), the casts' among them (FTEF-22),
//! the truth tests' (FTEF-30), with the defaults `#pragma noref`,
//! `#pragma nosave` and `#pragma defaultstatic` set for each definition, and the changes a
//! `#pragma` makes to them, which the parser applies as it reads the tokens, with the changes
//! its own `extern "C"` blocks make (FTEF-21.17).

use qc_lexer::{LexFlag, LexFlags};

use crate::fte_tables::FteFlag;

/// The flags [`CompilerFlags`] keeps, each with the bit it is, and the lexer's flag it is, if
/// the lexer reads it.
const KEPT: [(FteFlag, u32, Option<LexFlag>); 17] = [
    (FteFlag::Qcfuncs, 1, Some(LexFlag::QcFuncs)),
    (FteFlag::Qccx, 1 << 1, Some(LexFlag::Qccx)),
    (FteFlag::Utf8strings, 1 << 2, Some(LexFlag::Utf8Strings)),
    (FteFlag::Ilp32, 1 << 3, Some(LexFlag::Ilp32)),
    (FteFlag::Dblstarexp, 1 << 4, Some(LexFlag::Ssp)),
    (FteFlag::AssumeInteger, 1 << 5, Some(LexFlag::AssumeInt)),
    (
        FteFlag::Macroinstrings,
        1 << 6,
        Some(LexFlag::MacroInStrings),
    ),
    (FteFlag::AssumeDouble, 1 << 7, Some(LexFlag::AssumeDouble)),
    (FteFlag::Cpriority, 1 << 8, None),
    (FteFlag::Assumevar, 1 << 9, None),
    (FteFlag::PrSubscopedlocals, 1 << 10, None),
    (FteFlag::Attributes, 1 << 11, None),
    (FteFlag::Laxcasts, 1 << 16, None),
    (FteFlag::Typeexplicit, 1 << 17, None),
    (FteFlag::Undefwordsize, 1 << 18, None),
    (FteFlag::Ifvector, 1 << 19, None),
    (FteFlag::Vectorlogic, 1 << 20, None),
];

/// The bit of [`CompilerFlags`] that holds the operator priorities, C's when set.
const C_PRIORITIES: u32 = 1 << 12;

/// A default `QCC_PR_ParseDefs` starts each definition with, which a pragma sets (FTEF-12.6,
/// FTEF-21.14): `pbool isstatic=defaultstatic; ... pbool noref = defaultnoref; pbool nosave =
/// defaultnosave;`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PragmaDefault {
    /// `defaultstatic`, set by `#pragma defaultstatic N`.
    Static,
    /// `defaultnoref`, set by `#pragma noref N`.
    Noref,
    /// `defaultnosave`, set by `#pragma nosave N`.
    Nosave,
}

impl PragmaDefault {
    /// The default `pragma` sets, for `#pragma noref`, `nosave` and `defaultstatic`.
    #[must_use]
    pub const fn of_pragma(pragma: qc_lexer::Pragma) -> Option<Self> {
        use qc_lexer::Pragma as P;
        match pragma {
            P::Noref => Some(Self::Noref),
            P::Nosave => Some(Self::Nosave),
            P::DefaultStatic => Some(Self::Static),
            P::DontCompileThisFile
            | P::Copyright
            | P::Compress
            | P::ForceCrc
            | P::Framerate
            | P::Once
            | P::File
            | P::Line
            | P::IncludeDir
            | P::Autoproto
            | P::Wrasm
            | P::Optimise
            | P::Optimize
            | P::SourceFile
            | P::Target
            | P::ProgsSrc
            | P::ProgsDat
            | P::Opcode
            | P::Keyword
            | P::Flag
            | P::Warning => None,
        }
    }

    /// Its bit in [`CompilerFlags`].
    const fn bit(self) -> u32 {
        match self {
            Self::Static => 1 << 13,
            Self::Noref => 1 << 14,
            Self::Nosave => 1 << 15,
        }
    }
}

/// The operator priorities `QCC_PrioritiseOpcodes` set last (FTEF-21.4): it runs once a compile
/// starts, after the command line, and again at each `#pragma flag` that names `cpriority`, and
/// reads `flag_cpriority` then. So the priorities are not the flag: an `extern "C"` block sets
/// the flag and leaves the priorities as they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Priorities {
    /// QuakeC's, `flag_cpriority` off: `!` and the comparisons share a priority, and the
    /// bitwise operators are `*`'s.
    Qc,
    /// C's, `flag_cpriority` on.
    C,
}

/// Which of the flags the lexer, the parser and the semantic model read are on where a token
/// stands, and the operator priorities there (FTEF-12.12, FTEF-21). [`Default`] is FTEQCC's
/// default: `qcfuncs`, `ifvector` and `vectorlogic` on, QuakeC's priorities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CompilerFlags {
    bits: u32,
}

impl Default for CompilerFlags {
    fn default() -> Self {
        Self { bits: 0 }
            .with(FteFlag::Qcfuncs, true)
            .with(FteFlag::Ifvector, true)
            .with(FteFlag::Vectorlogic, true)
    }
}

impl CompilerFlags {
    /// The bit of `flag`, 0 for a flag these do not keep.
    fn bit(flag: FteFlag) -> u32 {
        KEPT.iter()
            .find(|(kept, _, _)| *kept == flag)
            .map_or(0, |&(_, bit, _)| bit)
    }

    /// Whether `flag` is on. A flag these do not keep is off.
    #[must_use]
    pub fn has(self, flag: FteFlag) -> bool {
        self.bits & Self::bit(flag) != 0
    }

    /// These flags with `flag` turned on or off; a flag these do not keep changes nothing.
    #[must_use]
    pub fn with(self, flag: FteFlag, on: bool) -> Self {
        self.set(Self::bit(flag), on)
    }

    /// The operator priorities.
    #[must_use]
    pub const fn priorities(self) -> Priorities {
        if self.bits & C_PRIORITIES == 0 {
            Priorities::Qc
        } else {
            Priorities::C
        }
    }

    /// These flags with the operator priorities `priorities`.
    #[must_use]
    pub const fn with_priorities(self, priorities: Priorities) -> Self {
        self.set(
            C_PRIORITIES,
            match priorities {
                Priorities::Qc => false,
                Priorities::C => true,
            },
        )
    }

    /// Whether the pragma default `default` is on.
    #[must_use]
    pub const fn default_on(self, default: PragmaDefault) -> bool {
        self.bits & default.bit() != 0
    }

    /// These flags with the pragma default `default` on or off.
    #[must_use]
    pub const fn with_default(self, default: PragmaDefault, on: bool) -> Self {
        self.set(default.bit(), on)
    }

    /// The flags among these the lexer reads (FTEF-12.12).
    #[must_use]
    pub fn lex(self) -> LexFlags {
        KEPT.iter()
            .fold(LexFlags::NONE, |flags, &(_, bit, lex)| match lex {
                Some(lex) => flags.with(lex, self.bits & bit != 0),
                None => flags,
            })
    }

    /// The flags in force at byte `at` of a text whose flags change at `marks`, each the byte it
    /// holds from and the flags, in order: the last mark at or before `at`, else the defaults.
    #[must_use]
    pub fn at(marks: &[(usize, Self)], at: usize) -> Self {
        marks
            .iter()
            .take_while(|(from, _)| *from <= at)
            .last()
            .map_or_else(Self::default, |(_, flags)| *flags)
    }

    const fn set(self, bit: u32, on: bool) -> Self {
        Self {
            bits: if on {
                self.bits | bit
            } else {
                self.bits & !bit
            },
        }
    }
}

/// A change to [`CompilerFlags`] at a token: the bits it sets and their values. The first of a
/// file holds the whole state; a `#pragma flag` sets only the flags it names, and the priorities
/// when it names `cpriority`, so a flag an `extern "C"` block changed holds unless the pragma
/// names it (FTEF-21.17).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FlagEdit {
    mask: u32,
    bits: u32,
}

impl FlagEdit {
    /// The edit that sets every flag, and the priorities, to `flags`'.
    #[must_use]
    pub const fn whole(flags: CompilerFlags) -> Self {
        Self {
            mask: u32::MAX,
            bits: flags.bits,
        }
    }

    /// The edit that sets the flags `set` to their values in `now`, and the priorities to
    /// `now`'s when `set` names `cpriority`; a flag [`CompilerFlags`] does not keep sets none.
    #[must_use]
    pub fn of_flags(set: &[FteFlag], now: CompilerFlags) -> Self {
        let priorities = set.contains(&FteFlag::Cpriority).then_some(C_PRIORITIES);
        let mask = set
            .iter()
            .map(|&flag| CompilerFlags::bit(flag))
            .chain(priorities)
            .fold(0_u32, std::ops::BitOr::bitor);
        Self {
            mask,
            bits: now.bits & mask,
        }
    }

    /// The edit a `#pragma noref`, `nosave` or `defaultstatic` makes: `default` set to `on`
    /// (FTEF-12.6, FTEF-21.14).
    #[must_use]
    pub const fn of_default(default: PragmaDefault, on: bool) -> Self {
        Self {
            mask: default.bit(),
            bits: if on { default.bit() } else { 0 },
        }
    }

    /// Whether it changes nothing.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.mask == 0
    }

    /// `flags` after this edit.
    #[must_use]
    pub const fn apply(self, flags: CompilerFlags) -> CompilerFlags {
        // The merge written with `^`, not `(flags & !mask) | (bits & mask)`, whose `|` joins
        // disjoint bits and so reads the same as `^` to a mutation test.
        CompilerFlags {
            bits: flags.bits ^ ((flags.bits ^ self.bits) & self.mask),
        }
    }
}

#[cfg(test)]
mod tests;
