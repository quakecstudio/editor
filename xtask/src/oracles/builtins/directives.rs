// SPDX-License-Identifier: GPL-3.0-or-later

//! The directive family of the builtin differential (P2-94 to P2-99): every directive and pragma
//! FTEQCC's `QCC_PR_Precompiler` knows, read where FTEQCC reads one, against what the FTEQCC
//! profile makes of it (`docs/spec/dialect-fteqcc.md` FTEF-14.33 to FTEF-14.47). Each program
//! binds `f` only when the directive did what FTEQCC does with it: ended its line where
//! `QCC_PR_SkipToEndOfLine` ends it, defined or skipped what it should, numbered the lines as
//! `#pragma line` says, or found the file an `#include` names.

use qc_syntax::Dialect;

use super::{Case, Encoding, ProgsSrc, Project};

/// What binds `f` once the directive before it has been read.
pub(super) const BIND: &str = "void() f = #5;\n";

/// Directives whose line `QCC_PR_SkipToEndOfLine` ends, each with what opens it before and
/// closes it after (FTEF-14.35): taken sides, so that what follows is read.
const LINE_DIRECTIVES: &[(&str, &str, &str)] = &[
    ("", "#define X 1", ""),
    ("#define X 1\n", "#undef X", ""),
    ("#define X 1\n", "#ifdef X", "#endif\n"),
    ("", "#ifndef X", "#endif\n"),
    ("", "#if 1", "#endif\n"),
    ("#if 1\n", "#endif", ""),
    ("#if 0\n", "#else", "#endif\n"),
    ("#if 1\n", "#else", "#endif\n"),
    ("#if 1\n", "#elif 1", "#endif\n"),
    ("#define X 1\n", "#append X 2", ""),
    ("", "#pragma noref 0", ""),
    ("", "#forcecrc 5", ""),
    ("", "#output \"progs.dat\"", ""),
    ("", "#copyright x", ""),
    ("", "#message x", ""),
    ("", "#warning x", ""),
    ("", "#pack 1", ""),
    ("", "#includelist\n#endlist", ""),
];

/// The tails after which `QCC_PR_SkipToEndOfLine` reads the next line as the directive's own
/// (FTEF-14.35).
const CARRYING_TAILS: &[&str] = &[" // c \\", " // c \\\r", " \\"];

/// What may end a directive's line (FTEF-14.35): nothing, a `//` comment, one with a `\` and a
/// line break in it, `/* */` comments on one line and over two, a `\` and a line break, a
/// string, and a word.
const LINE_TAILS: &[&str] = &[
    "",
    " // c",
    " // c \\",
    " // c \\\r",
    " /* c */",
    " /* c\n */",
    " \\",
    " \"s\"",
    " zz",
];

/// Every line ends and goes on to `zz zz zz`, which only a line that goes on hides, and then
/// to `f`'s binding.
fn line_end_cases() -> Vec<String> {
    let mut sources = Vec::new();
    for (open, directive, close) in LINE_DIRECTIVES {
        for tail in LINE_TAILS {
            sources.push(format!("{open}{directive}{tail}\nzz zz zz\n{BIND}{close}"));
            sources.push(format!("{open}{directive}{tail}\n{BIND}{close}"));
        }
    }
    // An `#includelist`'s words name files the graph compiles (PRJ-5.3), so its line is ended
    // before an empty list, or before words only a carrying tail hides.
    for tail in LINE_TAILS {
        sources.push(format!("#includelist{tail}\n#endlist\n{BIND}"));
    }
    for tail in CARRYING_TAILS {
        sources.push(format!("#includelist{tail}\nzz zz zz\n#endlist\n{BIND}"));
    }
    sources
}

/// Directive names matched by their first bytes, and the operands each reads (FTEF-14.52,
/// FTEF-14.57, FTEF-14.38, FTEF-14.39, FTEF-14.42).
const NAME_FORMS: &[&str] = &[
    "#defined N #5\nvoid() f = N;\n",
    "#define/**/N #5\nvoid() f = #5;\n",
    "#defineN #5\nvoid() f = #5;\n",
    "#define(N) #5\nvoid() f = #5;\n",
    "#define\nvoid() f = #5;\n",
    "#appendix N #5\nvoid() f = N;\n",
    "#define N #6\n#undefine N\nvoid() f = #5;\n",
    "#define N 1\n#undefN\n#ifdef N\nzz\n#endif\nvoid() f = #5;\n",
    "#define N 1\n#undef(N)\n#ifdef N\nvoid() f = #5;\n#endif\n",
    "#define N 1\n#undef\n#ifdef N\nvoid() f = #5;\n#endif\n",
    "#undef N\nvoid() f = #5;\n",
    "#ifdefX\nvoid() f = #5;\n",
    "#ifndefX\nvoid() f = #5;\n",
    "#iffy\nvoid() f = #5;\n",
    "#if(1)\nvoid() f = #5;\n",
    "#ifdef(X)\nvoid() f = #5;\n",
    "#ifdef\nvoid() f = #5;\n#endif\n",
    "#ifndef\nvoid() f = #5;\n#endif\n",
    "#elsewhere\nvoid() f = #5;\n",
    "#if 1\nvoid() f = #5;\n#elsewhere\nzz\n#endif\n",
    "#if 1\nvoid() f = #5;\n#endiff\n",
    "#if 1\nvoid() f = #5;\n#endif // c\n",
    "#endiff\nvoid() f = #5;\n",
    "void() f = #5;\n#eofx\nzz\n",
    "void() f = #5;\n#eof zz\nzz\n",
    "#errors\nvoid() f = #5;\n",
    "#errorx\nvoid() f = #5;\n",
    "#warnings x\nvoid() f = #5;\n",
    "#messages x\nvoid() f = #5;\n",
    "#copyrights x\nvoid() f = #5;\n",
    "void() f = #5;\n#packages\nzz\n",
    "void() f = #5;\n#package // c \\\nzz\n",
    "#packid\nvoid() f = #5;\n",
    "#pack id\nvoid() f = #5;\n",
    "#pack\nzz zz zz\nvoid() f = #5;\n",
    "#pack 6\nzz\nvoid() f = #5;\n",
    "#forcecrc\nzz zz zz\nvoid() f = #5;\n",
    "#forcecrcx\nvoid() f = #5;\n",
    "#outputs \"progs.dat\"\nvoid() f = #5;\n",
    "#output\nvoid() f = #5;\n",
    "#includes \"b.qh\"\nvoid() f = #5;\n",
    "#include b.qh\nvoid() f = #5;\n",
    "#include \"b.qh\nvoid() f = #5;\n",
    "#include \"\"\nvoid() f = #5;\n",
    "#pragmas once\nvoid() f = #5;\n",
    "#pragma\nvoid() f = #5;\n",
    "#pragma unknown x\nvoid() f = #5;\n",
    "#foo\nvoid() f = #5;\n",
    "# \nvoid() f = #5;\n",
    "#\nvoid() f = #5;\n",
    "#  define N #5\nvoid() f = N;\n",
    "#\tdefine N #5\nvoid() f = N;\n",
    "#\x0bdefine N #5\nvoid() f = N;\n",
    "#endlist\nvoid() f = #5;\n",
    "#define N #5\nvoid() f =\n#N;\n",
];

/// Macro names about FTEQCC's limit of 63 bytes (FTEF-14.42).
const NAME_LENGTHS: [usize; 5] = [1, 62, 63, 64, 65];

