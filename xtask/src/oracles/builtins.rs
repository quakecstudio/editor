// SPDX-License-Identifier: GPL-3.0-or-later

//! The builtin-initialiser differential (P2-69, P2-70): every form of a function's builtin
//! initialiser in a cross product, compiled by the pinned FTEQCC under the FTEQCC profile and by
//! id's qcc under the vanilla profile, against what the profile reads.
//!
//! The forms are those of `docs/spec/dialect-fteqcc-rerelease.md` FTE-17.4 to FTE-17.6 and FTE-23: `#` and
//! `:`, with and without a space; integer, float, `f`, `d`, `i`, `u` and `l` literals in decimal,
//! octal and hex, with their signs; character constants and vectors; names; strings,
//! concatenated, across comments and lines; each with an engine name after it or none, after
//! each function declarator; and each pair of the forms either compiler may accept given twice
//! to one function, with and without another function bound by the first form before them.
//! Under FTEQCC it also holds macro uses (FTE-23.10), character-constant escapes (FTEF-13.13),
//! macro names before every kind of byte (FTEF-14.13), and macro arguments against a label's
//! or a builtin's body token (QC-9.16).
//!
//! An outcome is what the compiler did with the program: bound the function `f` to a builtin
//! number and engine name (read from the `progs.dat` it wrote, PROGS-10.2), accepted it without a
//! builtin number (id's qcc for `#0` and negative numbers, QC-13.4), refused it as a
//! redeclaration ("incompatible redeclaration", "redeclared"), or refused it otherwise. The
//! profile's outcome is read from its diagnostics, `qc_syntax::builtin_binum` and
//! `Builtin::engine_name`. The two must agree, except for the cases [`expected_difference`]
//! names, each with the spec statement that says why.
//!
//! The oracles' outcomes are recorded in [`FIXTURE`], so that `cargo test -p xtask` checks the
//! profile against them on every host without the oracles; `cargo xtask oracles` checks the
//! oracles against the recording, and `cargo xtask oracles builtins --record` writes it again.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use qc_progs::{Progs, ReadOptions};
use qc_sema::{Db, FileId, ModuleInput};
use qc_syntax::{Code, Dialect, ast};

use super::Oracles;

mod directives;

/// The recorded outcomes of the oracles, relative to the repository.
pub(crate) const FIXTURE: &str = "xtask/fixtures/oracles/builtin-outcomes.json";

/// What a compiler, or the profile, did with one program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// `f` is the builtin `number`, named `name`.
    Bound { number: i32, name: String },
    /// Accepted, with no builtin number in the progs (QC-13.4).
    Accepted,
    /// Refused as a redeclaration.
    Redeclared,
    /// Refused for another reason.
    Rejected,
    /// Still running after [`ORACLE_LIMIT`], and killed: FTEQCC's endless loop for a code of
    /// 0x80000000 or more (FTEF-13.8).
    Hangs,
}

/// How long an oracle may take over one case before it is taken to hang.
const ORACLE_LIMIT: Duration = Duration::from_secs(20);

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bound { number, name } => write!(f, "bound {number} {name}"),
            Self::Accepted => f.write_str("accepted"),
            Self::Redeclared => f.write_str("redeclared"),
            Self::Rejected => f.write_str("rejected"),
            Self::Hangs => f.write_str("hangs"),
        }
    }
}

impl Outcome {
    /// Reads the form [`fmt::Display`] writes; an engine name may hold spaces.
    fn parse(text: &str) -> Option<Self> {
        match text.splitn(3, ' ').collect::<Vec<_>>().as_slice() {
            ["bound", number, name] => Some(Self::Bound {
                number: number.parse().ok()?,
                name: (*name).to_owned(),
            }),
            ["accepted"] => Some(Self::Accepted),
            ["redeclared"] => Some(Self::Redeclared),
            ["rejected"] => Some(Self::Rejected),
            ["hangs"] => Some(Self::Hangs),
            _ => None,
        }
    }
}

/// One program of the cross product.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Case {
    pub(crate) dialect: Dialect,
    /// The text of the file `progs.src` lists.
    pub(crate) source: String,
    /// The files around it, or `None` for `t.qc` alone.
    pub(crate) project: Option<Project>,
}

/// A program of more than one file: the file `progs.src` lists and the others.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Project {
    /// The name `progs.src` lists the source by, which may name a directory.
    pub(crate) compiled: String,
    /// The other files, each by its name from `progs.src`'s directory, in order.
    pub(crate) files: Vec<(String, String)>,
    /// The include directories FTEQCC is given, `-I`, each by its name from `progs.src`'s
    /// directory (FTEF-14.49).
    pub(crate) include_dirs: Vec<String>,
}

/// The name a case's source has when it has no [`Project`].
const SOURCE_NAME: &str = "t.qc";

impl Case {
    /// The case of `source` alone, as `t.qc`.
    pub(crate) fn new(dialect: Dialect, source: String) -> Self {
        Self {
            dialect,
            source,
            project: None,
        }
    }

    /// The case's key in [`FIXTURE`].
    fn key(&self) -> String {
        let mut key = format!("{}|", dialect_name(self.dialect));
        if let Some(project) = &self.project {
            key.push_str(&project.compiled);
            key.push('|');
        }
        key.push_str(&self.source);
        for (name, text) in self.project.iter().flat_map(|p| &p.files) {
            key.push_str(&format!("|{name}={text}"));
        }
        for dir in self.include_dirs() {
            key.push_str(&format!("|-I{dir}"));
        }
        key
    }

    /// The text of every file of the case, the source's first.
    fn texts(&self) -> impl Iterator<Item = &str> {
        let others = self.project.iter().flat_map(|p| &p.files);
        std::iter::once(self.source.as_str()).chain(others.map(|(_, text)| text.as_str()))
    }

    /// The include directories FTEQCC is given (FTEF-14.49).
    fn include_dirs(&self) -> impl Iterator<Item = &String> {
        self.project.iter().flat_map(|p| &p.include_dirs)
    }

    /// The name `progs.src` lists the source by.
    fn compiled(&self) -> &str {
        self.project
            .as_ref()
            .map_or(SOURCE_NAME, |project| project.compiled.as_str())
    }
}

fn dialect_name(dialect: Dialect) -> &'static str {
    if dialect.has_fteqcc_extensions() {
        "fteqcc"
    } else {
        "vanilla"
    }
}

/// What introduces the builtin.
const PREFIXES: [&str; 4] = ["#", ":", "# ", ": "];

/// What follows the prefix: every literal form of FTEF-3.1 and FTEF-13.3, names and strings.
const VALUES: &[&str] = &[
    "",
    "0",
    "5",
    "16",
    "-5",
    "-0",
    "- 5",
    "+5",
    "010",
    "08",
    "0x10",
    "0X10",
    "-0x10",
    "0x1d",
    "0x10f",
    "0x10i",
    "0x10.0",
    "0x10000000000000005.0",
    "-0x10.0",
    "0x2.",
    "0x1.8",
    "0x1.5d",
    "0X1.0D",
    "010d",
    "-16d",
    "\t5",
    "/**/5",
    "16777217",
    "123456789",
    "16.0",
    "16.5",
    ".5",
    "-.5",
    "16.",
    "0.0",
    ".0",
    "1e2",
    "16f",
    "16F",
    "16.0f",
    "16.5f",
    "16d",
    "16D",
    "16.0d",
    "16.5d",
    "0d",
    "16i",
    "16I",
    "-5i",
    "010i",
    "0i",
    "16777217i",
    "4294967313i",
    "16u",
    "16U",
    "16l",
    "16L",
    "16ll",
    "16ul",
    "16lu",
    "16il",
    "16li",
    "16ui",
    "16a",
    "'a'",
    "'\\n'",
    "'1 2 3'",
    "(5)",
    "ex_a",
    "f",
    "\"ex_a\"",
    "\"\"",
    "\"a\\nb\"",
    "\"ab\" \"cd\"",
    "\"ab\"\"cd\"",
    "\"ab\" \"cd\" \"ef\"",
    "\"ab\" /* c */ \"cd\"",
    "\"ab\"\n\"cd\"",
    "\"ab\" // c\n\"cd\"",
    "\"\" \"\"",
    "\"\" \"ab\"",
    "\"ab\" \"\"",
    "\"a\\tb\"",
    "\"\\x00\"",
    "\"ab\\x00cd\"",
    "\"\\bab\"",
    "\"\\bcd\"",
    // FTEF-13.7 to FTEF-13.11: strings with each prefix, joined, raw, and with the escapes and
    // bytes each kind of string writes differently.
    "u8\"ex_a\"",
    "u\"ex_a\"",
    "U\"ex_a\"",
    "Q\"ex_a\"",
    "L\"ex_a\"",
    "R\"(ex_a)\"",
    "R\"x(ex_a)x\"",
    "R\"(a\nb)\"",
    "R\"x(a)\"b)x\"",
    "R\"(a\\nb)\"",
    "R\"(\\\nab)\"",
    "R\"\"",
    "\"ex_\" u8\"a\"",
    "\"ex_\" L\"a\"",
    "\"ex_\" R\"(a)\"",
    "\"ex_\" Q\"a\"",
    "u8\"ex_\" \"a\"",
    "\"\" u8\"ex_a\"",
    "\"\\[\\]a\"",
    "u8\"\\[\\]a\"",
    "Q\"\\[a\"",
    "\"\\{200}a\"",
    "u8\"\\{200}a\"",
    "\"\\{256}a\"",
    "\"\\{6z}\"",
    "\"\\u00e9a\"",
    "\"\\U00110000a\"",
    "u8\"\\bab\"",
    "Q\"\\bab\"",
    "\"\\b\\ta\"",
    "\"\\b a\"",
    "\"\\b\\.a\"",
    "u8\"\\b\\.a\"",
    "\"\\.a\"",
    "\"\u{e9}a\"",
    "Q\"\u{e9}a\"",
    "u8\"\u{e9}a\"",
    "Q\"\u{e010}a\"",
    "\"a\\\nb\"",
    "\"\\xg\"",
    "\"\\x41\\x4\"",
    "u8\"\\xe9\"",
];

