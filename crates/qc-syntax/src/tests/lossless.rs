// SPDX-License-Identifier: GPL-3.0-or-later

//! Every byte is in the tree once, in order, and trivia is classified.

use proptest::prelude::*;

use super::{SAMPLE, SAMPLE_FTEQCC};
use crate::ast::AstNode as _;
use crate::{Code, Dialect, SyntaxKind, parse, parse_with};

/// Checks that the tokens of `root` tile `source` exactly and give it back.
fn assert_lossless(source: &[u8]) {
    assert_lossless_in(source, Dialect::Vanilla);
}

/// [`assert_lossless`] with `source` parsed in `dialect`, and every diagnostic inside it.
fn assert_lossless_in(source: &[u8], dialect: Dialect) {
    let parsed = parse_with(source, dialect);
    for d in parsed.diagnostics() {
        assert!(
            d.span.start <= d.span.end && d.span.end <= source.len(),
            "{d:?}"
        );
    }
    let root = parsed.syntax();
    assert_eq!(root.kind(), SyntaxKind::SourceFile);
    assert_eq!(root.text(), source);
    assert_eq!(root.span().start, 0);
    assert_eq!(root.span().end, source.len());
    let mut at = 0;
    for token in root.tokens() {
        assert_eq!(token.span().start, at, "{token:?}");
        assert_eq!(source.get(token.span().range()), Some(token.text()));
        at = token.span().end;
    }
    assert_eq!(at, source.len());
    for node in root.descendants() {
        for child in node.children() {
            assert!(node.span().contains(child.span()));
            assert_eq!(child.parent().as_ref(), Some(&node));
        }
    }
}

/// The trivia tokens of a parse, as `(kind, text)`.
fn trivia(source: &str) -> Vec<(SyntaxKind, String)> {
    parse(source.as_bytes())
        .syntax()
        .tokens()
        .filter(|t| t.kind().is_trivia())
        .map(|t| (t.kind(), t.text_lossless()))
        .collect()
}

#[test]
fn empty_file() {
    let parsed = parse(b"");
    assert_eq!(parsed.syntax().debug_tree(), "SourceFile@0..0\n");
    assert!(parsed.diagnostics().is_empty());
}

#[test]
fn sample_is_lossless() {
    assert_lossless(SAMPLE.as_bytes());
}

#[test]
fn fteqcc_sample_is_lossless() {
    assert_lossless_in(SAMPLE_FTEQCC.as_bytes(), Dialect::Fteqcc);
    assert_lossless_in(SAMPLE_FTEQCC.as_bytes(), Dialect::Vanilla);
}

#[test]
fn comments_and_whitespace_are_trivia() {
    assert_eq!(
        trivia("// a\nfloat /*b*/ x;\t\r\n/* c */"),
        vec![
            (SyntaxKind::LineComment, "// a".to_owned()),
            (SyntaxKind::Whitespace, "\n".to_owned()),
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::BlockComment, "/*b*/".to_owned()),
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Whitespace, "\t\r\n".to_owned()),
            (SyntaxKind::BlockComment, "/* c */".to_owned()),
        ]
    );
}

// spec QC-2.3: the search for `*/` starts at the `*` of `/*`, so `/*/` is a whole comment
#[test]
fn slash_star_slash_is_a_comment() {
    assert_eq!(
        trivia("/*/ float x;").first(),
        Some(&(SyntaxKind::BlockComment, "/*/".to_owned()))
    );
}

// spec QC-2.4: an unterminated comment runs to the end of the file; it is kept as trivia
#[test]
fn unterminated_comment_is_trivia() {
    let parsed = parse(b"float a;\n/* abc");
    assert_eq!(
        trivia("float a;\n/* abc").last(),
        Some(&(SyntaxKind::BlockComment, "/* abc".to_owned()))
    );
    assert_eq!(parsed.errors().count(), 1);
}

// spec QC-1.3: bytes 0x80 to 0xFF outside a string are whitespace
#[test]
fn high_bytes_are_whitespace() {
    let source = b"float\xa0a;";
    assert_lossless(source);
    let parsed = parse(source);
    assert_eq!(parsed.errors().count(), 0);
    let ws: Vec<_> = parsed
        .syntax()
        .tokens()
        .filter(|t| t.kind() == SyntaxKind::Whitespace)
        .map(|t| t.text().to_vec())
        .collect();
    assert_eq!(ws, vec![b"\xa0".to_vec()]);
}

