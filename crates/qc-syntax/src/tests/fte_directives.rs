// SPDX-License-Identifier: GPL-3.0-or-later

//! The directive nodes of the FTEQCC profile end where the preprocessor ends the directive
//! (`docs/spec/dialect-fteqcc.md`, FTEF-14.51), and the vanilla dialect keeps its own (QC-9.1).

use std::path::Path;

use proptest::prelude::*;
use qc_preproc::{MemoryLoader, Preprocessor};

use crate::{Dialect, Span, SyntaxKind, SyntaxNode, parse_preprocessed, parse_with};

/// Whether `token`'s node, or a node above it, is a directive.
fn in_directive(node: &SyntaxNode) -> bool {
    node.ancestors()
        .chain(std::iter::once(node.clone()))
        .any(|n| n.kind() == SyntaxKind::Directive)
}

/// The text of each directive node of `source` parsed in `dialect`, in order.
fn directives(source: &str, dialect: Dialect) -> Vec<String> {
    let parsed = parse_with(source.as_bytes(), dialect);
    assert_eq!(parsed.syntax().text(), source.as_bytes());
    parsed
        .syntax()
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::Directive)
        .map(|n| String::from_utf8(n.text()).unwrap())
        .collect()
}

fn fte(source: &str) -> Vec<String> {
    directives(source, Dialect::Fteqcc)
}

fn vanilla(source: &str) -> Vec<String> {
    directives(source, Dialect::Vanilla)
}

/// The spans of the tokens the parser reads, outside directive nodes, of `source` parsed
/// under FTEQCC.
fn tree_code(source: &str) -> Vec<Span> {
    let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
    parsed
        .syntax()
        .tokens()
        .filter(|t| !t.kind().is_trivia() && !in_directive(&t.parent()))
        .map(|t| t.span())
        .collect()
}

/// The spans of the tokens the FTEQCC preprocessor gives for `source`; `None` when it uses a
/// macro, whose expansion the tree, which runs no directive, does not write.
fn preprocessed_code(source: &str) -> Option<Vec<Span>> {
    let mut loader = MemoryLoader::new();
    loader.insert("a.qc", source.as_bytes());
    let mut pp = Preprocessor::new(loader).with_dialect(Dialect::Fteqcc);
    let out = pp.preprocess_file(Path::new("a.qc")).unwrap();
    out.tokens
        .iter()
        .map(|t| t.expansion.is_none().then_some(t.span.span))
        .collect()
}

/// Checks that the tree reads as code exactly the tokens the preprocessor gives for each of
/// `sources`, files of no macro use.
fn assert_same_code(sources: &[&str]) {
    for source in sources {
        let preprocessed = preprocessed_code(source);
        assert!(preprocessed.is_some(), "{source:?} uses a macro");
        assert_eq!(Some(tree_code(source)), preprocessed, "{source:?}");
    }
}

// spec FTEF-14.51: a directive node starts at a `#` `QCC_PR_NewLine` reaches (FTEF-14.52):
// after a line ending and spaces and tabs, at the file's start after them, and with white
// space between the `#` and the name
#[test]
fn a_directive_starts_where_new_line_reads_a_hash() {
    let cases = [
        "  #define A 1\nfloat a;\n",
        "float a;\n \t#define B 2\nfloat b;\n",
        "# define E\n#\tdefine F\nfloat e;\n",
        "float a; // c\n#define G\nfloat g;\n",
        "float a;\r#define H\nfloat h;\n",
    ];
    assert_eq!(fte(cases[0]), ["  #define A 1".trim_start()]);
    assert_eq!(fte(cases[1]), ["#define B 2"]);
    assert_eq!(fte(cases[2]), ["# define E", "#\tdefine F"]);
    assert_eq!(fte(cases[3]), ["#define G"]);
    assert_eq!(fte(cases[4]), ["#define H"]);
    assert_same_code(&cases);
}