/// What may follow the value: an engine name, written in each way.
const SUFFIXES: [&str; 6] = ["", ":ex_b", ":\"ex_b\"", ": ex_b", " :ex_b", ":5"];

/// The function declarators, the first the ordinary one (FTE-17.1, FTE-17.2).
const DECLARATORS: [&str; 3] = ["void() f = ", "void f() = ", "void() f "];

/// The forms given twice: every way to write one builtin number or engine name that either
/// compiler accepts, and some that differ from them in one part.
const PAIRED: &[&str] = &[
    "#5",
    "#5.0",
    "#5i",
    "#0x5",
    "#05",
    "#6",
    "#-5",
    "#0",
    "#-0",
    "#0i",
    "#0:ex_a",
    "#0:ex_b",
    "#0:f",
    "#5:ex_a",
    "#5:ex_b",
    "#5:f",
    "#6:ex_a",
    "#-5:ex_a",
    ":ex_a",
    ":\"ex_a\"",
    "#\"ex_a\"",
    ":\"ex_\" \"a\"",
    "#\"ex_b\"",
    ":f",
    "#\"f\"",
    ":5",
    ":5:ex_a",
    ":0",
    ":0:ex_a",
    "#\"ex_a\\x00b\"",
    "#\"\\bab\"",
    "#\"\\bcd\"",
];

/// What the macro `N` of the FTEQCC macro cases stands for (FTE-23.10).
const MACRO_BODIES: &[&str] = &[
    "5",
    "-5",
    "0",
    "\"ex_a\"",
    "\"ex_\" \"a\"",
    "u8\"ex_a\"",
    "ex_a",
    "#5",
    "#5:ex_a",
    // QC-9.16: a macro's body keeps its spacing.
    "#-5",
    "#\"ex_a\"",
    "# 5",
    "#- 5",
    ":ex_a",
    "5:ex_b",
    "#",
    "",
];

/// Where the macro cases use `N`: after `#` and `:`, with and without a space, before an engine
/// name and as one, and with a byte after it that FTEQCC reads as part of the word.
const MACRO_USES: &[&str] = &[
    "#N", ":N", "# N", ": N", "#N:ex_b", "#5:N", "#N #5", ":#N", "#N!", "N", "#\"\":N", "##N",
];

/// The macro cases of other macros: a function-like one, the built-in constants and a name that
/// is no macro (FTE-23.10).
const OTHER_MACROS: &[(&str, &str)] = &[
    ("#define M(x) x\n", "#M(5)"),
    ("#define M(x) x\n", "#M(\"ex_a\")"),
    ("#define M(x) x\n", "#M (5)"),
    ("#define M(x) x\n", "#M"),
    ("#define M(x) x\n", "#M(#5)"),
    ("#define M(x) x\n", ":M(ex_a)"),
    ("#define M() #5\n", "#M()"),
    ("", "#__LINE__"),
    ("", "#__FILE__"),
    ("", "#__NULL__"),
    ("", "#U"),
    ("", ":U"),
    ("#define N 5\n#undef N\n", "#N"),
];

/// What the character-constant cases write after `:'\` (FTEF-13.13): every escape of
/// FTEF-13.11, with and without each of its errors, and bytes that are no escape.
const CHARACTER_ESCAPES: &[&str] = &[
    "n",
    "r",
    "t",
    "a",
    "v",
    "f",
    "#",
    "\"",
    "\\",
    "?",
    "'",
    "[",
    "]",
    ".",
    "<",
    "-",
    ">",
    "(",
    "=",
    ")",
    "+",
    "0",
    "9",
    "{65}",
    "{x41}",
    "{256}",
    "{6z}",
    "{xg}",
    "{65",
    "x41",
    "X41",
    "x4",
    "xg",
    "x",
    "u041",
    "u0041",
    "u00e9",
    "U0000041",
    "U00000041",
    "u",
    "s",
    "b",
    "q",
    " ",
    "\n",
    "\r\n",
];

/// After a byte that follows the macro name `N` in the macro-word cases (FTEF-14.13), the rest
/// of an expression the byte starts once `N` is `5`, if one does.
fn word_continuation(byte: char) -> &'static str {
    match byte {
        '!' | '=' => "=5",
        '?' => "1:2",
        '%' | '+' | '-' | '*' | '/' | '|' | '&' | '^' | '<' | '>' => "2",
        '\\' => "\n",
        _ => "",
    }
}

/// What follows the macro name `N` in the macro-word cases: every ASCII byte from 1 that is no
/// part of a name, with [`word_continuation`], and characters of two, three and four UTF-8
/// bytes (FTEF-14.13).
fn word_followers() -> Vec<String> {
    let mut followers = Vec::new();
    for byte in (1..=0x7f_u8).map(char::from) {
        if byte.is_ascii_alphanumeric() || byte == '_' {
            continue;
        }
        followers.push(format!("{byte}{}", word_continuation(byte)));
    }
    followers.extend(["\u{e9}", "\u{20ac}", "\u{10000}"].map(str::to_owned));
    followers
}

/// The macro bodies and uses of the macro-word cases in a body or an argument
/// (FTEF-14.13, QC-9.16): `N`, which stands for `5`, followed in the text FTEQCC reads by a
/// byte that ends its word or one that does not.
const WORD_MACROS: &[(&str, &str)] = &[
    ("#define M N!=5", "M"),
    ("#define M N !=5", "M"),
    ("#define M N/**/!=5", "M"),
    ("#define M N==5", "M"),
    ("#define M N?1:2", "M"),
    ("#define M N%2", "M"),
    ("#define M N", "M!=5"),
    ("#define M N", "M !=5"),
    ("#define A(a) a!=5", "A(N)"),
    ("#define A(a) a !=5", "A(N)"),
    ("#define A(a) a/**/!=5", "A(N)"),
    ("#define A(a) a==5", "A(N)"),
    ("#define A(a) a!=5", "A(N )"),
    ("#define A(a) a!=5", "A(N/**/)"),
    ("#define A(a) a", "A(N!=5)"),
    ("#define A(a, b) a b!=5", "A(, N)"),
    ("#define V(...) __VA_ARGS__!=5", "V(N)"),
    ("#define V(...) __VA_ARGS__!=5", "V(N )"),
    // QC-9.16: a parameter in a word FTEQCC stored across a dropped comment or line break.
    ("#define A(a) a/**/b", "0; local float ab; ab = A(5)"),
    ("#define A(a) b/**/a", "0; local float ba; ba = A(5)"),
    ("#define A(a) a\\\nb", "0; local float ab; ab = A(5)"),
];

/// The macros and statements of the label cases (QC-9.16, FTEF-18.23): an argument, or a body
/// token, written against a `:` or apart from it.
const LABEL_MACROS: &[(&str, &str)] = &[
    ("#define L lbl:", "L"),
    ("#define L lbl :", "L"),
    ("#define L lbl/**/:", "L"),
    ("#define L lbl\\\n:", "L"),
    ("#define L(x) x:", "L(lbl)"),
    ("#define L(x) x :", "L(lbl)"),
    ("#define L(x) x:", "L( lbl )"),
    ("#define L(x) x:", "L(/**/lbl)"),
    ("#define L(x) x:", "L(lbl/**/)"),
    ("#define L(x) x:", "L(lbl// c\n)"),
    ("#define L(x) x/**/:", "L(lbl)"),
    ("#define L(x) x\\\n:", "L(lbl)"),
    ("#define L(x, y) x y:", "L(, lbl)"),
    ("#define L(x, y) x y:", "L(lbl, )"),
    ("#define L(x, y) x:y", "L(lbl, )"),
    ("#define V(...) __VA_ARGS__:", "V(lbl)"),
    ("#define V(...) __VA_ARGS__:", "V( lbl)"),
    ("#define V(...) __VA_ARGS__:", "V(lbl )"),
    ("#define E\n#define L lbl E:", "L"),
    ("#define F(a) a\n#define L lbl F():", "L"),
];

