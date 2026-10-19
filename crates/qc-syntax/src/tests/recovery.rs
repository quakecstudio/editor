// SPDX-License-Identifier: GPL-3.0-or-later

//! Error recovery stays local (LANG-2), including the generated single-token deletions.

use proptest::prelude::*;

use super::{SAMPLE, SAMPLE_FTEQCC, errors};
use crate::recovery::{check_deletions, check_deletions_with, items};
use crate::{Code, Dialect, SyntaxKind, SyntaxNode, parse, parse_with};

/// The names declared at file level, in order.
fn declared(source: &str) -> Vec<String> {
    parse(source.as_bytes())
        .tree()
        .unwrap()
        .declarations()
        .flat_map(|d| d.declarators().collect::<Vec<_>>())
        .filter_map(|d| d.name().map(|n| n.text_lossless()))
        .collect()
}

// spec QCD-1.4: deleting any one token of the sample leaves an error that stays where the
// token was
#[test]
fn every_single_token_deletion_is_local() {
    let report = check_deletions(SAMPLE.as_bytes());
    assert!(report.deletions > 500, "{report:?}");
    assert_eq!(report.failures, Vec::<String>::new());
    assert!(report.silent < report.deletions / 5, "{report:?}");
}

// spec QCD-1.4: a missing `}` ends the body at the next declaration line
#[test]
fn missing_close_brace() {
    let source = "float a;\nvoid() f =\n{\n\ta = 1;\n\nvoid() g = { a = 2; };\nfloat b;\n";
    assert_eq!(
        errors(&parse(source.as_bytes())),
        vec![(Code::E0204, "expected }, found void".to_owned())]
    );
    assert_eq!(declared(source), ["a", "f", "g", "b"]);
}

// spec QCD-1.4: a missing `{` still reads the body that starts on the next line
#[test]
fn missing_open_brace() {
    let source = "float a;\nvoid() f =\n\ta = 1;\n};\nfloat b;\n";
    let parsed = parse(source.as_bytes());
    assert_eq!(
        errors(&parsed),
        vec![(Code::E0204, "expected {, found a".to_owned())]
    );
    assert_eq!(declared(source), ["a", "f", "b"]);
}

// spec QCD-1.4: a missing `;` after a declaration does not swallow the next one
#[test]
fn missing_semicolon_between_declarations() {
    let source = "float a\nfloat b;\nvoid() f = {}\nvoid() g = {};\n";
    assert_eq!(
        errors(&parse(source.as_bytes())),
        vec![
            (Code::E0204, "expected ;, found float".to_owned()),
            (Code::E0204, "expected ;, found void".to_owned()),
        ]
    );
    assert_eq!(declared(source), ["a", "b", "f", "g"]);
}

// spec QCD-1.4: junk at file level is skipped to the end of its declaration
#[test]
fn junk_at_file_level() {
    let source = "x1;\nfoo bar { baz; };\nfloat a;\n}\nfloat b;";
    assert_eq!(
        errors(&parse(source.as_bytes())),
        vec![
            (Code::E0203, "\"x1\" is not a type".to_owned()),
            (Code::E0203, "\"foo\" is not a type".to_owned()),
            (Code::E0203, "\"}\" is not a type".to_owned()),
        ]
    );
    assert_eq!(declared(source), ["a", "b"]);
}

// spec QCD-1.4: errors in separate statements are each reported
#[test]
fn errors_in_each_statement() {
    let source = "float a;\nvoid() f =\n{\n\ta = ;\n\ta = 1 2;\n\twhile a) a = 0;\n};";
    assert_eq!(
        errors(&parse(source.as_bytes())),
        vec![
            (Code::E0201, "not a name".to_owned()),
            (Code::E0204, "expected ;, found 2".to_owned()),
            (Code::E0204, "expected (, found a".to_owned()),
        ]
    );
}

// spec QCD-1.4: a body that runs to the end of the file ends there
#[test]
fn body_runs_to_end_of_file() {
    assert_eq!(
        errors(&parse(b"void() f = {\n\tf();\n")),
        vec![(Code::E0201, "not a name".to_owned())]
    );
    assert_eq!(
        errors(&parse(b"void() f =")),
        vec![(Code::E0204, "expected {, found ".to_owned())]
    );
}

