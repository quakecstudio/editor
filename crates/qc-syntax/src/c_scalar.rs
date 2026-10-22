// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_ParseType, QCC_PR_CheckKeyword

//! The C scalar type words of FTEQCC's `QCC_PR_ParseType` (FTEF-2.7 to FTEF-2.10): which
//! words its loop takes, in which order, and the type they make.
//!
//! The parser reads a type's words with [`CWords::read`], with the keywords in force at the type
//! (FTEF-20.9), so a word the loop would not take starts the declarator; the semantic model names
//! the type the words make with the same function.

use qc_preproc::{Enabled, Keyword, Keywords};

/// A C type word, as `QCC_PR_CheckKeyword` matches it (FTEF-2.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CWord {
    /// `register`, taken only before the others.
    Register,
    /// `signed`.
    Signed,
    /// `unsigned`.
    Unsigned,
    /// `long`.
    Long,
    /// `int` or `integer`.
    Int,
    /// `short`.
    Short,
    /// `char`.
    Char,
    /// `_Bool`.
    Bool,
    /// `float`.
    Float,
    /// `double`.
    Double,
}

/// The words of [`CWord`], each with the keyword `QCC_PR_CheckKeyword` is given for it.
const WORDS: [(&str, Enabled, CWord); 11] = [
    ("register", Enabled::By(Keyword::Register), CWord::Register),
    ("signed", Enabled::By(Keyword::Signed), CWord::Signed),
    ("unsigned", Enabled::By(Keyword::Unsigned), CWord::Unsigned),
    ("long", Enabled::By(Keyword::Long), CWord::Long),
    ("int", Enabled::By(Keyword::Int), CWord::Int),
    ("integer", Enabled::By(Keyword::Integer), CWord::Int),
    ("short", Enabled::By(Keyword::Short), CWord::Short),
    ("char", Enabled::By(Keyword::Char), CWord::Char),
    ("_Bool", Enabled::Always, CWord::Bool),
    ("float", Enabled::By(Keyword::Float), CWord::Float),
    ("double", Enabled::By(Keyword::Double), CWord::Double),
];

impl CWord {
    /// The C type word `word` is with the keywords `keywords` in force (FTEF-2.7, FTEF-20.9):
    /// `QCC_PR_CheckKeyword` takes `__word` in any case for every keyword, enabled or not, and a
    /// bare word only when its keyword is enabled, in any case under `caseinsens`. By default
    /// these are `unsigned`, `int`, `integer`, `float` and `_Bool`, always on (`qccmain.c`,
    /// `compiler_flag[]`: `double`, `long`, `short`, `char`, `signed` and `register` are
    /// `nondefaultkeyword`).
    #[must_use]
    pub fn of(word: &str, keywords: Keywords) -> Option<Self> {
        WORDS
            .iter()
            .find(|(spelling, enabled, _)| keywords.checks(*enabled, spelling, word))
            .map(|&(_, _, c_word)| c_word)
    }
}

/// The type a run of C type words makes (FTEF-2.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CScalar {
    /// `type_float`.
    Float,
    /// `type_double`, `__double`.
    Double,
    /// `type_integer`, `__int32`.
    Integer,
    /// `type_uint`, `__uint32`.
    Uint,
    /// `type_int64`, `__int64`.
    Int64,
    /// `type_uint64`, `__uint64`.
    Uint64,
    /// `type_sint16`, `__int16`.
    Int16,
    /// `type_uint16`, `__uint16`.
    Uint16,
    /// `type_sint8`, `__int8`.
    Int8,
    /// `type_uint8`, `__uint8`.
    Uint8,
    /// `type_bint`, `__bint`: `_Bool` with the `int` keyword enabled.
    Bool,
    /// `type_bfloat`, `__bfloat`: `_Bool` with the `int` keyword disabled (FTEF-20.9).
    BFloat,
}