/// The macros and initialisers of the cases where a macro's argument meets a builtin's body
/// token (QC-9.16, FTE-23.1, FTE-23.4).
const ARGUMENT_BUILTINS: &[(&str, &str)] = &[
    ("#define E(n) :n", "E(ex_a)"),
    ("#define E(n) : n", "E(ex_a)"),
    ("#define E(n) #5:n", "E(ex_a)"),
    ("#define E(n) #5: n", "E(ex_a)"),
    ("#define E(n) n:ex_a", "E(#5)"),
    ("#define E(n) n :ex_a", "E(#5)"),
    ("#define E(n) #n", "E(5)"),
    ("#define E(n) # n", "E(5)"),
    ("#define E(n) #-n", "E(5)"),
    ("#define E(n) #- n", "E(5)"),
    ("#define E(n) -n", "#E(5)"),
    ("#define E(n) n", "#E(-5)"),
    ("#define E(n) n", "E(#5)"),
    ("#define E(n) n", "E(# 5)"),
    ("#define E(n, m) n m", "E(#, 5)"),
    ("#define E(n, m) n m", "E(#,5)"),
    ("#define E(n, m) n:m", "E(#5, ex_a)"),
    ("#define N \"ex_a\"\n#define K #/**/N", "K"),
    ("#define N 5\n#define K #/**/N", "K"),
];

/// The macro-expansion cases whose expansion is `f`'s initialiser (FTEF-14, QC-9.16): token
/// pasting and stringising at each place in a body, empty arguments, nested and recursive
/// macros, argument lists that cross the end of an expansion or of the file, and builtin
/// initialisers a macro writes. Each is the definitions and the initialiser.
const EXPANSION_INITIALISERS: &[(&str, &str)] = &[
    // `##` with the arguments on each side, empty or not, and at each place in the body.
    ("#define P(a, b) a##b", "P(#, 5)"),
    ("#define P(a, b) a##b", "P(#5, )"),
    ("#define P(a, b) a##b", "P(, #5)"),
    ("#define P(a, b) a##b", "P(#1, 6)"),
    ("#define P(a, b) a##b", "P(#, -5)"),
    ("#define P(a, b) a##b", "P(:, ex_a)"),
    ("#define P(a, b) a##b", "P(#5:ex, _a)"),
    ("#define P(a, b) a##b", "P(#5:, ex_a)"),
    ("#define P(a, b) a##b", "P(#, \"ex_a\")"),
    ("#define P(a, b) a##b", "P(#5 :ex, _a)"),
    ("#define P(a, b) a ## b", "P(#5:ex, _a)"),
    ("#define P(a, b) a## b", "P(#5:ex, _a)"),
    ("#define P(a, b) a ##b", "P(#5:ex, _a)"),
    ("#define P(a, b, c) a##b##c", "P(#, 1, 6)"),
    ("#define P(a, b, c) a##b##c", "P(#5:, ex, _a)"),
    ("#define P(a) a##5", "P(#)"),
    ("#define P(a) a##5", "P(#1)"),
    ("#define P(a) a##5", "P()"),
    ("#define P(a) #5:ex_##a", "P(a)"),
    ("#define P(a) #5:a##_a", "P(ex)"),
    ("#define P(a) #5:a##_##a", "P(ex)"),
    ("#define P #1##6", "P"),
    ("#define P #5:ex_##a", "P"),
    ("#define P(a) ##a", "P(#5)"),
    ("#define P(a) a##", "P(#5)"),
    ("#define P(a) #5:a ## ex", "P()"),
    // `#` before each kind of argument, and before what is no parameter.
    ("#define S(a) #a", ":S(ex_a)"),
    ("#define S(a) #a", ":S( ex_a )"),
    ("#define S(a) #a", ":S(ex  a)"),
    ("#define S(a) #a", ":S(ex/**/a)"),
    ("#define S(a) #a", ":S(\"ex_a\")"),
    ("#define S(a) #a", ":S(ex\\a)"),
    ("#define S(a) #a", ":S()"),
    ("#define S(a) #a", ":S(ex_a, b)"),
    ("#define S(a) #a", ":S((ex, a))"),
    ("#define S(a) # a", ":S(ex_a)"),
    ("#define S(a) #a b", ":S(ex_a)"),
    ("#define S(a, b) #a #b", ":S(ex_, a)"),
    ("#define S(a) \"ex_\"#a", ":S(a)"),
    ("#define S(a) #a\"_a\"", ":S(ex)"),
    ("#define S(a) #b", ":S(ex_a)"),
    ("#define S(a) #5 #", "S(ex_a)"),
    ("#define S(a) #a:ex_b", "#5:S(ex_a)"),
    ("#define S(a) #a", "#S(ex_a)"),
    ("#define S(a) #a", "#5:S(ex_a)"),
    ("#define S(a) #a\n#define N ex_b", ":S(N)"),
    (
        "#define S(a) #a\n#define T(a) S(a)\n#define N ex_b",
        ":T(N)",
    ),
    ("#define S(a) #a##_a", ":S(ex)"),
    // Empty arguments, and a variadic macro given none.
    ("#define E(a) #5 a", "E()"),
    ("#define E(a) #5a", "E()"),
    ("#define E(a, b) a b", "E(#5, )"),
    ("#define E(a, b) a b", "E(, #5)"),
    ("#define E(a, b) a b", "E(, )"),
    ("#define E(a, b) a:b", "E(#5, )"),
    ("#define E(a, b) a#b", "E(, 5)"),
    ("#define V(...) #5 __VA_ARGS__", "V()"),
    ("#define V(...) #5 __VA_ARGS__", "V(:ex_a)"),
    ("#define V(a, ...) a __VA_ARGS__", "V(#5)"),
    ("#define V(a, ...) a __VA_ARGS__", "V(#5, )"),
    ("#define V(a, ...) a __VA_ARGS__", "V(#5, :ex_a)"),
    ("#define V(a, ...) a:__VA_ARGS__", "V(#5, ex_a)"),
    // Nested and recursive macros.
    ("#define A #5\n#define B A", "B"),
    ("#define A(x) x\n#define B A(#5)", "B"),
    ("#define A(x) x", "A(A(#5))"),
    ("#define A(x) x\n#define B(x) A(x)", "B(#5)"),
    ("#define A(x) B(x)\n#define B(x) A(x)", "A(#5)"),
    ("#define A A", "#5:A"),
    ("#define A B\n#define B A", "#5:A"),
    ("#define A #5 B\n#define B :A", "A"),
    ("#define A #5:B\n#define B A", "A"),
    ("#define A(x) #5:x\n#define B A(B)", "B"),
    ("#define A(x) x\n#define B A(:B)", "#5 B"),
    ("#define N 5\n#define A #N", "A"),
    ("#define N #5\n#define A(x) x", "A(N)"),
    ("#define N ex_a\n#define A(x) #5:x", "A(N)"),
    ("#define A(x) x:ex_a", "A(A(#5))"),
    ("#define M(x) x\n#define N M", "N(#5)"),
    ("#define M(x) x\n#define N M(", "N #5)"),
    ("#define M(x) x\n#define N(y) M(y", "N(#5))"),
    ("#define M(x) x", "M(\n#5)"),
    ("#define M(x) x", "M(#5"),
    ("#define M(x) x", "M\n(#5)"),
    ("#define M(x) x", "M /**/(#5)"),
    ("#define M(x) x", "M\t(#5)"),
    ("#define S(a) # 5", "S(x)"),
    ("#define S(a) # 5:ex_a", "S(x)"),
    ("#define S(a) #a", ":S(e\"x\"a)"),
    ("#define K(x) x", "#K(__LINE__)"),
    ("#define K(x) #5:x", "K(__FILE__)"),
    // From the self-review: arguments beside `##`, `#__VA_ARGS__`, built-in constants in
    // arguments, a pasted name after `#`, a paste that is a comment, and bytes in a string.
    ("#define P(a, b) a##b", "P(#5:ex , _a)"),
    ("#define P(a, b) a##b", "P(#5:e, x_a)"),
    ("#define P(a, b) a##b", "P(#5:e, x_a :ex_b)"),
    ("#define P(a, b) a # # b", "P(#, 5)"),
    ("#define P(a, b) a##b", "P(/, /) #5"),
    ("#define V(...) #__VA_ARGS__", "V(5)"),
    ("#define K(x) x", "K(#__LINE__)"),
    ("#define K(x) x##5", "K(#__LINE__)"),
    ("#define N1 #5\n#define P(x, a, b) x##a##b", "P(#, N, 1)"),
    ("#define S(a) #a", ":S(e\u{e9})"),
    ("#define P(a, b) a##b 5", "P(#, )"),
    ("#define P(a, b) a##b #6", "P(/, /) #5"),
    ("#define M(x) x\n#define K(y) y", "K(M\n(#5))"),
];