/// Words and strings about the 1023 bytes FTEQCC's word readers take, and `#pragma` text about
/// its `char msg[1024]` (FTEF-14.66): at 1024 each stops the compile, at 1023 none does, though
/// a 1023-byte frame name fails for its length. A comment the text ends in, before an
/// `#includelist` word or in a use's arguments, is "EOF inside comment" (FTEF-14.67).
fn word_bound_cases() -> Vec<String> {
    let mut sources = Vec::new();
    for length in [1023, 1024] {
        let word = "w".repeat(length);
        sources.push(format!("#undef {word}\nvoid() f = #5;\n"));
        sources.push(format!("#ifdef {word}\n#endif\nvoid() f = #5;\n"));
        sources.push(format!("#if defined {word}\n#endif\nvoid() f = #5;\n"));
        sources.push(format!("$frame {word}\nvoid() f = #5;\n"));
        let dir = "d".repeat(length);
        sources.push(format!("#pragma includedir {dir}\nvoid() f = #5;\n"));
    }
    // A byte more, past `msg[1024]` by two (`Overrun`).
    sources.push(format!(
        "#pragma includedir {}\nvoid() f = #5;\n",
        "d".repeat(1025)
    ));
    // An `#includelist` word and a `#merge` name of 1023 bytes name a file no file system holds,
    // and `#merge` fails at any name the build has no progs of (`QCC_ImportProgs`, not modelled),
    // so only their own errors are cases.
    let long = "w".repeat(1024);
    sources.push(format!("#includelist\n{long}\n#endlist\nvoid() f = #5;\n"));
    sources.push(format!("#merge \"{long}\"\nvoid() f = #5;\n"));
    sources.push("void() f = #5;\n#merge \"abc\n".to_owned());
    sources.push("void() f = #5;\n#includelist\n/* open\n".to_owned());
    sources.push("#define M(x) x\nvoid() f = #5;\nfloat y = M( /* open\n".to_owned());
    sources
}

/// `#define` and `#append` of names of each length, used.
fn name_length_cases() -> Vec<String> {
    let mut sources = Vec::new();
    for length in NAME_LENGTHS {
        let name = "n".repeat(length);
        sources.push(format!("#define {name} #5\nvoid() f = {name};\n"));
        sources.push(format!("#append {name} #5\nvoid() f = {name};\n"));
        sources.push(format!("#define {name}(a) a\nvoid() f = {name}(#5);\n"));
        sources.push(format!("#undef {name}\nvoid() f = #5;\n"));
    }
    sources
}

/// Conditionals as `QCC_PR_FalsePreProcessorIf` skips their sides, and `ParsePrecompilerIf`'s
/// expressions (FTEF-14.33, FTEF-14.37).
const CONDITIONALS: &[&str] = &[
    "#ifdef Q\n#iffy\n#endif\nzz\n#endif\nvoid() f = #5;\n",
    "#ifdef Q\n#ifdefQ\n#endif\nzz\n#endif\nvoid() f = #5;\n",
    "#ifdef Q\n// c \\\n#endif\nzz\n#endif\nvoid() f = #5;\n",
    "#ifdef Q\nzz \\\n#endif\nzz\n#endif\nvoid() f = #5;\n",
    "#ifdef Q\n#elif\nzz\n#endif\nvoid() f = #5;\n",
    "#ifdef Q\n#elif 1\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\n#elif 0\nzz\n#elif 1\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\n#elifdef Q\nzz\n#elifndef Q\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\n#elifdef  Q\nzz\n#else\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\n#elifdef\tQ\nzz\n#else\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\n#elifdefQ\nzz\n#endif\nvoid() f = #5;\n",
    "#ifdef Q\n#elif 1 zz\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\n#elif 0 zz\nzz\n#endif\nvoid() f = #5;\n",
    "#ifdef Q\n#else\nvoid() f = #5;\n#else\nzz\n#endif\n",
    "#ifdef Q\n#else zz\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\n#else // c \\\nzz\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\n#elsewhere\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\n  #  endif\nvoid() f = #5;\n",
    "#ifdef Q\n\t#\tendif\nvoid() f = #5;\n",
    "#ifdef Q\n\"#endif\nzz\n#endif\nvoid() f = #5;\n",
    "#ifdef Q\nx /* \n#endif */\nzz\n#endif\nvoid() f = #5;\n",
    "#ifdef Q\n#ifdef R\n#else\nzz\n#endif\nzz\n#else\nvoid() f = #5;\n#endif\n",
    "#ifdef Q\nvoid() f = #5;\n",
    "#ifdef Q\n#else\nvoid() f = #5;\n",
    "#if 1\nvoid() f = #5;\n",
    "#endif\nvoid() f = #5;\n",
    "#else\nvoid() f = #5;\n",
    "#elif 1\nvoid() f = #5;\n",
    "#if 1\nvoid() f = #5;\n#elif garbage(\nzz\n#endif\n",
    "#if 1\nvoid() f = #5;\n#elifdef\nzz\n#endif\n",
    "#if 1\nvoid() f = #5;\n#else\nzz\n#elif 1\nzz\n#endif\n",
    "#if 0 \\\n|| 1\nvoid() f = #5;\n#endif\n",
    "#if 1 \\\n|| 0\nvoid() f = #5;\n#endif\n",
    "#if (1\nvoid() f = #5;\n#endif\n",
    "#if defined(X\nvoid() f = #5;\n#endif\n",
    "#if \nvoid() f = #5;\n#endif\n",
    "#if 1 2\nvoid() f = #5;\n#endif\n",
    "#if 1 // c\nvoid() f = #5;\n#endif\n",
    "#if 1 // c \\\nzz\nvoid() f = #5;\n#endif\n",
    "#if !0\nvoid() f = #5;\n#endif\n",
    "#if defined X\nzz\n#else\nvoid() f = #5;\n#endif\n",
    "#define X 2\n#if X == 2\nvoid() f = #5;\n#endif\n",
    "#ifdef X /* c\n */ void() g = #4;\n#endif\nvoid() f = #5;\n",
    "#if 1 /* c\n */ void() g = #4;\n#endif\nvoid() f = #5;\n",
    "#ifdef FTEQCC\nvoid() f = #5;\n#endif\n",
    "#ifdef __FTEQCC__\nvoid() f = #5;\n#endif\n",
    "#if FTEQCC\nzz zz zz\n#endif\nvoid() f = #5;\n",
    "#undef FTEQCC\n#ifdef FTEQCC\nzz zz zz\n#endif\nvoid() f = #5;\n",
    // FTEF-14.55: each value is `atoi`'s `int`, the low 32 bits of a 64-bit `strtol`, which
    // passes over only the C locale's white space.
    "#if 4294967296\nzz\n#endif\nvoid() f = #5;\n",
    "#if -99999999999999999999\nzz\n#endif\nvoid() f = #5;\n",
    "#define X 4294967296\n#if X\nzz\n#endif\nvoid() f = #5;\n",
    "#if 4294967297 == 1\nvoid() f = #5;\n#endif\n",
    "#if 2147483648 < 0\nvoid() f = #5;\n#endif\n",
    "#if -2147483649 > 0\nvoid() f = #5;\n#endif\n",
    "#if 99999999999999999999 == -1\nvoid() f = #5;\n#endif\n",
    "#if 9223372036854775808 == -1\nvoid() f = #5;\n#endif\n",
    "#if -9223372036854775809 == 0\nvoid() f = #5;\n#endif\n",
    "#if 2147483647 > 0\nvoid() f = #5;\n#endif\n",
    "#if !4294967296\nvoid() f = #5;\n#endif\n",
    "#if 4294967296 || 0\nzz\n#endif\nvoid() f = #5;\n",
    "#if \u{a0}1\nzz\n#endif\nvoid() f = #5;\n",
    "#if \u{3000}1\nzz\n#endif\nvoid() f = #5;\n",
    "#if \u{85}1\nzz\n#endif\nvoid() f = #5;\n",
    "#define X \u{a0}1\n#if X\nzz\n#endif\nvoid() f = #5;\n",
    "#define X \u{3000}1\n#if X\nzz\n#endif\nvoid() f = #5;\n",
    "#define X \u{85}1\n#if X\nzz\n#endif\nvoid() f = #5;\n",
    "#define X \x0c1\n#if X\nvoid() f = #5;\n#endif\n",
    // `pr_file_p++;` after the number passes one byte, the space, not the line break.
    "#pack 4294967296 \nvoid() f = #5;\n",
    "#pack 4294967302 \nvoid() f = #5;\n",
];

