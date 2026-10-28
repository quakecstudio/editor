// SPDX-License-Identifier: GPL-3.0-or-later

//! Expressions as id's `qcc` checks them (`docs/spec/language.md` QC-16, QC-18). Each case's
//! diagnostic is the pinned `qcc`'s (2026-10-27).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId(0);

/// The diagnostics of a body holding `statement`, under the vanilla profile.
fn body(statement: &str) -> Vec<(Code, String)> {
    let source = format!(
        "float a; vector v; string s; entity e; .float fl; void(float x) f1;\n\
         void(...) va = {{}};\nvoid() g = {{ {statement} }};\nvoid(float x) f1 = {{}};\n"
    );
    let mut db = Db::new();
    db.set_dialect(Dialect::Vanilla);
    db.set_file(FILE, PathBuf::from("t.qc"), source.into_bytes());
    db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    })
    .diagnostics()
    .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
    .collect()
}

fn d(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

// spec QC-16.1: the operators take exactly the operand types of their rows
// spec QC-16.2: no `vector / float`, no ordering of vectors, no equality of fields, no `&&` but
// of floats
// spec QC-16.4: `!` of a field is a mismatch
// spec QCD-4.2: E0302 "type mismatch for OP"
#[test]
fn operators_take_their_rows_types() {
    for quiet in [
        "a = v * v; v = v * a; v = a * v;",
        "a = s == \"x\"; a = e != e; a = !s; a = !e; a = !v;",
        "e.fl = a; a = e.fl + 1;",
    ] {
        assert_eq!(body(quiet), [], "{quiet}");
    }
    for (statement, op) in [
        ("v = v / 2;", "/"),
        ("a = v < v;", "<"),
        ("a = fl == fl;", "=="),
        ("if (e && a) a = 1;", "&&"),
        ("a = !fl;", "!"),
        ("a = e.fl + s;", "+"),
    ] {
        assert_eq!(
            body(statement),
            [d(Code::E0302, &format!("type mismatch for {op}"))],
            "{statement}"
        );
    }
}

// spec QC-16.7: rows match by basic types alone, an assignment's two sides must be of one basic
// type and `.` takes an entity and a field
#[test]
fn basic_types_decide_the_row() {
    assert_eq!(body("a = g == g; a = f1 == g;"), []);
    for (statement, op) in [("a = s;", "="), ("e.fl = s;", "="), ("a = e.a;", ".")] {
        assert_eq!(
            body(statement),
            [d(Code::E0302, &format!("type mismatch for {op}"))],
            "{statement}"
        );
    }
}

// spec QC-18.8: a call needs a function; with arguments, one past a fixed list, one of another
// type and too few are errors; with none no count is checked
// spec QC-18.2: nine or more arguments are "More than eight parameters"
// spec QCD-4.3: E0303 "not a function"
// spec QCD-4.4: E0304 "too many parameters"
// spec QCD-4.5: E0305 "too few parameters"
// spec QCD-4.6: E0306 "type mismatch on parm N"
// spec QCD-4.7: E0307 "More than eight parameters"
#[test]
fn calls() {
    assert_eq!(body("f1(1); f1(); va(1, \"x\", v);"), []);
    let cases = [
        ("a();", d(Code::E0303, "not a function")),
        ("f1(1, 2);", d(Code::E0304, "too many parameters")),
        ("f1(s);", d(Code::E0306, "type mismatch on parm 0")),
        (
            "va(1, 2, 3, 4, 5, 6, 7, 8, 9);",
            d(Code::E0307, "More than eight parameters"),
        ),
    ];
    for (statement, expected) in cases {
        assert_eq!(body(statement), [expected], "{statement}");
    }
    let source =
        "void(float x, float y) f2;\nvoid() g = { f2(1); };\nvoid(float x, float y) f2 = {};\n";
    let mut db = Db::new();
    db.set_dialect(Dialect::Vanilla);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let found: Vec<(Code, String)> = db
        .analyze_module(&ModuleInput {
            compiled: vec![FILE],
            command_line: CommandLine::new::<&str>(&[]),
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect();
    assert_eq!(found, [d(Code::E0305, "too few parameters")]);
}

// spec QC-10.10: a variadic function type is never the same as another, so redeclaring a
// variadic function is a mismatch and such an argument matches no parameter
#[test]
fn variadic_types_are_never_the_same() {
    let analyze = |source: &str| -> Vec<(Code, String)> {
        let mut db = Db::new();
        db.set_dialect(Dialect::Vanilla);
        db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
        db.analyze_module(&ModuleInput {
            compiled: vec![FILE],
            command_line: CommandLine::new::<&str>(&[]),
            ..ModuleInput::default()
        })
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .filter(|(code, _)| *code != Code::E0211)
        .collect()
    };
    assert_eq!(
        analyze("float(...) va;\nfloat(...) va;\n"),
        [d(Code::E0205, "Type mismatch on redeclaration of va")]
    );
    assert_eq!(
        analyze("void(void(...) f) vh;\nvoid(void(...) f) vh;\n"),
        [d(Code::E0205, "Type mismatch on redeclaration of vh")]
    );
    assert_eq!(analyze("void() vc;\nvoid() vc = {};\n"), []);
    assert_eq!(
        analyze("void(...) va = {};\nvoid(void(...) f) h = {};\nvoid() g = { h(va); };\n"),
        [d(Code::E0306, "type mismatch on parm 0")]
    );
}
