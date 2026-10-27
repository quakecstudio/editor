// SPDX-License-Identifier: GPL-3.0-or-later

//! Constant arithmetic as FTEQCC folds it (`docs/spec/dialect-fteqcc.md` FTEF-26). Each case's
//! diagnostics are the pinned FTEQCC's (2026-10-27), with F332 turned on so its division
//! warnings show.

use std::path::PathBuf;

use qc_diag::Code;
use qc_preproc::CommandLine;
use qc_syntax::Dialect;

use crate::check::constant::Constant;
use crate::check::fold::c_g;
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

fn body(statements: &str) -> String {
    format!(
        "#pragma warning enable F332\nvoid() g = {{ local float a; local int i; {statements} }};\n"
    )
}

fn one(code: Code, message: &str) -> Vec<(Code, String)> {
    vec![(code, message.to_owned())]
}

// spec FTEF-26.2: a float division of two constants by 0 is "Division of %g by 0"
// spec QCD-7.75: W0233 for a division by 0 folding meets
#[test]
fn constant_division_by_zero() {
    assert_eq!(
        diags(&body("a = 1 / 0;")),
        one(Code::W0233, "Division of 1 by 0")
    );
    assert_eq!(
        diags(&body("a = 2.5 / 0;")),
        one(Code::W0233, "Division of 2.5 by 0")
    );
}

// spec FTEF-26.2: an integer division by a constant 0, and INT_MIN by -1
#[test]
fn integer_division_warnings() {
    let source = "#pragma target fte\n#pragma warning enable F332\n\
                  void() g = { local int i; i = 5i / 0i; i = 0x80000000i / -1i; i = i + 0i; };\n";
    assert_eq!(
        diags(source),
        [
            (Code::W0233, "Division by constant 0".to_owned()),
            (Code::W0233, "Signed overflow on division by -1".to_owned()),
        ]
    );
}

// spec FTEF-26.4: a division of a variable by a constant 0 is "Division by 0"
#[test]
fn variable_division_by_zero() {
    assert_eq!(
        diags(&body("a = a / 0;")),
        one(Code::W0233, "Division by 0")
    );
}

// spec FTEF-26.2: a bitwise operand that is no whole number is truncated, with a warning
// spec QCD-7.76: W0234 for a constant folding converts with a loss
#[test]
fn bitwise_operands_are_truncated() {
    assert_eq!(
        diags(&body("a = 3.5 & 1;")),
        one(
            Code::W0234,
            "Constant float operand 3.500000 will be truncated to 3"
        )
    );
    assert_eq!(
        diags(&body("a = 7 | 2.25;")),
        one(
            Code::W0234,
            "Constant float operand 2.250000 will be truncated to 2"
        )
    );
}

// spec FTEF-26.3: `&&` of two constants folds with no warning
#[test]
fn logic_of_constants_folds_quietly() {
    assert_eq!(diags(&body("a = 1 && 0;")), []);
}

// spec FTEF-26.1: a number literal's value, held to its type, the digits wrapping
#[test]
fn literal_values() {
    let flags = qc_lexer::LexFlags::default();
    assert_eq!(Constant::of_number("5i", flags), Constant::int(5));
    assert_eq!(
        Constant::of_number("0x80000000i", flags),
        Constant::int(i32::MIN)
    );
    assert_eq!(Constant::of_number("-3i", flags), Constant::int(-3));
    assert_eq!(Constant::of_number("2.5", flags), Constant::float(2.5));
    assert_eq!(Constant::of_number("7", flags), Constant::float(7.0));
    assert_eq!(Constant::of_number("10u", flags), Constant::uint(10));
    assert_eq!(Constant::of_number("3l", flags), Constant::int64(3));
}

// spec FTEF-26.2: C's %g, six significant digits
#[test]
fn c_g_prints_like_c() {
    assert_eq!(c_g(1.0), "1");
    assert_eq!(c_g(2.5), "2.5");
    assert_eq!(c_g(0.0001), "0.0001");
    assert_eq!(c_g(0.00001), "1e-05");
    assert_eq!(c_g(1_234_567.0), "1.23457e+06");
    assert_eq!(c_g(100_000.0), "100000");
}

// spec FTEF-26.5: an initialised global is a constant whose value folds, unless it is `var`
#[test]
fn initialised_globals_fold() {
    let source = "#pragma warning enable F332\nconst float zero = 0;\nfloat notconst = 0;\n\
                  var float v = 0;\n\
                  void() g = { local float a; a = 1 / zero; a = 1 / notconst; a = 1 / v; };\n";
    assert_eq!(
        diags(source),
        [
            (Code::W0233, "Division of 1 by 0".to_owned()),
            (Code::W0233, "Division of 1 by 0".to_owned()),
        ]
    );
}

// spec FTEF-26.3: the 64-bit and double conversions of a constant fold, warning of a fraction
// lost or bits a float cannot hold; a whole value folds with no warning
#[test]
fn wide_conversions_fold() {
    let cases = [
        (
            "__int64 x; void() g = { x = 1.5; };\n",
            "Numerical overflow. 1.500000 will be rounded to 1",
        ),
        (
            "__uint64 x; void() g = { x = -1.5; };\n",
            "Numerical overflow. -1.500000 will be rounded to 0",
        ),
        (
            "float f; void() g = { f = 16777217l; };\n",
            "Numerical truncation of 0x1000001 to 0x1000000.",
        ),
        (
            "__double d; void() g = { d = 9007199254740993l; };\n",
            "Numerical truncation of 0x20000000000001 to 0x20000000000000.",
        ),
    ];
    for (source, message) in cases {
        assert_eq!(
            diags(source),
            [(Code::W0234, message.to_owned())],
            "{source}"
        );
    }
    assert_eq!(diags("__int64 x; void() g = { x = 1; };\n"), []);
    assert_eq!(diags("__uint64 x; void() g = { x = 1; };\n"), []);
}
