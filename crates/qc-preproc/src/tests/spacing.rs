// SPDX-License-Identifier: GPL-3.0-or-later

//! What touches what in an expansion (`docs/spec/language.md` QC-9.16), and the word FTEQCC
//! reads as a macro's name (`docs/spec/dialect-fteqcc.md` FTEF-14.13).

use std::path::Path;

use super::tokens;
use crate::{FileOutput, MemoryLoader, PpToken, Preprocessor};
use qc_lexer::Dialect;

/// `text` as `a.qc`, preprocessed under `dialect`.
fn run(text: &[u8], dialect: Dialect) -> FileOutput {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", text);
    let mut pp = Preprocessor::new(loader).with_dialect(dialect);
    pp.preprocess_file(Path::new("a.qc")).unwrap()
}

/// The tokens of `text` under `dialect`, each pair that touches (QC-9.16) written together
/// and the rest apart.
fn touching(text: &str, dialect: Dialect) -> String {
    rendered(&run(text.as_bytes(), dialect))
}

/// `out`'s tokens, each pair that touches (QC-9.16) written together and the rest apart.
fn rendered(out: &FileOutput) -> String {
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

/// The bytes that end the word `QCC_PR_CheckCompConst` reads: white space and its stops.
const WORD_ENDS: &[u8] = b" \r\n\t\x0b)(+-*/|&=^~[]\"{};:<>,.#";

// spec FTEF-14.13: under FTEQCC a macro's name followed directly by a byte that does not end
// `QCC_PR_CheckCompConst`'s word stays a name, for every such byte; one followed by white
// space, one of the stop bytes or the end of the file expands
#[test]
fn a_macro_expands_only_where_fteqcc_s_word_ends() {
    for byte in 1..=u8::MAX {
        if byte.is_ascii_alphanumeric() || byte == b'_' {
            continue;
        }
        let mut text = b"#define N 5\nx = N".to_vec();
        text.push(byte);
        text.extend_from_slice(b" 1;\n");
        let out = run(&text, Dialect::Fteqcc);
        let expanded = out.tokens.iter().any(|t| t.expansion.is_some());
        assert_eq!(expanded, WORD_ENDS.contains(&byte), "{byte:#04x}");
    }
    assert_eq!(touching("#define N 5\nx = N", Dialect::Fteqcc), "x = 5");
    assert_eq!(
        touching("#define N 5\nx = N/**/!", Dialect::Fteqcc),
        "x = 5 !"
    );
    assert_eq!(
        touching("#define N 5\nx = N\\\n;", Dialect::Fteqcc),
        "x = N ;"
    );
    // A function-like macro and a built-in constant are words too.
    assert_eq!(
        touching("#define M(a) a\nx = M!(1)", Dialect::Fteqcc),
        "x = M!(1)"
    );
    assert_eq!(touching("x = __LINE__?", Dialect::Fteqcc), "x = __LINE__?");
    assert_eq!(touching("x = __LINE__ ?", Dialect::Fteqcc), "x = 1 ?");
}

// spec FTEF-14.13: the vanilla preprocessor expands a macro's name whatever follows it
#[test]
fn vanilla_expands_a_macro_before_any_byte() {
    for byte in [b'!', b'?', b'%', b'@', b'$', b'\'', b'`', 0x01, 0x7f] {
        let mut text = b"#define N 5\nx = N".to_vec();
        text.push(byte);
        text.extend_from_slice(b" 1;\n");
        let out = run(&text, Dialect::Vanilla);
        assert!(
            out.tokens.iter().any(|t| t.expansion.is_some()),
            "{byte:#04x}"
        );
    }
}

// spec FTEF-14.13: in an expansion the byte after a name is the one FTEQCC's text has there:
// the body's next byte, the body's byte after the parameter for an argument's last token, and
// at the end of the expansion the word ends
#[test]
fn the_byte_after_a_name_in_an_expansion_is_the_expansion_s() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define N 5\n#define M N!=5\nx = M;"), "x = N!=5 ;");
    assert_eq!(fte("#define N 5\n#define M N/**/!=5\nx = M;"), "x = N!=5 ;");
    assert_eq!(fte("#define N 5\n#define M N !=5\nx = M;"), "x = 5 !=5 ;");
    assert_eq!(fte("#define N 5\n#define M N\nx = M !=5;"), "x = 5 !=5;");
    assert_eq!(
        fte("#define N 5\n#define A(a) a!=5\nx = A(N);"),
        "x = N!=5 ;"
    );
    assert_eq!(
        fte("#define N 5\n#define A(a) a!=5\nx = A(N );"),
        "x = N!=5 ;"
    );
    assert_eq!(
        fte("#define N 5\n#define A(a) a !=5\nx = A(N);"),
        "x = 5 !=5 ;"
    );
    assert_eq!(
        fte("#define N 5\n#define A(a) a==5\nx = A(N);"),
        "x = 5 ==5 ;"
    );
    assert_eq!(fte("#define N 5\n#define A(a) a\nx = A(N!);"), "x = N! ;");
}

