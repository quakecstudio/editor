// SPDX-License-Identifier: GPL-3.0-or-later

//! What touches what in an expansion (`docs/spec/language.md` QC-9.15).

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

/// The tokens of `text` under `dialect`, each pair that touches (QC-9.15) written together
/// and the rest apart.
fn touching(text: &str, dialect: Dialect) -> String {
    let out = run(text.as_bytes(), dialect);
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

// spec QC-9.15: under FTEQCC an argument's tokens touch the body tokens the parameter
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

// spec QC-9.15: under FTEQCC two body tokens with only `/* */` comments and `\` line breaks
// between them touch, as `QCC_PR_PreProcessor_Define` drops those; the vanilla dialect keeps
// them apart, and keeps an argument apart from the body
#[test]
fn a_body_touches_where_fteqcc_stored_it_touching() {
    let fte = |text: &str| touching(text, Dialect::Fteqcc);
    let vanilla = |text: &str| touching(text, Dialect::Vanilla);
    assert_eq!(fte("#define L lbl/**/:\nL"), "lbl:");
    assert_eq!(fte("#define L lbl/* a */ /**/:\nL"), "lbl :");
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