/// Programs whose bytes FTEQCC's file loader changes (FTEF-14.64): each NUL byte is a space, and
/// a UTF-8 byte order mark at the start is skipped.
const LOADED: &[&str] = &[
    "void() f = #5;\n\0zz zz zz\n",
    "#define N #5\0zz\nvoid() f = N;\n",
    "#ifdef Q\nzz\0\n#endif\nvoid() f = #5;\n",
    "\0#define N #5\nvoid() f = N;\n",
    "void() f = #5;\0\n",
    "void() f\0= #5;\n",
    "// c\0\nvoid() f = #5;\n",
    "/* c\0 */ void() f = #5;\n",
    "#define N #5 // c\0\nvoid() f = N;\n",
    "\u{feff}#define N #5\nvoid() f = N;\n",
    "\u{feff}  #define N #5\nvoid() f = N;\n",
    "\u{feff}void() f = #5;\n",
    "\u{feff}\u{feff}#define N #5\nvoid() f = N;\n",
    "void() f = #5;\n\u{feff}#define N 1\n",
];

/// `$framevalue` numbers about 0 and past an `int`, each read by `atoi` (FTEF-14.55), before a
/// frame name that a function then uses: one numbered below 0 is unknown (FTEF-18.30).
const FRAME_VALUES: &[&str] = &[
    "-1",
    "-5",
    "0",
    "5",
    "2147483647",
    "-2147483648",
    "2147483648",
    "4294967295",
    "4294967296",
    "-99999999999",
    "99999999999",
];

/// Frame macro uses about 0 (FTEF-18.30): names counted up past a negative start, a
/// `$framerestore` of a negative macro, `$modelname`'s own lookup, and a lookup in any case.
const FRAME_USES: &[&str] = &[
    "$framevalue -2\n$frame a b c\nfloat x = $c;\n",
    "$framevalue -2\n$frame a b c\nfloat x = $b;\n",
    "$framevalue -4\n$frame a\n$framevalue 7\n$framerestore a\n$frame d\n",
    "$frame a b\n$framerestore b\n$frame d\nfloat x = $d;\n",
    "$framevalue 4\n$frame a\n$framevalue -7\n$framerestore a\n$frame d\nfloat x = $d;\n",
    "$framevalue -4\n$frame m\n$framevalue 3\n$modelname m\n$frame d e f g h\nfloat x = $h;\n",
    "$framevalue -4\n$frame m\n$framevalue 3\n$modelname m\n$frame d\nfloat x = $d;\n",
    "$framevalue -1\n$frame A\nfloat x = $a;\n",
    "$modelname m\n$frame a\nfloat x = $a;\n",
];

/// Each [`FRAME_VALUES`] number before `$frame a` and a use of `$a`, and each of
/// [`FRAME_USES`], then `f`'s binding.
fn frame_cases() -> Vec<String> {
    let values = FRAME_VALUES
        .iter()
        .map(|value| format!("$framevalue {value}\n$frame a\nfloat x = $a;\n{BIND}"));
    let uses = FRAME_USES.iter().map(|uses| format!("{uses}{BIND}"));
    values.chain(uses).collect()
}

/// Programs written as UTF-16 and as UTF-32 after their byte order marks, which FTEQCC's file
/// loader decodes (FTEF-14.64).
const WIDE: &[&str] = &[
    "void() f = #5;\n",
    "#define N #5\nvoid() f = N;\n",
    "// \u{e9}\u{20ac}\u{1f600}\nvoid() f = #5;\n",
    "void() f = #5;\n\0zz zz zz\n",
    "#ifdef Q\nzz\0\n#endif\nvoid() f = #5;\n",
];

/// The encodings each program of [`WIDE`] is written in.
const WIDE_ENCODINGS: [Encoding; 4] = [
    Encoding::Utf16Le,
    Encoding::Utf16Be,
    Encoding::Utf32Le,
    Encoding::Utf32Be,
];

/// Each program of [`WIDE`] in each wide encoding.
fn wide_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for source in WIDE {
        for encoding in WIDE_ENCODINGS {
            cases.push(Case {
                encoding,
                ..Case::new(Dialect::Fteqcc, (*source).to_owned())
            });
        }
    }
    cases
}

/// Directives on a line of their own after a use's `(` or `,`, or in the text of an argument
/// (FTEF-14.45), each with the argument that comes after it.
const ARGUMENT_DIRECTIVES: &[(&str, &str)] = &[
    ("#define Y #5", "Y"),
    ("#define Y #5 // c \\\nzz", "Y"),
    ("#undef M", "#5"),
    ("#ifdef Q\nzz\n#endif", "#5"),
    ("#if 0\nzz\n#else\n#define Y #5\n#endif", "Y"),
    ("#ifdef Q\nzz", "#5"),
    ("#pragma line(40)", ":__LINESTR__"),
    ("#error x", "#5"),
    ("#warning x", "#5"),
    ("#eof", "#5"),
    ("#package", "#5"),
    ("#includelist\n#endlist", "#5"),
    ("#pragma DONT_COMPILE_THIS_FILE", "#5"),
    ("#foo", "#5"),
];

/// A use's arguments with a directive after its `(`, after its `,`, after a `//` comment that
/// follows its `(`, and in the middle of an argument, whose text the expansion then holds
/// (FTEF-14.45, FTEF-14.46).
fn argument_cases() -> Vec<String> {
    let mut sources = Vec::new();
    for (directive, arg) in ARGUMENT_DIRECTIVES {
        sources.push(format!(
            "#define M(x) x\nvoid() f = M(\n{directive}\n{arg});\n"
        ));
        sources.push(format!(
            "#define M(x, y) y\nvoid() f = M(1,\n{directive}\n{arg});\n"
        ));
        sources.push(format!(
            "#define M(x) x\nvoid() f = M(// c\n{directive}\n{arg});\n"
        ));
        sources.push(format!(
            "#define M(x) x\nvoid() f = M(  \n  {directive}\n{arg});\n"
        ));
    }
    sources.extend(
        [
            "#define M(x) x\nvoid() f = M(#5\n#define Y 6\n);\n",
            "#define M(x) x\nvoid() f = M(#5\n#error x\n);\n",
            "#define M(x) #x\nvoid() f = :M(\n#define Y 6\nex_a);\n",
            "#define M(x) x\nvoid() f = M(\n#define Y #5\nY) ;\n#ifndef Y\nzz\n#endif\n",
            "#define M(x) x\n#define K M(\\\n#define Y #5\\\nY)\nvoid() f = K;\n",
        ]
        .map(str::to_owned),
    );
    sources
}

/// `#pragma warning` settings of F303, the warning for a directive in a macro's body.
const EVIL_SETTINGS: &[&str] = &[
    "",
    "#pragma warning disable F303\n",
    "#pragma warning off F303\n",
    "#pragma warning ignore f303\n",
    "#pragma warning enable F303\n",
    "#pragma warning on F303\n",
    "#pragma warning error F303\n",
    "#pragma warning toggle F303\n",
    "#pragma warning disable F303\n#pragma warning toggle F303\n",
    "#pragma warning disable F302\n",
    "#pragma warning disabled F303\n",
];

/// Bodies that hold a directive on a line of their own, which runs where the expansion is read
/// (FTEF-14.46).
const EVIL_BODIES: &[(&str, &str)] = &[
    ("#define D \\\n#define Y #5 \\\nY", "D"),
    ("#define D :ex_a \\\n#define Y #5 \\\n", "Y D"),
    ("#define D \\\n#undef D \\\n#5", "D"),
    ("#define D \\\n#ifdef Q \\\nzz \\\n#endif \\\n#5", "D"),
    ("#define D \\\n#pragma line(30) \\\n:__LINESTR__", "D"),
    ("#define D \\\n#error x \\\n#5", "D"),
    ("#define D \\\n#eof \\\nzz", "D #5"),
    ("#define D \\\n  #define Y #5 \\\nY", "D"),
    ("#define D(a) a \\\n#define Y #5 \\\n", "Y D(1)"),
    ("#define D \\\n\n#define Y #5", "D Y"),
];

