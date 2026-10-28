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

// spec FTEF-28.1: the same operand on both sides of && and ||
// spec QCD-7.81: W0239 for the constant-comparison warnings
#[test]
fn the_same_operand() {
    assert_eq!(
        warned("a = (a && a);"),
        ["Parameter offsets for && are the same"]
    );
    assert_eq!(warned("a = (a || a);"), ["Parameters for || are the same"]);
}

// spec FTEF-28.1: a constant operand of ||, two constants or the same operand of a comparison
// other than a float equality, make the result constant; a function is no constant
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

// spec FTEF-28.2: the comparison warns of operands of different basic types only
// spec QCD-7.82: W0240 "'OP' type mismatch: A with B"
#[test]
fn type_mismatch_needs_different_basic_types() {
    let source = "#pragma warning enable F307\n\
                  void() g = { local float a; local entity e; local entity f; a = (e == f); };\n";
    assert_eq!(diags(source), []);
}