// spec QC-9.16: under FTEQCC an argument's tokens touch the body tokens the parameter
// touches, after white space and comments before the first are dropped and white space and
// `//` comments after the last; a `/* */` comment after it, and in the variadic argument
// anything before the `)`, keeps it apart
#[test]
fn an_argument_touches_the_body_tokens_its_parameter_touches() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define L(x) x:\nL(lbl)"), "lbl:");
    assert_eq!(fte("#define L(x) x:\nL( lbl )"), "lbl:");
    assert_eq!(fte("#define L(x) x:\nL(/**/lbl// c\n)"), "lbl:");
    assert_eq!(fte("#define L(x) x:\nL(lbl/**/)"), "lbl :");
    assert_eq!(fte("#define L(x) x :\nL(lbl)"), "lbl :");
    assert_eq!(fte("#define L(x) -x\nL(5)"), "-5");
    assert_eq!(fte("#define L(x) - x\nL(5)"), "- 5");
    assert_eq!(fte("#define L(x) x:\nL(a b)"), "a b:");
    assert_eq!(fte("#define L(x) x:\nL(a:b)"), "a:b:");
    assert_eq!(fte("#define L(x,y) x y:\nL(,lbl)"), "lbl:");
    assert_eq!(fte("#define L(x,y) x y:\nL(lbl,)"), "lbl :");
    assert_eq!(fte("#define L(x,y) x:y\nL(lbl,)"), "lbl:");
    assert_eq!(fte("#define L(x,y) x:y;\nL(lbl,)"), "lbl:;");
    assert_eq!(fte("#define V(...) __VA_ARGS__:\nV(lbl)"), "lbl:");
    assert_eq!(fte("#define V(...) __VA_ARGS__:\nV( lbl)"), "lbl:");
    assert_eq!(fte("#define V(...) __VA_ARGS__:\nV(lbl )"), "lbl :");
    assert_eq!(fte("#define V(a, ...) a:__VA_ARGS__:\nV(l, m)"), "l:m:");
    // A `#` string touches as written, but is made, so a renderer keeps it apart.
    assert_eq!(fte("#define S(x) #x:\nS(lbl)"), "\"lbl\":");
}

// spec QC-9.16: under FTEQCC two body tokens with only `/* */` comments and `\` line breaks
// between them touch, as `QCC_PR_PreProcessor_Define` drops those; the vanilla dialect keeps
// them apart, and keeps an argument apart from the body
#[test]
fn a_body_touches_where_fteqcc_stored_it_touching() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    let vanilla = |text: &str| touching(text, Dialect::Vanilla);
    assert_eq!(fte("#define L lbl/**/:\nL"), "lbl:");
    assert_eq!(fte("#define L lbl/* a */ /**/:\nL"), "lbl :");
    assert_eq!(fte("#define L lbl/* a */:\nL"), "lbl:");
    assert_eq!(fte("#define L lbl\\\n:\nL"), "lbl:");
    assert_eq!(fte("#define L lbl\\\r\n:\nL"), "lbl:");
    assert_eq!(fte("#define L(x) x/**/:\nL(lbl)"), "lbl:");
    assert_eq!(vanilla("#define L x/**/;\nL"), "x ;");
    assert_eq!(vanilla("#define L x;\nL"), "x;");
    assert_eq!(vanilla("#define L(x) x;\nL(y)"), "y ;");
    assert_eq!(vanilla("#define L(x) x;\nL(a;b)"), "a;b ;");
    // Tokens of two expansions, or of an expansion and the source, never touch.
    assert_eq!(fte("#define A a\n#define B b\nA+B"), "a + b");
    assert_eq!(fte("#define A a\nA:"), "a :");
}

