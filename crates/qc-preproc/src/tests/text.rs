// SPDX-License-Identifier: GPL-3.0-or-later

//! FTEQCC's macros read as text (`docs/spec/dialect-fteqcc.md` FTEF-14.22 to FTEF-14.28): the
//! argument scanner, the `#define` reader, `QCC_COM_Parse2`'s tokens, variadic macros, built-in
//! constants' lines, and the bound on what expansions write under both dialects (QC-9.18). The
//! outcomes of the programs these cases come from were compared with the pinned FTEQCC in the
//! builtins differential's macro-text family (`cargo xtask oracles builtins`).

use std::path::Path;

use super::{codes, one, one_fte, tokens};
use crate::{
    Code, FTE_MAX_EXPANDED_TOKENS, FileOutput, MemoryLoader, PpToken, Preprocessor, TokenKind,
};
use qc_lexer::Dialect;

/// The tokens of `text` under FTEQCC, those that touch written together (QC-9.16), and its
/// diagnostic codes.
fn fte(text: &str) -> (String, Vec<Code>) {
    let (_, out) = one_fte(text);
    (touching(&out), codes(&out))
}

/// `out`'s tokens, each pair that touches written together and the rest apart.
fn touching(out: &FileOutput) -> String {
    let mut rendered = String::new();
    let mut last: Option<&PpToken> = None;
    for token in &out.tokens {
        if last.is_some_and(|last| !token.touches(last)) {
            rendered.push(' ');
        }
        rendered.push_str(&tokens(&FileOutput {
            tokens: vec![token.clone()],
            ..FileOutput::default()
        }));
        last = Some(token);
    }
    rendered
}

/// Whether `codes` has an error.
fn refused(codes: &[Code]) -> bool {
    codes.iter().any(|code| code.is_error())
}

// spec FTEF-14.22: a `,`, `(` or `)` in a character constant ends an argument or nests, so `'`
// is one argument and `'` another, and a `'` alone is no character constant
#[test]
fn a_character_constant_s_separator_splits_the_arguments() {
    for sep in [",", ")"] {
        let (_, errors) = fte(&format!("#define E(n) :n\nf = E('{sep}');\n"));
        assert!(refused(&errors), "{sep}: {errors:?}");
    }
    // `(` nests, so the arguments run to the end of the file.
    assert_eq!(fte("#define E(n) :n\nf = E('(');\n").1, vec![Code::E0117]);
    // FTEQCC binds `:S(e'x)` by the name `e'x`.
    let (_, out) = one_fte("#define S(a) #a\n:S(e'x)\n");
    assert_eq!(
        out.tokens.get(1).map(|t| t.kind.clone()),
        Some(TokenKind::String(b"e'x".to_vec()))
    );
    assert_eq!(codes(&out), vec![]);
}

// spec FTEF-14.22: a `,` or `)` in a comment ends an argument too, unless the comment comes right
// after the `(` or a `,`, where the white space and comments before an argument are skipped
#[test]
fn a_comment_s_separator_splits_the_arguments_after_its_start() {
    let (toks, errors) = fte("#define M(x) x\nM(#5 /* ) */)\n");
    assert!(refused(&errors), "{toks} {errors:?}");
    let (toks, errors) = fte("#define M(x) x\nf = M(#5 // )\n);\n");
    assert_eq!((toks.as_str(), errors), ("f = #5 );", vec![]));
    assert_eq!(
        fte("#define M(x) x\nM(/* ) */ #5)\n"),
        ("#5".to_owned(), vec![])
    );
    assert_eq!(
        fte("#define M(x) x\nM(// )\n#5)\n"),
        ("#5".to_owned(), vec![])
    );
    assert_eq!(
        fte("#define M(x, y) x\nM(#5, /* ) */ 6)\n"),
        ("#5".to_owned(), vec![])
    );
}

// spec FTEF-14.22: a string is passed over, its `,` and `)` in it, but ends only at a `"` with no
// `\` before it, so one ending in `\\` runs on to the next `"` or the end of the line
#[test]
fn a_string_ends_at_a_quote_with_no_backslash_before_it() {
    assert_eq!(
        fte("#define E(n) :n\nE(\"ex,a\")\n"),
        (":\"ex,a\"".to_owned(), vec![])
    );
    assert_eq!(
        fte("#define E(n) n\nf = E(:\"ex\\\\\");\n").1,
        vec![Code::E0117]
    );
    let (_, errors) = fte("#define E(n) n\nf = E(:\"ex\\\\\") \")\";\n");
    assert!(refused(&errors), "{errors:?}");
    // A line break ends the skip, and the argument goes on past it.
    assert_eq!(
        fte("#define E(n) n\nf = E(\"a\n#5);\n").1,
        vec![Code::E0102]
    );
}