// spec FTEF-14.51: a `#` after a `/* */` comment on its line, in the middle of a line, or
// before a name that matches no directive is read as code (FTEF-14.52, FTE-23.10)
#[test]
fn a_hash_new_line_does_not_reach_is_code() {
    let cases = [
        "/* c */ #define C 3\nfloat c;\n",
        "/* a\nb */ #define C 3\nfloat c;\n",
        "float a; #define D\nfloat d;\n",
    ];
    for source in cases {
        assert_eq!(fte(source), Vec::<String>::new(), "{source:?}");
    }
    assert_same_code(&cases);
    assert_eq!(fte("#foo\nfloat f;\n"), Vec::<String>::new());
    assert_eq!(vanilla("#foo\nfloat f;\n"), ["#foo"]);
}

// spec FTEF-14.53: a `#` on a line a `\` joins to the line of code before it starts no
// directive node; after a `//` comment's `\`, a blank line, or a directive whose text holds the
// `\`, it does, as the preprocessor reads it
#[test]
fn a_joined_line_starts_no_directive_node() {
    let cases = [
        "float a; \\\n#define C 3\nfloat c;\n",
        "float a; \\\r\n  #undef C\nfloat c;\n",
        "float a; // c \\\n#define C 3\nfloat c;\n",
        "float a; \\\n\n#define C 3\nfloat c;\n",
        "#pragma p \\\n#define C 3\nfloat c;\n",
    ];
    assert_eq!(fte(cases[0]), Vec::<String>::new());
    assert_eq!(fte(cases[1]), Vec::<String>::new());
    assert_eq!(fte(cases[2]), ["#define C 3"]);
    assert_eq!(fte(cases[3]), ["#define C 3"]);
    assert_eq!(fte(cases[4]), ["#pragma p \\", "#define C 3"]);
    assert_same_code(&cases);
}

// spec FTEF-14.51: a directive's name is matched by its first bytes and a `#` alone on its
// line is a directive, so each is one node (FTEF-14.52)
#[test]
fn names_matched_by_their_first_bytes_are_directives() {
    let cases = [
        "#defined G 1\n#undefine N\n#endiff\nfloat g;\n",
        "#\nfloat h;\n",
        "#pragmas x\nfloat p;\n",
    ];
    assert_eq!(fte(cases[0]), ["#defined G 1", "#undefine N", "#endiff"]);
    assert_eq!(fte(cases[1]), ["#"]);
    assert_eq!(fte(cases[2]), ["#pragmas x"]);
    assert_same_code(&cases);
}

// spec FTEF-14.51: a directive's line ends where `QCC_PR_SkipToEndOfLine` ends it (FTEF-14.35):
// on past a `\` and a line break, in a `//` comment too, and with CR LF
#[test]
fn a_continued_line_is_part_of_the_directive() {
    let cases = [
        "#undef X // c \\\nfloat y;\nfloat z;\n",
        "#ifndef X // c \\\nfloat y;\n#endif\nfloat z;\n",
        "#undef X \\\r\nfloat y;\r\nfloat z;\n",
        "#if 1 // c \\\nfloat y;\n#endif\nfloat z;\n",
        "#error e // c \\\nfloat y;\nfloat z;\n",
    ];
    assert_eq!(fte(cases[0]), ["#undef X // c \\\nfloat y;"]);
    assert_eq!(fte(cases[1]), ["#ifndef X // c \\\nfloat y;", "#endif"]);
    assert_eq!(fte(cases[2]), ["#undef X \\\r\nfloat y;\r"]);
    assert_eq!(fte(cases[3]), ["#if 1 // c \\\nfloat y;", "#endif"]);
    assert_eq!(fte(cases[4]), ["#error e // c \\\nfloat y;"]);
    assert_same_code(&cases);
}

// spec FTEF-14.51: the vanilla dialect keeps its node, a `#` and a name to the first line
// break (QC-9.1)
#[test]
fn the_vanilla_dialect_ends_a_directive_at_its_line_break() {
    assert_eq!(vanilla("#undef X // c \\\nfloat y;\n"), ["#undef X"]);
    assert_eq!(
        vanilla("#define M a /* \n */ b\nfloat z;\n"),
        ["#define M a"]
    );
    assert_eq!(
        vanilla("  #define A 1\n/* c */ #define C\n"),
        ["#define A 1"]
    );
}