/// Each body under each setting of F303.
fn evil_cases() -> Vec<String> {
    let mut sources = Vec::new();
    for setting in EVIL_SETTINGS {
        for (define, use_) in EVIL_BODIES {
            sources.push(format!("{setting}{define}\nvoid() f = {use_};\n"));
        }
    }
    sources
}

/// Where a `#` is met: at a line's start after spaces and tabs, after a `//` comment's line,
/// after a `/* */` comment on its line or over lines, and in the middle of a line
/// (FTEF-14.52).
const PLACES: &[&str] = &[
    "", "  ", "\t", " \t ", "// c\n", "/* c */", "/* c\n*/", "x; ",
];

/// Directives each place reads, and what they leave for `f`.
const PLACED: &[(&str, &str)] = &[
    ("#define N #5", "void() f = N;\n"),
    ("#undef N", BIND),
    ("#ifdef Q\nzz\n#endif", BIND),
];

/// Each placed directive at each place.
fn place_cases() -> Vec<String> {
    let mut sources = Vec::new();
    for place in PLACES {
        for (directive, after) in PLACED {
            sources.push(format!("{place}{directive}\n{after}"));
        }
    }
    sources
}

/// Every directive and pragma form `QCC_PR_Precompiler` reads, each on lines of its own that
/// leave `f`'s binding as it is when they run as FTEQCC runs them, and whether a line of it is
/// read as code, which in an `#includelist` names a file the graph opens (PRJ-5.3).
const SNIPPETS: &[(&str, bool)] = &[
    ("#define Y #6", false),
    ("#define Y(a) a", false),
    ("#append Y #6", false),
    ("#undef Y", false),
    ("#ifdef Q\nzz zz zz\n#endif", false),
    ("#ifndef Q\n#endif", false),
    ("#if 0\nzz zz zz\n#elif 1\n#else\nzz zz zz\n#endif", false),
    ("#if 1\n#endif", false),
    ("#ifdef Q\n#elifdef Q\nzz zz zz\n#elifndef Q\n#endif", false),
    ("#endif", false),
    ("#else", false),
    ("#eof", false),
    ("#error x", false),
    ("#warning x", false),
    ("#message x", false),
    ("#copyright x", false),
    ("#package", false),
    ("#pack 1 ", false),
    ("#forcecrc 5", false),
    ("#output \"progs.dat\"", false),
    ("#merge \"x.dat\"", false),
    ("#datafile \"x.dat\"", false),
    ("#includelist\n#endlist", false),
    ("#include \"\"", false),
    ("#pragma once", false),
    ("#pragma line(40)", false),
    ("#pragma noref 0", false),
    ("#pragma warning disable F302", false),
    ("#pragma TARGET ID", false),
    ("#pragma unknown", false),
    ("#pragma DONT_COMPILE_THIS_FILE", false),
    ("#defined Z #6", false),
    ("#undefine Z", false),
    ("#iffy", false),
    ("#foo", true),
    ("# ", true),
    ("#endlist", true),
];

/// `snippet`'s lines each continued with `\` as a macro's body, which keeps a line break before
/// each that starts with `#` and after it (FTEF-14.24).
fn continued(snippet: &str) -> String {
    snippet.replace('\n', " \\\n")
}

/// Every snippet where FTEQCC meets a `#`: at a line's start, after spaces and tabs, after a
/// `//` comment's line, after a `/* */` comment and in the middle of a line (FTEF-14.52); in a
/// skipped side (FTEF-14.37); after a use's `(` or `,` and in the middle of an argument
/// (FTEF-14.45); in a macro's body (FTEF-14.46); in an `#includelist` (FTEF-14.41); after
/// `#pragma DONT_COMPILE_THIS_FILE` (FTEF-14.70).
fn snippet_cases() -> Vec<String> {
    let mut sources = Vec::new();
    for &(snippet, words) in SNIPPETS {
        for prefix in ["", "  ", "\t", "// c\n", "/* c */", "void() g = {}; "] {
            sources.push(format!("{prefix}{snippet}\n{BIND}"));
        }
        sources.push(format!("#ifdef Q\n{snippet}\n#endif\n{BIND}"));
        sources.push(format!("#define M(x) x\nvoid() f = M(\n{snippet}\n#5);\n"));
        sources.push(format!(
            "#define M(x, y) y\nvoid() f = M(1,\n{snippet}\n#5);\n"
        ));
        sources.push(format!("#define M(x) x\nvoid() f = M(#5\n{snippet}\n);\n"));
        sources.push(format!(
            "#pragma warning disable F303\n#define D \\\n{} \\\n#5\nvoid() f = D;\n",
            continued(snippet)
        ));
        sources.push(format!("{BIND}#pragma DONT_COMPILE_THIS_FILE\n{snippet}\n"));
        if !words {
            sources.push(format!("#includelist\n{snippet}\n#endlist\n{BIND}"));
        }
    }
    sources
}

/// `#pragma line` (FTEF-14.43) in each form, with the line it gives written by `__LINESTR__`
/// and `__LINE__`.
const LINE_PRAGMAS: &[&str] = &[
    "#pragma line(10)",
    "#pragma line 10",
    "#pragma LINE(10)",
    "#pragma line( 10 )",
    "#pragma line 0x10",
    "#pragma line 010",
    "#pragma line 08",
    "#pragma line -1",
    "#pragma line 0",
    "#pragma line",
    "#pragma line x",
    "#pragma line 4294967297",
    "#pragma line 2147483647",
    "#pragma line 99999999999999999999",
    "#pragma line 7 // c",
    "#pragma line(7)zz",
    "#pragma linex 7",
];

/// Each `#pragma line`, then a line between, then the line written.
fn line_pragma_cases() -> Vec<String> {
    let mut sources = Vec::new();
    for pragma in LINE_PRAGMAS {
        sources.push(format!("{pragma}\nvoid() f = :__LINESTR__;\n"));
        sources.push(format!("{pragma}\n\n\nvoid() f = :__LINESTR__;\n"));
        // A builtin number past 2^24 is no float's, and C leaves its conversion to `int`
        // undefined past 2^31 (QC-13.10).
        let digits = pragma.bytes().filter(u8::is_ascii_digit).count();
        if digits <= 7 {
            sources.push(format!(
                "#define E(n) n\n{pragma}\nvoid() f = E(# __LINE__);\n"
            ));
        }
    }
    sources.extend(
        [
            "#define L :__LINESTR__\n#pragma line(20)\nvoid() f = L;\n",
            "#define M(x) :__LINESTR__ x\n#pragma line(20)\nvoid() f = M(\n);\n",
            "#pragma line(20)\n#pragma line(30)\nvoid() f = :__LINESTR__;\n",
            "#ifdef Q\n#pragma line(20)\n#endif\nvoid() f = :__LINESTR__;\n",
            "#define X \\\n1\n#pragma line(20)\nvoid() f = :__LINESTR__;\n",
        ]
        .map(str::to_owned),
    );
    sources
}