// spec QC-1.1: the text ends at the first NUL byte; what follows is kept as skipped bytes
#[test]
fn text_after_nul_is_skipped() {
    let source = b"float a;\0float b junk";
    assert_lossless(source);
    let parsed = parse(source);
    assert_eq!(parsed.errors().count(), 0);
    let tokens: Vec<_> = parsed.syntax().tokens().collect();
    let last = tokens.last().unwrap();
    assert_eq!(last.kind(), SyntaxKind::Skipped);
    assert_eq!(last.text(), b"\0float b junk");
}

// spec QC-8.4, QC-8.2: ignored `$` commands are skipped with their line; `$frame` lines are
// kept
#[test]
fn dollar_commands_are_trivia() {
    let source = "$cd /a/b, c;\n$frame s1 s2\nfloat x = $s2;\n";
    assert_eq!(
        trivia(source),
        vec![
            (SyntaxKind::Skipped, "$cd /a/b, c;".to_owned()),
            (SyntaxKind::Whitespace, "\n".to_owned()),
            (SyntaxKind::Frames, "$frame s1 s2".to_owned()),
            (SyntaxKind::Whitespace, "\n".to_owned()),
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Whitespace, "\n".to_owned()),
        ]
    );
    assert_eq!(parse(source.as_bytes()).errors().count(), 0);
}

// spec QC-7.2: an unknown byte is an error; the tree keeps it as skipped bytes
#[test]
fn unknown_bytes_are_skipped() {
    let parsed = parse(b"float a;\nfloat b % c;\nfloat d;");
    assert!(
        parsed
            .syntax()
            .tokens()
            .any(|t| t.kind() == SyntaxKind::Skipped && t.text() == b"%")
    );
    assert_lossless(b"%%%`~?@\\");
}

// spec QC-9.1: a line whose first token is `#` and a name is a directive, kept whole as trivia
#[test]
fn directives_are_nodes_the_parser_steps_over() {
    let source = "#define X 1 // one\nfloat a;\nvoid() f = {\n#ifdef X\n\ta = 1;\n#endif\n};\n";
    let parsed = parse(source.as_bytes());
    assert_eq!(super::errors(&parsed), vec![]);
    let directives: Vec<String> = parsed
        .tree()
        .unwrap()
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::Directive)
        .map(|n| String::from_utf8(n.text()).unwrap())
        .collect();
    assert_eq!(directives, vec!["#define X 1", "#ifdef X", "#endif"]);
    assert_eq!(parsed.tree().unwrap().directives().count(), 1);
}

// spec QC-9.1: `#` followed by a number is the builtin marker, even first on its line
#[test]
fn hash_number_is_not_a_directive() {
    let parsed = parse(b"void() f =\n#1;\n");
    assert_eq!(super::errors(&parsed), vec![]);
    assert!(!super::kinds(&parsed).contains(&SyntaxKind::Directive));
    assert!(super::kinds(&parsed).contains(&SyntaxKind::Builtin));
}

// spec QC-9.19: text a preprocessor wrote has no directive lines, so a `#` and a name first on
// a line are the tokens the parser reads, and the tree still gives the text back
#[test]
fn preprocessed_text_has_no_directive_lines() {
    for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
        let source = b"#foo\nfloat a;\n";
        let parsed = crate::parse_preprocessed(source, dialect);
        assert_eq!(
            super::errors(&parsed),
            vec![(Code::E0203, "\"#\" is not a type".to_owned())],
            "{dialect:?}"
        );
        assert!(!super::kinds(&parsed).contains(&SyntaxKind::Directive));
        assert_eq!(parsed.syntax().text(), source);
    }
    // In a source file the vanilla dialect's `#foo` is a directive line (QC-9.1), and FTEQCC's
    // names no directive, so it is code there too (FTEF-14.51).
    let source = b"#foo\nfloat a;\n";
    assert_eq!(super::errors(&parse_with(source, Dialect::Vanilla)), vec![]);
    assert_eq!(
        super::errors(&parse_with(source, Dialect::Fteqcc)),
        super::errors(&crate::parse_preprocessed(source, Dialect::Fteqcc))
    );
}

#[test]
fn directive_at_end_of_file() {
    assert_lossless(b"#define X");
    assert_lossless(b"#define X \"a\n");
    assert_lossless(b"#define X 'a\n1 2 3'\nfloat a;");
}