// spec QC-9.16: a token touches only the one written right before it, so two body tokens
// that a macro expanding to nothing came to stand between stay apart, in both dialects
#[test]
fn tokens_with_an_empty_expansion_between_them_stay_apart() {
    for dialect in [Dialect::Fteqcc, Dialect::Vanilla] {
        let at = |text: &str| touching(text, dialect);
        assert_eq!(at("#define E\n#define K lbl E;\nK"), "lbl ;");
        assert_eq!(at("#define F(a) a\n#define K lbl F();\nK"), "lbl ;");
        assert_eq!(at("#define F(a) a\n#define K lbl F(1);\nK"), "lbl 1 ;");
        assert_eq!(at("#define E\n#define K(x) x E;\nK(lbl)"), "lbl ;");
    }
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define E\n#define K(x, y) x y:\nK(lbl, E)"), "lbl :");
}

// spec QC-9.16: under FTEQCC a parameter that touches a word only across a dropped comment or
// line break is part of that word, as FTEQCC stored it, and no parameter; a number before it
// ends there, so the parameter stays one
#[test]
fn a_parameter_inside_a_stored_word_is_no_parameter() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define A(a) a/**/b\nA(5)"), "ab");
    assert_eq!(fte("#define A(a) b/**/a\nA(5)"), "ba");
    assert_eq!(fte("#define A(a) a\\\nb\nA(5)"), "ab");
    assert_eq!(fte("#define A(a) a/**/1\nA(5)"), "a1");
    assert_eq!(fte("#define A(a) 1/**/a\nA(x)"), "1x");
    assert_eq!(fte("#define A(a) a/**/+\nA(5)"), "5+");
    assert_eq!(
        touching("#define A(a) a/**/b\nA(5)", Dialect::Vanilla),
        "5 b"
    );
    assert_eq!(touching("#define L(x) -x\nL(5)", Dialect::Vanilla), "- 5");
}

// spec FTE-23.10, FTEF-14.13: a `#` reads the name that touches it in the text FTEQCC reads,
// a body's as stored included, so `#/**/N` in a body is `N`'s expansion with no `#`
#[test]
fn a_hash_in_a_body_reads_the_name_that_touches_it() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define N 5\n#define K #/**/N\nx = K;"), "x = 5 ;");
    assert_eq!(fte("#define N 5\n#define K #N\nx = K;"), "x = 5 ;");
    assert_eq!(fte("#define N 5\n#define K # N\nx = K;"), "x = # 5 ;");
    assert_eq!(
        fte("#define N 5\n#define K(a) #a\nx = K(N);"),
        "x = \"N\" ;"
    );
    assert_eq!(fte("#define N 5\n#define K #N!\nx = K;"), "x = #N! ;");
}

/// The tokens of `text` under FTEQCC as [`touching`] writes them, and its diagnostic codes.
fn fte_with_codes(text: &str) -> (String, Vec<crate::Code>) {
    let out = run(text.as_bytes(), Dialect::Fteqcc);
    let codes = out.diagnostics.iter().map(|d| d.code).collect();
    (touching(text, Dialect::Fteqcc), codes)
}

// spec FTEF-14.14: `##` writes the next text right after the last, either side possibly
// empty, and the text is lexed again; an argument after `##` is copied whole
#[test]
fn a_paste_writes_its_sides_together() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define P(a, b) a##b\nP(#5, )"), "#5");
    assert_eq!(fte("#define P(a, b) a##b\nP(, #5)"), "#5");
    assert_eq!(fte("#define P(a, b) a##b\nP(#1, 6)"), "#16");
    assert_eq!(fte("#define P(a, b) a##b\nP(, )"), "");
    assert_eq!(fte("#define P(a) ##a\nP(#5)"), "#5");
    assert_eq!(fte("#define P(a) a##\nP(#5)"), "#5");
    assert_eq!(fte("#define P(a, b) a##b:\nP(l, bl)"), "lbl:");
    assert_eq!(fte("#define P(a, b) a##b :\nP(l, bl)"), "lbl :");
    assert_eq!(fte("#define P(a, b) a##b:\nP(l, bl )"), "lbl :");
    assert_eq!(fte("#define P(a, b) a##b\nP(x, y z)"), "xy z");
}

/// The tokens of `a.qc` and the files it includes, `files`, under FTEQCC, written as
/// [`touching`] writes them.
fn touching_in(files: &[(&str, &str)]) -> String {
    let mut loader = MemoryLoader::new();
    for (path, text) in files {
        loader.insert(*path, text.as_bytes());
    }
    let mut pp = Preprocessor::new(loader).with_dialect(Dialect::Fteqcc);
    rendered(&pp.preprocess_file(Path::new("a.qc")).unwrap())
}

