// SPDX-License-Identifier: GPL-3.0-or-later

//! Error recovery stays local (LANG-2), including the generated single-token deletions.

use proptest::prelude::*;

use super::{SAMPLE, errors};
use crate::recovery::check_deletions;
use crate::{Code, parse};

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
}