/// The other pragmas FTEQCC knows, each of which this profile accepts and reads to its line's
/// break (FTEF-14.70).
const PRAGMAS: &[&str] = &[
    "#pragma DONT_COMPILE_THIS_FILE\nzz zz zz\n",
    "#pragma dont_compile_this_file\nzz zz zz\n",
    "#pragma DONT_COMPILE_THIS_FILE\nzz zz zz\n#error x\n",
    "#pragma DONT_COMPILE_THIS_FILE\n  #error x\n",
    "#pragma DONT_COMPILE_THIS_FILE\n\"#error x\n",
    "#pragma DONT_COMPILE_THIS_FILE\nzz /* \n#error x */\n",
    "#pragma DONT_COMPILE_THIS_FILE zz\nzz\n",
    "#pragma COPYRIGHT \"x\"\n",
    "#pragma compress 0\n",
    "#pragma forcecrc 5\n",
    "#pragma framerate 10\n",
    "#pragma once\n",
    "#pragma noref(1)\n",
    "#pragma nosave 1\n",
    "#pragma defaultstatic 0\n",
    "#pragma autoproto 0\n",
    "#pragma wrasm 0\n",
    "#pragma optimise 0\n",
    "#pragma optimize addon\n",
    "#pragma optimise no-xyz\n",
    "#pragma TARGET ID\n",
    "#pragma TARGET xyz\n",
    "#pragma PROGS_SRC x\n",
    "#pragma PROGS_DAT progs.dat\n",
    "#pragma opcode enable xyz\n",
    "#pragma opcode xyz\n",
    "#pragma flag enable xyz\n",
    "#pragma keyword disable xyz\n",
    "#pragma flag xyz\n",
    "#pragma warning disable xyz\n",
    "#pragma warning xyz F303\n",
    "#pragma includedir x\n",
    "#pragma unknown\n",
    "#pragma once // c \\\nzz zz zz\n",
    "#pragma once /* c\n */\n",
    "#pragma once \"s\n",
];

/// Each pragma before `f`'s binding.
fn pragma_cases() -> Vec<String> {
    PRAGMAS
        .iter()
        .map(|pragma| format!("{pragma}{BIND}"))
        .collect()
}

/// `#includelist` as `QCC_PR_Precompiler` reads it (FTEF-14.41), with no file listed.
const INCLUDE_LISTS: &[&str] = &[
    "#includelist\n#endlist\nvoid() f = #5;\n",
    "#includelist\n#define N #5\n#endlist\nvoid() f = N;\n",
    "#includelist\n  #endlist\nvoid() f = #5;\n",
    "#includelist\n// c\n#endlist\nvoid() f = #5;\n",
    "#includelist\n/* c */ #endlist\nvoid() f = #5;\n",
    "#includelist\n#ifdef Q\nzz\n#endif\n#endlist\nvoid() f = #5;\n",
    "#includelist\n#endlist zz\nvoid() f = #5;\n",
    "#includelist\n#endlist // c\nvoid() f = #5;\n",
    "#includelist // c\n#endlist\nvoid() f = #5;\n",
    "#includelist\nvoid() f = #5;\n",
    "#includelist\n",
    "void() f = #5;\n#includelist\n",
];

/// Lines of code a `\` joins to the next, which holds a directive's text, read as code with no
/// `QCC_PR_NewLine` (FTEF-14.53): refused where the text's name is no macro, taken as code where
/// it is one, and a directive after a `//` comment's `\` or a line ending after the join.
const JOINED: &[&str] = &[
    "float g; \\\n#define N 1\nvoid() f = #5;\n",
    "float g; \\\n#undef N\nvoid() f = #5;\n",
    "float g; \\\r\n#define N 1\nvoid() f = #5;\n",
    "float g; \\\r#undef N\nvoid() f = #5;\n",
    "float g; \\\n  #undef N\nvoid() f = #5;\n",
    "#define undef 6;\n#define N\nfloat g = \\\n#undef N\n#ifdef N\nvoid() f = #5;\n#endif\n",
    "#define undef 6;\n#define N\nfloat g = \\\r\n#undef N\n#ifdef N\nvoid() f = #5;\n#endif\n",
    "#define define 6; float\nfloat g = \\\n#define N;\n#ifndef N\nvoid() f = #5;\n#endif\n",
    "#define include 6; string s =\nfloat g = \\\n#include \"b.qh\";\nvoid() f = #5;\n",
    "#define M(x) x\nvoid() f = M(#5 \\\n#define Y 6\n);\n",
    "float g; // c \\\n#define N #5\nvoid() f = N;\n",
    "float g; \\\n\n#define N #5\nvoid() f = N;\n",
];

/// The file a nested `#include` reaches and one it would reach beside the including file.
const NESTED: &str = "#include \"x/b.qh\"\n";

/// Files that bind `f` to another builtin, which a case binds only by reading the wrong file.
const WRONG: &str = "void() f = #6;\n";

/// Projects whose files include each other, beside the file being compiled and in include
/// directories (FTEF-14.40, P2-96): the name `progs.src` lists, its text and the other files.
type IncludeProject = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
);

/// The include projects.
const INCLUDE_PROJECTS: &[IncludeProject] = &[
    (
        "src/a.qc",
        NESTED,
        &[
            ("src/x/b.qh", "#include \"c.qh\"\n"),
            ("src/c.qh", BIND),
            ("src/x/c.qh", WRONG),
        ],
    ),
    (
        "src/a.qc",
        NESTED,
        &[("src/x/b.qh", "#include \"c.qh\"\n"), ("src/x/c.qh", WRONG)],
    ),
    (
        "src/a.qc",
        NESTED,
        &[
            ("src/x/b.qh", "#include \"y/c.qh\"\n"),
            ("src/y/c.qh", BIND),
        ],
    ),
    (
        "src/a.qc",
        "#pragma includedir x\n#include \"c.qh\"\n",
        &[("src/x/c.qh", BIND)],
    ),
    (
        "src/a.qc",
        "#pragma includedir x/\n#include \"c.qh\"\n",
        &[("src/x/c.qh", BIND)],
    ),
    (
        "src/a.qc",
        "#pragma includedir x\n#include \"c.qh\"\n",
        &[("src/x/c.qh", WRONG), ("src/c.qh", BIND)],
    ),
    (
        "src/a.qc",
        "#pragma includedir \"x\"\n#include \"c.qh\"\n",
        &[("src/x/c.qh", BIND)],
    ),
    (
        "src/a.qc",
        NESTED,
        &[
            ("src/x/b.qh", "#pragma includedir y\n"),
            ("src/x/c.qh", WRONG),
        ],
    ),
    (
        "src/a.qc",
        "#include \"x/b.qh\"\n#include \"c.qh\"\n",
        &[
            ("src/x/b.qh", "#pragma includedir y\n"),
            ("src/y/c.qh", BIND),
        ],
    ),
    ("t.qc", "#include <b.qh>\n", &[("b.qh", BIND)]),
    ("t.qc", "#include \"b.qh\" zz\n", &[("b.qh", BIND)]),
    ("t.qc", "#include \"b.qh\" // c\n", &[("b.qh", BIND)]),
    (
        "t.qc",
        "#include \"b.qh\"\n",
        &[("b.qh", "#define N #5\nvoid() f = N;\n")],
    ),
    (
        "t.qc",
        "#include \"b.qh\"\n",
        &[("b.qh", "  #define N #5\nvoid() f = N;\n")],
    ),
    (
        "t.qc",
        "#include \"b.qh\"\n",
        &[("b.qh", "\n#define N #5\nvoid() f = N;\n")],
    ),
    ("t.qc", "#include \"b.qh\"\n", &[("b.qh", "#if 1\n")]),
    (
        "t.qc",
        "#if 1\n#include \"b.qh\"\nvoid() f = #5;\n",
        &[("b.qh", "#endif\n")],
    ),
    (
        "t.qc",
        "#ifdef X\n#include \"b.qh\"\nvoid() f = #5;\n",
        &[("b.qh", "#endif\n")],
    ),
    (
        "t.qc",
        "#include \"b.qh\"\nvoid() f = :__LINESTR__;\n",
        &[("b.qh", "#pragma line(20)\n")],
    ),
    (
        "t.qc",
        "#include \"b.qh\"\n",
        &[("b.qh", "#pragma line(20)\nvoid() f = :__LINESTR__;\n")],
    ),
    (
        "t.qc",
        "#pragma once\n#include \"t.qc\"\nvoid() f = #5;\n",
        &[],
    ),
    (
        "t.qc",
        "#include \"b.qh\"\n#include \"b.qh\"\n",
        &[(
            "b.qh",
            "#pragma once\n#ifdef SEEN\nzz\n#endif\n#define SEEN\nvoid() f = #5;\n",
        )],
    ),
    (
        "t.qc",
        "#include \"b.qh\"\nvoid() f = #5;\n",
        &[("b.qh", "#eof\nzz\n")],
    ),
    (
        "t.qc",
        "#include \"b.qh\"\nvoid() f = #5;\n",
        &[("b.qh", "#package\nzz\n")],
    ),
    (
        "t.qc",
        "#define M(x) x\nvoid() f = M(\n#include \"b.qh\"\n);\n",
        &[("b.qh", "#5\n")],
    ),
    (
        "t.qc",
        "#pragma warning disable F303\n#define D \\\n#include \"b.qh\" \\\n\nvoid() f = D;\n",
        &[("b.qh", "#5\n")],
    ),
    // FTEF-14.64: an included file's text as FTEQCC's file loader gives it, whose first byte
    // `QCC_PR_Precompiler` reads.
    (
        "t.qc",
        "#include \"b.qh\"\nvoid() f = N;\n",
        &[("b.qh", "\u{feff}#define N #5\n")],
    ),
    (
        "t.qc",
        "#include \"b.qh\"\nvoid() f = N;\n",
        &[("b.qh", "\0#define N #5\n")],
    ),
    (
        "t.qc",
        "#include \"b.qh\"\nzz\n",
        &[("b.qh", "void() f = #5;\n\0#ifdef Q\n")],
    ),
];