impl CScalar {
    /// The scalar a name FTEQCC registers as a type is (`qccmain.c`, `QCC_PR_NewType`), which
    /// `QCC_TypeForName` finds before the C type words are read (FTEF-2.8); `float` included.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "float" => Some(Self::Float),
            "__double" => Some(Self::Double),
            "__int" | "__int32" => Some(Self::Integer),
            "__uint" | "__uint32" => Some(Self::Uint),
            "__int64" => Some(Self::Int64),
            "__uint64" => Some(Self::Uint64),
            "__int16" => Some(Self::Int16),
            "__uint16" => Some(Self::Uint16),
            "__int8" => Some(Self::Int8),
            "__uint8" => Some(Self::Uint8),
            "__bint" => Some(Self::Bool),
            "__bfloat" => Some(Self::BFloat),
            _ => None,
        }
    }

    /// The name FTEQCC registers the type by (`QCC_PR_NewType`), which `TypeName` writes.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Float => "float",
            Self::Double => "__double",
            Self::Integer => "__int32",
            Self::Uint => "__uint32",
            Self::Int64 => "__int64",
            Self::Uint64 => "__uint64",
            Self::Int16 => "__int16",
            Self::Uint16 => "__uint16",
            Self::Int8 => "__int8",
            Self::Uint8 => "__uint8",
            Self::Bool => "__bint",
            Self::BFloat => "__bfloat",
        }
    }
}

/// What a C type's words make, and how FTEQCC narrowed it (FTEF-2.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CType {
    /// The type.
    pub scalar: CScalar,
    /// `unsigned` made a float type, which is signed.
    unsigned_float: bool,
    /// `long double`, which is a double.
    long_double: bool,
}

impl CType {
    /// `WARN_IGNOREDKEYWORD`'s messages for the narrowing, in FTEQCC's order.
    pub fn warnings(&self) -> impl Iterator<Item = &'static str> {
        let unsigned = self
            .unsigned_float
            .then_some("ignoring unsupported unsigned keyword, type will be signed");
        let long = self
            .long_double
            .then_some("long doubles are not supported, using double");
        unsigned.into_iter().chain(long)
    }
}

/// The state of `QCC_PR_ParseType`'s loop over C type words.
#[derive(Clone, Copy, Debug, Default)]
pub struct CWords {
    okay: bool,
    signed: bool,
    unsigned: bool,
    long: bool,
    float: bool,
    bits: u8,
}

/// `longlongbits`.
const LONG_LONG_BITS: u8 = 64;
/// `longbits` without `flag_ILP32`.
const LONG_BITS: u8 = 64;
/// `longbits` under `flag_ILP32`, which `-std=c`, `-FILP32` and `#pragma flag enable ILP32` set
/// (FTEF-20.9).
const ILP32_LONG_BITS: u8 = 32;

/// What `QCC_PR_ParseType`'s loop over C type words reads besides the words (FTEF-20.9): the
/// keywords in force, which decide the words it takes and whether `_Bool` is `__bint` or
/// `__bfloat`, and `flag_ILP32`, which decides `longbits`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CContext {
    /// The keywords in force at the type.
    pub keywords: Keywords,
    /// Whether `ILP32` is on.
    pub ilp32: bool,
}