/// A small program from generated statements, for deletion checks.
fn program() -> impl Strategy<Value = String> {
    let stmt = prop_oneof![
        Just("a = b + c * 2;"),
        Just("if (a) b = 1; else b = 2;"),
        Just("while (a < 3) a = a + 1;"),
        Just("do { a = a - 1; } while (a);"),
        Just("local float x = 1;"),
        Just("e.fl = g (a, e.fl);"),
        Just("return;"),
        Just("if (!a && b || e.en == e) { c = 1; }"),
    ];
    proptest::collection::vec(stmt, 1..6).prop_map(|stmts| {
        format!(
            "float a, b, c;\nentity e;\n.float fl;\n.entity en;\nfloat(float x, float y) g;\n\
             void() f =\n{{\n\t{}\n}};\nfloat z;\n",
            stmts.join("\n\t")
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    // Deleting any token of a generated program leaves a local error.
    #[test]
    fn generated_deletions_are_local(source in program()) {
        prop_assert_eq!(parse(source.as_bytes()).errors().count(), 0);
        let report = check_deletions(source.as_bytes());
        prop_assert_eq!(report.failures, Vec::<String>::new());
    }

    // Under the FTEQCC profile deleting any token of a generated program leaves a local
    // error.
    #[test]
    fn generated_fteqcc_deletions_are_local(source in fteqcc_program()) {
        let parsed = parse_with(source.as_bytes(), Dialect::Fteqcc);
        prop_assert_eq!(parsed.errors().count(), 0);
        let report = check_deletions_with(source.as_bytes(), Dialect::Fteqcc);
        prop_assert_eq!(report.failures, Vec::<String>::new());
    }
}

/// A small program from generated FTEQCC statements and declarations, for deletion checks.
fn fteqcc_program() -> impl Strategy<Value = String> {
    let stmt = prop_oneof![
        Just("a += b % 2;"),
        Just("for (int i = 0; i < 3; i++) a++;"),
        Just("switch (a) { case 1: b = 2; break; default: b = 3; }"),
        Just("a = b ? c : 1;"),
        Just("float d = a << 2;"),
        Just("e->fl = [a, b, c].x;"),
        Just("if not (a) b = (float)c;"),
        Just("do a--; __until (a < 0);"),
    ];
    let decl = prop_oneof![
        Just("enum { A, B = 2 };"),
        Just("typedef struct { float x; } point_t;"),
        Just("class C : D { float x; };"),
        Just("const float K = 3;"),
        Just("int n = 0x10;"),
    ];
    (
        proptest::collection::vec(stmt, 1..6),
        proptest::collection::vec(decl, 0..3),
    )
        .prop_map(|(stmts, decls)| {
            format!(
                "float a, b, c;\nentity e;\n.float fl;\n{}\nvoid() f =\n{{\n\t{}\n}};\nfloat z;\n",
                decls.join("\n"),
                stmts.join("\n\t")
            )
        })
}

/// The kinds of the file-level items of `source` under the FTEQCC profile.
fn fte_items(source: &str) -> Vec<SyntaxKind> {
    items(&parse_with(source.as_bytes(), Dialect::Fteqcc).syntax())
        .iter()
        .map(SyntaxNode::kind)
        .collect()
}

// spec FTEF-18.27: a struct, class or accessor body that lost its `}` ends at the next
// declaration line, a declaration's shape or a file-level keyword at column 0
#[test]
fn fteqcc_bodies_end_at_declaration_lines() {
    use SyntaxKind as K;
    for (source, kinds) in [
        (
            "struct s { float a;\nfloat b;\n",
            vec![K::StructDecl, K::Declaration],
        ),
        (
            "typedef struct { float a;\nenum { A };\n",
            vec![K::TypedefDecl, K::EnumDecl],
        ),
        (
            "class C { float a;\naccessor h : float;\n",
            vec![K::ClassDecl, K::AccessorDecl],
        ),
        (
            "accessor h : float { get float v = f;\nclass C;\n",
            vec![K::AccessorDecl, K::ClassDecl],
        ),
        (
            "class C { float a;\ntypedef float T;\nunion u { float f; };\nstruct s;\n",
            vec![K::ClassDecl, K::TypedefDecl, K::StructDecl, K::StructDecl],
        ),
    ] {
        assert_eq!(fte_items(source), kinds, "{source}");
        assert!(
            parse_with(source.as_bytes(), Dialect::Fteqcc)
                .errors()
                .next()
                .is_some(),
            "{source}"
        );
    }
    // Indented, the same lines are fields and members.
    assert_eq!(
        fte_items("struct s {\n\tfloat a;\n\tfloat b;\n};\nclass C {\n\tfloat a;\n};\n"),
        [K::StructDecl, K::ClassDecl]
    );
}

// spec FTEF-18.27: a class member's recovery stops at the `}` that closes the class
#[test]
fn class_member_recovery_keeps_the_class_brace() {
    use SyntaxKind as K;
    let source = "class C { float a, , b };\nfloat c;\n";
    assert_eq!(fte_items(source), [K::ClassDecl, K::Declaration]);
    let class = parse_with(source.as_bytes(), Dialect::Fteqcc)
        .syntax()
        .children()
        .find(|n| n.kind() == K::ClassDecl)
        .unwrap();
    assert!(class.child_tokens().any(|t| t.kind() == K::RBrace));
}

// spec QCD-1.4: under the FTEQCC profile deleting any one token of the FTEQCC sample leaves an
// error that stays where the token was
#[test]
fn every_single_fteqcc_token_deletion_is_local() {
    let report = check_deletions_with(SAMPLE_FTEQCC.as_bytes(), Dialect::Fteqcc);
    assert!(report.deletions > 500, "{report:?}");
    assert_eq!(report.failures, Vec::<String>::new());
}

/// The statement kinds of the body of `f` in `source`.
fn body_kinds(source: &str) -> Vec<crate::SyntaxKind> {
    use crate::ast::AstNode as _;
    super::statements(&parse(source.as_bytes()))
        .iter()
        .map(|s| s.syntax().kind())
        .collect()
}

// spec QCD-1.4: recovery stops at a statement keyword that starts a line
#[test]
fn recovery_stops_at_a_statement_line() {
    use crate::SyntaxKind::{ExprStmt, IfStmt};
    let source = "float a, b;\nvoid() f =\n{\n\ta = 1 b\n\tif (a) b = 2;\n};";
    assert_eq!(
        errors(&parse(source.as_bytes())),
        vec![(Code::E0204, "expected ;, found b".to_owned())]
    );
    assert_eq!(body_kinds(source), [ExprStmt, IfStmt]);
    // Not first on its line, or not a statement word: skipped with the rest.
    let source = "float a, b;\nvoid() f =\n{\n\ta = 1 b if (a) b = 2;\n\tb = 3;\n};";
    assert_eq!(body_kinds(source), [ExprStmt, ExprStmt]);
    let source = "float a, b;\nvoid() f =\n{\n\ta = 1 b\n\tiff (a) b = 2;\n\tb = 3;\n};";
    assert_eq!(body_kinds(source), [ExprStmt, ExprStmt]);
}

// spec QCD-1.4: recovery skips a braced group whole, and stops at the `}` closing the block
#[test]
fn recovery_tracks_braces() {
    use crate::SyntaxKind::{Block, ExprStmt, IfStmt};
    let source = "float a, b;\nvoid() f =\n{\n\ta = 1 b {\n\treturn;\n\t};\n\tb = 2;\n};";
    assert_eq!(body_kinds(source), [ExprStmt, ExprStmt]);
    let source = "float a, b;\nvoid() f =\n{\n\tif (a) { a = 1 b }\n\tb = 2;\n};";
    let parsed = parse(source.as_bytes());
    assert_eq!(
        errors(&parsed),
        vec![(Code::E0204, "expected ;, found b".to_owned())]
    );
    assert_eq!(body_kinds(source), [IfStmt, ExprStmt]);
    let inner = parsed
        .syntax()
        .descendants()
        .filter(|n| n.kind() == Block)
        .nth(1)
        .unwrap();
    assert_eq!(inner.text(), b"{ a = 1 b }");
}

mod checker {
    use crate::recovery::{check_one, greens, items};
    use crate::{Dialect, GreenNode, parse};

    fn before(source: &str) -> Vec<GreenNode> {
        greens(&items(&parse(source.as_bytes()).syntax()))
    }

    const FOUR: &str = "float a;\nfloat b;\nfloat c;\nfloat d;\n";

    #[test]
    fn counts_errors() {
        assert_eq!(
            check_one(FOUR.as_bytes(), Dialect::Vanilla, &before(FOUR), 1),
            Ok(0)
        );
        let edited = "float a;\nfloat b\nfloat c;\nfloat d;\n";
        assert_eq!(
            check_one(edited.as_bytes(), Dialect::Vanilla, &before(FOUR), 1),
            Ok(1)
        );
    }

    #[test]
    fn a_changed_declaration_before_the_damage_fails() {
        let edited = "float x;\nfloat b;\nfloat c;\nfloat d;\n";
        let result = check_one(edited.as_bytes(), Dialect::Vanilla, &before(FOUR), 2);
        assert_eq!(
            result,
            Err("declaration 0 before the deletion changed".to_owned())
        );
        assert_eq!(
            check_one(edited.as_bytes(), Dialect::Vanilla, &before(FOUR), 0),
            Ok(0)
        );
    }

    #[test]
    fn at_most_the_next_declaration_may_change() {
        let edited = "float a;\nfloat b;\nfloat x;\nfloat d;\n";
        assert_eq!(
            check_one(edited.as_bytes(), Dialect::Vanilla, &before(FOUR), 1),
            Ok(0)
        );
        let result = check_one(edited.as_bytes(), Dialect::Vanilla, &before(FOUR), 0);
        assert_eq!(
            result,
            Err("declaration 2 changed, more than one after the deletion".to_owned())
        );
    }

    #[test]
    fn an_error_outside_the_damage_fails() {
        let original = "x1;\nfloat a;\nfloat b;\nfloat c;\n";
        let result = check_one(original.as_bytes(), Dialect::Vanilla, &before(original), 2);
        assert!(
            result
                .as_ref()
                .is_err_and(|e| e.starts_with("error E0203 \"x1\" is not a type at 0")),
            "{result:?}"
        );
        assert_eq!(
            check_one(original.as_bytes(), Dialect::Vanilla, &before(original), 0),
            Ok(1)
        );
        let late = "float a;\nfloat b;\nfloat c;\nx1;\n";
        let result = check_one(late.as_bytes(), Dialect::Vanilla, &before(late), 0);
        assert!(result.is_err(), "{result:?}");
    }
}