/// A file that binds `f` to the name `__FILE__` writes for it (FTEF-14.74).
const NAMED: &str = "void() f = :__FILE__;\n";

/// Projects whose files bind `f` to their names, which the profile reads from the graph's
/// names in a directory of its own (FTEF-14.75, PRJ-6.7, PRJ-6.8): a listed file in a
/// subdirectory, a file it includes beside it and in another directory, one a `#pragma
/// includedir` finds, a name with `./` in it, and names with `\` in them, which the name is
/// joined after and which are no separator where the host's is `/`.
const NAMED_PROJECTS: &[IncludeProject] = &[
    ("src/a.qc", NAMED, &[]),
    ("src/a.qc", NESTED, &[("src/x/b.qh", NAMED)]),
    (
        "src/a.qc",
        NESTED,
        &[("src/x/b.qh", "#include \"c.qh\"\n"), ("src/c.qh", NAMED)],
    ),
    (
        "src/a.qc",
        "#pragma includedir y\n#include \"c.qh\"\n",
        &[("src/y/c.qh", NAMED)],
    ),
    ("src/./a.qc", "#include \"b.qh\"\n", &[("src/b.qh", NAMED)]),
    ("sub\\top.qc", NAMED, &[]),
    (
        "sub\\top.qc",
        "#include \"tab.qh\"\n",
        &[("sub\\tab.qh", NAMED)],
    ),
];

/// The named projects with include directories, FTEQCC's `-I`, which name a file found in them
/// by the directory as given and a `/` (FTEF-14.70, PRJ-6.7).
const NAMED_DIR_PROJECTS: &[DirProject] = &[
    (
        "t.qc",
        "#include \"c.qh\"\n",
        &[("inc/c.qh", NAMED)],
        &["inc"],
    ),
    (
        "t.qc",
        "#include \"c.qh\"\n",
        &[("inc/c.qh", NAMED)],
        &["inc/"],
    ),
];

/// The case of `source` compiled as `compiled` with `files` beside it.
fn project_case(compiled: &str, source: &str, files: &[(&str, String)]) -> Case {
    let mut files: Vec<(String, String)> = files
        .iter()
        .map(|(name, text)| ((*name).to_owned(), text.clone()))
        .collect();
    files.sort();
    Case {
        dialect: Dialect::Fteqcc,
        source: source.to_owned(),
        encoding: Encoding::Utf8,
        project: Some(Project {
            compiled: compiled.to_owned(),
            files,
            include_dirs: Vec::new(),
            progs: None,
        }),
        options: Vec::new(),
    }
}

/// Include projects compiled with include directories, FTEQCC's `-I` (FTEF-14.49): the name
/// `progs.src` lists, its text, the other files and the directories.
type DirProject = (
    &'static str,
    &'static str,
    &'static [(&'static str, &'static str)],
    &'static [&'static str],
);

/// The include projects with include directories: a nested `#include` found only in one,
/// as Rad-Therapy's `shared/defs.h` is found with `-I../shared`; one before a `#pragma
/// includedir` one; one with the second autoprototyping pass.
const DIR_PROJECTS: &[DirProject] = &[
    (
        "src/a.qc",
        NESTED,
        &[("src/x/b.qh", "#include \"c.qh\"\n"), ("src/x/c.qh", BIND)],
        &["src/x"],
    ),
    (
        "src/a.qc",
        NESTED,
        &[("src/x/b.qh", "#include \"c.qh\"\n"), ("src/x/c.qh", BIND)],
        &["src/x/"],
    ),
    (
        "src/a.qc",
        "#pragma includedir y\n#include \"c.qh\"\n",
        &[("inc/c.qh", BIND), ("src/y/c.qh", WRONG)],
        &["inc"],
    ),
    (
        "src/a.qc",
        "#pragma includedir y\n#include \"c.qh\"\n",
        &[("inc/c.qh", WRONG), ("src/y/c.qh", BIND)],
        &["nowhere"],
    ),
    (
        "t.qc",
        "#pragma autoproto\n#include \"c.qh\"\n",
        &[("inc/c.qh", BIND)],
        &["inc"],
    ),
];

/// The include projects, and each snippet on the first line of an included file, where
/// `QCC_PR_Precompiler` reads it with no spaces passed over (FTEF-14.52).
fn include_cases() -> Vec<Case> {
    let mut cases: Vec<Case> = INCLUDE_PROJECTS
        .iter()
        .chain(NAMED_PROJECTS)
        .map(|&(compiled, source, files)| {
            let files: Vec<(&str, String)> = files
                .iter()
                .map(|&(name, text)| (name, text.to_owned()))
                .collect();
            project_case(compiled, source, &files)
        })
        .collect();
    for &(compiled, source, files, dirs) in DIR_PROJECTS.iter().chain(NAMED_DIR_PROJECTS) {
        let files: Vec<(&str, String)> = files
            .iter()
            .map(|&(name, text)| (name, text.to_owned()))
            .collect();
        let mut case = project_case(compiled, source, &files);
        if let Some(project) = &mut case.project {
            project.include_dirs = dirs.iter().map(|&dir| dir.to_owned()).collect();
        }
        cases.push(case);
    }
    // FTEF-14.53: an `#include` a `\` joins to a line of code is code, which FTEQCC refuses.
    let joined = "float g; \\\n#include \"b.qh\"\n";
    cases.push(project_case("t.qc", joined, &[("b.qh", BIND.to_owned())]));
    let includer = format!("#include \"b.qh\"\n{BIND}");
    for (snippet, _) in SNIPPETS {
        for indent in ["", "  "] {
            let header = format!("{indent}{snippet}\n");
            cases.push(project_case("t.qc", &includer, &[("b.qh", header)]));
        }
    }
    cases
}

/// Where a graph form puts the line that brings the file in that binds `f`.
const HERE: &str = "@@";