// spec FTEF-14.22: reading goes on after the `)`, read again from there where a token stood
// across it, a directive's `#` at a line start included; the arguments' lexical errors are
// reported only where the expansion writes them, and an error right after the `)` is the text's
#[test]
fn reading_goes_on_after_the_close() {
    // The `)` in the `//` comment closes the use; the rest of that line and the next are read
    // again from there, and `#define` still starts its line.
    assert_eq!(
        fte("#define M(x) x\nM(#5 // )\n x)\n#define X 1\nX\n"),
        ("#5 x) 1".to_owned(), vec![])
    );
    // So is the rest of a `/* */` comment the `)` was in.
    let (toks, _) = fte("#define M(x) x\nM(#5 /* ) */ y)\n");
    assert!(toks.ends_with("*/ y)"), "{toks}");
    // `'x` is a string's text, not code.
    let (_, out) = one_fte("#define S(a) #a\nS('x)\n");
    let kinds: Vec<TokenKind> = out.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(kinds, [TokenKind::String(b"'x".to_vec())]);
    assert_eq!(codes(&out), vec![]);
    // The unclosed string right after the `)` is the source's.
    assert_eq!(fte("#define M(x) x\nM(1)\"ab\n").1, vec![Code::E0102]);
    // An argument no body writes is never read as code.
    assert_eq!(
        fte("#define E(a, b) a\nE(#5, 'x)\n"),
        ("#5".to_owned(), vec![])
    );
    // A `"` in it still opens a string the scan passes over, here to the end of the file.
    assert_eq!(
        fte("#define E(a, b) a\nE(#5, '\"x)\n").1,
        vec![Code::E0117, Code::E0104]
    );
}