// spec FTEF-14.29: a function-like macro's name at the end of a file's line is not a call,
// even when the file it includes next starts with a `(`
#[test]
fn a_paren_in_an_included_file_opens_no_call() {
    let files = [
        ("a.qc", "#define M(x) [x]\nM\n#include \"b.qc\"\n"),
        ("b.qc", "(1)\n"),
    ];
    assert_eq!(touching_in(&files), "M (1)");
}

// spec FTEF-14.39: `#append` writes a space before the text it adds, so the added text
// touches nothing of the body before it, in another file too
#[test]
fn appended_text_stays_apart_from_the_body() {
    let files = [
        ("a.qc", "#define M a\n#include \"b.qc\"\nM\n"),
        ("b.qc", "#append M :\n"),
    ];
    assert_eq!(touching_in(&files), "a :");
}

// spec FTEF-14.13: a NUL byte after a macro's name ends the word, so the name expands, and the
// text goes on after it, which FTEQCC's file loader made a space (FTEF-14.64)
#[test]
fn a_nul_after_a_name_ends_the_word() {
    assert_eq!(
        touching("#define N 5\nx = N\0;\n", Dialect::Fteqcc),
        "x = 5 ;"
    );
}

// spec FTEF-14.31: `#param` is the argument's text as written, white space and comments
// included, with no lexical error; a `#` before no parameter writes the next word against
// it, and one at the end of the body writes nothing
#[test]
fn a_string_is_the_argument_s_text() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define S(a) #a\nS( ex_a )"), "\"ex_a \"");
    assert_eq!(fte("#define S(a) #a\nS(ex  a)"), "\"ex  a\"");
    assert_eq!(fte("#define S(a) #a\nS(ex/**/a)"), "\"ex/**/a\"");
    assert_eq!(fte("#define S(a) #a\nS((ex, a))"), "\"(ex, a)\"");
    assert_eq!(fte("#define S(a) #a\nS()"), "\"\"");
    assert_eq!(
        fte_with_codes("#define S(a) #a\nS(ex\\a)"),
        ("\"ex\\\\a\"".to_owned(), vec![])
    );
    assert_eq!(fte("#define S(a) # 5\nS(x)"), "#5");
    assert_eq!(fte("#define S(a) #5 #\nS(x)"), "#5");
    assert_eq!(fte("#define S(a) #a\n#define T(a) S(a)\nT(N)"), "\"N\"");
}

// spec FTEF-14.16: an object-like macro's `#` and `##` are tokens like any other
#[test]
fn an_object_like_macro_keeps_its_hashes() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define P ex_##a\nP"), "ex_##a");
    assert_eq!(fte("#define a x\n#define P #a\nP"), "x");
}

// spec FTEF-14.17: a macro is not expanded in its own chunk, and otherwise while fewer than
// 8 chunks of it are open, so one used again through another is expanded again; a built-in
// constant's value touches neither token beside it
#[test]
fn a_macro_is_expanded_again_through_another() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define A A\nA"), "A");
    assert_eq!(fte("#define A B\n#define B A\nA"), "A");
    assert_eq!(
        fte("#define A x B\n#define B +A\nA"),
        format!("{}x +A", "x + ".repeat(7))
    );
    assert_eq!(
        touching("#define A x B\n#define B +A\nA", Dialect::Vanilla),
        "x +A"
    );
    assert_eq!(fte("#define A(x) x\nA(A(1))"), "A(1)");
    assert_eq!(fte("#define K x__LINE__:\nK"), "x__LINE__:");
    assert_eq!(fte("#define K x __LINE__:\nK"), "x 2 :");
}

// spec FTEF-14.29: a function-like macro takes its `(` after spaces and tabs only, in the
// chunk of its name, and its arguments from that chunk only; one that runs past it is E0117
// spec QCD-8.10: E0117 "unterminated argument list for macro NAME"
#[test]
fn arguments_stay_in_the_chunk_of_the_name() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define M(x) x\nM \t(1)"), "1");
    assert_eq!(fte("#define M(x) x\nM\n(1)"), "M (1)");
    assert_eq!(fte("#define M(x) x\nM/**/(1)"), "M (1)");
    assert_eq!(fte("#define M(x) x\n#define N M\nN(1)"), "M (1)");
    assert_eq!(touching("#define M(x) x\nM\n(1)", Dialect::Vanilla), "1");
    let (toks, codes) = fte_with_codes("#define L(x) x:\n#define LL L(lbl\nLL)");
    assert_eq!(
        (toks.as_str(), codes),
        ("L(lbl )", vec![crate::Code::E0117])
    );
    let (_, codes) = fte_with_codes("#define M(x) x\n#define N(y) M(y\nN(1))");
    assert_eq!(codes, vec![crate::Code::E0117]);
}