#[test]
fn debug_tree_format() {
    assert_eq!(
        parse(b"float a;").syntax().debug_tree(),
        "SourceFile@0..8\n  Declaration@0..8\n    SimpleType@0..5\n      FloatKw@0..5 \"float\"\n    \
         Whitespace@5..6 \" \"\n    Declarator@6..7\n      Name@6..7 \"a\"\n    Semicolon@7..8 \";\"\n"
    );
}

/// Tokens of vanilla QuakeC and of other dialects, for generated sources.
fn fragment() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("float".to_owned()),
        Just("void".to_owned()),
        Just("vector".to_owned()),
        Just("entity".to_owned()),
        Just("string".to_owned()),
        Just("local".to_owned()),
        Just("if".to_owned()),
        Just("else".to_owned()),
        Just("while".to_owned()),
        Just("do".to_owned()),
        Just("return".to_owned()),
        Just("for".to_owned()),
        "[a-z_][a-z0-9_]{0,6}",
        "-?[0-9]{1,3}(\\.[0-9]{1,2})?",
        Just("'1 2 3'".to_owned()),
        Just("\"s\"".to_owned()),
        Just("$frame a b\n".to_owned()),
        Just("$a".to_owned()),
        Just("#define X 1\n".to_owned()),
        Just("// c\n".to_owned()),
        Just("/* c */".to_owned()),
        "[-+*/&|!=<>;,.(){}\\[\\]#:?%]{1,2}",
        Just("...".to_owned()),
        Just("\n".to_owned()),
    ]
}

/// Tokens and shapes of the FTEQCC dialect (`dialect-fteqcc-rerelease.md`,
/// `dialect-fteqcc.md`), with the vanilla ones, for generated sources.
fn fteqcc_fragment() -> impl Strategy<Value = String> {
    let fteqcc = prop_oneof![
        prop_oneof![
            Just("++"),
            Just("--"),
            Just("+="),
            Just("-="),
            Just("*="),
            Just("/="),
            Just("%"),
            Just("?"),
            Just(":"),
            Just("::"),
            Just("->"),
            Just(".."),
            Just("~"),
            Just("^"),
            Just("<<"),
            Just(">>="),
            Just("&~="),
            Just("(+)"),
            Just("(-)"),
        ],
        prop_oneof![
            Just("const"),
            Just("var"),
            Just("nosave"),
            Just("static"),
            Just("int"),
            Just("unsigned"),
            Just("switch (a) { case 1: break; default: }"),
            Just("for (int i = 0; i < 3; i++)"),
            Just("typedef struct { float x; } T;"),
            Just("struct s { float a; };"),
            Just("enum { A, B = 2 };"),
            Just("class C : D { float x; };"),
            Just("accessor h : float { get float v = f; };"),
            Just("struct S { public: float a; private: float b; };"),
            Just("struct S : P { struct { float a; }; union { float b; vector v; }; };"),
            Just("struct S { enum E { A } e; struct T { float a; } *t; float c : 3; };"),
            Just("class C; class C x; class C { float a; } *p;"),
            Just("class C { protected: : float a; private: void() m = {}; };"),
            Just("typedef float t, *u, v[2];"),
            Just("typedef typedef struct S { float a; } T, *PT;"),
            Just("void() g = { struct { float a; } x; (struct S *)0; };"),
            Just("accessor A : float; typedef accessor B : float;"),
            Just("sizeof(int)"),
            Just("(float)"),
            Just("[1, 2, 3]"),
            Just("[++ 1 .. 4]"),
            Just("goto l;"),
            Just("l:"),
            Just("__until"),
            Just("#0:name"),
        ],
        prop_oneof![
            Just("0x1f"),
            Just("1.0f"),
            Just(".5"),
            Just("'a'"),
            Just("'8 8'"),
            Just("\"\\x41\""),
            Just("#if 1\n"),
            Just("#elif 0\n"),
            Just("#warning w\n"),
            Just("$framevalue 3\n"),
            Just("\\\n"),
        ],
    ];
    prop_oneof![fteqcc.prop_map(str::to_owned), fragment()]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    // The tree gives back any bytes, and parsing always ends.
    #[test]
    fn any_bytes_round_trip(source in proptest::collection::vec(any::<u8>(), 0..512)) {
        assert_lossless(&source);
    }

    // Token soup in the shape of QuakeC round-trips too.
    #[test]
    fn token_soup_round_trips(parts in proptest::collection::vec(fragment(), 0..80)) {
        assert_lossless(parts.join(" ").as_bytes());
    }

    // Parsing the same text twice gives the same tree.
    #[test]
    fn parsing_is_deterministic(parts in proptest::collection::vec(fragment(), 0..40)) {
        let text = parts.join(" ");
        prop_assert_eq!(parse(text.as_bytes()), parse(text.as_bytes()));
    }

    // Under the FTEQCC profile the tree gives back any bytes, and parsing always ends.
    #[test]
    fn fteqcc_any_bytes_round_trip(source in proptest::collection::vec(any::<u8>(), 0..512)) {
        assert_lossless_in(&source, Dialect::Fteqcc);
    }

    // FTEQCC token soup round-trips under the FTEQCC profile, and under vanilla.
    #[test]
    fn fteqcc_token_soup_round_trips(parts in proptest::collection::vec(fteqcc_fragment(), 0..80)) {
        let text = parts.join(" ");
        assert_lossless_in(text.as_bytes(), Dialect::Fteqcc);
        assert_lossless_in(text.as_bytes(), Dialect::Vanilla);
    }

    // Under the FTEQCC profile parsing the same text twice gives the same tree.
    #[test]
    fn fteqcc_parsing_is_deterministic(parts in proptest::collection::vec(fteqcc_fragment(), 0..40)) {
        let text = parts.join(" ");
        prop_assert_eq!(
            parse_with(text.as_bytes(), Dialect::Fteqcc),
            parse_with(text.as_bytes(), Dialect::Fteqcc)
        );
    }
}