// spec FTEF-14.51: a directive's line runs over a `/* */` comment that spans lines, and a
// string before any `//` is passed over, so a `/*` in it opens no comment (FTEF-14.35)
#[test]
fn comments_and_strings_on_a_directive_line() {
    let cases = [
        "#undef X /* a\n b */ float y;\nfloat z;\n",
        "#undef X \"/*\"\nfloat y;\n/* */\n",
        "#ifndef X Y\nfloat z;\n#endif\n",
    ];
    assert_eq!(fte(cases[0]), ["#undef X /* a\n b */ float y;"]);
    assert_eq!(fte(cases[1]), ["#undef X \"/*\""]);
    assert_eq!(fte(cases[2]), ["#ifndef X Y", "#endif"]);
    assert_same_code(&cases);
}

// spec FTEF-14.51: an `#elif` ends where its test stops reading, as the preprocessor reads one
// after a side it skipped, and its line goes on as `QCC_PR_SkipToEndOfLine` reads it
#[test]
fn an_elif_ends_after_its_test() {
    let cases = [
        "#if 0\n#elif (1/*)\n*/ float y;\n#endif\n",
        "#if 0\n#elifndef Q // \\\nfloat y;\n#endif\nfloat z;\n",
    ];
    assert_eq!(fte(cases[0]), ["#if 0", "#elif (1/*)", "#endif"]);
    assert_eq!(
        fte(cases[1]),
        ["#if 0", "#elifndef Q // \\\nfloat y;", "#endif"]
    );
    assert_same_code(&cases);
}

// spec FTEF-14.51: the preprocessor reads a directive's text as bytes, so a lexical error in
// it is none, and one after it is the new lexing's
#[test]
fn a_directive_s_text_has_no_lexical_error() {
    for source in [
        "#define X \"a\nfloat y;\n",
        "#pragma x \"abc\nfloat y;\n",
        "#if 1\"a\nfloat y;\n#endif\n",
    ] {
        let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
        assert_eq!(super::errors(&parsed), vec![], "{source:?}");
    }
    // The first lexing's string `"a b` and its error stand across the node's end at 5, and the
    // new lexing's `"c` is unclosed.
    let parsed = parse_with(b"#if\"a b\"c\n", Dialect::Fteqcc);
    let lexical: Vec<_> = parsed
        .diagnostics()
        .iter()
        .filter(|d| d.code == crate::Code::E0102)
        .map(|d| d.span.start)
        .collect();
    assert_eq!(lexical, [7]);
}

// spec FTEF-14.51: a node ends after the operand the directive reads: a `#define`'s body,
// which ends at a `//` and goes on past a `/* */` comment and a `\` (FTEF-14.24), and an `#if`
// expression (FTEF-14.33)
#[test]
fn a_directive_ends_after_its_operand() {
    let cases = [
        "#define M 1 // c \\\nfloat y;\nfloat z;\n",
        "#define M a /* \n */ b\nfloat z;\n",
        "#define M(a, b) a \\\n b\nfloat z;\n",
        "#if 1 \\\n|| 0\nfloat z;\n#endif\n",
        "#if 1 /* a\nb */\nfloat z;\n#endif\n",
    ];
    assert_eq!(fte(cases[0]), ["#define M 1 // c \\\nfloat y;"]);
    assert_eq!(fte(cases[1]), ["#define M a /* \n */ b"]);
    assert_eq!(fte(cases[2]), ["#define M(a, b) a \\\n b"]);
    assert_eq!(fte(cases[3]), ["#if 1 \\\n|| 0", "#endif"]);
    assert_eq!(fte(cases[4]), ["#if 1 /* a\nb */", "#endif"]);
    assert_same_code(&cases);
    // An expression FTEQCC refuses ends where its reader stopped; the preprocessor then skips
    // the side, which the tree parses.
    let refused = "#if (1 \\\n|| 0)\nfloat z;\n#endif\n";
    assert_eq!(fte(refused), ["#if (1 \\\n|| 0)", "#endif"]);
    // One nested past the limit of FTEF-14.12 leaves reading at the expression's start, so the
    // `/*` of the word `x/*` opens a comment.
    let deep = format!("#if 1 == x/*{}\n*/\nfloat z;\n#endif\n", " == 1".repeat(70));
    let line = deep.split("\nfloat").next().unwrap();
    assert_eq!(fte(&deep), [line, "#endif"]);
}