/// Directive forms of FTEF-14.33 to FTEF-14.70 before, around or after the line that brings in
/// the one file that binds `f`, so that FTEQCC binds `f` only by reading that file: the project
/// graph lists the file the pinned FTEQCC compiles when it lists that file exactly when FTEQCC
/// binds `f` (PRJ-5.13). A form that FTEQCC refuses reads no file the graph lists.
const GRAPH_FORMS: &[&str] = &[
    "@@",
    // FTEF-14.52: names matched by their first bytes, and where a `#` is a directive's.
    "#defined N 1\n@@",
    "#appendix N 1\n@@",
    "#iffy\n@@",
    "#undefN\n@@",
    "  #define N 1\n@@",
    "\t# define N 1\n@@",
    "/* c\n#define N 1 */\n@@",
    "/* c\n@@\n*/",
    "// c\n@@",
    // FTEF-14.35: where a directive's line ends.
    "#define N 1 // c\n@@",
    "#undef N /* c\n */\n@@",
    "#ifdef N // c \\\n@@\n#endif",
    "#ifndef N // c\n@@\n#endif",
    "#pragma once // c \\\n@@",
    // FTEF-14.33 and FTEF-14.57: conditionals.
    "#ifdef Q\n@@\n#endif",
    "#ifndef Q\n@@\n#endif",
    "#if 0 && 0 || 1\n@@\n#endif",
    "#if 1 < 2 == 1\n@@\n#endif",
    "#if 1||0\n@@\n#endif",
    "#if defined(Q) || !defined Q\n@@\n#endif",
    "#define V 3\n#if V == 3\n@@\n#endif",
    "#ifdefQ\n@@",
    "#ifdef\n@@\n#endif",
    "#ifndef\n@@\n#endif",
    "#ifdef FTEQCC\n@@\n#endif",
    "#ifdef __FTEQCC__\n@@\n#endif",
    "#if FTEQCC\n@@\n#endif",
    "#undef FTEQCC\n#ifdef FTEQCC\n@@\n#endif",
    // FTEF-14.37: skipped sides, and one count of open sides.
    "#if 0\n#else\n@@\n#endif",
    "#if 1\n#else\n@@\n#endif",
    "#if 1\n#elif 1\n@@\n#endif",
    "#if 0\n#elif 1\n@@\n#endif",
    "#if 0\n#elif 0\n#else\n@@\n#endif",
    "#ifdef Q\n#elifdef Q\n#elifndef Q\n@@\n#endif",
    "#ifdef Q\n#iffy\n#endif\n#else\n@@\n#endif",
    "#ifdef Q\n// c \\\n#endif\n#endif\n@@",
    "#ifdef Q\n\"#endif\n#endif\n@@",
    "#ifdef Q\nx /* \n#endif */\n#endif\n@@",
    "#ifdef Q\n  #  endif\n@@",
    "#ifdef Q\n#else\n#else\n@@\n#endif",
    "#if 1\n@@",
    // FTEF-14.38 and FTEF-14.39: the build directives, and those that end the text.
    "#warning x\n@@",
    "#message x\n@@",
    "#copyright x\n@@",
    "#pack 1 \n@@",
    "#forcecrc 5\n@@",
    "#output \"progs.dat\"\n@@",
    "#append N 1\n@@",
    "#eof\n@@",
    "#eofx\n@@",
    "#package\n@@",
    // FTEF-14.40 to FTEF-14.70: includes, lists and pragmas.
    "#include \"\"\n@@",
    "#includelist\n#endlist\n@@",
    "#pragma line(10)\n@@",
    "#pragma once\n@@",
    "#pragma warning disable F302\n@@",
    "#pragma unknown\n@@",
    "#pragma DONT_COMPILE_THIS_FILE\n@@",
    "#pragmas once\n@@",
    // FTEF-14.64: the text FTEQCC's file loader gives.
    "\u{feff}@@",
    "\u{feff}  @@",
    "// c\0\n@@",
    "/* c\0 */\n@@",
    "\0@@",
    "#ifdef Q\nzz\0\n#endif\n@@",
    "#if 4294967296\n#else\n@@\n#endif",
    "#if \u{a0}1\n#else\n@@\n#endif",
    // FTEF-14.53: a `\` that joins a line of code to the next leaves its `#` code.
    "#define undef 6;\n#define N\nfloat g = \\\n#undef N\n#ifdef N\n@@\n#endif",
    "#define undef 6;\n#define N\nfloat g = \\\r\n#undef N\n#ifdef N\n@@\n#endif",
    "#define define 6; float\nfloat g = \\\n#define N;\n#ifndef N\n@@\n#endif",
    "float g; // c \\\n@@",
    // PRJ-5.13: code passed over a token at a time.
    "string s = \"// \\\" #\";\n@@",
    "string s = R\"x(\n@@\n)x\";",
    "float c = '\"';\n@@",
    "float c = '\\'';\n@@",
    "vector v = '1 2 3';\n@@",
];

/// Forms whose directive's line `QCC_PR_SkipToEndOfLine` carries on to the next one, which holds
/// the `#include` (FTEF-14.35): an `#includelist` there would leave its words to be read as code.
const CARRIED_FORMS: &[&str] = &[
    "#define N 1 // c \\\n@@",
    "#undef N // c \\\n@@",
    "#ifndef N // c \\\n@@\n#endif",
    "#if 1 // c \\\n@@\n#endif",
    "#output \"progs.dat\" // c \\\n@@",
    "#define N 1 // c \\\r\n@@",
];

/// The lines of a graph form that bring in the file that binds `f`: an `#include`, and an
/// `#includelist` entry; and that file's name.
const GRAPH_LINES: [(&str, &str); 2] = [
    ("#include \"b.qh\"", "b.qh"),
    ("#includelist\nl.qc\n#endlist", "l.qc"),
];

/// Lists whose lines hold directives and words of each form (FTEF-14.41), each with the files
/// it may name; `l.qc` binds `f` and the others do not exist.
const GRAPH_LISTS: &[&str] = &[
    "#includelist\n#ifdef Q\nx.qc\n#endif\nl.qc\n#endlist\n",
    "#includelist\n#ifndef Q\nl.qc\n#endif\n#endlist\n",
    "#includelist\n// c\nl.qc // x.qc\n#endlist\n",
    "#includelist\n/* x.qc */ l.qc\n#endlist\n",
    "#includelist\n#define N 1\nl.qc\n#endlist\n",
    "#includelist\n  l.qc  #endlist\n",
    "#includelist // c \\\nx.qc\nl.qc\n#endlist\n",
    "#pragma DONT_COMPILE_THIS_FILE\n#includelist\nl.qc\n#endlist\n",
    "#pragma DONT_COMPILE_THIS_FILE\n#include \"b.qh\"\n",
];

/// Projects of several files for the graph: a side opened in one file and closed in the next
/// (FTEF-14.37), and a side an included file opens and closes.
const GRAPH_PROJECTS: &[(&str, &[(&str, &str)])] = &[
    (
        "#include \"o.qh\"\n#else\n#include \"b.qh\"\n#endif\n",
        &[("o.qh", "#if 1\n")],
    ),
    (
        "#include \"o.qh\"\n#include \"b.qh\"\n#endif\n",
        &[("o.qh", "#if 1\n")],
    ),
    (
        "#include \"o.qh\"\n",
        &[("o.qh", "#ifdef Q\n#include \"b.qh\"\n#endif\n")],
    ),
];

/// Programs whose `#include` a `\` joins to a line of code before it, which so brings in no
/// file (FTEF-14.53, PRJ-5.13).
const JOINED_INCLUDES: &[&str] =
    &["#define include 6; string s =\nfloat g = \\\n#include \"b.qh\";\n"];

/// The project graph's cases (PRJ-5.13): each graph form with each line, each carried form with
/// the `#include`, each joined `#include`, each list, and each project, the file that binds `f`
/// beside them.
pub(super) fn graph_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for form in GRAPH_FORMS {
        for (line, name) in GRAPH_LINES {
            let source = format!("{}\n", form.replace(HERE, line));
            cases.push(project_case("t.qc", &source, &[(name, BIND.to_owned())]));
        }
    }
    for form in CARRIED_FORMS {
        let (line, name) = GRAPH_LINES[0];
        let source = format!("{}\n", form.replace(HERE, line));
        cases.push(project_case("t.qc", &source, &[(name, BIND.to_owned())]));
    }
    for source in JOINED_INCLUDES {
        cases.push(project_case("t.qc", source, &[("b.qh", BIND.to_owned())]));
    }
    for list in GRAPH_LISTS {
        let files = [("b.qh", BIND.to_owned()), ("l.qc", BIND.to_owned())];
        cases.push(project_case("t.qc", list, &files));
    }
    for &(source, others) in GRAPH_PROJECTS {
        let mut files = vec![("b.qh", BIND.to_owned())];
        files.extend(others.iter().map(|&(name, text)| (name, text.to_owned())));
        cases.push(project_case("t.qc", source, &files));
    }
    // FTEF-14.64: a file of UTF-16 or UTF-32 is read decoded.
    for encoding in WIDE_ENCODINGS {
        for (line, name) in GRAPH_LINES {
            let source = format!("// c\n{line}\n");
            cases.push(Case {
                encoding,
                ..project_case("t.qc", &source, &[(name, BIND.to_owned())])
            });
        }
    }
    cases.extend(progs_cases());
    cases
}

