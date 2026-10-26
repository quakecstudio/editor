// SPDX-License-Identifier: GPL-3.0-or-later

//! The FTEQCC compiler flags its lexer reads (`docs/spec/dialect-fteqcc.md` FTEF-12.12), which a
//! `#pragma flag` turns on and off as the file is read.

/// One of FTEQCC's compiler flags that changes how it lexes (FTEF-12.12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexFlag {
    /// `qcfuncs`, on by default: `\0` to `\9` in a string are Quake's yellow digits, not C's
    /// octal escapes.
    QcFuncs,
    /// `qccx`: QCCX's `:` between a string's parts, hexadecimal numbers and character constants
    /// that are a float's bits, and `%N` immediates.
    Qccx,
    /// `utf8`: a plain string's input and output are UTF-8.
    Utf8Strings,
    /// `ILP32`: a single `l` suffix keeps a number to 32 bits.
    Ilp32,
    /// `ssp`: `**` is the power operator `*^`.
    Ssp,
    /// `assumeint`: a number with no suffix is an integer.
    AssumeInt,
    /// `macroinstrings`: `#NAME` in a string is the value of the macro `NAME`.
    MacroInStrings,
    /// `assumedouble`: a number with a `.` is a `double` (FTEF-19.24). The lexer reads it as it
    /// reads a float; the semantic model types it.
    AssumeDouble,
}

impl LexFlag {
    /// The flag's bit in [`LexFlags`].
    const fn bit(self) -> u8 {
        match self {
            Self::QcFuncs => 1,
            Self::Qccx => 2,
            Self::Utf8Strings => 4,
            Self::Ilp32 => 8,
            Self::Ssp => 16,
            Self::AssumeInt => 32,
            Self::MacroInStrings => 64,
            Self::AssumeDouble => 128,
        }
    }
}

/// Which of the [`LexFlag`]s are on. [`Default`] is FTEQCC's default, only `qcfuncs` on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LexFlags {
    bits: u8,
}

impl Default for LexFlags {
    fn default() -> Self {
        Self::NONE.with(LexFlag::QcFuncs, true)
    }
}

impl LexFlags {
    /// No flag on.
    pub const NONE: Self = Self { bits: 0 };

    /// These flags with `flag` turned on or off.
    #[must_use]
    pub const fn with(self, flag: LexFlag, on: bool) -> Self {
        let bits = if on {
            self.bits | flag.bit()
        } else {
            self.bits & !flag.bit()
        };
        Self { bits }
    }

    /// The flags in force at byte `at` of a text whose flags change at `marks`, each the byte it
    /// holds from and the flags, in order: the last mark at or before `at`, else the defaults
    /// (FTEF-12.12).
    #[must_use]
    pub fn at(marks: &[(usize, Self)], at: usize) -> Self {
        marks
            .iter()
            .take_while(|(from, _)| *from <= at)
            .last()
            .map_or_else(Self::default, |(_, flags)| *flags)
    }

    /// Whether `flag` is on.
    #[must_use]
    pub const fn has(self, flag: LexFlag) -> bool {
        self.bits & flag.bit() != 0
    }
}

#[cfg(test)]
mod tests;