/// The macro-expansion cases written as statements of a function `g` after `f`, which binds
/// `f` only when `g` compiles (QC-9.16, FTEF-18.23): labels a paste, a string, a nested or an
/// unfinished macro writes. Each is the definitions and the statement.
const EXPANSION_STATEMENTS: &[(&str, &str)] = &[
    ("#define P(a, b) a##b:", "P(l, bl) goto lbl;"),
    ("#define P(a, b) a##b :", "P(l, bl) goto lbl;"),
    ("#define P(a, b) a ## b:", "P(l, bl) goto lbl;"),
    ("#define P(a, b) a##b", "P(l, bl): goto lbl;"),
    ("#define P(a) lbl##a", "P(): goto lbl;"),
    ("#define P(a) a##:", "P(lbl) goto lbl;"),
    ("#define P(a) :##a", "P(lbl) goto lbl;"),
    ("#define L(x) x:\n#define M(x) L(x)", "M(lbl) goto lbl;"),
    ("#define L(x) x:\n#define M L(lbl)", "M goto lbl;"),
    ("#define L(x) x:\n#define M lbl", "L(M) goto lbl;"),
    ("#define L(x) x:\n#define LL L(lbl", "LL) goto lbl;"),
    ("#define L(x) x:\n#define LL L(", "LL lbl) goto lbl;"),
    ("#define L(x) x:\n#define LL L", "LL(lbl) goto lbl;"),
    ("#define L(x) x:", "L(lbl\n) goto lbl;"),
    ("#define S(x) #x", "x = S(5) == \"5\";"),
    ("#define P(a, b) a##b", "P(l, bl:) goto lbl;"),
    ("#define P(a, b) a##b", "P(l , bl:) goto lbl;"),
    ("#define K(x) x", "K(__LINE__:) goto lbl;"),
];

/// The bytes that end an argument where FTEQCC's argument scanner finds them (FTEF-14.22).
const SEPARATORS: [&str; 3] = [",", ")", "("];

/// What stands between a function-like macro's name and its `(` in another macro's body
/// (FTEF-14.24): white space, comments and line continuations, which the stored body drops.
const BODY_GAPS: &[&str] = &[
    " ", "\t", " \t ", "/**/", " /**/ ", "/**/ ", " /**/", "/* x */", "/*\n*/", "\\\n", " \\\n",
    "\\\n ", " \\\n ", "\\\r\n", "/**/\\\n",
];

/// A macro case: the definitions and the use that initialises `f`.
type MacroUse = (String, String);

/// `(define, use)` pairs written as [`MacroUse`]s.
fn macro_uses(pairs: &[(&str, &str)]) -> Vec<MacroUse> {
    pairs
        .iter()
        .map(|&(define, use_)| (define.to_owned(), use_.to_owned()))
        .collect()
}

/// Arguments as FTEQCC's argument scanner finds them (FTEF-14.22): a `,`, `(` or `)` in a
/// character constant, a comment or a string, against 15 uses each; strings that end in `\`; a
/// `\` and line break in an argument; and a lexical error in arguments a chunk's macro takes.
fn argument_uses() -> Vec<MacroUse> {
    let mut uses = Vec::new();
    for sep in SEPARATORS {
        for (define, use_) in [
            ("#define E(n) :n", format!("E('{sep}')")),
            ("#define E(n) #n", format!("E('{sep}')")),
            ("#define M(x) x", format!("M(#5 /* {sep} */)")),
            ("#define M(x) x", format!("M(#5 // {sep}\n)")),
            ("#define M(x) x", format!("M(/* {sep} */ #5)")),
            ("#define M(x) x", format!("M(// {sep}\n#5)")),
            ("#define M(x, y) x", format!("M(#5, /* {sep} */)")),
            ("#define M(x, y) x", format!("M(#5, /* {sep} */ 6)")),
            ("#define M(x, y) y", format!("M(/* {sep} */, #5)")),
            ("#define E(n) :n", format!("E(\"ex{sep}a\")")),
            ("#define E(n, m) n m", format!("E(#5, :\"a{sep}b\")")),
            ("#define S(a) #a", format!(":S(e'{sep}')")),
            ("#define S(a) #a", format!(":S(\"{sep}\")")),
            ("#define S(a) #a", format!(":S(e/*{sep}*/x)")),
            ("#define V(...) __VA_ARGS__", format!("V(#5:'{sep}')")),
        ] {
            uses.push((define.to_owned(), use_));
        }
    }
    for arg in [
        ":\"ex\\\\\"",
        ":\"ex\\\\\") \")\"",
        ":\"e\\\"x\"",
        ":\"e\\\\\\\"x\"",
        ":\"ex\\\\\" \")\"",
        "#5:ex_a\\\n",
        "#5:\"ex_a\"\\\n",
        "#5:ex\\\n_a",
    ] {
        uses.push(("#define E(n) n".to_owned(), format!("E({arg})")));
    }
    uses.extend(macro_uses(&[
        ("#define S(a) #a", ":S(e'x)"),
        ("#define S(a) #a", ":S(e\\\"x)"),
        ("#define M(x) x", "M(// c\r#5)"),
        ("#define M(x) x", "M(// c\r\n#5)"),
        ("#define M(a) #5\n#define K M('x)", "K"),
        ("#define K(x) x(#5)\n#define M(y) y", "K(M)"),
    ]));
    uses
}

/// `QCC_COM_Parse2`'s tokens in a body and an argument (FTEF-14.25): strings after `#` and `##`,
/// numbers and the words written right after them.
fn parse2_uses() -> Vec<MacroUse> {
    macro_uses(&[
        ("#define S(a) #\"5\"", "S(x)"),
        ("#define S(a) #\"5\"x a", "S(:ex_a)"),
        ("#define S(a) #\"5\" a", "S(:ex_a)"),
        ("#define P(a) a##\"5\"", "P(#)"),
        ("#define P(a) a##\"5\"x:ex_a", "P(#)"),
        ("#define E(a) #5a", "E(:ex_a)"),
        ("#define E(a) #5.a", "E(:ex_a)"),
        ("#define E(a) #0x5a", "E(:ex_a)"),
        ("#define E(a) #05a", "E(:ex_a)"),
        ("#define E(a) #0x1F a", "E(:ex_a)"),
        ("#define E(n) n", "E(#16)"),
        ("#define J(a) a##b", "J(#5:e)"),
    ])
}

/// Built-in constants an argument copies, after white space, comments and line breaks, and
/// `__LINESTR__` read in nested and multi-line expansions (FTEF-14.20, FTEF-14.27).
fn line_uses() -> Vec<MacroUse> {
    let mut uses = Vec::new();
    for arg in [
        "# __LINE__",
        "#\t__LINE__",
        "#//\n__LINE__",
        "#\n__LINE__",
        "#/**/__LINE__",
        "#__LINE__ ",
        "#__LINE__\n",
        "\n#__LINE__",
        "#__LINE__/**/",
        ":__LINESTR__",
        ":\n__LINESTR__",
        ": __LINESTR__",
        "\n:__LINESTR__",
        ":__LINESTR__\n",
    ] {
        uses.push(("#define E(n) n".to_owned(), format!("E({arg})")));
    }
    uses.extend(macro_uses(&[
        ("#define K(x) x##5", "K(# __LINE__)"),
        ("#define K(x) #x", "#5:K(__LINE__)"),
        ("#define L :__LINESTR__", "\nL"),
        ("#define L :__LINESTR__\n#define M L", "M"),
        ("#define L :__LINESTR__\n#define M(x) L x", "M(\n)"),
        ("#define M(x) :__LINESTR__ x", "M(\n)"),
        ("#define K(x) x\n#define M K(:__LINESTR__)", "M"),
        ("#define K(x) x\n#define M(y) K(y)", "M(\n:__LINESTR__)"),
        ("#define K(x) x\n#define M(y) K(y)", "M(:\n__LINESTR__)"),
        ("#define K(x) x\n#define M(y) K(\\\n:__LINESTR__ y)", "M()"),
        (
            "#define L __LINESTR__\n#define V(...) __VA_ARGS__ L",
            "V(:\n\n)",
        ),
        (
            "#define L __LINESTR__\n#define V(...) __VA_ARGS__ L",
            "V(:)",
        ),
    ]));
    uses
}

/// Bodies as `QCC_PR_PreProcessor_Define` stores them (FTEF-14.24): a function-like macro's name
/// and `(` across 15 gaps a stored body drops, escaped quotes before a `//`, comments and
/// continuations, and a body stored again.
fn body_uses() -> Vec<MacroUse> {
    let mut uses = Vec::new();
    for gap in BODY_GAPS {
        let define = format!("#define M(x) x\n#define K M{gap}(#5)");
        uses.push((define, "K".to_owned()));
    }
    for body in [
        ":\"ex\\\"_a//\"",
        ":\"ex\\\\\"_a//\"",
        ":\"e//x\"",
        ":\"ex\\\" /* c */ _a\"",
        "#5 // c \\\n:ex_a",
        "#5 /* c \\\n */ :ex_a",
        "#5 :ex\\\n_a",
        "#5 :ex/**/_a",
        "#5 :\"ex /* c */ a\"",
        "#5 \\\n\\\n:ex_a",
        "#5 // c \\\nzz zz",
    ] {
        uses.push((format!("#define N {body}"), "N".to_owned()));
    }
    uses.extend(macro_uses(&[("#define A #5\n#define A #6", "A")]));
    uses
}