// spec FTEF-14.23: a parameter list is read as bytes: a name twice, 33 names, a name of 32 bytes,
// a `...` that is not last and a list that does not end are refused; white space around a name
// is not part of it, `()` is one parameter with no name, and a name that is no word is never
// matched
#[test]
fn parameter_lists_are_read_as_fteqcc_reads_them() {
    for define in [
        "#define E(n, n) n",
        "#define E(n,n) n",
        "#define E(a, b, a) a",
        "#define E(..., a) a",
        "#define E(a, ..., b) a",
        "#define E(a",
        "#define E(a,) a",
    ] {
        assert_eq!(fte(&format!("{define}\n")).1, vec![Code::E0115], "{define}");
    }
    let names = |n: usize| {
        (0..n)
            .map(|i| format!("p{i}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    assert_eq!(fte(&format!("#define E({})\n", names(32))).1, vec![]);
    assert_eq!(
        fte(&format!("#define E({})\n", names(33))).1,
        vec![Code::E0115]
    );
    let long = "p".repeat(32);
    assert_eq!(fte(&format!("#define E({long}) 1\n")).1, vec![Code::E0115]);
    let fits = "p".repeat(31);
    assert_eq!(
        fte(&format!("#define E({fits}) {fits}\nE(5)\n")),
        ("5".to_owned(), vec![])
    );
    assert_eq!(fte("#define E( n ) n\nE(5)\n"), ("5".to_owned(), vec![]));
    assert_eq!(fte("#define E(n\t, m\t) m\nE(5, 6)\n").0, "6");
    assert_eq!(fte("#define E() k\nE(1, 2)\n"), ("k".to_owned(), vec![]));
    assert_eq!(fte("#define E(a b) a\nE(5)\n"), ("a".to_owned(), vec![]));
}

// spec FTEF-14.23: a macro's name is the word `QCC_PR_SimpleGetToken` reads, up to white space,
// `,`, `;`, `(`, `)`, `]` or `//`, so a comment right after `define` names nothing and one after
// the name is part of it
#[test]
fn a_macro_s_name_is_fteqcc_s_word() {
    assert_eq!(fte("#define /* c */ N 5\n").1, vec![Code::E0114]);
    assert_eq!(fte("#define\nN\n"), ("N".to_owned(), vec![Code::E0114]));
    assert_eq!(fte("#define N/**/ 5\nN\n").0, "N");
    // The name stops at the `;`, which starts the body.
    assert_eq!(fte("#define N; 5\nN\n").0, "; 5");
}

// spec FTEF-14.24: a body is stored with its `/* */` comments and its `\` and line breaks dropped,
// so a function-like macro's name and `(` with only those between them in a body are a call
#[test]
fn a_stored_body_calls_across_dropped_text() {
    for gap in [
        " ", "\t", "/**/", " /**/ ", "/**/ ", " /**/", "/* x */", "/*\n*/", "\\\n", " \\\n",
        "\\\n ", "\\\r\n",
    ] {
        let text = format!("#define M(x) x\n#define K M{gap}(#5)\nK\n");
        assert_eq!(fte(&text), ("#5".to_owned(), vec![]), "{gap:?}");
    }
}

// spec FTEF-14.24: the quote flag flips at every `"`, an escaped one too, and a `//` outside a
// quote ends the body; a `\` and line break is dropped, the byte after it stored whatever it is;
// a body's text is read as code only where it is expanded
#[test]
fn a_body_ends_where_fteqcc_s_quote_flag_says() {
    assert_eq!(fte("#define N :\"ex\\\"_a//\"\nN\n").1, vec![Code::E0101]);
    assert_eq!(
        fte("#define N :\"e//x\"\nN\n"),
        (":\"e//x\"".to_owned(), vec![])
    );
    assert_eq!(
        fte("#define N #5 :ex\\\n_a\nN\n"),
        ("#5 :ex_a".to_owned(), vec![])
    );
    // The second `\` is stored, so the body ends at its line break and `2` is code.
    let (toks, errors) = fte("#define N 1 \\\n\\\n2\nN\n");
    assert_eq!(toks, "2 1");
    assert!(refused(&errors), "{errors:?}");
    // An unclosed string in a body is no error until the body is expanded.
    assert_eq!(fte("#define N \"ab\nx\n"), ("x".to_owned(), vec![]));
    assert_eq!(fte("#define N \"ab\nN\n").1, vec![Code::E0101]);
    // A comment that does not end runs to the end of the file.
    assert_eq!(fte("#define N 1 /* c\nN\n"), (String::new(), vec![]));
}

// spec FTEF-14.24: a `\` and line break before a line starting with `#` keeps the line break, and
// so does the next continued line
#[test]
fn a_continued_directive_line_keeps_its_line_break() {
    let (pp, out) = one_fte("#define K a\\\n#b\\\nc\nK\n");
    let lines: Vec<u32> = out
        .tokens
        .iter()
        .filter_map(|t| pp.sources().line(t.span))
        .collect();
    assert_eq!(tokens(&out), "a # b c");
    // `a` and `#` were stored apart by a line break, `#b` and `c` by another, `b` and `c` not.
    let first = out.tokens.first().map(|t| t.span.span.end);
    let hash = out.tokens.get(1).map(|t| t.span.span.start);
    assert!(first < hash, "{lines:?}");
    assert!(!out.tokens[1].touches(&out.tokens[0]));
    assert!(!out.tokens[3].touches(&out.tokens[2]));
}

// spec FTEF-14.24: `#append` adds a space and its body to the stored body when its parameters are
// the macro's, and with others defines it again with the warning W0102
#[test]
fn append_needs_the_macro_s_parameters() {
    assert_eq!(
        fte("#define A(x) x\n#append A(x) y\nA(1)\n"),
        ("1 y".to_owned(), vec![])
    );
    assert_eq!(
        fte("#define A(x) x\n#append A 2\nA\n"),
        ("2".to_owned(), vec![Code::W0102])
    );
    assert_eq!(
        fte("#define A(x) x\n#append A(y) 2\nA(1)\n"),
        ("2".to_owned(), vec![Code::W0102])
    );
    assert_eq!(fte("#define A 1\n#append A\nA\n"), ("1".to_owned(), vec![]));
}

// spec FTEF-14.25: `QCC_COM_Parse2` reads a number of digits and `.`, or `0x` and hex digits, and
// a word, so a parameter written right after a number is still one; a string after `#` or `##` is
// its text with no quotes, and reading goes on a byte past its `"`; a token keeps 1023 bytes
#[test]
fn body_tokens_are_parse2_s() {
    assert_eq!(fte("#define E(a) #5a\nE(:ex_a)\n").0, "#5:ex_a");
    let (pp, out) = one_fte("#define E(a) #5.a\nE(:ex_a)\n");
    let number = out.tokens.get(1).and_then(|t| pp.sources().text(t.span));
    assert_eq!(number, Some(&b"5."[..]));
    assert_eq!(touching(&out), "#5:ex_a");
    assert_eq!(fte("#define E(a) #0x5a\nE(:ex_a)\n").0, "#90");
    assert_eq!(fte("#define E(a) #5x\nE(:ex_a)\n").0, "#5x");
    assert_eq!(fte("#define S(a) #\"5\"\nS(x)\n").0, "#5");
    assert_eq!(fte("#define S(a) #\"5\"x a\nS(:ex_a)\n").0, "#5 :ex_a");
    // `\"` in the string is its `"`, so the expansion's text `#a"b` leaves a string open.
    let (_, out) = one_fte("#define S(a) #\"a\\\"b\"\nS(x)\n");
    assert_eq!(
        out.tokens.get(1).map(|t| t.kind.clone()),
        Some(TokenKind::Name("a".to_owned()))
    );
    assert_eq!(codes(&out), vec![Code::E0101]);
    assert_eq!(fte("#define P(a) a##\"5\"x:ex_a\nP(#)\n").0, "#5:ex_a");
    let long = "n".repeat(1100);
    let (_, out) = one_fte(&format!("#define E(a) {long}\nE()\n"));
    assert_eq!(
        out.tokens.first().map(|t| t.kind.clone()),
        Some(TokenKind::Name("n".repeat(1023)))
    );
}

// spec FTEF-14.26: `__VA_COUNT__` is the arguments found less one and the commas after the named
// ones; `##__VA_ARGS__` with no variadic argument takes away a `,` right before it
#[test]
fn variadic_counts_and_commas() {
    for (use_, count) in [
        ("V()", "0"),
        ("V(a)", "0"),
        ("V(a, b)", "1"),
        ("V(a, b, c)", "2"),
    ] {
        let text = format!("#define V(...) :__VA_COUNT__\n{use_}\n");
        assert_eq!(fte(&text), (format!(":{count}"), vec![]), "{use_}");
    }
    assert_eq!(
        fte("#define V(a, ...) #5:__VA_COUNT__\nV(a, b)\n").0,
        "#5:1"
    );
    assert_eq!(fte("#define V(a) :__VA_COUNT__\nV(a)\n").0, ":__VA_COUNT__");
    for define in ["a,##__VA_ARGS__", "a , ## __VA_ARGS__", "a,## __VA_ARGS__"] {
        let text = format!("#define V(a, ...) {define}\nV(#5)\n");
        assert_eq!(fte(&text).0, "#5", "{define}");
    }
    assert_eq!(fte("#define V(a, ...) a,__VA_ARGS__\nV(#5)\n").0, "#5,");
    assert_eq!(
        fte("#define V(a, ...) a,##__VA_ARGS__\nV(#5, :ex_a)\n").0,
        "#5,:ex_a"
    );
    assert_eq!(fte("#define E(...) #5 __VA_ARGS__\nE(, )\n").0, "#5 ,");
}

// spec FTEF-14.27: a built-in constant an argument copies is written in place of the white space
// and `//` comments before it, at the line of the use's `)`; in a chunk a constant has the line
// FTEQCC was at in the chunk it was used in, counted from 1
#[test]
fn built_in_constants_have_fteqcc_s_lines() {
    assert_eq!(fte("#define E(n) n\nE(# __LINE__)\n").0, "#2");
    assert_eq!(fte("#define E(n) n\nE(#\t__LINE__)\n").0, "#2");
    assert_eq!(fte("#define E(n) n\nE(#//\n__LINE__)\n").0, "#3");
    assert_eq!(fte("#define E(n) n\nE(#\n__LINE__)\n").0, "#3");
    assert_eq!(fte("#define E(n) n\nE(#__LINE__\n)\n").0, "#3");
    assert_eq!(fte("#define E(n) n\nE(:__LINESTR__)\n").0, ":\"2\"");
    // A built-in constant read in a chunk is a chunk of its own, apart from the `:`.
    assert_eq!(
        fte("#define L :__LINESTR__\n#define M L\n\nM\n").0,
        ": \"1\""
    );
    assert_eq!(
        fte("#define L __LINESTR__\n#define V(...) __VA_ARGS__ L\nV(:\n\n)\n").0,
        ": \"3\""
    );
    assert_eq!(fte("#define M(x) :__LINESTR__ x\nM(\n)\n").0, ": \"3\"");
    // An argument copies the constant at the line of the chunk the use is in.
    assert_eq!(
        fte("#define K(x) x\n#define M K(:__LINESTR__)\n\nM\n").0,
        ":\"4\""
    );
}

/// The reviewer's file (P2-83): `#define D(x)` and `n` times `x`, then `D(` with `n` times `1`
/// and `)`, `4n + 18` bytes.
fn square(n: usize) -> String {
    format!("#define D(x) {}\nD({})\n", "x ".repeat(n), "1 ".repeat(n))
}

/// `text` as `a.qc` under `dialect`.
fn run(text: &str, dialect: Dialect) -> FileOutput {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", text.as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(dialect);
    pp.preprocess_file(Path::new("a.qc")).unwrap()
}

/// The bytes `out`'s tokens hold: the memory of the preprocessor's output.
fn output_bytes(out: &FileOutput) -> usize {
    out.tokens
        .capacity()
        .saturating_mul(std::mem::size_of::<PpToken>())
}

// spec FTEF-14.28, QC-9.18: one expansion of a body that names its parameter 8000 times, given
// 8000 tokens (the fourth P2-R2 review's 32018-byte file, 64000000 tokens unbounded), is E0120
// under each dialect, left as written, its output a few megabytes
#[test]
fn one_expansion_is_bounded_as_it_is_written() {
    assert_eq!(square(8000).len(), 32018);
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let out = run(&square(8000), dialect);
        assert_eq!(codes(&out), vec![Code::E0120], "{dialect:?}");
        assert_eq!(out.tokens.len(), 8003, "{dialect:?}");
        assert!(out.tokens.iter().all(|t| t.expansion.is_none()));
        assert!(output_bytes(&out) < 16 << 20, "{}", output_bytes(&out));
    }
}

// spec FTEF-14.28, QC-9.18: 1400 by 1400, 1960000 tokens, is under the bound and written under
// each dialect, its output under a gigabyte
#[test]
fn an_expansion_under_the_bound_is_written() {
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let out = run(&square(1400), dialect);
        assert_eq!(codes(&out), vec![], "{dialect:?}");
        assert_eq!(out.tokens.len(), 1_960_000);
        assert!(output_bytes(&out) < 1 << 30, "{}", output_bytes(&out));
    }
}

// spec FTEF-14.28: under FTEQCC an expansion's text may hold no more bytes than the tokens left,
// so 2000 by 2000, 4000000 tokens in 8000000 bytes, is E0120, which the vanilla dialect, counting
// its tokens, writes
#[test]
fn fteqcc_bounds_an_expansion_s_bytes() {
    assert_eq!(
        codes(&run(&square(2000), Dialect::Fteqcc)),
        vec![Code::E0120]
    );
    let vanilla = run(&square(2000), Dialect::Vanilla);
    assert_eq!(codes(&vanilla), vec![]);
    assert_eq!(vanilla.tokens.len(), 4_000_000);
    const { assert!(4_000_000 < FTE_MAX_EXPANDED_TOKENS) };
}

// spec QC-9.18: the vanilla dialect bounds the expansions of a file, an exponential chain of
// distinct macros stopping with E0120 once, its first use left as written
#[test]
fn vanilla_expansions_are_bounded() {
    let mut text = String::from("#define A0 x\n");
    for i in 1..24 {
        text.push_str(&format!("#define A{i} A{p} A{p}\n", p = i - 1));
    }
    text.push_str("A23 A23\n");
    let (_, out) = one(&text);
    assert_eq!(codes(&out), vec![Code::E0120]);
    let a23 = TokenKind::Name("A23".to_owned());
    assert!(out.tokens.iter().any(|t| t.kind == a23));
    let mut wide = format!("#define A0 {}\n", "x ".repeat(100));
    for i in 1..17 {
        wide.push_str(&format!("#define A{i} A{p} A{p}\n", p = i - 1));
    }
    wide.push_str("A16\n");
    let (_, out) = one(&wide);
    assert_eq!(codes(&out), vec![Code::E0120]);
    assert!(out.tokens.len() <= FTE_MAX_EXPANDED_TOKENS);
}

// spec FTEF-14.22: the white space and comments skipped before an argument end a `//` comment at
// a line feed or a carriage return not before one, and one the text ends in, or a `/* */` comment
// it ends in, runs the arguments to the end
#[test]
fn comments_before_an_argument_end_where_fteqcc_s_do() {
    assert_eq!(
        fte("#define M(x) x\nM(// c\r#5)\n"),
        ("#5".to_owned(), vec![])
    );
    assert_eq!(
        fte("#define M(x) x\nM(// c\r\n#5)\n"),
        ("#5".to_owned(), vec![])
    );
    assert!(fte("#define M(x) x\nM(// c").1.contains(&Code::E0117));
    assert!(fte("#define M(x) x\nM(/* c").1.contains(&Code::E0117));
    assert!(fte("#define M(x) x\nM(/* c */").1.contains(&Code::E0117));
}

// spec FTEF-14.22: a 33rd argument is "Too many parameters in macro call" even for a macro with
// 32 parameters, which takes 32
#[test]
fn thirty_two_arguments_is_fteqcc_s_most() {
    let params: Vec<String> = (0..32).map(|i| format!("p{i}")).collect();
    let args = |n: usize| vec!["1"; n].join(", ");
    let define = format!("#define E({}) p0\n", params.join(", "));
    assert_eq!(
        fte(&format!("{define}E({})\n", args(32))),
        ("1".to_owned(), vec![])
    );
    assert_eq!(
        fte(&format!("{define}E({})\n", args(33))).1,
        vec![Code::E0116]
    );
}

// spec FTEF-14.24: a name at a comment or a `//` ends there, a `\` and line break at the end of
// the file ends the body, and the line a refused `#define` ends on passes over strings and `/* */`
// comments before any `//`, and a `\` and CR LF after one
#[test]
fn define_lines_end_where_fteqcc_s_do() {
    assert_eq!(
        fte("#define // c\nx\n"),
        ("x".to_owned(), vec![Code::E0114])
    );
    assert_eq!(fte("#define N// c\nN\n"), (String::new(), vec![]));
    assert_eq!(fte("#define N"), (String::new(), vec![]));
    assert_eq!(fte("#define N 1 \\\n"), (String::new(), vec![]));
    assert_eq!(
        fte("#define /* never closed"),
        (String::new(), vec![Code::E0114])
    );
    assert_eq!(
        fte("#define /* a*b */ N 5\nx\n"),
        ("x".to_owned(), vec![Code::E0114])
    );
    assert_eq!(fte("#define N 1 // c \\\r\n2\nN\n").0, "1");
    for text in [
        "\"//\" /* c\nzz */ q\nr\n",
        "\"a\\\"//\" /* c\nzz */ q\nr\n",
        "\"a\\\\\" /* c\nzz */ q\nr\n",
    ] {
        assert_eq!(
            fte(&format!("#define E(n, n) {text}")),
            ("r".to_owned(), vec![Code::E0115]),
            "{text:?}"
        );
    }
}

// spec FTEF-14.25: a number is digits and `.`, `0` before any other byte one, and `0x` and hex
// digits and `.`
#[test]
fn numbers_are_parse2_s() {
    assert_eq!(fte("#define E(a) #05a\nE(:ex_a)\n").0, "#5:ex_a");
    assert_eq!(fte("#define E(a) #0x1F a\nE(:ex_a)\n").0, "#31 :ex_a");
    assert_eq!(fte("#define E(n) n\nE(#16)\n").0, "#16");
    // A number is one token, so it keeps 1023 bytes as a word does.
    let digits = "1".repeat(1100);
    let (pp, out) = one_fte(&format!("#define E(a) {digits}.5\nE()\n"));
    let number = out.tokens.first().and_then(|t| pp.sources().text(t.span));
    assert_eq!(number.map(<[u8]>::len), Some(1023));
}

// spec FTEF-14.26: `##__VA_ARGS__` with no variadic argument takes away only a `,`
#[test]
fn a_paste_before_no_variadic_argument_keeps_other_text() {
    assert_eq!(fte("#define V(a, ...) a##__VA_ARGS__\nV(#5)\n").0, "#5");
    assert_eq!(fte("#define V(a, ...) a:##__VA_ARGS__\nV(#5)\n").0, "#5:");
}

// spec FTEF-14.24: a macro defined again with another stored body warns, with the same one does
// not
#[test]
fn a_body_stored_again_otherwise_warns() {
    assert_eq!(
        fte("#define A 1\n#define A 2\nA\n"),
        ("2".to_owned(), vec![Code::W0102])
    );
    assert_eq!(
        fte("#define A 1\n#define A 1 /* c */\nA\n"),
        ("1".to_owned(), vec![])
    );
}

// spec FTEF-14.22: a lexical error in the text a macro used in a chunk took as its arguments is
// not the chunk's
#[test]
fn a_chunk_s_consumed_arguments_report_nothing() {
    assert_eq!(
        fte("#define M(a) 1\n#define K M('x)\nK\n"),
        ("1".to_owned(), vec![])
    );
}

// spec QC-9.7: a token pasted from an argument and a body token stands where the argument's
// byte is, its text its spelling; a use stands from its name to its `)` when they are in that
// order in one file, and at its name otherwise
#[test]
fn made_tokens_and_uses_keep_their_places() {
    let (pp, out) = one_fte("#define J(a) a##b\nJ(x)\n");
    let token = out.tokens.first().unwrap();
    assert_eq!(token.spelling.as_deref(), Some("xb"));
    assert_eq!(pp.sources().text(token.span), Some(&b"x"[..]));
    let (pp, out) = one_fte("#define K(x) x(#5)\n#define M(y) y\nK(M)\n");
    assert_eq!(touching(&out), "#5");
    let m = pp.expansions().iter().find(|e| e.name == "M").unwrap();
    assert_eq!(pp.sources().text(m.use_span), Some(&b"M"[..]));
    let k = pp.expansions().iter().find(|e| e.name == "K").unwrap();
    assert_eq!(pp.sources().text(k.use_span), Some(&b"K(M)"[..]));
    let mut loader = MemoryLoader::new();
    loader.insert(
        "a.qc",
        "#include \"k.qh\"\n#define M(y) y\nK(M)\n".as_bytes(),
    );
    loader.insert("k.qh", "#define K(x) x(#5)\n".as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    assert_eq!(touching(&out), "#5");
    let m = pp.expansions().iter().find(|e| e.name == "M").unwrap();
    assert_eq!(pp.sources().text(m.use_span), Some(&b"M"[..]));
}

// spec FTEF-14.22: reading goes on after each of 20000 uses whose `)` stands in a comment, the
// text after each lexed again as far as the next line
#[test]
fn many_uses_closed_in_comments_read_on() {
    let mut text = String::from("#define M(x) x\n");
    for _ in 0..20_000 {
        text.push_str("M(#5 // )\n) M(/* ) */ 6)\n");
    }
    let (_, out) = one_fte(&text);
    assert_eq!(codes(&out), vec![]);
    // `#5`, the `)` after the first use's line, and `6`.
    assert_eq!(out.tokens.len(), 20_000 * 4);
}

// spec FTEF-14.22: what is read again after a `)` that stood in a token or a comment is as a
// whole reading of the text from there gives: a string, a `#` at a line start (a directive), a
// `#warning`'s raw text, an unclosed comment and text with no white space before the next token
// are each read on whole, and a lexical error right after the `)` is reported once
#[test]
fn text_read_again_is_the_text_read_whole() {
    let cases = [
        ("M(#5 // )\n\"a\" \"b\")\nx\n", "#5 \"a\" \"b\") x"),
        ("M(#5 // )\n)#define X 1\nX\n", "#5 )#define X 1 X"),
        ("M(#5 // )\n)\n#define X 1\nX\n", "#5 ) 1"),
        ("M(#5 /* ) */ y)y z\n", "#5 */ y)y z"),
        ("M(1 // )\n/* a\n*/ b)\n", "1 b)"),
        ("M(1 // )\n)\n#warning a 'b\nc\n", "1 ) c"),
    ];
    let got: Vec<(&str, String)> = cases
        .iter()
        .map(|&(text, _)| (text, fte(&format!("#define M(x) x\n{text}")).0))
        .collect();
    let want: Vec<(&str, String)> = cases
        .iter()
        .map(|&(text, want)| (text, want.to_owned()))
        .collect();
    assert_eq!(got, want);
    let (_, errors) = fte("#define M(x) x\nM(1 // )\n)'x\n");
    assert_eq!(
        errors.iter().filter(|c| c.is_error()).count(),
        1,
        "{errors:?}"
    );
}

// spec FTEF-14.35: a directive other than `#define` goes on past a line break after `\` (LF or CR
// LF) and inside a `/* */` comment, a `*` or `/` in it no end, and a `//` comment ends its text
#[test]
fn other_directives_continue_as_fteqcc_s_do() {
    for (text, want) in [
        ("#undef N \\\nx\ny\n", "y"),
        ("#undef N \\\r\nx\ny\n", "y"),
        ("#undef N\r\nx\n", "x"),
        ("#undef N \\ \r\nx\n", "x"),
        ("#undef N /* a*b/c\n*/ x\ny\n", "y"),
        ("#undef N // c\nx\n", "x"),
        ("#undef N\nx\n", "x"),
        ("#undef N /* a */\nx\n", "x"),
        ("#undef N \\x\ny\n", "y"),
    ] {
        assert_eq!(fte(text).0, want, "{text:?}");
    }
}

// spec FTEF-14.28, QC-9.18: the most expansions a file makes is 2^20, each written to nothing
// here, so the tokens stay under theirs; a use past it is E0120 under each dialect
#[test]
fn the_expansions_of_a_file_are_bounded() {
    let mut text = String::from("#define A0\n");
    for i in 1..=20 {
        text.push_str(&format!("#define A{i} A{p} A{p}\n", p = i - 1));
    }
    text.push_str("A20 A0\n");
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let out = run(&text, dialect);
        assert_eq!(codes(&out), vec![Code::E0120], "{dialect:?}");
        assert!(out.tokens.len() < 4, "{dialect:?}");
    }
}

// spec FTEF-14.28: an expansion whose text is exactly the tokens left is written, under FTEQCC
// as under the vanilla dialect (QC-9.18)
#[test]
fn an_expansion_exactly_at_the_bound_is_written() {
    let n = 1_398_102;
    let fte_text = format!("#define D(x) {}\nD(1)\n", vec!["x"; n].join(", "));
    let out = run(&fte_text, Dialect::Fteqcc);
    assert_eq!(codes(&out), vec![]);
    assert_eq!(out.tokens.len(), 2 * n - 1);
    let out = run(&square(2048), Dialect::Vanilla);
    assert_eq!(codes(&out), vec![]);
    assert_eq!(out.tokens.len(), FTE_MAX_EXPANDED_TOKENS);
}

// spec FTEF-14.24: a string in a function-like macro's body is copied as written; after a `//`
// a `/*` is no comment, and a string the line goes past ends at a line break
#[test]
fn bodies_and_their_lines_keep_strings_and_comments_apart() {
    let (_, out) = one_fte("#define P(a) \"ex_a\" a\nP(1)\n");
    let kinds: Vec<TokenKind> = out.tokens.iter().map(|t| t.kind.clone()).collect();
    assert_eq!(
        kinds,
        [TokenKind::String(b"ex_a".to_vec()), TokenKind::Float(1.0)]
    );
    assert_eq!(fte("#define N 1 // a /* b\nc */ x\nN\n").0, "c */ x 1");
    assert_eq!(
        fte("#define E(n, n) \"ab\nzz\n"),
        ("zz".to_owned(), vec![Code::E0115])
    );
    // A `(` or other stop byte right after `define` names nothing.
    assert_eq!(fte("#define (x) 5\n").1, vec![Code::E0114]);
}

// spec QC-9.7: a token pasted from two arguments stands from the first's byte to the second's
#[test]
fn a_paste_of_two_arguments_stands_over_both() {
    let (pp, out) = one_fte("#define J(a, b) a##b\nJ(x, y)\n");
    let token = out.tokens.first().unwrap();
    assert_eq!(pp.sources().text(token.span), Some(&b"x, y"[..]));
}

// spec FTEF-14.32: the tokens of a built-in constant's text read in a chunk touch each other, and
// a lexical error a chunk's text holds right after a macro used in it took its arguments is
// reported
#[test]
fn chunk_text_after_a_use_is_read() {
    let (_, out) = one_fte_in("a\"b.qc", "#define F __FILE__\nF\n");
    assert!(out.tokens[1].touches(&out.tokens[0]));
    let (_, out) = one_fte_in("a\"b.qc", "__FILE__\n");
    assert!(!out.tokens[1].touches(&out.tokens[0]));
    // A token of no expansion follows none.
    assert!(out.tokens.iter().all(|t| t.follows.is_none()));
    let (_, errors) = fte("#define M(a) 1\n#define K M(1)\"ab\nK\n");
    assert!(errors.contains(&Code::E0101), "{errors:?}");
    // `0x1.2a` is one number, which takes the `a`.
    assert!(!fte("#define E(a) #0x1.2a\nE(:ex_a)\n").0.contains("ex_a"));
}

// spec FTEF-14.23: a parameter's name is its bytes, which need not be UTF-8
#[test]
fn a_parameter_s_name_is_bytes() {
    let mut loader = MemoryLoader::new();
    loader.insert("t.qc", b"#define E(\xe9) :\xe9\nE(ex_a)\n");
    let mut pp = Preprocessor::new(loader).with_dialect(Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new("t.qc")).unwrap();
    assert_eq!(touching(&out), ":ex_a");
}

/// `text` as the file `path` under FTEQCC.
fn one_fte_in(path: &str, text: &str) -> (Preprocessor<MemoryLoader>, FileOutput) {
    let mut loader = MemoryLoader::new();
    loader.insert(path, text.as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new(path)).unwrap();
    (pp, out)
}

/// Pieces of text whose lexing depends on what comes before: strings, comments, character
/// constants, directives and raw text.
const PIECES: &[&str] = &[
    "\"", "'", "/*", "*/", "//", "\n", " ", "a", "1", ".", "#", "\\", "(", ")", ",", "warning",
    "\"x\"", "R\"(", ")\"", "$frame", "u8", "\r",
];

/// A token's kind, place and whether it starts a line.
type Read = (TokenKind, qc_lexer::Span, bool);

/// The tokens of `stream` from where reading stands, and its lexical errors (code and place, in
/// the order they were found).
fn read_on(stream: &crate::TokenStream) -> (Vec<Read>, Vec<(Code, usize)>) {
    let tokens = stream
        .tokens
        .get(stream.pos..)
        .unwrap_or_default()
        .iter()
        .map(|t| (t.kind.clone(), t.span, t.line_start))
        .collect();
    let errors = stream
        .diagnostics
        .iter()
        .map(|d| (d.code, d.span.start))
        .collect();
    (tokens, errors)
}

/// What reading `stream`, lexed from `text`, on at `offset` must give: the whole rest of the
/// text from there lexed, after the lexical errors `stream` found before it.
fn whole_rest(
    stream: &crate::TokenStream,
    text: &[u8],
    offset: usize,
    dialect: Dialect,
) -> crate::TokenStream {
    let rest = crate::lex_from(text, offset..text.len(), dialect);
    let shift = |s: qc_lexer::Span| {
        qc_lexer::Span::new(s.start.saturating_add(offset), s.end.saturating_add(offset))
    };
    let tokens = rest
        .tokens
        .into_iter()
        .map(|mut t| {
            t.span = shift(t.span);
            t.kind = crate::moved_frames(t.kind, shift);
            t
        })
        .collect();
    let diagnostics = stream
        .diagnostics
        .iter()
        .filter(|d| d.span.start < offset)
        .cloned()
        .chain(rest.diagnostics.into_iter().map(|mut d| {
            d.span = shift(d.span);
            d
        }))
        .collect();
    crate::TokenStream {
        tokens,
        pos: 0,
        diagnostics,
        read_on: offset,
    }
}

/// Whether `offset` is inside a line that starts with `#`, before its line break. No use's `)`
/// and no `#define`'s line ends there, so reading never goes on there, and the text after it, a
/// `#warning`'s raw text among it, is not lexed as the rest of a line.
fn in_directive_line(text: &[u8], offset: usize) -> bool {
    let line_start = text
        .get(..offset)
        .and_then(|before| before.iter().rposition(|&b| b == b'\n'))
        .map_or(0, |at| at.saturating_add(1));
    let in_line = text.get(offset).is_some_and(|&b| b != b'\n');
    in_line
        && text
            .get(line_start..offset)
            .and_then(|line| line.iter().find(|&&b| !qc_lexer::is_whitespace(b)))
            == Some(&b'#')
}

/// Each reading on of `text` at `offset` and then at `second`, under each dialect, with reading
/// first at the start or right at `offset`, and what lexing the whole rest would give: the
/// first difference, if any.
fn reading_on_differs(text: &[u8], offset: usize, second: usize) -> Option<String> {
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let stream = crate::TokenStream::lex(text, dialect);
        if in_directive_line(text, offset) {
            continue;
        }
        let before = stream.tokens.partition_point(|t| t.span.start < offset);
        let whole = whole_rest(&stream, text, offset, dialect);
        let shown = String::from_utf8_lossy(text).into_owned();
        // Reading stood at the start, which leaves the new tokens room before the kept ones,
        // or right at `offset`, which leaves none.
        for start in [0, before] {
            let mut read = crate::TokenStream::lex(text, dialect);
            read.pos = start;
            read.resume_at(text, offset, dialect);
            if read_on(&read) != read_on(&whole) {
                return Some(format!("{shown:?} at {offset} from {start}, {dialect:?}"));
            }
            // Reading on again, further on, from text read on once already: reading goes on
            // after a use's `)`, which stands after where reading last went on.
            if second <= offset || in_directive_line(text, second) {
                continue;
            }
            let again = whole_rest(&read, text, second, dialect);
            read.resume_at(text, second, dialect);
            if read_on(&read) != read_on(&again) {
                return Some(format!(
                    "{shown:?} at {offset} then {second} from {start}, {dialect:?}"
                ));
            }
        }
    }
    None
}