// spec QCD-8.13: E0120 "macro expansion too large", reported once
// spec FTEF-14.28: an expansion that grows past the bound stops there
#[test]
fn an_expansion_past_the_bound_is_reported_once() {
    let _one = super::one_bound_test_at_a_time();
    let mut text = String::from("#define A0 x\n");
    for i in 1..24 {
        text.push_str(&format!("#define A{i} A{p} A{p}\n", p = i - 1));
    }
    text.push_str("A23 A23\n");
    let out = run(text.as_bytes(), Dialect::Fteqcc);
    let codes: Vec<_> = out.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, vec![crate::Code::E0120]);
    let a23 = qc_lexer::TokenKind::Name("A23".to_owned());
    assert!(out.tokens.iter().any(|t| t.kind == a23));
}

// spec FTEF-14.14: an argument beside `##` is copied whole: its later tokens touch the
// joined one, white space after the left one keeps the sides apart, a paste that lexes as a
// comment writes nothing, and `#` `#` apart is no paste
#[test]
fn an_argument_beside_a_paste_is_copied_whole() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define P(a, b) a##b\nP(x, y:z)"), "xy:z");
    assert_eq!(fte("#define P(a, b) a##b\nP(x , y)"), "x y");
    assert_eq!(fte("#define P(a, b) a##b\nP(x/**/, y)"), "x y");
    assert_eq!(fte("#define P(a, b) a##b\nx = P(/, /) 5;"), "x = 5;");
    assert_eq!(fte("#define P(a, b) a # # b\nP(x, y)"), "x ## y");
    assert_eq!(fte("#define P(a, b) a#/**/#b\nP(x, y)"), "xy");
}

// spec FTEF-14.31: only a named parameter is turned into a string, so `#__VA_ARGS__` stays
// as written; the argument's bytes are kept as they are, and its text is still checked when
// the body also writes it as code
#[test]
fn a_string_keeps_the_argument_s_bytes() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define V(...) #__VA_ARGS__\nV(ab)"), "#__VA_ARGS__");
    let out = run(b"#define S(a) #a\nS(a\x80b)", Dialect::Fteqcc);
    assert_eq!(
        out.tokens[0].kind,
        qc_lexer::TokenKind::String(b"a\x80b".to_vec())
    );
    assert_eq!(out.tokens[0].spelling.as_deref(), Some("\"a\\x80b\""));
    let out = run("#define S(a) #a\nS(\u{e9})".as_bytes(), Dialect::Fteqcc);
    assert_eq!(
        out.tokens[0].kind,
        qc_lexer::TokenKind::String("\u{e9}".as_bytes().to_vec())
    );
    // The argument `"ab` and its line break: written as code, its last token is the string,
    // with the line break after it dropped, so the expansion ends inside it.
    let (_, codes) = fte_with_codes("#define S(a) #a a\nS(\"ab\n)");
    assert!(codes.contains(&crate::Code::E0101), "{codes:?}");
    let (_, codes) = fte_with_codes("#define S(a) #a\nS(\"ab\n)");
    assert!(
        !codes.contains(&crate::Code::E0101) && !codes.contains(&crate::Code::E0102),
        "{codes:?}"
    );
}

// spec FTEF-14.20: a built-in constant in an argument not beside `##` is written as its
// value in the macro's chunk, so it touches the tokens beside it; beside `##` it stays a name
// spec FTE-23.10: a `#` before a name a paste made reads it as a macro use
#[test]
fn a_built_in_constant_in_an_argument_is_its_value() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define K(x) x\nK(__LINE__:)"), "2:");
    assert_eq!(fte("#define K(x) x\nK(#__LINE__)"), "#2");
    assert_eq!(fte("#define K(x) x##_a\nK(__LINE__)"), "__LINE___a");
    assert_eq!(
        fte("#define N1 5\n#define P(x, a, b) x##a##b\nP(#, N, 1)"),
        "5"
    );
    // A name a paste made in an outer expansion is text in the argument like any other.
    assert_eq!(
        fte("#define K(x) x:\n#define O(x) K(__LI##x)\nO(NE__)"),
        "3:"
    );
}