// spec QC-7.2: unknown bytes are skipped up to the next whitespace, comment or `$` command
#[test]
fn skipped_bytes_stop_at_comments_and_commands() {
    assert_eq!(
        trivia("float a; %%// c\n"),
        vec![
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Skipped, "%%".to_owned()),
            (SyntaxKind::LineComment, "// c".to_owned()),
            (SyntaxKind::Whitespace, "\n".to_owned()),
        ]
    );
    assert_eq!(
        trivia("float a; %/* c */"),
        vec![
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Skipped, "%".to_owned()),
            (SyntaxKind::BlockComment, "/* c */".to_owned()),
        ]
    );
    assert_eq!(
        trivia("float a; %$cd x\n"),
        vec![
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Whitespace, " ".to_owned()),
            (SyntaxKind::Skipped, "%".to_owned()),
            (SyntaxKind::Skipped, "$cd x".to_owned()),
            (SyntaxKind::Whitespace, "\n".to_owned()),
        ]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    // spec QCD-1.5: no two diagnostics the lexer and parser give for one input have the same
    // code and span, in either profile, over token soup in the shape of QuakeC.
    #[test]
    fn a_syntax_diagnostic_is_reported_once(parts in proptest::collection::vec(fragment(), 0..60)) {
        let source = parts.join(" ");
        for dialect in [Dialect::Vanilla, Dialect::Fteqcc] {
            let parsed = crate::parse_with(source.as_bytes(), dialect);
            let mut seen = std::collections::HashSet::new();
            for d in parsed.diagnostics() {
                prop_assert!(seen.insert((d.code, d.span)), "{dialect:?} {:?}: {d:?} twice", source);
            }
        }
    }
}

// spec QCD-1.5: an accessor member its error stops short is not read again as another member, so
// the place is reported once (the parser fuzz target's find)
#[test]
fn an_accessor_member_error_is_reported_once() {
    let at = |source: &str| -> Vec<(usize, String)> {
        crate::parse_with(source.as_bytes(), Dialect::Fteqcc)
            .diagnostics()
            .iter()
            .map(|d| (d.span.start, d.message.clone()))
            .collect()
    };
    let found = |list: &[(usize, &str)]| -> Vec<(usize, String)> {
        list.iter().map(|&(at, m)| (at, m.to_owned())).collect()
    };
    assert_eq!(
        at("accessor h : float { set v v g; };"),
        found(&[(29, "expected =, found g")])
    );
    assert_eq!(
        at("accessor h : float { set v v g };"),
        found(&[(29, "expected =, found g")])
    );
    assert_eq!(
        at("\naccessor h : float { get float = = f; set v v g;float v = g; };"),
        found(&[
            (34, "\"=\" - not a name"),
            (47, "expected =, found g"),
            (49, "expected }, found float"),
        ])
    );
}
