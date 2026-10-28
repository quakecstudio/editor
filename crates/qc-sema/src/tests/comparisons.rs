// SPDX-License-Identifier: GPL-3.0-or-later

//! The warnings of the comparisons and logic opcodes FTEQCC emits
//! (`docs/spec/dialect-fteqcc.md` FTEF-28). Each case's diagnostics are the pinned FTEQCC's
//! (2026-10-27), with F319 turned on so they show.

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId(0);

fn diags(source: &str) -> Vec<(Code, String)> {
    let mut db = Db::new();
    db.set_dialect(Dialect::Fteqcc);
    db.set_file(FILE, PathBuf::from("t.qc"), source.as_bytes().to_vec());
    let analysis = db.analyze_module(&ModuleInput {
        compiled: vec![FILE],
        command_line: CommandLine::new::<&str>(&[]),
        ..ModuleInput::default()
    });
    analysis
        .diagnostics()
        .map(|d| (d.diagnostic.code, d.diagnostic.message.clone()))
        .collect()
}

fn body(statement: &str) -> String {
    format!(
        "#pragma warning enable F319\nconst float k = 3;\nfloat() ff = {{ return 1; }};\n\
         void() vf = {{ }};\n.float fl; .vector fv;\n\
         void() g = {{ local float a; local float b; local string s; local entity e; \
         {statement} }};\n"
    )
}

fn warned(statement: &str) -> Vec<String> {
    diags(&body(statement))
        .into_iter()
        .map(|(code, message)| {
            assert_eq!(code, Code::W0239);
            message
        })
        .collect()
}

// spec FTEF-28.4: the same operand on both sides of && and ||
// spec QCD-7.81: W0239 for the constant-comparison warnings
#[test]
fn the_same_operand() {
    assert_eq!(
        warned("a = (a && a);"),
        ["Parameter offsets for && are the same"]
    );
    assert_eq!(warned("a = (a || a);"), ["Parameters for || are the same"]);
}

// spec FTEF-28.4: a constant operand of ||, two constants or the same operand of a comparison
// other than a float equality, make the result constant; types `typecmp_lax` tells apart do not
#[test]
fn constant_results() {
    let constant = ["Result of comparison is constant"];
    for case in [
        "a = (a || k);",
        "a = (s == s);",
        "a = (e == e);",
        "a = (a < a);",
        "a = (\"x\" == \"y\");",
        "a = (ff == ff);",
    ] {
        assert_eq!(warned(case), constant, "{case}");
    }
    for quiet in [
        "a = (a == a);",
        "a = (a == b);",
        "a = (k == k);",
        "a = (ff != vf);",
        "a = (fl == fv);",
    ] {
        assert_eq!(warned(quiet), Vec::<String>::new(), "{quiet}");
    }
}

// spec FTEF-28.3: a comparison never warns of a type mismatch, F307 on, whatever its types
#[test]
fn a_comparison_warns_of_no_type_mismatch() {
    let found = |statement: &str| {
        diags(&format!(
            "#pragma warning enable F307\n.float fl; .vector fv; void() vf = {{}}; \
             float() ff = {{ return 1; }}; __uint u; float *p; entity e, f;\n\
             void() g = {{ local float a; {statement} }};\n"
        ))
    };
    for statement in [
        "a = (e == f);",
        "a = !u;",
        "a = !p;",
        "a = (fl == fv);",
        "a = (ff != vf);",
    ] {
        assert_eq!(found(statement), [], "{statement}");
    }
}

// spec FTEF-23.9: an operand converted to the opcode's type is of that type, so a null constant
// compared with a string, entity or field is no type mismatch; FreeCS compares its strings and
// entities with `__NULL__` so, over a thousand times
#[test]
fn null_constants_compare_with_any_type() {
    let found = |statement: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\nstring s; entity e; float x; .float fl;\nvoid() f = {{ {statement} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    for quiet in [
        "if (s == __NULL__) x = 1;",
        "if (s != __NULL__) x = 1;",
        "if (e == __NULL__) x = 1;",
        "if (e == 0i) x = 1;",
        "if (s == 0) x = 1;",
        "if (fl == 0) x = 1;",
        "if (x == 1i) x = 1;",
    ] {
        assert!(found(quiet).is_empty(), "{quiet}: {:?}", found(quiet));
    }
}

// spec FTEF-28.4: functions and fields are constant symbols, compared with no warning when
// `typecmp_lax` tells their types apart; a null constant takes the row's type, and a case value
// keeps its own
#[test]
fn functions_and_fields_are_constants() {
    let found = |statement: &str| -> Vec<String> {
        diags(&format!(
            "void() fn = {{}}; void() fn2 = {{}}; float() ff = {{ return 1; }}; .float fl, fl2; \
             .vector fv; .int fi; float q;\nvoid() g = {{ {statement} }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    for constant in [
        "q = fn == fn2;",
        "q = fl == fl2;",
        "q = fn == 0;",
        "q = fn == 0i;",
    ] {
        assert_eq!(
            found(constant),
            ["Result of comparison is constant"],
            "{constant}"
        );
    }
    for quiet in [
        "q = ff != fn;",
        "q = fl == fv;",
        "q = fl == fi;",
        "q = ff == 0;",
        "q = fl == 0;",
        "switch (ff) { case 1i: break; }",
    ] {
        assert!(found(quiet).is_empty(), "{quiet}");
    }
}