/// Parameter lists (FTEF-14.23): duplicate, empty, spaced and misplaced names, and 32 and 33
/// arguments to 32 parameters.
fn parameter_uses() -> Vec<MacroUse> {
    let mut uses = macro_uses(&[
        ("#define E(n, n) n", "E(#5, 6)"),
        ("#define E(n,n) n", "E(#5, 6)"),
        ("#define E(a, b, a) a", "E(#5, 6, 7)"),
        ("#define E( n ) n", "E(#5)"),
        ("#define E(n ,m) n", "E(#5, 6)"),
        ("#define E(n\t, m\t) m", "E(6, #5)"),
        ("#define E(a,) a", "E(#5)"),
        ("#define E(a,b) b", "E(6,#5)"),
        ("#define E() #5", "E(1, 2)"),
        ("#define E() #5", "E(1, 2, 3)"),
        ("#define E(a b) #5", "E(1)"),
        ("#define E(..., a) a", "E(#5)"),
        ("#define E(a, ..., b) a", "E(#5)"),
        ("#define E(a, ...) a", "E(#5, 1, 2)"),
    ]);
    let params: Vec<String> = (0..32).map(|i| format!("p{i}")).collect();
    for more in [31, 32] {
        let define = format!("#define E({}) p0", params.join(", "));
        uses.push((define, format!("E(#5{})", ", 1".repeat(more))));
    }
    uses
}

/// `__VA_COUNT__`, `,##__VA_ARGS__` and empty variadic arguments (FTEF-14.26).
fn variadic_uses() -> Vec<MacroUse> {
    macro_uses(&[
        ("#define V(...) :__VA_COUNT__", "V()"),
        ("#define V(...) :__VA_COUNT__", "V(a)"),
        ("#define V(...) :__VA_COUNT__", "V(a, b)"),
        ("#define V(...) :__VA_COUNT__", "V(a, b, c)"),
        ("#define V(...) :__VA_COUNT__", "V(,)"),
        ("#define V(a, ...) #5:__VA_COUNT__", "V(a)"),
        ("#define V(a, ...) #5:__VA_COUNT__", "V(a, b)"),
        ("#define V(a, ...) #__VA_COUNT__:ex_a", "V(a, b, c)"),
        ("#define V(a) #5:__VA_COUNT__", "V(a)"),
        ("#define V(a, ...) a,##__VA_ARGS__", "V(#5)"),
        ("#define V(a, ...) a , ## __VA_ARGS__", "V(#5)"),
        ("#define V(a, ...) a,##__VA_ARGS__", "V(#5, :ex_a)"),
        ("#define V(a, ...) a##__VA_ARGS__", "V(#5)"),
        ("#define V(a, ...) a,__VA_ARGS__", "V(#5)"),
        ("#define E(...) #5 __VA_ARGS__", "E(, )"),
        ("#define E(...) #5 __VA_ARGS__", "E(,)"),
        ("#define E(...) #5 __VA_ARGS__", "E( , , )"),
        ("#define E(...) #5 __VA_ARGS__", "E(:ex_a, )"),
    ])
}

/// `__FILE__`, the name `progs.src` lists or a `#pragma file` gives, as FTEQCC writes it
/// (FTEF-14.32), under 12 pragmas and through two macros, and after three pragmas whose text
/// holds a comment, a string or a `\` that would run across their line break.
fn file_sources() -> Vec<String> {
    let long = format!("#pragma file({})\n", "n".repeat(260));
    let mut sources: Vec<String> = [
        "",
        "#pragma file(ex_a:b)\n",
        "#pragma file ex_b\n",
        "#pragma FILE(e\\\\x)\n",
        "#pragma file(e\\x)\n",
        "#pragma file(e\\\"x)\n",
        "#pragma file(e\"x)\n",
        "#pragma file\n",
        "#pragma file(sub/ex_c)\n",
        "#pragma file( ex_d )\n",
        "#pragma file:x\n",
        &long,
        "#pragma message x /* y\n",
        "#pragma message x \"y\n",
        "#pragma message x \\\n",
    ]
    .iter()
    .map(|pragma| format!("{pragma}void() f = :__FILE__;\n"))
    .collect();
    sources.push("#define F :__FILE__\n#pragma file(ex_c)\nvoid() f = F;\n".to_owned());
    sources.push("#define E(n) n\n#pragma file(ex_d)\nvoid() f = E(:__FILE__);\n".to_owned());
    sources
}

/// Functions with no body, `noref` or not and referred to or not, plain and as macros write them
/// (FTEF-17.11), and declarators ended by a `,` (FTEF-15.8).
const DECLARATION_SOURCES: &[&str] = &[
    "void() f = #5;\nvoid() g;\n",
    "void() f = #5, g;\n",
    "void() f = #5;\nnoref void() g;\n",
    "void() f = #5;\nvoid() g;\nvoid() h = {g();};\n",
    "void() f = #5;\nnoref void() g;\nvoid() h = {g();};\n",
    "void() f = #5;\n#pragma noref 1\nvoid() g;\n",
    "void() f = #5;\n#pragma noref 1\nvoid() g;\n#pragma noref 0\nvoid() h = {g();};\n",
    "#define V(a, ...) a,##__VA_ARGS__\nvoid() f = V(#5, g);\n",
    "#define V(a, ...) a __VA_ARGS__\nvoid() f = V(#5, :ex_a, b);\n",
    "void() f = #5, ;\n",
    "void() f = #5,;\n",
    "void() f = #5, , ;\n",
    "void() g, f = #5, ;\n",
];

/// The macro-text family (FTEF-14.22 to FTEF-14.27, FTEF-14.32, P2-80 to P2-84 and P2-87, from
/// the fourth P2-R2 review): programs whose macros FTEQCC reads as text where a token model
/// would not, and the declarations they write. Each is a whole program.
fn macro_text_cases() -> Vec<String> {
    let uses = [
        argument_uses(),
        parse2_uses(),
        line_uses(),
        body_uses(),
        parameter_uses(),
        variadic_uses(),
    ];
    let mut sources: Vec<String> = uses
        .into_iter()
        .flatten()
        .map(|(define, use_)| format!("{define}\nvoid() f = {use_};\n"))
        .collect();
    sources.extend(file_sources());
    sources.extend(DECLARATION_SOURCES.iter().map(|&source| source.to_owned()));
    sources
}

/// The lengths of the long names given after `#5:` and as a builtin's own name: about id's
/// limit and FTEQCC's (QC-3.3, FTEF-15.7).
const NAME_LENGTHS: [usize; 6] = [62, 63, 70, 254, 255, 300];

/// Strings FTEQCC never finishes reading (FTEF-13.8): the oracle is killed after
/// [`ORACLE_LIMIT`].
const HANGING: &[&str] = &["\"\\UFFFFFFFF\"", "u8\"\\U80000000\""];

/// The forms of [`PAIRED`] the vanilla dialect is given twice: those of QC-13.3.
fn vanilla_paired(form: &str) -> bool {
    form.starts_with('#') && !form.contains([':', '"'])
}

/// Every case, in a fixed order.
pub(crate) fn cases() -> Vec<Case> {
    // FTE-23.6: names and strings longer than FTEQCC's 255-byte `fname`, and than id's
    // 63-byte names (QC-3.3).
    let mut values: Vec<String> = VALUES.iter().map(|&value| value.to_owned()).collect();
    for length in [70, 300] {
        values.push("n".repeat(length));
        values.push(format!("\"{}\"", "s".repeat(length)));
    }
    let mut sources = Vec::new();
    for prefix in PREFIXES {
        for value in &values {
            for suffix in SUFFIXES {
                sources.push(format!("{}{prefix}{value}{suffix};\n", DECLARATORS[0]));
            }
            for declarator in DECLARATORS.iter().skip(1) {
                sources.push(format!("{declarator}{prefix}{value};\n"));
            }
        }
    }
    let mut cases = Vec::new();
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        for source in &sources {
            cases.push(Case::new(dialect, source.clone()));
        }
        let paired: Vec<&str> = PAIRED
            .iter()
            .copied()
            .filter(|form| dialect.has_fteqcc_extensions() || vanilla_paired(form))
            .collect();
        for first in &paired {
            for second in &paired {
                for other in [false, true] {
                    let mut source = String::new();
                    if other {
                        source.push_str(&format!("void() g = {first};\n"));
                    }
                    source.push_str(&format!("void() f = {first};\nvoid() f = {second};\n"));
                    cases.push(Case::new(dialect, source));
                }
            }
        }
    }
    // FTE-23.10: id's qcc has no preprocessor, so the macro cases are FTEQCC's only.
    let fte = |source: String| Case::new(Dialect::Fteqcc, source);
    for body in MACRO_BODIES {
        for use_ in MACRO_USES {
            cases.push(fte(format!("#define N {body}\nvoid() f = {use_};\n")));
        }
    }
    for (define, use_) in OTHER_MACROS {
        cases.push(fte(format!("{define}void() f = {use_};\n")));
    }
    for value in HANGING {
        cases.push(fte(format!("void() f = :{value};\n")));
    }
    // FTEF-13.13: after `:`, a character constant's code is `f`'s builtin number, or an
    // escape's error refuses it.
    for escape in CHARACTER_ESCAPES {
        cases.push(fte(format!("void() f = :'\\{escape}';\n")));
    }
    // FTEF-14.13, QC-9.16: `f` is bound only when `g`'s body, which uses the macros, has no
    // error; `f` comes first, so its function is the first (`bound`).
    let with_body = |define: &str, statement: &str| {
        format!(
            "#define N 5\n{define}\nvoid() f = #5;\nvoid() g = {{ local float x; {statement} }};\n"
        )
    };
    for follower in word_followers() {
        cases.push(fte(with_body("", &format!("x = N{follower};"))));
    }
    for (define, use_) in WORD_MACROS {
        cases.push(fte(with_body(define, &format!("x = {use_};"))));
    }
    for (define, use_) in LABEL_MACROS {
        cases.push(fte(with_body(define, &format!("{use_} goto lbl;"))));
    }
    for (define, use_) in ARGUMENT_BUILTINS {
        cases.push(fte(format!("{define}\nvoid() f = {use_};\n")));
    }
    for (define, use_) in EXPANSION_INITIALISERS {
        cases.push(fte(format!("{define}\nvoid() f = {use_};\n")));
    }
    for (define, statement) in EXPANSION_STATEMENTS {
        cases.push(fte(with_body(define, statement)));
    }
    for source in macro_text_cases() {
        cases.push(fte(source));
    }
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        for length in NAME_LENGTHS {
            let name = "n".repeat(length);
            for source in [
                format!("void() f = #5:{name};\n"),
                format!("void() {name} = #5;\n"),
            ] {
                cases.push(Case::new(dialect, source));
            }
        }
    }
    cases.extend(directives::cases());
    // `# :ex_b` is both `# ` before nothing and `#` before ` :ex_b`.
    let mut seen = HashSet::new();
    cases.retain(|case| seen.insert(case.key()));
    cases
}