/// Reading `text`, lexed under FTEQCC, on at `offset`: the bytes lexed again, the tokens the
/// stream then holds and its lexical errors.
fn relexed(text: &str, offset: usize) -> (usize, usize, usize) {
    let mut stream = crate::TokenStream::lex(text.as_bytes(), Dialect::Fteqcc);
    let bytes = stream.resume_at(text.as_bytes(), offset, Dialect::Fteqcc);
    (bytes, stream.tokens.len(), stream.diagnostics.len())
}

// spec FTEF-14.22: the text is lexed again only as far as the next token of the first lexing
// the new lexing reaches as that one did, and not at all after white space, the new tokens taking
// places already read
#[test]
fn reading_on_lexes_again_only_as_far_as_it_must() {
    // Only white space before the offset: nothing is lexed again.
    assert_eq!(relexed("a b", 2), (0, 2, 0));
    assert_eq!(relexed("a  b", 2), (0, 2, 0));
    // A comment stood across the offset: `*/ ` is lexed again, its two tokens in the places of
    // `M` and `(`.
    assert_eq!(relexed("M(/*)*/ x) y", 5), (3, 5, 0));
    // A token at a line's start in the text lexed again.
    assert_eq!(relexed("a/*)\nq*/ b", 4), (5, 5, 0));
    // A lexical error before the end of the text lexed again.
    assert_eq!(relexed("a/*)\"\n*/ b", 4), (5, 5, 1));
    // An error the first lexing found at the offset, after a comment, is found once: lexed
    // again from there, or kept with the token it is in.
    assert_eq!(relexed("/**//*", 4), (2, 0, 1));
    let long = format!("/**/{}", "n".repeat(3000));
    assert_eq!(relexed(&long, 4), (0, 1, 1));
}

// spec FTEF-14.22: reading on inside a comment before `warning` reads the name, not the raw text
// a `#warning` would take, after it
#[test]
fn reading_on_reads_a_word_after_a_comment_whole() {
    for (text, offset) in [(&b"#/* */warning x"[..], 3), (b"#/* */warning x\ny", 2)] {
        assert_eq!(reading_on_differs(text, offset, offset), None);
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig::with_cases(2048))]

    // spec FTEF-14.22: reading on at a byte gives what lexing the whole rest of the text from
    // there gives, though only as much of it as needs it is lexed again: the same tokens,
    // places, line starts and lexical errors, after the errors of the text before it
    #[test]
    fn reading_on_lexes_again_as_the_whole_rest_would(
        pieces in proptest::collection::vec(proptest::sample::select(PIECES), 0..40),
        at in 0_usize..400,
        further in 0_usize..400,
    ) {
        let text = pieces.concat().into_bytes();
        let offset = at.min(text.len());
        let second = offset.saturating_add(further).min(text.len());
        proptest::prop_assert_eq!(reading_on_differs(&text, offset, second), None);
    }
}
