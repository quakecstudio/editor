// SPDX-License-Identifier: GPL-3.0-or-later

//! The directive family of the builtin differential (P2-94 to P2-99): every directive and pragma
//! FTEQCC's `QCC_PR_Precompiler` knows, read where FTEQCC reads one, against what the FTEQCC
//! profile makes of it (`docs/spec/dialect-fteqcc.md` FTEF-14.33 to FTEF-14.47). Each program
//! binds `f` only when the directive did what FTEQCC does with it: ended its line where
//! `QCC_PR_SkipToEndOfLine` ends it, defined or skipped what it should, numbered the lines as
//! `#pragma line` says, or found the file an `#include` names.

use qc_syntax::Dialect;

use super::{Case, Project};

/// What binds `f` once the directive before it has been read.
const BIND: &str = "void() f = #5;\n";

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
/// FTEF-14.36, FTEF-14.38, FTEF-14.39, FTEF-14.42).
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
];

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
/// `#pragma DONT_COMPILE_THIS_FILE` (FTEF-14.44).
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
/// break (FTEF-14.44).
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
        project: Some(Project {
            compiled: compiled.to_owned(),
            files,
            include_dirs: Vec::new(),
        }),
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
        .map(|&(compiled, source, files)| {
            let files: Vec<(&str, String)> = files
                .iter()
                .map(|&(name, text)| (name, text.to_owned()))
                .collect();
            project_case(compiled, source, &files)
        })
        .collect();
    for &(compiled, source, files, dirs) in DIR_PROJECTS {
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
    let includer = format!("#include \"b.qh\"\n{BIND}");
    for (snippet, _) in SNIPPETS {
        for indent in ["", "  "] {
            let header = format!("{indent}{snippet}\n");
            cases.push(project_case("t.qc", &includer, &[("b.qh", header)]));
        }
    }
    cases
}

/// Every case of the directive family.
pub(super) fn cases() -> Vec<Case> {
    let sources = [
        line_end_cases(),
        NAME_FORMS.iter().map(|&s| s.to_owned()).collect(),
        name_length_cases(),
        CONDITIONALS.iter().map(|&s| s.to_owned()).collect(),
        argument_cases(),
        evil_cases(),
        place_cases(),
        snippet_cases(),
        line_pragma_cases(),
        pragma_cases(),
        INCLUDE_LISTS.iter().map(|&s| s.to_owned()).collect(),
    ];
    let mut cases: Vec<Case> = sources
        .into_iter()
        .flatten()
        .map(|source| Case::new(Dialect::Fteqcc, source))
        .collect();
    cases.extend(include_cases());
    cases
}