/// How wide C's `unsigned long` is where an oracle ran, which decides where FTEQCC's `strtoul`
/// saturates (FTEF-14.50).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ULong {
    /// 32 bits: an LLP64 host, 64-bit Windows.
    Bits32,
    /// 64 bits: an LP64 host, 64-bit Linux and macOS, as the profile models and [`FIXTURE`]
    /// records.
    Bits64,
}

impl ULong {
    /// The host's, which the oracles are compiled for.
    pub(crate) const HOST: Self = if size_of::<std::ffi::c_ulong>() == size_of::<u32>() {
        Self::Bits32
    } else {
        Self::Bits64
    };

    /// Whether an oracle on a host of this width may read `case` otherwise than the profile
    /// does (FTEF-14.50): a `#pragma line` number whose low 32 bits a 32-bit `strtoul` reads
    /// otherwise than a 64-bit one.
    fn reads_otherwise(self, case: &Case) -> bool {
        self == Self::Bits32
            && case.texts().any(|text| {
                text.lines().filter_map(line_pragma_text).any(|number| {
                    low_32_bits(c_strtoul(number, u64::MAX))
                        != low_32_bits(c_strtoul(number, u64::from(u32::MAX)))
                })
            })
    }
}

/// The text a `#pragma line` on `line` gives `strtoul` (FTEF-14.43): after the name, matched in
/// any case and ending at a space, tab or `(`, and after a `(` the text starts with.
fn line_pragma_text(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('#')?;
    let rest = rest.trim_start_matches([' ', '\t']);
    let pragma = rest
        .get(..6)
        .is_some_and(|name| name.eq_ignore_ascii_case("pragma"));
    let rest = rest.get(6..).filter(|_| pragma)?;
    let rest = rest.trim_start_matches([' ', '\t']);
    let end = rest.find([' ', '\t', '(']).unwrap_or(rest.len());
    let (name, text) = rest.split_at(end);
    let text = text.trim_start_matches([' ', '\t']);
    name.eq_ignore_ascii_case("line")
        .then(|| text.strip_prefix('(').unwrap_or(text))
}

/// C's `strtoul(text, &end, 0)` where `unsigned long` holds at most `max`: white space, a sign,
/// then `0x` and hex digits, `0` and octal digits, or decimal digits; a value past `max` is
/// `max`, and a `-` negates the value modulo `max + 1`.
fn c_strtoul(text: &str, max: u64) -> u64 {
    let text = text.trim_start_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
    let (negative, text) = match text.as_bytes().first() {
        Some(b'-') => (true, text.get(1..).unwrap_or_default()),
        Some(b'+') => (false, text.get(1..).unwrap_or_default()),
        _ => (false, text),
    };
    let bytes = text.as_bytes();
    let hex = matches!(bytes, [b'0', b'x' | b'X', digit, ..] if digit.is_ascii_hexdigit());
    let (radix, digits) = if hex {
        (16, text.get(2..).unwrap_or_default())
    } else if bytes.first() == Some(&b'0') {
        (8, text)
    } else {
        (10, text)
    };
    let mut value = 0_u64;
    for digit in digits.chars().map_while(|c| c.to_digit(radix)) {
        match value
            .checked_mul(u64::from(radix))
            .and_then(|v| v.checked_add(u64::from(digit)))
            .filter(|&v| v <= max)
        {
            Some(next) => value = next,
            None => return max,
        }
    }
    if negative {
        value.wrapping_neg() & max
    } else {
        value
    }
}

/// The low 32 bits of `value`, which FTEQCC's `int` keeps (FTEF-14.43).
fn low_32_bits(value: u64) -> u64 {
    value & u64::from(u32::MAX)
}

/// The spec statement that says why the profile's outcome `ours` for `case`, with the error
/// codes `errors`, may differ from `oracle`'s, an oracle that ran where `unsigned long` is
/// `ulong` wide, if one does (FTE-23.15).
pub(crate) fn expected_difference(
    case: &Case,
    ours: &Outcome,
    errors: &[Code],
    oracle: &Outcome,
    ulong: ULong,
) -> Option<&'static str> {
    let vanilla = !case.dialect.has_fteqcc_extensions();
    if !vanilla {
        // FTEF-14.50: the profile reads a `#pragma line` number as a 64-bit `strtoul` does, so
        // only the line `__LINESTR__` names may differ.
        let same_number = matches!(
            (ours, oracle),
            (Outcome::Bound { number: a, .. }, Outcome::Bound { number: b, .. }) if a == b
        );
        if same_number && ulong.reads_otherwise(case) {
            return Some("FTEF-14.50");
        }
        // FTEF-13.8: FTEQCC never finishes writing a code of 0x80000000 or more.
        if *oracle == Outcome::Hangs && HANGING.iter().any(|value| case.source.contains(value)) {
            return Some("FTEF-13.8");
        }
        // FTEF-14.47: an `#eof` that is not a file's first line ends the text here, where the
        // pinned FTEQCC crashes, reading on from the end it made; and `#merge` and `#datafile`
        // open no file here, where FTEQCC refuses a missing one.
        let refused_here_only = *oracle == Outcome::Rejected && errors.is_empty();
        let crashed_at_eof = case.texts().any(eof_past_first_line);
        let opened_a_file = case
            .texts()
            .any(|t| t.contains("#merge") || t.contains("#datafile"));
        if refused_here_only && (crashed_at_eof || opened_a_file) {
            return Some("FTEF-14.47");
        }
        // FTE-23.11: a macro after `#` may leave `f` initialised with a number, which FTEQCC
        // cannot cast to a function and this profile, which checks no types, accepts.
        let uncast = *oracle == Outcome::Rejected
            && *ours == Outcome::Accepted
            && errors.is_empty()
            && value_initialiser(case);
        return uncast.then_some("FTE-23.11");
    }
    // QC-27.9: the vanilla profile makes `#0` and negative numbers errors, which id's qcc
    // accepts with no builtin number (QC-13.4). Only that error may differ: without it the
    // profile's outcome must be the oracle's.
    if !errors.contains(&Code::E0507) {
        return None;
    }
    let rest: Vec<Code> = errors
        .iter()
        .copied()
        .filter(|code| *code != Code::E0507)
        .collect();
    let without = if rest.is_empty() {
        Outcome::Accepted
    } else {
        refusal(case, &rest)
    };
    (without == *oracle).then_some("QC-27.9")
}

/// The profile's outcome for `case`, and the codes of its errors in the order of their place.
pub(crate) fn profile(case: &Case) -> (Outcome, Vec<Code>) {
    let (mut db, mut errors) = analysed(case);
    errors.sort_by_key(|(start, _)| *start);
    let errors: Vec<Code> = errors.into_iter().map(|(_, code)| code).collect();
    if errors.is_empty() {
        return (binding(&mut db, case).unwrap_or(Outcome::Accepted), errors);
    }
    (refusal(case, &errors), errors)
}

/// `case` as the compiler reads it, through the preprocessor (`Db::analyze_module`): the
/// database, and each error's place and code.
fn analysed(case: &Case) -> (Db, Vec<(usize, Code)>) {
    let mut db = Db::new();
    db.set_dialect(case.dialect);
    db.set_file(
        FileId(0),
        PathBuf::from(case.compiled()),
        case.source.as_bytes().to_vec(),
    );
    // The other files are the preprocessor's to `#include` (PRJ-5.2, FTEF-14.40).
    for (index, (name, text)) in case.project.iter().flat_map(|p| &p.files).enumerate() {
        let id = FileId(index.saturating_add(1));
        db.set_file(id, PathBuf::from(name), text.as_bytes().to_vec());
    }
    let analysis = db.analyze_module(&ModuleInput {
        prelude: Vec::new(),
        compiled: vec![FileId(0)],
        autoproto: None,
        include_dirs: case.include_dirs().map(PathBuf::from).collect(),
    });
    let errors = analysis
        .diagnostics()
        .filter(|d| d.diagnostic.code.is_error())
        .map(|d| (d.diagnostic.span.start, d.diagnostic.code))
        .collect();
    (db, errors)
}