// spec FTEF-14.31: a string skips the white space and both kinds of comment before the
// argument, keeps a line break as an escape, and an argument from another expansion is
// written as its tokens touch
#[test]
fn a_string_skips_what_lex_whitespace_skips() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define S(a) #a\nS(/* c */ex)"), "\"ex\"");
    assert_eq!(fte("#define S(a) #a\nS(// c\nex)"), "\"ex\"");
    assert_eq!(fte("#define S(a) #a\nS(/**/ex/**/)"), "\"ex/**/\"");
    let out = run(b"#define S(a) #a\nS(a\nb)", Dialect::Fteqcc);
    assert_eq!(out.tokens[0].spelling.as_deref(), Some("\"a\\nb\""));
    assert_eq!(
        fte("#define S(a) #a\n#define T(x) S(x+y z)\nT(1)"),
        "\"1+y z\""
    );
}

// spec FTEF-14.17: a function-like macro used again through another is expanded while at
// most 8 chunks of it are open, so 9 times
#[test]
fn a_function_like_macro_opens_nine_chunks() {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", &b"#define F(x) G(x)\n#define G(x) F(x)\nF(1)\n"[..]);
    let mut pp = Preprocessor::new(loader).with_dialect(Dialect::Fteqcc);
    pp.preprocess_file(Path::new("a.qc")).unwrap();
    let f = pp.expansions().iter().filter(|e| e.name == "F").count();
    assert_eq!(f, 9);
}

// spec FTEF-14.29: a function-like macro's name and `(` in one expansion open its arguments
// whatever spacing the stored body has
#[test]
fn a_call_written_in_a_body_expands() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define M(x) x\n#define K M (1)\nK"), "1");
    assert_eq!(fte("#define M(x) x\n#define K M/**/(1)\nK"), "1");
}

// spec FTEF-14.28: the bound on tokens holds even when the expansions stay under theirs
#[test]
fn an_expansion_writing_too_many_tokens_is_reported() {
    let _one = super::one_bound_test_at_a_time();
    let mut text = format!("#define A0 {}\n", "x ".repeat(100));
    for i in 1..17 {
        text.push_str(&format!("#define A{i} A{p} A{p}\n", p = i - 1));
    }
    text.push_str("A16\n");
    let out = run(text.as_bytes(), Dialect::Fteqcc);
    let codes: Vec<_> = out.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, vec![crate::Code::E0120]);
}

// spec FTEF-14.14: an empty argument after `##` joins nothing, a paste that makes a comment
// ends the chunk, and a pasted text is lexed again into its own tokens
#[test]
fn a_paste_is_text_lexed_again() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define P(a, b) a##b c\nP(x, )"), "x c");
    assert_eq!(fte("#define P(a, b, c) a##b##c\nP(x, , y)"), "xy");
    assert_eq!(fte("#define P(a, b) a##b z\nx = P(/, /) 5;"), "x = 5;");
    let out = run(b"#define P(a, b) a##b\nP(+, +=)", Dialect::Fteqcc);
    let kinds: Vec<_> = out.tokens.iter().map(|t| t.kind.clone()).collect();
    let relexed: Vec<_> = qc_lexer::lex_with(b"++=", Dialect::Fteqcc)
        .tokens
        .into_iter()
        .map(|t| t.kind)
        .collect();
    assert_eq!(kinds, relexed);
}

// spec FTEF-14.31: a string made from an argument of another expansion keeps its bytes, and
// a carriage return is escaped like a line break
#[test]
fn a_nested_string_keeps_its_bytes() {
    let out = run(
        b"#define S(a) #a\n#define T(x) S(x)\nT(\"\x80\")",
        Dialect::Fteqcc,
    );
    assert_eq!(
        out.tokens[0].kind,
        qc_lexer::TokenKind::String(b"\"\x80\"".to_vec())
    );
    let out = run(b"#define S(a) #a\nS(a\r\nb)", Dialect::Fteqcc);
    assert_eq!(out.tokens[0].spelling.as_deref(), Some("\"a\\r\\nb\""));
}

// spec FTEF-14.29: in an expansion a `(` opens a call only after spaces and tabs, as the
// text of the argument it came from has them
#[test]
fn a_call_in_an_argument_needs_its_parenthesis_on_the_line() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    assert_eq!(fte("#define M(x) x\n#define K(y) y\nK(M\n(1))"), "M (1)");
    assert_eq!(fte("#define M(x) x\n#define K(y) y\nK(M/**/(1))"), "M (1)");
    assert_eq!(fte("#define M(x) x\n#define K(y) y\nK(M (1))"), "1");
}