/// `progs.src` files of either form that FTEQCC's file loader changes (PRJ-4.6, FTEF-14.64),
/// written as UTF-8: each NUL byte is a space, and a UTF-8 byte order mark at the start is
/// skipped, once. Each names `b.qh`, which binds `f`, and `x.qc`, which binds nothing, so that
/// the words `QCC_COM_Parse` reads decide whether `f` is bound. None is the list of
/// `progs.dat` and one file, which a case without a `progs.src` of its own writes.
const LOADED_PROGS: &[&str] = &[
    "progs.dat\nx.qc\0b.qh\n",
    "progs.dat\0b.qh\n",
    "progs.dat\nb.qh\0\n",
    "progs.dat\n\0b.qh\nx.qc\n",
    "\0#include \"b.qh\"\n",
    "\0\0 #include \"x.qc\"\n#include \"b.qh\"\n",
    "progs.dat\nx.qc\0\0b.qh\n",
    "\u{feff}#include \"b.qh\"\n",
    "\u{feff}progs.dat\nb.qh\nx.qc\n",
];

/// `progs.src` files whose first line starts with `#!`, which FTEQCC's `QCC_main` passes over
/// (PRJ-4.10), written as UTF-8: before a list and before a source form, holding a `\`, a `/*`
/// or a `"`, after a UTF-8 byte order mark and with CRLF line ends. All but the one whose line
/// names `b.qh` bind `f`.
pub(super) const SCRIPT_PROGS: &[&str] = &[
    "#!/usr/bin/fteqcc\nprogs.dat\nb.qh\n",
    "#!/usr/bin/fteqcc\n#include \"b.qh\"\n",
    "#!\nprogs.dat\nb.qh\n",
    "#!x \\\n#include \"b.qh\"\n",
    "#!x \\\nprogs.dat\nb.qh\n",
    "#!x /*\n#include \"b.qh\"\n// */\n",
    "#!x /*\nprogs.dat\nb.qh\n",
    "#!x \"\n#include \"b.qh\"\n",
    "\u{feff}#!x\nprogs.dat\nb.qh\n",
    "\u{feff}#!x\n#include \"b.qh\"\n",
    "#!x\r\nprogs.dat\r\nb.qh\r\n",
    "#!b.qh\nprogs.dat\nx.qc\n",
];

/// `progs.src` files of either form written in each wide encoding, which FTEQCC's file loader
/// decodes (PRJ-4.6, FTEF-14.64), with a `#!` first line too (PRJ-4.10).
const WIDE_PROGS: &[&str] = &[
    "#include \"b.qh\"\n",
    "progs.dat\nb.qh\n",
    "progs.dat\nx.qc\0b.qh\n",
    "// c\n#include \"b.qh\"\n",
    "#!x\nprogs.dat\nb.qh\n",
];

/// Vanilla `progs.src` lists with a NUL byte, where id's qcc ends the list (QC-23.7, PRJ-4.11):
/// between words, in and after a word, in a `//` comment, at a line's start, and in a quoted
/// word, which reads on past it. Those that list `b.qh` before the NUL, or after it in the
/// quoted word's case, bind `f`.
pub(super) const VANILLA_PROGS: &[&str] = &[
    "progs.dat\0b.qh\n",
    "progs.dat\nx.qc\0b.qh\n",
    "progs.dat\nb.qh\0x.qc\n",
    "progs.dat\nx.qc\n\0b.qh\n",
    "progs.dat\nx.qc \0 b.qh\n",
    "progs.dat\nb.qh\n\0x.qc\n",
    "progs.dat\n// c\0\nb.qh\n",
    "progs.dat\n// \"\0\nb.qh\n",
    "progs.dat\n\"x.qc\0junk\" b.qh\n",
    "progs.dat\nx.qc\u{e9}b.qh\n",
    "progs.dat\n\u{e9}b.qh\u{e9}\n",
    "progs.dat\n\"b.qh\nx.qc\" b.qh\n",
    "progs.dat\nb.qh \"x.qc\n",
];

/// FTEQCC list-form `progs.src` files, read as `QCC_main` and `QCC_ContinueCompile` read them
/// (PRJ-4.9): `,` a word of its own and `;` none, a word that a `"` ends, comments, directives at
/// the lines' starts, a list with no word, and a comment the list ends in.
pub(super) const FTE_LIST_PROGS: &[&str] = &[
    "progs.dat,b.qh\n",
    ",b.qh\n",
    "progs.dat\nb.qh,x.qc\n",
    "progs.dat;x\nb.qh\n",
    "progs.dat\n\"x.qc\" /* c */ b.qh\n",
    "progs.dat\nx.qc\"b.qh\"\n",
    "progs.dat\n#define X\n#ifdef X\nb.qh\n#else\nx.qc\n#endif\n",
    "progs.dat\n#ifdef X\nb.qh\n#else\nx.qc\n#endif\n",
    "",
    "// c\n",
    "progs.dat\nb.qh /* open\n",
    "progs.dat\nb.qh(x.qc)\n",
];

/// The graph cases of a `progs.src` of their own (PRJ-4.6, PRJ-4.11, PRJ-4.10): [`LOADED_PROGS`]
/// and [`SCRIPT_PROGS`] as UTF-8 and [`WIDE_PROGS`] in each wide encoding under the FTEQCC
/// dialect, and [`VANILLA_PROGS`] under the vanilla dialect, beside `b.qh` and `x.qc`.
fn progs_cases() -> Vec<Case> {
    let fteqcc = |text| (Dialect::Fteqcc, text, Encoding::Utf8);
    let utf8 = LOADED_PROGS
        .iter()
        .chain(SCRIPT_PROGS)
        .chain(FTE_LIST_PROGS)
        .map(|&text| fteqcc(text));
    let wide = WIDE_PROGS
        .iter()
        .flat_map(|&text| WIDE_ENCODINGS.map(|encoding| (Dialect::Fteqcc, text, encoding)));
    let vanilla = VANILLA_PROGS
        .iter()
        .map(|&text| (Dialect::Vanilla, text, Encoding::Utf8));
    utf8.chain(wide)
        .chain(vanilla)
        .map(|(dialect, text, encoding)| {
            let files = [("b.qh", BIND.to_owned()), ("x.qc", "float y;\n".to_owned())];
            let mut case = project_case("t.qc", "", &files);
            case.dialect = dialect;
            if let Some(project) = &mut case.project {
                project.progs = Some(ProgsSrc {
                    text: text.to_owned(),
                    encoding,
                });
            }
            case
        })
        .collect()
}

/// Every case of the directive family.
pub(super) fn cases() -> Vec<Case> {
    let sources = [
        line_end_cases(),
        NAME_FORMS.iter().map(|&s| s.to_owned()).collect(),
        name_length_cases(),
        word_bound_cases(),
        CONDITIONALS.iter().map(|&s| s.to_owned()).collect(),
        argument_cases(),
        evil_cases(),
        place_cases(),
        snippet_cases(),
        line_pragma_cases(),
        pragma_cases(),
        INCLUDE_LISTS.iter().map(|&s| s.to_owned()).collect(),
        JOINED.iter().map(|&s| s.to_owned()).collect(),
        LOADED.iter().map(|&s| s.to_owned()).collect(),
        frame_cases(),
    ];
    let mut cases: Vec<Case> = sources
        .into_iter()
        .flatten()
        .map(|source| Case::new(Dialect::Fteqcc, source))
        .collect();
    cases.extend(include_cases());
    cases.extend(wide_cases());
    cases
}