impl CWords {
    /// Reads the C type words at the start of `words` as `QCC_PR_ParseType` does in `context`
    /// (FTEF-2.8, FTEF-20.9): how many it takes, and the type they make, `None` when it takes
    /// none. A first word that is a registered type name is that type alone; else the first may
    /// be a `register`, and a `_Bool` ends the type, `__bint` when the `int` keyword is on and
    /// `__bfloat` when it is not.
    #[must_use]
    pub fn read<'a>(
        words: impl IntoIterator<Item = &'a str>,
        context: CContext,
    ) -> (usize, Option<CType>) {
        let keywords = context.keywords;
        let mut state = Self::default();
        let mut taken = 0_usize;
        let mut words = words.into_iter().peekable();
        if let Some(scalar) = words.peek().and_then(|w| CScalar::named(w)) {
            return (
                1,
                Some(CType {
                    scalar,
                    unsigned_float: false,
                    long_double: false,
                }),
            );
        }
        if words.peek().and_then(|w| CWord::of(w, keywords)) == Some(CWord::Register) {
            state.okay = true;
            words.next();
            taken = 1;
        }
        for word in words {
            let Some(word) = CWord::of(word, keywords) else {
                break;
            };
            if word == CWord::Bool && state.bits == 0 && !state.signed && !state.unsigned {
                let scalar = if keywords.has(Keyword::Int) {
                    CScalar::Bool
                } else {
                    CScalar::BFloat
                };
                return (
                    taken.saturating_add(1),
                    Some(CType {
                        scalar,
                        unsigned_float: false,
                        long_double: false,
                    }),
                );
            }
            if !state.take(word) {
                break;
            }
            taken = taken.saturating_add(1);
        }
        (taken, state.made(context.ilp32))
    }

    /// Takes `word` if the loop's test for it holds; `false` ends the loop.
    fn take(&mut self, word: CWord) -> bool {
        let none = self.bits == 0;
        match word {
            CWord::Signed if !self.unsigned && !self.signed => self.signed = true,
            CWord::Unsigned if !self.signed && !self.unsigned => self.unsigned = true,
            CWord::Long if none => {
                if self.long {
                    self.bits = LONG_LONG_BITS;
                }
                self.long = true;
            }
            // `long int`, `short int` and the like are allowed.
            CWord::Int if none || self.bits == 16 => {
                if none {
                    self.bits = 32;
                }
            }
            CWord::Short if none => self.bits = 16,
            CWord::Char if none => self.bits = 8,
            CWord::Float if none && !self.long => {
                self.bits = 32;
                self.float = true;
            }
            CWord::Double if none || self.long => {
                self.bits = if self.long { 128 } else { 64 };
                self.long = false;
                self.float = true;
            }
            CWord::Register
            | CWord::Signed
            | CWord::Unsigned
            | CWord::Long
            | CWord::Int
            | CWord::Short
            | CWord::Char
            | CWord::Bool
            | CWord::Float
            | CWord::Double => return false,
        }
        self.okay = true;
        true
    }

    /// The type the words taken make, as the code after the loop picks it, `longbits` 32 under
    /// `ILP32`.
    fn made(self, ilp32: bool) -> Option<CType> {
        if !self.okay {
            return None;
        }
        let bits = match self.bits {
            0 if self.long && ilp32 => ILP32_LONG_BITS,
            0 if self.long => LONG_BITS,
            0 => 32,
            bits => bits,
        };
        let pick = |signed: CScalar, unsigned: CScalar| {
            if self.unsigned { unsigned } else { signed }
        };
        let scalar = if self.float {
            if bits >= 64 {
                CScalar::Double
            } else {
                CScalar::Float
            }
        } else if bits > 32 {
            pick(CScalar::Int64, CScalar::Uint64)
        } else if bits <= 8 {
            pick(CScalar::Int8, CScalar::Uint8)
        } else if bits <= 16 {
            pick(CScalar::Int16, CScalar::Uint16)
        } else {
            pick(CScalar::Integer, CScalar::Uint)
        };
        Some(CType {
            scalar,
            unsigned_float: self.float && self.unsigned,
            long_double: self.float && bits > 64,
        })
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use super::{CContext, CScalar, CWord, CWords};
    use qc_preproc::{Keyword, Keywords};

    fn read(words: &str) -> (usize, Option<CScalar>) {
        read_in(words, CContext::default())
    }

    fn read_in(words: &str, context: CContext) -> (usize, Option<CScalar>) {
        let (taken, ty) = CWords::read(words.split(' '), context);
        (taken, ty.map(|t| t.scalar))
    }

    fn of(word: &str) -> Option<CWord> {
        CWord::of(word, Keywords::default())
    }

    // spec FTEF-20.9: the words the loop takes, `_Bool`'s type and `longbits` follow the keywords
    // and `ILP32` in force at the type
    #[test]
    fn words_by_the_keywords_in_force() {
        let all = Keyword::ALL
            .into_iter()
            .fold(Keywords::NONE, |k, keyword| k.with(keyword, true));
        let with = |keywords| CContext {
            keywords,
            ilp32: false,
        };
        assert_eq!(read_in("long", with(all)), (1, Some(CScalar::Int64)));
        assert_eq!(read_in("long int", with(all)), (2, Some(CScalar::Integer)));
        assert_eq!(read_in("double", with(all)), (1, Some(CScalar::Double)));
        assert_eq!(
            read_in("long double", with(all)),
            (2, Some(CScalar::Double))
        );
        assert_eq!(read_in("signed char", with(all)), (2, Some(CScalar::Int8)));
        assert_eq!(read_in("short", with(all)), (1, Some(CScalar::Int16)));
        assert_eq!(
            read_in("register int", with(all)),
            (2, Some(CScalar::Integer))
        );
        let no_int = Keywords::default().with(Keyword::Int, false);
        assert_eq!(read_in("int", with(no_int)), (0, None));
        assert_eq!(
            read_in("integer", with(no_int)),
            (1, Some(CScalar::Integer))
        );
        assert_eq!(read_in("_Bool", with(no_int)), (1, Some(CScalar::BFloat)));
        assert_eq!(read("_Bool"), (1, Some(CScalar::Bool)));
        let no_unsigned = Keywords::default().with(Keyword::Unsigned, false);
        assert_eq!(read_in("unsigned", with(no_unsigned)), (0, None));
        assert_eq!(
            read_in("__unsigned", with(no_unsigned)),
            (1, Some(CScalar::Uint))
        );
        let any_case = Keywords::default().with_case_insensitive(true);
        assert_eq!(read_in("INT", with(any_case)), (1, Some(CScalar::Integer)));
        assert_eq!(read_in("LONG", with(any_case)), (0, None));
        let ilp32 = CContext {
            keywords: Keywords::default(),
            ilp32: true,
        };
        assert_eq!(read_in("__long", ilp32), (1, Some(CScalar::Integer)));
        assert_eq!(read_in("__long __long", ilp32), (2, Some(CScalar::Int64)));
        assert_eq!(read_in("unsigned __long", ilp32), (2, Some(CScalar::Uint)));
        assert_eq!(of("___BOOL"), Some(CWord::Bool));
        for (scalar, name) in [
            (CScalar::Float, "float"),
            (CScalar::Integer, "__int32"),
            (CScalar::BFloat, "__bfloat"),
            (CScalar::Uint8, "__uint8"),
        ] {
            assert_eq!(scalar.name(), name);
            assert_eq!(CScalar::named(name), Some(scalar));
        }
    }

    // spec FTEF-2.7: a bare word is a C type word only with its keyword enabled by default;
    // after `__` any keyword is, in any case
    #[test]
    fn words_by_the_default_keywords() {
        for word in ["unsigned", "int", "integer", "_Bool", "float"] {
            assert!(of(word).is_some(), "{word}");
        }
        for word in [
            "double", "long", "short", "char", "signed", "register", "Int", "bool",
        ] {
            assert_eq!(of(word), None, "{word}");
        }
        assert_eq!(of("__LONG"), Some(CWord::Long));
        assert_eq!(of("__double"), Some(CWord::Double));
        assert_eq!(of("__register"), Some(CWord::Register));
        assert_eq!(of("__signed"), Some(CWord::Signed));
        assert_eq!(of("__short"), Some(CWord::Short));
        assert_eq!(of("__char"), Some(CWord::Char));
        assert_eq!(of("__integer"), Some(CWord::Int));
        assert_eq!(of("__unsigned"), Some(CWord::Unsigned));
        assert_eq!(of("__float"), Some(CWord::Float));
        assert_eq!(of("__vector"), None);
    }

    // spec FTEF-2.8: the loop takes a word only while its test holds, and the type is picked
    // from the bits and the flags it set
    #[test]
    fn the_loop_and_the_type() {
        for (words, taken, scalar) in [
            ("int", 1, Some(CScalar::Integer)),
            ("integer", 1, Some(CScalar::Integer)),
            ("unsigned", 1, Some(CScalar::Uint)),
            ("unsigned int", 2, Some(CScalar::Uint)),
            ("int unsigned", 2, Some(CScalar::Uint)),
            ("unsigned unsigned", 1, Some(CScalar::Uint)),
            ("int int", 1, Some(CScalar::Integer)),
            ("int float", 1, Some(CScalar::Integer)),
            ("unsigned float", 2, Some(CScalar::Float)),
            ("__long", 1, Some(CScalar::Int64)),
            ("__long int", 2, Some(CScalar::Integer)),
            ("int __long", 1, Some(CScalar::Integer)),
            ("__long __long", 2, Some(CScalar::Int64)),
            ("__long __long int", 2, Some(CScalar::Int64)),
            ("unsigned __long", 2, Some(CScalar::Uint64)),
            ("__short", 1, Some(CScalar::Int16)),
            ("__short int", 2, Some(CScalar::Int16)),
            ("unsigned __char", 2, Some(CScalar::Uint8)),
            ("__char int", 1, Some(CScalar::Int8)),
            ("__signed int", 2, Some(CScalar::Integer)),
            ("__signed unsigned", 1, Some(CScalar::Integer)),
            ("__double", 1, Some(CScalar::Double)),
            ("__long __double", 2, Some(CScalar::Double)),
            ("__long float", 1, Some(CScalar::Int64)),
            ("__register", 1, Some(CScalar::Integer)),
            ("__register int", 2, Some(CScalar::Integer)),
            ("int __register", 1, Some(CScalar::Integer)),
            ("_Bool", 1, Some(CScalar::Bool)),
            ("__long _Bool", 2, Some(CScalar::Bool)),
            ("unsigned _Bool", 1, Some(CScalar::Uint)),
            ("int _Bool", 1, Some(CScalar::Integer)),
            ("__int unsigned", 1, Some(CScalar::Integer)),
            ("__double", 1, Some(CScalar::Double)),
            ("float int", 1, Some(CScalar::Float)),
            ("__uint64", 1, Some(CScalar::Uint64)),
            ("unsigned __int", 2, Some(CScalar::Uint)),
            ("__float", 1, Some(CScalar::Float)),
            ("__signed __signed", 1, Some(CScalar::Integer)),
            ("unsigned __signed", 1, Some(CScalar::Uint)),
            ("int __short", 1, Some(CScalar::Integer)),
            ("int __char", 1, Some(CScalar::Integer)),
            ("int __double", 1, Some(CScalar::Integer)),
            ("__DOUBLE", 1, Some(CScalar::Double)),
            ("__long __DOUBLE", 2, Some(CScalar::Double)),
            ("long", 0, None),
            ("double", 0, None),
            ("x", 0, None),
        ] {
            assert_eq!(read(words), (taken, scalar), "{words}");
        }
    }

    // spec FTEF-2.8: a registered type name is its type alone, before the C words
    #[test]
    fn registered_type_names() {
        for (name, scalar) in [
            ("float", CScalar::Float),
            ("__double", CScalar::Double),
            ("__int", CScalar::Integer),
            ("__int32", CScalar::Integer),
            ("__uint", CScalar::Uint),
            ("__uint32", CScalar::Uint),
            ("__int64", CScalar::Int64),
            ("__uint64", CScalar::Uint64),
            ("__int16", CScalar::Int16),
            ("__uint16", CScalar::Uint16),
            ("__int8", CScalar::Int8),
            ("__uint8", CScalar::Uint8),
            ("__bint", CScalar::Bool),
        ] {
            assert_eq!(CScalar::named(name), Some(scalar), "{name}");
            assert_eq!(
                read(&format!("{name} unsigned")),
                (1, Some(scalar)),
                "{name}"
            );
        }
        assert_eq!(CScalar::named("__INT"), None);
        assert_eq!(CScalar::named("int"), None);
    }

    // spec FTEF-2.9: an unsigned float and a long double are narrowed with a warning
    #[test]
    fn narrowing_warns() {
        let warnings = |words: &str| -> Vec<&str> {
            CWords::read(words.split(' '), CContext::default())
                .1
                .unwrap()
                .warnings()
                .collect()
        };
        let unsigned = "ignoring unsupported unsigned keyword, type will be signed";
        let long = "long doubles are not supported, using double";
        assert_eq!(warnings("unsigned float"), [unsigned]);
        assert_eq!(warnings("__long __double"), [long]);
        assert_eq!(warnings("unsigned __long __double"), [unsigned, long]);
        assert!(warnings("__double").is_empty());
        assert!(warnings("__DOUBLE").is_empty());
        assert_eq!(warnings("__long __DOUBLE"), [long]);
        assert!(warnings("unsigned").is_empty());
        assert!(warnings("unsigned int").is_empty());
    }
}
