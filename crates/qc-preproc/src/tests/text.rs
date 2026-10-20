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
// under each dialect, left as written, its output a few megabytes; 1400 by 1400, 1960000 tokens,
// is under the bound and written, in a few hundred megabytes
#[test]
fn one_expansion_is_bounded_as_it_is_written() {
    assert_eq!(square(8000).len(), 32018);
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let out = run(&square(8000), dialect);
        assert_eq!(codes(&out), vec![Code::E0120], "{dialect:?}");
        assert_eq!(out.tokens.len(), 8003, "{dialect:?}");
        assert!(out.tokens.iter().all(|t| t.expansion.is_none()));
        assert!(output_bytes(&out) < 16 << 20, "{}", output_bytes(&out));
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
