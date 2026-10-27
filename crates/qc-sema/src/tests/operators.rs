// SPDX-License-Identifier: GPL-3.0-or-later

//! The types of FTEQCC's operators, from the opcode each is given (`docs/spec/dialect-fteqcc.md`
//! FTEF-23).

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::db::Db;
use crate::expand::ModuleInput;
use crate::symbol::FileId;

const FILE: FileId = FileId(0);

/// The diagnostics of the FTEQCC file `source`.
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

/// A function body around `statements`, with locals of each basic type.
fn body(statements: &str) -> String {
    format!(
        "void() f = {{ local float a; local vector v; local string s; local entity e; \
         local int i; {statements} }};\n"
    )
}

// spec FTEF-23.4: operands an opcode of the spelling takes are no error, with the conversions
// FTEQCC makes
#[test]
fn operands_an_opcode_takes() {
    for statement in [
        "a = a + a;",
        "v = v + v;",
        "a = v * v;",
        "v = a * v;",
        "i = i + i;",
        "a = a + i;",
        "a = s == s;",
        "a = e == e;",
        "a = a && s;",
        "s = s + a;",
    ] {
        assert_eq!(diags(&body(statement)), [], "{statement}");
    }
}

fn one(code: Code, message: &str) -> (Code, String) {
    (code, message.to_owned())
}

// spec FTEF-23.4: E0302 "type mismatch for OP (A and B)" when no opcode of the spelling takes
// the operands, and the first is used, its cast of the operands then refused
// spec QCD-4.2: under FTEQCC the mismatch names the operands' types
// spec FTEF-23.6: a refused cast ends the statement's checks, and the next statement is checked
#[test]
fn operands_no_opcode_takes() {
    assert_eq!(
        diags(&body("a = s * a;")),
        [
            one(Code::E0302, "type mismatch for * (string and float)"),
            one(Code::E0213, "Cannot cast from string to float"),
        ]
    );
    assert_eq!(
        diags(&body("a = e + s;")),
        [
            one(Code::E0302, "type mismatch for + (entity and string)"),
            one(Code::E0213, "Cannot cast from entity to float"),
        ]
    );
    assert_eq!(diags(&body("a = e + s; a = s * a;")).len(), 4);
}

// spec FTEF-23.5: a conversion costing 100 or more warns, the left operand's first, and the
// cast that follows is made or refused as an initialiser's
// spec QCD-7.74: W0232 "Implicit conversion from A to B"
#[test]
fn costly_conversions_warn() {
    assert_eq!(
        diags(&body("a = v < v;")),
        [
            one(Code::W0232, "Implicit conversion from vector to float"),
            one(Code::E0213, "Cannot cast from vector to float"),
        ]
    );
}

// spec FTEF-23.4: with `lax` on the mismatch is the warning W0231, and the cast a lax one
// spec QCD-7.73: W0231 "type mismatch for OP (A and B)" under `lax`
#[test]
fn lax_makes_the_mismatch_a_warning() {
    let source = format!("#pragma flag enable lax\n{}", body("a = s * a;"));
    assert_eq!(
        diags(&source),
        [
            one(Code::W0231, "type mismatch for * (string and float)"),
            one(Code::W0229, "Implicit lax cast from string to float"),
        ]
    );
}

// spec FTEF-17.14: in a member function a field named bare is `this.field`, its value of the
// type the field holds
#[test]
fn a_bare_field_in_a_member_function_is_its_value() {
    let class = "entity self;\nclass C { float x; void() m; };\n";
    assert_eq!(
        diags(&format!("{class}void() C::m = {{ local float a; a = x + 1; }};\n")),
        []
    );
    assert_eq!(
        diags(&format!(
            "{class}void() C::m = {{ local float a; a = x * \"s\"; }};\n"
        )),
        [
            one(Code::E0302, "type mismatch for * (float and string)"),
            one(Code::E0213, "Cannot cast from string to float"),
        ]
    );
}