/// How the profile refuses `case` with the errors `errors`, in the order of their place.
///
/// id's qcc stops at the first error, and finds a redeclaration at its `=`, before the
/// initialiser (`PR_ParseDefs`); FTEQCC reports one only once it has read the initialiser
/// (FTE-23.7), so any other error comes first.
fn refusal(case: &Case, errors: &[Code]) -> Outcome {
    let redeclared = if case.dialect.has_fteqcc_extensions() {
        errors.iter().all(|code| *code == Code::E0206)
    } else {
        errors.first() == Some(&Code::E0206)
    };
    if redeclared {
        Outcome::Redeclared
    } else {
        Outcome::Rejected
    }
}

/// The builtin number and engine name the profile binds the first function of `case` to:
/// under FTEQCC the builtin function qc-sema lowers its initialiser to (FTE-23.7), from the
/// file's expansion (FTE-23.10, QC-9.16); under the vanilla dialect the number
/// `builtin_binum` gives and the definition's name (QC-13.10).
fn binding(db: &mut Db, case: &Case) -> Option<Outcome> {
    let name = function_name(case);
    if case.dialect.has_fteqcc_extensions() {
        let function = expanded_literal(db, name)?.builtin_function(name)?;
        return Some(Outcome::Bound {
            number: function.number,
            name: function.name,
        });
    }
    let parse = db.parse(FileId(0));
    let builtin = parse
        .tree()?
        .declarations()
        .flat_map(|decl| decl.declarators().collect::<Vec<_>>())
        .find(|declarator| declarator.name().is_some_and(|n| n.text_lossless() == name))
        .and_then(|declarator| match declarator.initializer() {
            Some(ast::Initializer::Builtin(builtin)) => Some(builtin),
            _ => None,
        })?;
    let number = builtin.number()?;
    let number = qc_syntax::builtin_binum(&number.text_lossless(), case.dialect)?;
    Some(Outcome::Bound {
        number,
        name: name.to_owned(),
    })
}

/// The literal the analysed `db` lowers the first definition `name` of its file's expansion
/// to, if any.
fn expanded_literal<'a>(db: &'a Db, name: &str) -> Option<&'a qc_sema::LiteralValue> {
    db.expanded_decls(FileId(0))?
        .decls
        .iter()
        .find(|decl| decl.name == name)?
        .literal
        .as_ref()
}

/// The name of the function `case` binds: `f`, or the long name of `n` bytes a case of
/// [`NAME_LENGTHS`] declares with `void() ` (FTEF-15.7).
fn function_name(case: &Case) -> &str {
    case.source
        .lines()
        .last()
        .and_then(|line| line.strip_prefix("void() "))
        .and_then(|rest| rest.split([' ', '=', ';']).next())
        .filter(|name| name.len() > 1 && name.bytes().all(|b| b == b'n'))
        .unwrap_or("f")
}

/// Whether `text` has an `#eof` that FTEQCC would run past its first line: one that starts a
/// line after spaces and tabs (FTEF-14.52, FTEF-14.47). A line of a skipped side matches too, as
/// the text alone does not say which sides FTEQCC skips; FTEQCC accepts those, and only a
/// refusal is matched.
fn eof_past_first_line(text: &str) -> bool {
    text.split(['\n', '\r'])
        .skip(1)
        .any(|line| line.trim_start_matches([' ', '\t']).starts_with("#eof"))
}

/// Whether the FTEQCC `case`, which uses a macro or a built-in constant (FTEF-14.7), initialises
/// its function with a number or a vector once preprocessed, such as the number a macro after
/// `#` stands for (FTE-23.11).
fn value_initialiser(case: &Case) -> bool {
    let uses_macros = case.source.contains("#define") || case.source.contains("#__");
    let (db, _) = analysed(case);
    uses_macros
        && matches!(
            expanded_literal(&db, function_name(case)),
            Some(qc_sema::LiteralValue::Float(_) | qc_sema::LiteralValue::Vector(_))
        )
}

/// Whether the first builtin of the vanilla `case` is a number whose value is 0, read from the
/// source text with Rust's own float parser, not the profile's: id's qcc skips the spaces
/// after `#` and reads digits, a `-` and dots (`PR_LexNumber`).
fn vanilla_zero(case: &Case) -> bool {
    let Some((_, after)) = case.source.split_once('#') else {
        return false;
    };
    let number: String = after
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-' || *c == '.')
        .collect();
    number.parse::<f64>().is_ok_and(|value| value == 0.0)
}

/// The oracle's outcome for `case`, compiled in `dir`, or for a case of a [`Project`] in a
/// directory of its own in `dir`, made afresh, so that no file of another case is there.
fn oracle(oracles: &Oracles, dir: &Path, case: &Case) -> Result<Outcome, String> {
    let fresh = dir.join("project");
    let dir = if case.project.is_some() {
        if fresh.exists() {
            fs::remove_dir_all(&fresh).map_err(|e| format!("{}: {e}", fresh.display()))?;
        }
        fresh.as_path()
    } else {
        dir
    };
    let write = |name: &str, text: &[u8]| {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    };
    let compiled = case.compiled();
    write("progs.src", format!("progs.dat\n{compiled}\n").as_bytes())?;
    write(compiled, case.source.as_bytes())?;
    for (name, text) in case.project.iter().flat_map(|p| &p.files) {
        write(name, text.as_bytes())?;
    }
    let progs = dir.join("progs.dat");
    if progs.exists() {
        fs::remove_file(&progs).map_err(|e| format!("{}: {e}", progs.display()))?;
    }
    let fteqcc = case.dialect.has_fteqcc_extensions();
    let mut command = if fteqcc {
        let mut command = Command::new(&oracles.fteqcc);
        command.arg("-Tid");
        command.args(case.include_dirs().map(|dir| format!("-I{dir}")));
        command
    } else {
        Command::new(&oracles.qcc)
    };
    let output = oracles
        .exec
        .output_within(command.current_dir(dir), ORACLE_LIMIT)
        .map_err(|e| format!("cannot run {}: {e}", if fteqcc { "fteqcc" } else { "qcc" }))?;
    let Some(output) = output else {
        return Ok(Outcome::Hangs);
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        let redeclared = if fteqcc {
            first_error(&text).is_some_and(|line| line.ends_with("incompatible redeclaration"))
        } else {
            first_error(&text).is_some_and(|line| line.ends_with(" redeclared"))
        };
        return Ok(if redeclared {
            Outcome::Redeclared
        } else {
            Outcome::Rejected
        });
    }
    // FTE-23.11: FTEQCC writes no progs when it has made nothing to write, as for a function
    // initialised with 0 a macro gave.
    if fteqcc && !progs.exists() && text.contains("nothing to write") {
        return Ok(Outcome::Accepted);
    }
    let bytes =
        fs::read(&progs).map_err(|e| format!("{}: {}: {e}", show(case), progs.display()))?;
    // QC-13.4: id's qcc leaves `#0`'s `first_statement` uninitialised, so what it wrote
    // depends on the host's heap and is not recorded.
    if !fteqcc && vanilla_zero(case) {
        return Ok(Outcome::Accepted);
    }
    // PROGS-15.8: these programs have none of id's system definitions.
    let options = ReadOptions {
        check_header_crc: false,
    };
    match Progs::read_with(&bytes, &options) {
        Ok(read) => bound(&read, case)
            .ok_or_else(|| format!("{}: no function f in {}", show(case), progs.display())),
        // QC-13.4: id's qcc leaves `#0`'s `first_statement` uninitialised, which qc-progs may
        // refuse (PROGS-10.2).
        Err(_) if !fteqcc => Ok(Outcome::Accepted),
        Err(e) => Err(format!(
            "{}: qc-progs refuses {}: {e:?}",
            show(case),
            progs.display()
        )),
    }
}

/// The first line of a compiler's output that reports an error at a line of `t.qc`.
fn first_error(text: &str) -> Option<&str> {
    text.lines().find(|line| {
        line.starts_with("t.qc:")
            && !line.contains(": warning")
            && !line.contains("is defined here")
    })
}