// spec FTEF-14.51: a `#pragma` ends at its line break (FTEF-14.43), an `#if` with no white
// space after its name after its word (FTEF-14.36), and an `#eof` at its line break
#[test]
fn pragmas_words_and_eof_end_early() {
    let cases = ["#pragma p // c \\\nfloat y;\n", "#if(1)\nfloat z;\n"];
    assert_eq!(fte(cases[0]), ["#pragma p // c \\"]);
    assert_eq!(fte(cases[1]), ["#if"]);
    assert_same_code(&cases);
    assert_eq!(fte("#eof // c \\\nfloat z;\n"), ["#eof // c \\"]);
    let parsed = parse_with(b"#eof\nfloat z;\n", Dialect::Fteqcc);
    assert_eq!(tree_code("#eof\nfloat z;\n").len(), 3);
    assert!(parsed.syntax().text().ends_with(b"float z;\n"));
}

// spec FTEF-14.51: where a token or comment of the file as first lexed stands across a
// directive's end, the text from there is lexed again, as the preprocessor reads on there
#[test]
fn text_across_a_directive_end_is_lexed_again() {
    let cases = [
        "#if\"a b\"\n",
        "#undef X // a \\\nb /* c\nd */ e;\n",
        "#pragma p \"a\\\nb\" c;\n",
        "#if/* a */ b;\n",
    ];
    assert_eq!(fte(cases[0]), ["#if\"a"]);
    assert_eq!(fte(cases[1]), ["#undef X // a \\\nb /* c"]);
    assert_eq!(fte(cases[2]), ["#pragma p \"a\\"]);
    assert_eq!(fte(cases[3]), ["#if/*"]);
    assert_same_code(&cases);
    let parsed = parse_with(cases[1].as_bytes(), Dialect::Fteqcc);
    let names: Vec<_> = parsed
        .syntax()
        .tokens()
        .filter(|t| t.kind() == SyntaxKind::Name && !in_directive(&t.parent()))
        .map(|t| t.text().to_vec())
        .collect();
    assert_eq!(names, [b"d".to_vec(), b"e".to_vec()]);
}

/// A line of a file of directives and code with no macro use and no side a conditional skips.
fn line() -> impl Strategy<Value = &'static str> {
    prop_oneof![
        Just("float y;\n"),
        Just("z = \"a\\\nb\";\n"),
        Just("/* c */ #define C 1\n"),
        Just("/* a\n*/\n"),
        Just(" \t#undef X // c \\\n"),
        Just("#undef X /* a\n"),
        Just("b */ float y;\n"),
        Just("#define M 1 // c \\\n"),
        Just("#define M(a) a /* \n */ b \\\n"),
        Just("#ifndef Q Y\n"),
        Just("#if 1 \\\n"),
        Just("#endif // \\\r\n"),
        Just("#pragma p \\\n"),
        Just("#error \"e\n"),
        Just("#\n"),
        Just("#iffy\n"),
        Just("#if\"a b\";\n"),
        Just("x #define D\n"),
        Just("\\\n"),
    ]
}

proptest! {
    // spec FTEF-14.51: over files of directive lines and code, the tree reads as code exactly
    // the tokens the preprocessor gives
    #[test]
    fn the_tree_reads_as_code_what_the_preprocessor_gives(
        lines in proptest::collection::vec(line(), 0..24)
    ) {
        let source = lines.concat();
        // A `\` line before a `#define M 1` line joins it to the code (FTEF-14.53), where a
        // macro defined before it is used.
        let preprocessed = preprocessed_code(&source);
        prop_assume!(preprocessed.is_some());
        prop_assert_eq!(Some(tree_code(&source)), preprocessed);
    }
}

// spec FTEF-14.51: the FTEQCC nodes are a source file's; a rendering of the preprocessor's
// output has none, and GMQCC reads a file as FTEQCC does
#[test]
fn only_a_source_file_has_fteqcc_nodes() {
    let source = "#undef X // c \\\nfloat y;\n";
    let rendered = parse_preprocessed(source.as_bytes(), Dialect::Fteqcc);
    assert!(
        !rendered
            .syntax()
            .descendants()
            .any(|n| n.kind() == SyntaxKind::Directive)
    );
    assert_eq!(
        directives(source, Dialect::Gmqcc),
        ["#undef X // c \\\nfloat y;"]
    );
}