/// What `f`'s function is in a `progs.dat`: minus its `first_statement` (PROGS-10.2) and its
/// name, or [`Outcome::Accepted`] when `first_statement` is no builtin's (QC-13.4).
///
/// FTEQCC writes no global definition for an unreferenced constant, so `f`'s function is found
/// by its place: both compilers number functions in the order they make them, from 1, and a
/// case makes `g`'s before `f`'s and none after `f`'s that it accepts.
fn bound(progs: &Progs, case: &Case) -> Option<Outcome> {
    let index = if case.source.starts_with("void() g") {
        2
    } else {
        1
    };
    let Some(function) = progs.functions().get(index) else {
        // FTE-23.11: FTEQCC makes no function for `f` initialised with a string a macro gave,
        // which it casts with a warning.
        let fteqcc = case.dialect.has_fteqcc_extensions();
        return (fteqcc && progs.functions().len() == index).then_some(Outcome::Accepted);
    };
    // FTEQCC writes every builtin's `first_statement` as `-binum`, a negative number's
    // included (`qccmain.c`, `if (functions[i].code == -1) funcs[i].first_statement =
    // PRLittleLong (-functions[i].builtin);`); id's qcc only a nonzero one's, and `#0`'s is
    // uninitialised (QC-13.4).
    if !case.dialect.has_fteqcc_extensions() && function.first_statement >= 0 {
        return Some(Outcome::Accepted);
    }
    let name = progs.string(function.s_name)?;
    Some(Outcome::Bound {
        number: function.first_statement.wrapping_neg(),
        // Each byte one `char` (Latin-1), as `Builtin::engine_name` gives a name.
        name: name.iter().copied().map(char::from).collect(),
    })
}

/// Reads [`FIXTURE`]: each case's key and the oracle's outcome.
pub(crate) fn read_fixture(root: &Path) -> Result<BTreeMap<String, Outcome>, String> {
    let path = root.join(FIXTURE);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let raw: BTreeMap<String, String> =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    raw.into_iter()
        .map(|(key, outcome)| {
            Outcome::parse(&outcome)
                .map(|outcome| (key.clone(), outcome))
                .ok_or_else(|| format!("{FIXTURE}: {key}: bad outcome `{outcome}`"))
        })
        .collect()
}

/// Compares the profile with `recorded`, the outcomes of oracles that ran where `unsigned long`
/// is `ulong` wide; prints each disagreement and returns how many cases agree and how many do
/// not.
pub(crate) fn compare(cases: &[Case], recorded: &BTreeMap<String, Outcome>, ulong: ULong) -> Tally {
    let mut tally = Tally::default();
    for case in cases {
        let Some(oracle) = recorded.get(&case.key()) else {
            tally.differ = tally.differ.saturating_add(1);
            println!("FAIL  {}: not recorded in {FIXTURE}", show(case));
            continue;
        };
        let (ours, errors) = profile(case);
        if ours == *oracle {
            tally.agree = tally.agree.saturating_add(1);
        } else if let Some(statement) = expected_difference(case, &ours, &errors, oracle, ulong) {
            let count = tally.expected.entry(statement).or_default();
            *count = count.saturating_add(1);
        } else {
            tally.differ = tally.differ.saturating_add(1);
            println!("FAIL  {}: oracle {oracle}, profile {ours}", show(case));
        }
    }
    tally
}

/// Whether the project graph of `case`, a graph case of the directive family, lists a file that
/// binds `f` (PRJ-5.13): the module is the `progs.src` the oracle reads, and under the FTEQCC
/// dialect.
fn graph_binds(case: &Case) -> bool {
    let mut loader = qc_project::MemoryLoader::new();
    let compiled = case.compiled();
    loader.insert("progs.src", format!("progs.dat\n{compiled}\n").as_bytes());
    loader.insert(compiled, case.source.as_bytes());
    let mut binding = Vec::new();
    for (name, text) in case.project.iter().flat_map(|p| &p.files) {
        loader.insert(name.as_str(), text.as_bytes());
        if text == directives::BIND {
            binding.push(PathBuf::from(name));
        }
    }
    let dirs: Vec<PathBuf> = case.include_dirs().map(PathBuf::from).collect();
    let graph = qc_project::build_graph(
        &mut loader,
        Path::new("progs.src"),
        &[],
        &dirs,
        Dialect::Fteqcc,
    );
    graph.files.iter().any(|file| binding.contains(&file.path))
}

/// The project graph's cases (PRJ-5.13): programs whose one file that binds `f` FTEQCC reads,
/// binding `f` to `#5`, or does not, accepting the program with no `f`. The preprocessor reads no
/// `#includelist`, so the profile is not compared on them.
pub(crate) fn graph_cases() -> Vec<Case> {
    directives::graph_cases()
}

/// The graph cases whose project graph lists the file that binds `f` where `outcomes` say the
/// oracle did not read it, or the other way round, or whose oracle neither bound `f` to `#5` nor
/// accepted the program, so that the case says nothing of what it read (PRJ-5.13).
pub(crate) fn graph_differences(outcomes: &BTreeMap<String, Outcome>) -> Vec<String> {
    let mut differ = Vec::new();
    for case in graph_cases() {
        let read = match outcomes.get(&case.key()) {
            Some(Outcome::Bound { number: 5, .. }) => true,
            Some(Outcome::Accepted) => false,
            Some(oracle) => {
                differ.push(format!(
                    "{}: oracle {oracle}, which reads no file",
                    show(&case)
                ));
                continue;
            }
            None => {
                differ.push(format!("{}: not recorded in {FIXTURE}", show(&case)));
                continue;
            }
        };
        if graph_binds(&case) != read {
            let which = if read {
                "the oracle reads the file, the graph does not"
            } else {
                "the oracle does not read the file, the graph does"
            };
            differ.push(format!("{}: {which}", show(&case)));
        }
    }
    differ
}

/// How the cases of [`compare`] came out.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    /// The profile's outcome is the oracle's.
    pub(crate) agree: usize,
    /// The outcomes differ as each spec statement says they do, with how many cases.
    pub(crate) expected: BTreeMap<&'static str, usize>,
    /// The outcomes differ, and no statement says why.
    pub(crate) differ: usize,
}

impl fmt::Display for Tally {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} agree with the profile", self.agree)?;
        for (statement, count) in &self.expected {
            write!(f, ", {count} differ as {statement} says")?;
        }
        write!(f, ", {} differ", self.differ)
    }
}

fn show(case: &Case) -> String {
    match &case.project {
        None => format!("{} {:?}", dialect_name(case.dialect), case.source),
        Some(project) => format!(
            "{} {}: {:?}, with {:?}",
            dialect_name(case.dialect),
            project.compiled,
            case.source,
            project.files
        ),
    }
}

/// Runs every case through the oracles; checks them against [`FIXTURE`] (or, with `record`,
/// writes it) and the profile against them.
pub(crate) fn check(root: &Path, oracles: &Oracles, record: bool) -> Result<bool, String> {
    check_where(root, oracles, record, ULong::HOST)
}

/// [`check`] with oracles that run where `unsigned long` is `ulong` wide, which records
/// nothing unless it is 64 bits (FTEF-14.50).
fn check_where(root: &Path, oracles: &Oracles, record: bool, ulong: ULong) -> Result<bool, String> {
    if record && ulong != ULong::Bits64 {
        return Err(format!(
            "{FIXTURE} records the outcomes where `unsigned long` is 64 bits (FTEF-14.50); \
             record it on Linux or macOS"
        ));
    }
    let work = oracles.work.join("builtins");
    fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
    let cases = cases();
    let graph_cases = graph_cases();
    let mut outcomes = BTreeMap::new();
    for case in cases.iter().chain(&graph_cases) {
        // A graph case may be a case of the cross product too.
        if outcomes.contains_key(&case.key()) {
            continue;
        }
        match oracle(oracles, &work, case) {
            Ok(outcome) => outcomes.insert(case.key(), outcome),
            Err(problem) => {
                // An oracle that misbehaves on one case says nothing about the profile.
                println!("FAIL  builtin differential: {problem}");
                return Ok(false);
            }
        };
    }
    let mut ok = true;
    if record {
        let raw: BTreeMap<&String, String> =
            outcomes.iter().map(|(k, v)| (k, v.to_string())).collect();
        let text = serde_json::to_string_pretty(&raw).map_err(|e| e.to_string())?;
        let path = root.join(FIXTURE);
        fs::write(&path, format!("{text}\n")).map_err(|e| format!("{}: {e}", path.display()))?;
        println!(
            "oracles: recorded {} builtin outcomes in {FIXTURE}",
            outcomes.len()
        );
    } else {
        let recorded = read_fixture(root)?;
        // A changed cross product is recorded again with `--record` once it is right. Where
        // `unsigned long` is 32 bits a wide `#pragma line` may read otherwise (FTEF-14.50).
        for case in cases.iter().chain(&graph_cases) {
            let key = case.key();
            let Some(outcome) = outcomes.get(&key) else {
                continue;
            };
            if recorded.get(&key) != Some(outcome) && !ulong.reads_otherwise(case) {
                ok = false;
                println!(
                    "FAIL  {key:?}: the oracle gives {outcome}, not as {FIXTURE} records (--record)"
                );
            }
        }
        if recorded.len() != outcomes.len() {
            ok = false;
            println!("FAIL  {FIXTURE} records cases the cross product does not make (--record)");
        }
    }
    let tally = compare(&cases, &outcomes, ulong);
    println!(
        "oracles: {} builtin initialiser cases: {tally}",
        cases.len()
    );
    let graph = graph_differences(&outcomes);
    for problem in &graph {
        println!("FAIL  project graph: {problem}");
    }
    println!(
        "oracles: {} project graph cases, {} differ",
        graph_cases.len(),
        graph.len()
    );
    Ok(ok && tally.differ == 0 && graph.is_empty())
}

#[cfg(test)]
mod tests;
