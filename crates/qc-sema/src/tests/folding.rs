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

const FILE: FileId = FileId::new(0);

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

// spec FTEF-26.13: an array's `length` is its size, a constant that folds: the pinned FTEQCC
// warns of the division by `names.length - 2` (2026-10-29)
#[test]
fn an_array_length_folds() {
    let source = "#pragma warning enable F332\nstring names[] = { \"a\", \"b\" };\n\
                  void() g = { local float a; a = a / (names.length - 2); };\n";
    assert_eq!(diags(source), one(Code::W0233, "Division by 0"));
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
    assert_eq!(c_g(f64::INFINITY), "inf");
    assert_eq!(c_g(f64::NEG_INFINITY), "-inf");
    assert_eq!(c_g(-0.00001), "-1e-05");
    assert_eq!(c_g(1e20), "1e+20");
}

// spec FTEF-26.13: an initialised global is a constant whose value folds, unless it is `var`
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

/// Every opcode `QCC_PR_StatementFlags` folds with both operands constant, as an index of an
/// array of one, whose bounds warning prints the folded value (`QCC_Eval_Int`, FTEF-31.8),
/// with the diagnostics the pinned FTEQCC gives for each (2026-10-27): each opcode with its
/// operands one way, the other, a zero right operand and a second pair.
const FOLDS: &[(&str, &str, &[&str])] = &[
    (
        "AddF",
        "(13.5 + 5.5)",
        &["(constant) array index out of bounds (0 <= 19 < 1)"],
    ),
    (
        "AddF",
        "(5.5 + 13.5)",
        &["(constant) array index out of bounds (0 <= 19 < 1)"],
    ),
    (
        "AddF",
        "(13.5 + 0.0)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "AddF",
        "(6.5 + 3.5)",
        &["(constant) array index out of bounds (0 <= 10 < 1)"],
    ),
    (
        "AddFi",
        "(13.5 + 5i)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddFi",
        "(5.5 + 13i)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddFi",
        "(13.5 + 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "AddFi",
        "(6.5 + 3i)",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "AddI",
        "(13i + 5i)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddI",
        "(5i + 13i)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddI",
        "(13i + 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "AddI",
        "(6i + 3i)",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "AddI64",
        "((__int64)13i + (__int64)5i)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddI64",
        "((__int64)5i + (__int64)13i)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddI64",
        "((__int64)13i + (__int64)0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "AddI64",
        "((__int64)6i + (__int64)3i)",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "AddIf",
        "(13i + 5.5)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddIf",
        "(5i + 13.5)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddIf",
        "(13i + 0.0)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "AddIf",
        "(6i + 3.5)",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "AndF",
        "(13.5 && 5.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndF",
        "(5.5 && 13.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("AndF", "(13.5 && 0.0)", &[]),
    (
        "AndF",
        "(6.5 && 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndFi",
        "(13.5 && 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndFi",
        "(5.5 && 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("AndFi", "(13.5 && 0i)", &[]),
    (
        "AndFi",
        "(6.5 && 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndI",
        "(13i && 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndI",
        "(5i && 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("AndI", "(13i && 0i)", &[]),
    (
        "AndI",
        "(6i && 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndIf",
        "(13i && 5.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndIf",
        "(5i && 13.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("AndIf", "(13i && 0.0)", &[]),
    (
        "AndIf",
        "(6i && 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "BitandF",
        "(13.5 & 5.5)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 5 < 1)",
        ],
    ),
    (
        "BitandF",
        "(5.5 & 13.5)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 5 < 1)",
        ],
    ),
    (
        "BitandF",
        "(13.5 & 0.0)",
        &["Constant float operand 13.500000 will be truncated to 13"],
    ),
    (
        "BitandF",
        "(6.5 & 3.5)",
        &[
            "Constant float operand 6.500000 will be truncated to 6",
            "Constant float operand 3.500000 will be truncated to 3",
            "(constant) array index out of bounds (0 <= 2 < 1)",
        ],
    ),
    (
        "BitandFi",
        "(13.5 & 5i)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 5 < 1)",
        ],
    ),
    (
        "BitandFi",
        "(5.5 & 13i)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 5 < 1)",
        ],
    ),
    (
        "BitandFi",
        "(13.5 & 0i)",
        &["Constant float operand 13.500000 will be truncated to 13"],
    ),
    (
        "BitandFi",
        "(6.5 & 3i)",
        &[
            "Constant float operand 6.500000 will be truncated to 6",
            "(constant) array index out of bounds (0 <= 2 < 1)",
        ],
    ),
    (
        "BitandI",
        "(13i & 5i)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "BitandI",
        "(5i & 13i)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    ("BitandI", "(13i & 0i)", &[]),
    (
        "BitandI",
        "(6i & 3i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "BitandI64",
        "((__int64)13i & (__int64)5i)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "BitandI64",
        "((__int64)5i & (__int64)13i)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    ("BitandI64", "((__int64)13i & (__int64)0i)", &[]),
    (
        "BitandI64",
        "((__int64)6i & (__int64)3i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "BitandIf",
        "(13i & 5.5)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 5 < 1)",
        ],
    ),
    (
        "BitandIf",
        "(5i & 13.5)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 5 < 1)",
        ],
    ),
    ("BitandIf", "(13i & 0.0)", &[]),
    (
        "BitandIf",
        "(6i & 3.5)",
        &[
            "Constant float operand 3.500000 will be truncated to 3",
            "(constant) array index out of bounds (0 <= 2 < 1)",
        ],
    ),
    (
        "BitorF",
        "(13.5 | 5.5)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "BitorF",
        "(5.5 | 13.5)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "BitorF",
        "(13.5 | 0.0)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "BitorF",
        "(6.5 | 3.5)",
        &[
            "Constant float operand 6.500000 will be truncated to 6",
            "Constant float operand 3.500000 will be truncated to 3",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "BitorFi",
        "(13.5 | 5i)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "BitorFi",
        "(5.5 | 13i)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "BitorFi",
        "(13.5 | 0i)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "BitorFi",
        "(6.5 | 3i)",
        &[
            "Constant float operand 6.500000 will be truncated to 6",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "BitorI",
        "(13i | 5i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitorI",
        "(5i | 13i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitorI",
        "(13i | 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitorI",
        "(6i | 3i)",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorI64",
        "((__int64)13i | (__int64)5i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitorI64",
        "((__int64)5i | (__int64)13i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitorI64",
        "((__int64)13i | (__int64)0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitorI64",
        "((__int64)6i | (__int64)3i)",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorIf",
        "(13i | 5.5)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "BitorIf",
        "(5i | 13.5)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "BitorIf",
        "(13i | 0.0)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitorIf",
        "(6i | 3.5)",
        &[
            "Constant float operand 3.500000 will be truncated to 3",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "BitxorF",
        "(13.5 ^ 5.5)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 8 < 1)",
        ],
    ),
    (
        "BitxorF",
        "(5.5 ^ 13.5)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 8 < 1)",
        ],
    ),
    (
        "BitxorF",
        "(13.5 ^ 0.0)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "BitxorF",
        "(6.5 ^ 3.5)",
        &[
            "Constant float operand 6.500000 will be truncated to 6",
            "Constant float operand 3.500000 will be truncated to 3",
            "(constant) array index out of bounds (0 <= 5 < 1)",
        ],
    ),
    (
        "BitxorI",
        "(13i ^ 5i)",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "BitxorI",
        "(5i ^ 13i)",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "BitxorI",
        "(13i ^ 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitxorI",
        "(6i ^ 3i)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "BitxorI64",
        "((__int64)13i ^ (__int64)5i)",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "BitxorI64",
        "((__int64)5i ^ (__int64)13i)",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "BitxorI64",
        "((__int64)13i ^ (__int64)0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitxorI64",
        "((__int64)6i ^ (__int64)3i)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "DivF",
        "(13.5 / 5.5)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    ("DivF", "(5.5 / 13.5)", &[]),
    (
        "DivF",
        "(13.5 / 0.0)",
        &["(constant) array index out of bounds (0 <= -1 < 1)"],
    ),
    (
        "DivF",
        "(6.5 / 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "DivFi",
        "(13.5 / 5i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    ("DivFi", "(5.5 / 13i)", &[]),
    (
        "DivFi",
        "(13.5 / 0i)",
        &["(constant) array index out of bounds (0 <= -1 < 1)"],
    ),
    (
        "DivFi",
        "(6.5 / 3i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "DivI",
        "(13i / 5i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    ("DivI", "(5i / 13i)", &[]),
    ("DivI", "(13i / 0i)", &[]),
    (
        "DivI",
        "(6i / 3i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "DivI64",
        "((__int64)13i / (__int64)5i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    ("DivI64", "((__int64)5i / (__int64)13i)", &[]),
    ("DivI64", "((__int64)13i / (__int64)0i)", &[]),
    (
        "DivI64",
        "((__int64)6i / (__int64)3i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "DivIf",
        "(13i / 5.5)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    ("DivIf", "(5i / 13.5)", &[]),
    (
        "DivIf",
        "(13i / 0.0)",
        &["(constant) array index out of bounds (0 <= -1 < 1)"],
    ),
    (
        "DivIf",
        "(6i / 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "DivU",
        "((__uint)13i / (__uint)5i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    ("DivU", "((__uint)5i / (__uint)13i)", &[]),
    ("DivU", "((__uint)13i / (__uint)0i)", &[]),
    (
        "DivU",
        "((__uint)6i / (__uint)3i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "DivU64",
        "((__uint64)13i % (__uint64)5i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    ("DivU64", "((__uint64)5i % (__uint64)13i)", &[]),
    ("DivU64", "((__uint64)13i % (__uint64)0i)", &[]),
    (
        "DivU64",
        "((__uint64)6i % (__uint64)3i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    ("EqF", "(13.5 == 5.5)", &[]),
    ("EqF", "(5.5 == 13.5)", &[]),
    ("EqF", "(13.5 == 0.0)", &[]),
    ("EqF", "(6.5 == 3.5)", &[]),
    ("EqFi", "(13.5 == 5i)", &[]),
    ("EqFi", "(5.5 == 13i)", &[]),
    ("EqFi", "(13.5 == 0i)", &[]),
    ("EqFi", "(6.5 == 3i)", &[]),
    ("EqI", "(13i == 5i)", &[]),
    ("EqI", "(5i == 13i)", &[]),
    ("EqI", "(13i == 0i)", &[]),
    ("EqI", "(6i == 3i)", &[]),
    ("EqI64", "((__int64)13i == (__int64)5i)", &[]),
    ("EqI64", "((__int64)5i == (__int64)13i)", &[]),
    ("EqI64", "((__int64)13i == (__int64)0i)", &[]),
    ("EqI64", "((__int64)6i == (__int64)3i)", &[]),
    ("EqIf", "(13i == 5.5)", &[]),
    ("EqIf", "(5i == 13.5)", &[]),
    ("EqIf", "(13i == 0.0)", &[]),
    ("EqIf", "(6i == 3.5)", &[]),
    (
        "GeFi",
        "(13.5 >= 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("GeFi", "(5.5 >= 13i)", &[]),
    (
        "GeFi",
        "(13.5 >= 0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GeFi",
        "(6.5 >= 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GeI",
        "(13i >= 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("GeI", "(5i >= 13i)", &[]),
    (
        "GeI",
        "(13i >= 0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GeI",
        "(6i >= 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GeIf",
        "(13i >= 5.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("GeIf", "(5i >= 13.5)", &[]),
    (
        "GeIf",
        "(13i >= 0.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GeIf",
        "(6i >= 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GtFi",
        "(13.5 > 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("GtFi", "(5.5 > 13i)", &[]),
    (
        "GtFi",
        "(13.5 > 0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GtFi",
        "(6.5 > 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GtI",
        "(13i > 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("GtI", "(5i > 13i)", &[]),
    (
        "GtI",
        "(13i > 0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GtI",
        "(6i > 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GtIf",
        "(13i > 5.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("GtIf", "(5i > 13.5)", &[]),
    (
        "GtIf",
        "(13i > 0.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GtIf",
        "(6i > 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LeFi", "(13.5 <= 5i)", &[]),
    (
        "LeFi",
        "(5.5 <= 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LeFi", "(13.5 <= 0i)", &[]),
    ("LeFi", "(6.5 <= 3i)", &[]),
    ("LeI", "(13i <= 5i)", &[]),
    (
        "LeI",
        "(5i <= 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LeI", "(13i <= 0i)", &[]),
    ("LeI", "(6i <= 3i)", &[]),
    ("LeI64", "((__int64)13i <= (__int64)5i)", &[]),
    (
        "LeI64",
        "((__int64)5i <= (__int64)13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LeI64", "((__int64)13i <= (__int64)0i)", &[]),
    ("LeI64", "((__int64)6i <= (__int64)3i)", &[]),
    ("LeIf", "(13i <= 5.5)", &[]),
    (
        "LeIf",
        "(5i <= 13.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LeIf", "(13i <= 0.0)", &[]),
    ("LeIf", "(6i <= 3.5)", &[]),
    ("LeU", "((__uint)13i <= (__uint)5i)", &[]),
    (
        "LeU",
        "((__uint)5i <= (__uint)13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LeU", "((__uint)13i <= (__uint)0i)", &[]),
    ("LeU", "((__uint)6i <= (__uint)3i)", &[]),
    ("LeU64", "((__uint64)13i < (__uint64)5i)", &[]),
    (
        "LeU64",
        "((__uint64)5i < (__uint64)13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LeU64", "((__uint64)13i < (__uint64)0i)", &[]),
    ("LeU64", "((__uint64)6i < (__uint64)3i)", &[]),
    (
        "LshiftF",
        "(13.5 << 5.5)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 416 < 1)",
        ],
    ),
    (
        "LshiftF",
        "(5.5 << 13.5)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 40960 < 1)",
        ],
    ),
    (
        "LshiftF",
        "(13.5 << 0.0)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "LshiftF",
        "(6.5 << 3.5)",
        &[
            "Constant float operand 6.500000 will be truncated to 6",
            "Constant float operand 3.500000 will be truncated to 3",
            "(constant) array index out of bounds (0 <= 48 < 1)",
        ],
    ),
    (
        "LshiftFi",
        "(13.5 << 5i)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 416 < 1)",
        ],
    ),
    (
        "LshiftFi",
        "(5.5 << 13i)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 40960 < 1)",
        ],
    ),
    (
        "LshiftFi",
        "(13.5 << 0i)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "LshiftFi",
        "(6.5 << 3i)",
        &[
            "Constant float operand 6.500000 will be truncated to 6",
            "(constant) array index out of bounds (0 <= 48 < 1)",
        ],
    ),
    (
        "LshiftI",
        "(13i << 5i)",
        &["(constant) array index out of bounds (0 <= 416 < 1)"],
    ),
    (
        "LshiftI",
        "(5i << 13i)",
        &["(constant) array index out of bounds (0 <= 40960 < 1)"],
    ),
    (
        "LshiftI",
        "(13i << 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "LshiftI",
        "(6i << 3i)",
        &["(constant) array index out of bounds (0 <= 48 < 1)"],
    ),
    (
        "LshiftI64i",
        "((__int64)13i << 5i)",
        &["(constant) array index out of bounds (0 <= 416 < 1)"],
    ),
    (
        "LshiftI64i",
        "((__int64)5i << 13i)",
        &["(constant) array index out of bounds (0 <= 40960 < 1)"],
    ),
    (
        "LshiftI64i",
        "((__int64)13i << 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "LshiftI64i",
        "((__int64)6i << 3i)",
        &["(constant) array index out of bounds (0 <= 48 < 1)"],
    ),
    (
        "LshiftIf",
        "(13i << 5.5)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= 416 < 1)",
        ],
    ),
    (
        "LshiftIf",
        "(5i << 13.5)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 40960 < 1)",
        ],
    ),
    (
        "LshiftIf",
        "(13i << 0.0)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "LshiftIf",
        "(6i << 3.5)",
        &[
            "Constant float operand 3.500000 will be truncated to 3",
            "(constant) array index out of bounds (0 <= 48 < 1)",
        ],
    ),
    ("LtFi", "(13.5 < 5i)", &[]),
    (
        "LtFi",
        "(5.5 < 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LtFi", "(13.5 < 0i)", &[]),
    ("LtFi", "(6.5 < 3i)", &[]),
    ("LtI", "(13i < 5i)", &[]),
    (
        "LtI",
        "(5i < 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LtI", "(13i < 0i)", &[]),
    ("LtI", "(6i < 3i)", &[]),
    ("LtI64", "((__int64)13i < (__int64)5i)", &[]),
    (
        "LtI64",
        "((__int64)5i < (__int64)13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LtI64", "((__int64)13i < (__int64)0i)", &[]),
    ("LtI64", "((__int64)6i < (__int64)3i)", &[]),
    ("LtIf", "(13i < 5.5)", &[]),
    (
        "LtIf",
        "(5i < 13.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LtIf", "(13i < 0.0)", &[]),
    ("LtIf", "(6i < 3.5)", &[]),
    ("LtU", "((__uint)13i < (__uint)5i)", &[]),
    (
        "LtU",
        "((__uint)5i < (__uint)13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LtU", "((__uint)13i < (__uint)0i)", &[]),
    ("LtU", "((__uint)6i < (__uint)3i)", &[]),
    ("LtU64", "((__uint64)13i < (__uint64)5i)", &[]),
    (
        "LtU64",
        "((__uint64)5i < (__uint64)13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LtU64", "((__uint64)13i < (__uint64)0i)", &[]),
    ("LtU64", "((__uint64)6i < (__uint64)3i)", &[]),
    (
        "ModF",
        "(13.5 % 5.5)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "ModF",
        "(5.5 % 13.5)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "ModF",
        "(13.5 % 0.0)",
        &[
            "Constant float operand inf will be truncated to 2147483647",
            "Constant float operand inf will be truncated to 2147483647",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "ModF",
        "(6.5 % 3.5)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "ModI",
        "(13i % 5i)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "ModI",
        "(5i % 13i)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "ModI",
        "(13i % 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    ("ModI", "(6i % 3i)", &[]),
    (
        "ModU",
        "((__uint)13i % (__uint)5i)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "ModU",
        "((__uint)5i % (__uint)13i)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "ModU",
        "((__uint)13i % (__uint)0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    ("ModU", "((__uint)6i % (__uint)3i)", &[]),
    (
        "MulF",
        "(13.5 * 5.5)",
        &["(constant) array index out of bounds (0 <= 74 < 1)"],
    ),
    (
        "MulF",
        "(5.5 * 13.5)",
        &["(constant) array index out of bounds (0 <= 74 < 1)"],
    ),
    ("MulF", "(13.5 * 0.0)", &[]),
    (
        "MulF",
        "(6.5 * 3.5)",
        &["(constant) array index out of bounds (0 <= 22 < 1)"],
    ),
    (
        "MulFi",
        "(13.5 * 5i)",
        &["(constant) array index out of bounds (0 <= 67 < 1)"],
    ),
    (
        "MulFi",
        "(5.5 * 13i)",
        &["(constant) array index out of bounds (0 <= 71 < 1)"],
    ),
    ("MulFi", "(13.5 * 0i)", &[]),
    (
        "MulFi",
        "(6.5 * 3i)",
        &["(constant) array index out of bounds (0 <= 19 < 1)"],
    ),
    (
        "MulI",
        "(13i * 5i)",
        &["(constant) array index out of bounds (0 <= 65 < 1)"],
    ),
    (
        "MulI",
        "(5i * 13i)",
        &["(constant) array index out of bounds (0 <= 65 < 1)"],
    ),
    ("MulI", "(13i * 0i)", &[]),
    (
        "MulI",
        "(6i * 3i)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "MulI64",
        "((__int64)13i * (__int64)5i)",
        &["(constant) array index out of bounds (0 <= 65 < 1)"],
    ),
    (
        "MulI64",
        "((__int64)5i * (__int64)13i)",
        &["(constant) array index out of bounds (0 <= 65 < 1)"],
    ),
    ("MulI64", "((__int64)13i * (__int64)0i)", &[]),
    (
        "MulI64",
        "((__int64)6i * (__int64)3i)",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "MulIf",
        "(13i * 5.5)",
        &["(constant) array index out of bounds (0 <= 71 < 1)"],
    ),
    (
        "MulIf",
        "(5i * 13.5)",
        &["(constant) array index out of bounds (0 <= 67 < 1)"],
    ),
    ("MulIf", "(13i * 0.0)", &[]),
    (
        "MulIf",
        "(6i * 3.5)",
        &["(constant) array index out of bounds (0 <= 21 < 1)"],
    ),
    (
        "MulV",
        "('13 5 2' * '5 2 7')",
        &["(constant) array index out of bounds (0 <= 89 < 1)"],
    ),
    (
        "MulV",
        "('5 2 7' * '13 5 2')",
        &["(constant) array index out of bounds (0 <= 89 < 1)"],
    ),
    ("MulV", "('13 5 2' * '0 0 0')", &[]),
    (
        "MulV",
        "('6 3 1' * '3 1 4')",
        &["(constant) array index out of bounds (0 <= 25 < 1)"],
    ),
    (
        "NeF",
        "(13.5 != 5.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeF",
        "(5.5 != 13.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeF",
        "(13.5 != 0.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeF",
        "(6.5 != 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeFi",
        "(13.5 != 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeFi",
        "(5.5 != 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeFi",
        "(13.5 != 0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeFi",
        "(6.5 != 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeI",
        "(13i != 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeI",
        "(5i != 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeI",
        "(13i != 0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeI",
        "(6i != 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeI64",
        "((__int64)13i != (__int64)5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeI64",
        "((__int64)5i != (__int64)13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeI64",
        "((__int64)13i != (__int64)0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeI64",
        "((__int64)6i != (__int64)3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeIf",
        "(13i != 5.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeIf",
        "(5i != 13.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeIf",
        "(13i != 0.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "NeIf",
        "(6i != 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrF",
        "(13.5 || 5.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrF",
        "(5.5 || 13.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrF",
        "(13.5 || 0.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrF",
        "(6.5 || 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrFi",
        "(13.5 || 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrFi",
        "(5.5 || 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrFi",
        "(13.5 || 0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrFi",
        "(6.5 || 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrI",
        "(13i || 5i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrI",
        "(5i || 13i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrI",
        "(13i || 0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrI",
        "(6i || 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrIf",
        "(13i || 5.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrIf",
        "(5i || 13.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrIf",
        "(13i || 0.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrIf",
        "(6i || 3.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "RshiftF",
        "(13.5 >> 5.5)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "Constant float operand 5.500000 will be truncated to 5",
        ],
    ),
    (
        "RshiftF",
        "(5.5 >> 13.5)",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "Constant float operand 13.500000 will be truncated to 13",
        ],
    ),
    (
        "RshiftF",
        "(13.5 >> 0.0)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "RshiftF",
        "(6.5 >> 3.5)",
        &[
            "Constant float operand 6.500000 will be truncated to 6",
            "Constant float operand 3.500000 will be truncated to 3",
        ],
    ),
    (
        "RshiftFi",
        "(13.5 >> 5i)",
        &["Constant float operand 13.500000 will be truncated to 13"],
    ),
    (
        "RshiftFi",
        "(5.5 >> 13i)",
        &["Constant float operand 5.500000 will be truncated to 5"],
    ),
    (
        "RshiftFi",
        "(13.5 >> 0i)",
        &[
            "Constant float operand 13.500000 will be truncated to 13",
            "(constant) array index out of bounds (0 <= 13 < 1)",
        ],
    ),
    (
        "RshiftFi",
        "(6.5 >> 3i)",
        &["Constant float operand 6.500000 will be truncated to 6"],
    ),
    ("RshiftI", "(13i >> 5i)", &[]),
    ("RshiftI", "(5i >> 13i)", &[]),
    (
        "RshiftI",
        "(13i >> 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    ("RshiftI", "(6i >> 3i)", &[]),
    ("RshiftI64i", "((__int64)13i >> 5i)", &[]),
    ("RshiftI64i", "((__int64)5i >> 13i)", &[]),
    (
        "RshiftI64i",
        "((__int64)13i >> 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    ("RshiftI64i", "((__int64)6i >> 3i)", &[]),
    (
        "RshiftIf",
        "(13i >> 5.5)",
        &["Constant float operand 5.500000 will be truncated to 5"],
    ),
    (
        "RshiftIf",
        "(5i >> 13.5)",
        &["Constant float operand 13.500000 will be truncated to 13"],
    ),
    (
        "RshiftIf",
        "(13i >> 0.0)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "RshiftIf",
        "(6i >> 3.5)",
        &["Constant float operand 3.500000 will be truncated to 3"],
    ),
    ("RshiftU", "((__uint)13i >> 5i)", &[]),
    ("RshiftU", "((__uint)5i >> 13i)", &[]),
    (
        "RshiftU",
        "((__uint)13i >> 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    ("RshiftU", "((__uint)6i >> 3i)", &[]),
    (
        "SubF",
        "(13.5 - 5.5)",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "SubF",
        "(5.5 - 13.5)",
        &["(constant) array index out of bounds (0 <= -8 < 1)"],
    ),
    (
        "SubF",
        "(13.5 - 0.0)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "SubF",
        "(6.5 - 3.5)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "SubFi",
        "(13.5 - 5i)",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "SubFi",
        "(5.5 - 13i)",
        &["(constant) array index out of bounds (0 <= -7 < 1)"],
    ),
    (
        "SubFi",
        "(13.5 - 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "SubFi",
        "(6.5 - 3i)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "SubI",
        "(13i - 5i)",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "SubI",
        "(5i - 13i)",
        &["(constant) array index out of bounds (0 <= -8 < 1)"],
    ),
    (
        "SubI",
        "(13i - 0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "SubI",
        "(6i - 3i)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "SubI64",
        "((__int64)13i - (__int64)5i)",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "SubI64",
        "((__int64)5i - (__int64)13i)",
        &["(constant) array index out of bounds (0 <= -8 < 1)"],
    ),
    (
        "SubI64",
        "((__int64)13i - (__int64)0i)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "SubI64",
        "((__int64)6i - (__int64)3i)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "SubIf",
        "(13i - 5.5)",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "SubIf",
        "(5i - 13.5)",
        &["(constant) array index out of bounds (0 <= -8 < 1)"],
    ),
    (
        "SubIf",
        "(13i - 0.0)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "SubIf",
        "(6i - 3.5)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    // Equal operands, and each component of a vector result (FTEF-31.9).
    (
        "AddF",
        "(7.5 + 7.5)",
        &["(constant) array index out of bounds (0 <= 15 < 1)"],
    ),
    (
        "AddFi",
        "(7.5 + 7i)",
        &["(constant) array index out of bounds (0 <= 14 < 1)"],
    ),
    (
        "AddI",
        "(7i + 7i)",
        &["(constant) array index out of bounds (0 <= 14 < 1)"],
    ),
    (
        "AddI64",
        "((__int64)7i + (__int64)7i)",
        &["(constant) array index out of bounds (0 <= 14 < 1)"],
    ),
    (
        "AddIf",
        "(7i + 7.5)",
        &["(constant) array index out of bounds (0 <= 14 < 1)"],
    ),
    (
        "AddV",
        "(('13 5 2' + '5 2 7'))[0]",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddV",
        "(('13 5 2' + '5 2 7'))[1]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "AddV",
        "(('13 5 2' + '5 2 7'))[2]",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "AddV",
        "(('5 2 7' + '13 5 2'))[0]",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "AddV",
        "(('5 2 7' + '13 5 2'))[1]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "AddV",
        "(('5 2 7' + '13 5 2'))[2]",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "AddV",
        "(('6 3 1' + '3 1 4'))[0]",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "AddV",
        "(('6 3 1' + '3 1 4'))[1]",
        &["(constant) array index out of bounds (0 <= 4 < 1)"],
    ),
    (
        "AddV",
        "(('6 3 1' + '3 1 4'))[2]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "AddV",
        "(('7 3 1' + '7 3 1'))[0]",
        &["(constant) array index out of bounds (0 <= 14 < 1)"],
    ),
    (
        "AddV",
        "(('7 3 1' + '7 3 1'))[1]",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "AddV",
        "(('7 3 1' + '7 3 1'))[2]",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "AndF",
        "(7.5 && 7.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndFi",
        "(7.5 && 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndI",
        "(7i && 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "AndIf",
        "(7i && 7.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "BitandF",
        "(7.5 & 7.5)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "Constant float operand 7.500000 will be truncated to 7",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "BitandFi",
        "(7.5 & 7i)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "BitandI",
        "(7i & 7i)",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitandI64",
        "((__int64)7i & (__int64)7i)",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitandIf",
        "(7i & 7.5)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "BitandV",
        "(('13 5 2' & '5 2 7'))[0]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    ("BitandV", "(('13 5 2' & '5 2 7'))[1]", &[]),
    (
        "BitandV",
        "(('13 5 2' & '5 2 7'))[2]",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "BitandV",
        "(('5 2 7' & '13 5 2'))[0]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    ("BitandV", "(('5 2 7' & '13 5 2'))[1]", &[]),
    (
        "BitandV",
        "(('5 2 7' & '13 5 2'))[2]",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "BitandV",
        "(('6 3 1' & '3 1 4'))[0]",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "BitandV",
        "(('6 3 1' & '3 1 4'))[1]",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("BitandV", "(('6 3 1' & '3 1 4'))[2]", &[]),
    (
        "BitandV",
        "(('7 3 1' & '7 3 1'))[0]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitandV",
        "(('7 3 1' & '7 3 1'))[1]",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "BitandV",
        "(('7 3 1' & '7 3 1'))[2]",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "BitorF",
        "(7.5 | 7.5)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "Constant float operand 7.500000 will be truncated to 7",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "BitorFi",
        "(7.5 | 7i)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "BitorI",
        "(7i | 7i)",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorI64",
        "((__int64)7i | (__int64)7i)",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorIf",
        "(7i | 7.5)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "BitorV",
        "(('13 5 2' | '5 2 7'))[0]",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitorV",
        "(('13 5 2' | '5 2 7'))[1]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorV",
        "(('13 5 2' | '5 2 7'))[2]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorV",
        "(('5 2 7' | '13 5 2'))[0]",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "BitorV",
        "(('5 2 7' | '13 5 2'))[1]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorV",
        "(('5 2 7' | '13 5 2'))[2]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorV",
        "(('6 3 1' | '3 1 4'))[0]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorV",
        "(('6 3 1' | '3 1 4'))[1]",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "BitorV",
        "(('6 3 1' | '3 1 4'))[2]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "BitorV",
        "(('7 3 1' | '7 3 1'))[0]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitorV",
        "(('7 3 1' | '7 3 1'))[1]",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "BitorV",
        "(('7 3 1' | '7 3 1'))[2]",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "BitxorF",
        "(7.5 ^ 7.5)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "Constant float operand 7.500000 will be truncated to 7",
        ],
    ),
    ("BitxorI", "(7i ^ 7i)", &[]),
    ("BitxorI64", "((__int64)7i ^ (__int64)7i)", &[]),
    (
        "BitxorV",
        "(('13 5 2' ^ '5 2 7'))[0]",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "BitxorV",
        "(('13 5 2' ^ '5 2 7'))[1]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitxorV",
        "(('13 5 2' ^ '5 2 7'))[2]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "BitxorV",
        "(('5 2 7' ^ '13 5 2'))[0]",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "BitxorV",
        "(('5 2 7' ^ '13 5 2'))[1]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "BitxorV",
        "(('5 2 7' ^ '13 5 2'))[2]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "BitxorV",
        "(('6 3 1' ^ '3 1 4'))[0]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "BitxorV",
        "(('6 3 1' ^ '3 1 4'))[1]",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "BitxorV",
        "(('6 3 1' ^ '3 1 4'))[2]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    ("BitxorV", "(('7 3 1' ^ '7 3 1'))[0]", &[]),
    ("BitxorV", "(('7 3 1' ^ '7 3 1'))[1]", &[]),
    ("BitxorV", "(('7 3 1' ^ '7 3 1'))[2]", &[]),
    (
        "DivF",
        "(7.5 / 7.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "DivFi",
        "(7.5 / 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "DivI",
        "(7i / 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "DivI64",
        "((__int64)7i / (__int64)7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("DivIf", "(7i / 7.5)", &[]),
    (
        "DivU",
        "((__uint)7i / (__uint)7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "DivU64",
        "((__uint64)7i % (__uint64)7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "EqF",
        "(7.5 == 7.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("EqFi", "(7.5 == 7i)", &[]),
    (
        "EqI",
        "(7i == 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "EqI64",
        "((__int64)7i == (__int64)7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("EqIf", "(7i == 7.5)", &[]),
    (
        "GeFi",
        "(7.5 >= 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "GeI",
        "(7i >= 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("GeIf", "(7i >= 7.5)", &[]),
    (
        "GtFi",
        "(7.5 > 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("GtI", "(7i > 7i)", &[]),
    ("GtIf", "(7i > 7.5)", &[]),
    ("LeFi", "(7.5 <= 7i)", &[]),
    (
        "LeI",
        "(7i <= 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "LeI64",
        "((__int64)7i <= (__int64)7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "LeIf",
        "(7i <= 7.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "LeU",
        "((__uint)7i <= (__uint)7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "LshiftF",
        "(7.5 << 7.5)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "Constant float operand 7.500000 will be truncated to 7",
            "(constant) array index out of bounds (0 <= 896 < 1)",
        ],
    ),
    (
        "LshiftFi",
        "(7.5 << 7i)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "(constant) array index out of bounds (0 <= 896 < 1)",
        ],
    ),
    (
        "LshiftI",
        "(7i << 7i)",
        &["(constant) array index out of bounds (0 <= 896 < 1)"],
    ),
    (
        "LshiftI64i",
        "((__int64)7i << 7i)",
        &["(constant) array index out of bounds (0 <= 896 < 1)"],
    ),
    (
        "LshiftIf",
        "(7i << 7.5)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "(constant) array index out of bounds (0 <= 896 < 1)",
        ],
    ),
    ("LtFi", "(7.5 < 7i)", &[]),
    ("LtI", "(7i < 7i)", &[]),
    ("LtI64", "((__int64)7i < (__int64)7i)", &[]),
    (
        "LtIf",
        "(7i < 7.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("LtU", "((__uint)7i < (__uint)7i)", &[]),
    ("ModF", "(7.5 % 7.5)", &[]),
    ("ModI", "(7i % 7i)", &[]),
    ("ModU", "((__uint)7i % (__uint)7i)", &[]),
    (
        "MulF",
        "(7.5 * 7.5)",
        &["(constant) array index out of bounds (0 <= 56 < 1)"],
    ),
    (
        "MulFi",
        "(7.5 * 7i)",
        &["(constant) array index out of bounds (0 <= 52 < 1)"],
    ),
    (
        "MulFv",
        "((13.5 * '5 2 7'))[0]",
        &["(constant) array index out of bounds (0 <= 67 < 1)"],
    ),
    (
        "MulFv",
        "((13.5 * '5 2 7'))[1]",
        &["(constant) array index out of bounds (0 <= 27 < 1)"],
    ),
    (
        "MulFv",
        "((13.5 * '5 2 7'))[2]",
        &["(constant) array index out of bounds (0 <= 94 < 1)"],
    ),
    (
        "MulFv",
        "((5.5 * '13 5 2'))[0]",
        &["(constant) array index out of bounds (0 <= 71 < 1)"],
    ),
    (
        "MulFv",
        "((5.5 * '13 5 2'))[1]",
        &["(constant) array index out of bounds (0 <= 27 < 1)"],
    ),
    (
        "MulFv",
        "((5.5 * '13 5 2'))[2]",
        &["(constant) array index out of bounds (0 <= 11 < 1)"],
    ),
    (
        "MulFv",
        "((6.5 * '3 1 4'))[0]",
        &["(constant) array index out of bounds (0 <= 19 < 1)"],
    ),
    (
        "MulFv",
        "((6.5 * '3 1 4'))[1]",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "MulFv",
        "((6.5 * '3 1 4'))[2]",
        &["(constant) array index out of bounds (0 <= 26 < 1)"],
    ),
    (
        "MulFv",
        "((7.5 * '7 3 1'))[0]",
        &["(constant) array index out of bounds (0 <= 52 < 1)"],
    ),
    (
        "MulFv",
        "((7.5 * '7 3 1'))[1]",
        &["(constant) array index out of bounds (0 <= 22 < 1)"],
    ),
    (
        "MulFv",
        "((7.5 * '7 3 1'))[2]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "MulI",
        "(7i * 7i)",
        &["(constant) array index out of bounds (0 <= 49 < 1)"],
    ),
    (
        "MulI64",
        "((__int64)7i * (__int64)7i)",
        &["(constant) array index out of bounds (0 <= 49 < 1)"],
    ),
    (
        "MulIf",
        "(7i * 7.5)",
        &["(constant) array index out of bounds (0 <= 52 < 1)"],
    ),
    (
        "MulIv",
        "((13i * '5 2 7'))[0]",
        &["(constant) array index out of bounds (0 <= 65 < 1)"],
    ),
    (
        "MulIv",
        "((13i * '5 2 7'))[1]",
        &["(constant) array index out of bounds (0 <= 26 < 1)"],
    ),
    (
        "MulIv",
        "((13i * '5 2 7'))[2]",
        &["(constant) array index out of bounds (0 <= 91 < 1)"],
    ),
    (
        "MulIv",
        "((5i * '13 5 2'))[0]",
        &["(constant) array index out of bounds (0 <= 65 < 1)"],
    ),
    (
        "MulIv",
        "((5i * '13 5 2'))[1]",
        &["(constant) array index out of bounds (0 <= 25 < 1)"],
    ),
    (
        "MulIv",
        "((5i * '13 5 2'))[2]",
        &["(constant) array index out of bounds (0 <= 10 < 1)"],
    ),
    (
        "MulIv",
        "((6i * '3 1 4'))[0]",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "MulIv",
        "((6i * '3 1 4'))[1]",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "MulIv",
        "((6i * '3 1 4'))[2]",
        &["(constant) array index out of bounds (0 <= 24 < 1)"],
    ),
    (
        "MulIv",
        "((7i * '7 3 1'))[0]",
        &["(constant) array index out of bounds (0 <= 49 < 1)"],
    ),
    (
        "MulIv",
        "((7i * '7 3 1'))[1]",
        &["(constant) array index out of bounds (0 <= 21 < 1)"],
    ),
    (
        "MulIv",
        "((7i * '7 3 1'))[2]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "MulV",
        "('7 3 1' * '7 3 1')",
        &["(constant) array index out of bounds (0 <= 59 < 1)"],
    ),
    (
        "MulVf",
        "(('13 5 2' * 5.5))[0]",
        &["(constant) array index out of bounds (0 <= 71 < 1)"],
    ),
    (
        "MulVf",
        "(('13 5 2' * 5.5))[1]",
        &["(constant) array index out of bounds (0 <= 27 < 1)"],
    ),
    (
        "MulVf",
        "(('13 5 2' * 5.5))[2]",
        &["(constant) array index out of bounds (0 <= 11 < 1)"],
    ),
    (
        "MulVf",
        "(('5 2 7' * 13.5))[0]",
        &["(constant) array index out of bounds (0 <= 67 < 1)"],
    ),
    (
        "MulVf",
        "(('5 2 7' * 13.5))[1]",
        &["(constant) array index out of bounds (0 <= 27 < 1)"],
    ),
    (
        "MulVf",
        "(('5 2 7' * 13.5))[2]",
        &["(constant) array index out of bounds (0 <= 94 < 1)"],
    ),
    (
        "MulVf",
        "(('6 3 1' * 3.5))[0]",
        &["(constant) array index out of bounds (0 <= 21 < 1)"],
    ),
    (
        "MulVf",
        "(('6 3 1' * 3.5))[1]",
        &["(constant) array index out of bounds (0 <= 10 < 1)"],
    ),
    (
        "MulVf",
        "(('6 3 1' * 3.5))[2]",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "MulVf",
        "(('7 3 1' * 7.5))[0]",
        &["(constant) array index out of bounds (0 <= 52 < 1)"],
    ),
    (
        "MulVf",
        "(('7 3 1' * 7.5))[1]",
        &["(constant) array index out of bounds (0 <= 22 < 1)"],
    ),
    (
        "MulVf",
        "(('7 3 1' * 7.5))[2]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "MulVi",
        "(('13 5 2' * 5i))[0]",
        &["(constant) array index out of bounds (0 <= 65 < 1)"],
    ),
    (
        "MulVi",
        "(('13 5 2' * 5i))[1]",
        &["(constant) array index out of bounds (0 <= 25 < 1)"],
    ),
    (
        "MulVi",
        "(('13 5 2' * 5i))[2]",
        &["(constant) array index out of bounds (0 <= 10 < 1)"],
    ),
    (
        "MulVi",
        "(('5 2 7' * 13i))[0]",
        &["(constant) array index out of bounds (0 <= 65 < 1)"],
    ),
    (
        "MulVi",
        "(('5 2 7' * 13i))[1]",
        &["(constant) array index out of bounds (0 <= 26 < 1)"],
    ),
    (
        "MulVi",
        "(('5 2 7' * 13i))[2]",
        &["(constant) array index out of bounds (0 <= 91 < 1)"],
    ),
    (
        "MulVi",
        "(('6 3 1' * 3i))[0]",
        &["(constant) array index out of bounds (0 <= 18 < 1)"],
    ),
    (
        "MulVi",
        "(('6 3 1' * 3i))[1]",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "MulVi",
        "(('6 3 1' * 3i))[2]",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "MulVi",
        "(('7 3 1' * 7i))[0]",
        &["(constant) array index out of bounds (0 <= 49 < 1)"],
    ),
    (
        "MulVi",
        "(('7 3 1' * 7i))[1]",
        &["(constant) array index out of bounds (0 <= 21 < 1)"],
    ),
    (
        "MulVi",
        "(('7 3 1' * 7i))[2]",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    ("NeF", "(7.5 != 7.5)", &[]),
    (
        "NeFi",
        "(7.5 != 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("NeI", "(7i != 7i)", &[]),
    ("NeI64", "((__int64)7i != (__int64)7i)", &[]),
    (
        "NeIf",
        "(7i != 7.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrF",
        "(7.5 || 7.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrFi",
        "(7.5 || 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrI",
        "(7i || 7i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "OrIf",
        "(7i || 7.5)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "RshiftF",
        "(7.5 >> 7.5)",
        &[
            "Constant float operand 7.500000 will be truncated to 7",
            "Constant float operand 7.500000 will be truncated to 7",
        ],
    ),
    (
        "RshiftFi",
        "(7.5 >> 7i)",
        &["Constant float operand 7.500000 will be truncated to 7"],
    ),
    ("RshiftI", "(7i >> 7i)", &[]),
    ("RshiftI64i", "((__int64)7i >> 7i)", &[]),
    (
        "RshiftIf",
        "(7i >> 7.5)",
        &["Constant float operand 7.500000 will be truncated to 7"],
    ),
    ("RshiftU", "((__uint)7i >> 7i)", &[]),
    ("SubF", "(7.5 - 7.5)", &[]),
    ("SubFi", "(7.5 - 7i)", &[]),
    ("SubI", "(7i - 7i)", &[]),
    ("SubI64", "((__int64)7i - (__int64)7i)", &[]),
    ("SubIf", "(7i - 7.5)", &[]),
    (
        "SubV",
        "(('13 5 2' - '5 2 7'))[0]",
        &["(constant) array index out of bounds (0 <= 8 < 1)"],
    ),
    (
        "SubV",
        "(('13 5 2' - '5 2 7'))[1]",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "SubV",
        "(('13 5 2' - '5 2 7'))[2]",
        &["(constant) array index out of bounds (0 <= -5 < 1)"],
    ),
    (
        "SubV",
        "(('5 2 7' - '13 5 2'))[0]",
        &["(constant) array index out of bounds (0 <= -8 < 1)"],
    ),
    (
        "SubV",
        "(('5 2 7' - '13 5 2'))[1]",
        &["(constant) array index out of bounds (0 <= -3 < 1)"],
    ),
    (
        "SubV",
        "(('5 2 7' - '13 5 2'))[2]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "SubV",
        "(('6 3 1' - '3 1 4'))[0]",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "SubV",
        "(('6 3 1' - '3 1 4'))[1]",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "SubV",
        "(('6 3 1' - '3 1 4'))[2]",
        &["(constant) array index out of bounds (0 <= -3 < 1)"],
    ),
    ("SubV", "(('7 3 1' - '7 3 1'))[0]", &[]),
    ("SubV", "(('7 3 1' - '7 3 1'))[1]", &[]),
    ("SubV", "(('7 3 1' - '7 3 1'))[2]", &[]),
];

// spec FTEF-26.2: each opcode with both operands constant folds to C's result of its operands
// spec FTEF-26.3: and warns of a division by 0 or an overflow as FTEQCC does
// spec FTEF-26.6: an unsigned division by 0 folds to 0, and a remainder by 0 folds through its
// emulation
// spec FTEF-31.8: a constant index is `QCC_Eval_Int` of it, its low word
// spec FTEF-31.9: a constant vector's component at a constant index is a constant
#[test]
fn every_folded_opcode_gives_fteqccs_value() {
    for &(op, expr, expected) in FOLDS {
        let found: Vec<String> = diags(&format!(
            "#pragma target fte\nfloat arr[1]; float x;\nvoid() g = {{ x = arr[{expr}]; }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect();
        assert_eq!(found, expected, "{op} {expr}");
    }
}

/// Every opcode `QCC_PR_StatementFlags` folds with one operand constant, as a global's
/// initialiser, where an opcode left to emit is "Unable to generate statements at global scope"
/// and a folded constant initialises: the pinned FTEQCC's diagnostics for each (2026-10-27).
const ONE_SIDED: &[(&str, &str, &str, &[&str])] = &[
    (
        "AddF",
        "float",
        "(0.0 + f)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "AddF",
        "float",
        "(1.0 + f)",
        &[
            "Unable to generate statements at global scope (ADD_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AddF",
        "float",
        "(2.0 + f)",
        &[
            "Unable to generate statements at global scope (ADD_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AddF",
        "float",
        "(f + 0.0)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "AddF",
        "float",
        "(f + 1.0)",
        &[
            "Unable to generate statements at global scope (ADD_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AddF",
        "float",
        "(f + 2.0)",
        &[
            "Unable to generate statements at global scope (ADD_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorF",
        "float",
        "(0.0 | f)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "BitorF",
        "float",
        "(1.0 | f)",
        &[
            "Unable to generate statements at global scope (BITOR).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorF",
        "float",
        "(2.0 | f)",
        &[
            "Unable to generate statements at global scope (BITOR).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorF",
        "float",
        "(f | 0.0)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "BitorF",
        "float",
        "(f | 1.0)",
        &[
            "Unable to generate statements at global scope (BITOR).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorF",
        "float",
        "(f | 2.0)",
        &[
            "Unable to generate statements at global scope (BITOR).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulF",
        "float",
        "(0.0 * f)",
        &[
            "Unable to generate statements at global scope (MUL_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulF",
        "float",
        "(1.0 * f)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "MulF",
        "float",
        "(2.0 * f)",
        &[
            "Unable to generate statements at global scope (MUL_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulF",
        "float",
        "(f * 0.0)",
        &[
            "Unable to generate statements at global scope (MUL_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulF",
        "float",
        "(f * 1.0)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "MulF",
        "float",
        "(f * 2.0)",
        &[
            "Unable to generate statements at global scope (MUL_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("BitandF", "float", "(0.0 & f)", &[]),
    (
        "BitandF",
        "float",
        "(1.0 & f)",
        &[
            "Unable to generate statements at global scope (BITAND).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandF",
        "float",
        "(2.0 & f)",
        &[
            "Unable to generate statements at global scope (BITAND).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandF",
        "float",
        "(f & 0.0)",
        &[
            "Unable to generate statements at global scope (BITAND).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandF",
        "float",
        "(f & 1.0)",
        &[
            "Unable to generate statements at global scope (BITAND).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandF",
        "float",
        "(f & 2.0)",
        &[
            "Unable to generate statements at global scope (BITAND).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("BitandFi", "int", "(0.0 & i)", &[]),
    (
        "BitandFi",
        "int",
        "(1.0 & i)",
        &[
            "Unable to generate statements at global scope (BITAND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandFi",
        "int",
        "(2.0 & i)",
        &[
            "Unable to generate statements at global scope (BITAND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandFi",
        "int",
        "(f & 0i)",
        &[
            "Unable to generate statements at global scope (BITAND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandFi",
        "int",
        "(f & 1i)",
        &[
            "Unable to generate statements at global scope (BITAND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandFi",
        "int",
        "(f & -1i)",
        &[
            "Unable to generate statements at global scope (BITAND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandFi",
        "int",
        "(f & 2i)",
        &[
            "Unable to generate statements at global scope (BITAND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("LshiftU", "__uint", "((__uint)0i << u)", &[]),
    (
        "LshiftU",
        "__uint",
        "((__uint)1i << u)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftU",
        "__uint",
        "((__uint)2i << u)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftU",
        "__uint",
        "(u << (__uint)0i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftU",
        "__uint",
        "(u << (__uint)1i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftU",
        "__uint",
        "(u << (__uint)2i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("RshiftU", "__uint", "((__uint)0i >> i)", &[]),
    (
        "RshiftU",
        "__uint",
        "((__uint)1i >> i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftU",
        "__uint",
        "((__uint)2i >> i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftU",
        "__uint",
        "(u >> 0i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftU",
        "__uint",
        "(u >> 1i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftU",
        "__uint",
        "(u >> -1i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftU",
        "__uint",
        "(u >> 2i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("LshiftI", "int", "(0i << i)", &[]),
    (
        "LshiftI",
        "int",
        "(1i << i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftI",
        "int",
        "(-1i << i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftI",
        "int",
        "(2i << i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftI",
        "int",
        "(i << 0i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftI",
        "int",
        "(i << 1i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftI",
        "int",
        "(i << -1i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "LshiftI",
        "int",
        "(i << 2i)",
        &[
            "Unable to generate statements at global scope (LSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("RshiftI", "int", "(0i >> i)", &[]),
    (
        "RshiftI",
        "int",
        "(1i >> i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftI",
        "int",
        "(-1i >> i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftI",
        "int",
        "(2i >> i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftI",
        "int",
        "(i >> 0i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftI",
        "int",
        "(i >> 1i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftI",
        "int",
        "(i >> -1i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "RshiftI",
        "int",
        "(i >> 2i)",
        &[
            "Unable to generate statements at global scope (RSHIFT_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("BitandI", "int", "(0i & i)", &[]),
    (
        "BitandI",
        "int",
        "(1i & i)",
        &[
            "Unable to generate statements at global scope (BITAND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandI",
        "int",
        "(-1i & i)",
        &[
            "Unable to generate statements at global scope (BITAND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandI",
        "int",
        "(2i & i)",
        &[
            "Unable to generate statements at global scope (BITAND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandI",
        "int",
        "(i & 0i)",
        &[
            "Unable to generate statements at global scope (BITAND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandI",
        "int",
        "(i & 1i)",
        &[
            "Unable to generate statements at global scope (BITAND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandI",
        "int",
        "(i & -1i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "BitandI",
        "int",
        "(i & 2i)",
        &[
            "Unable to generate statements at global scope (BITAND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("BitandIf", "int", "(0i & f)", &[]),
    (
        "BitandIf",
        "int",
        "(1i & f)",
        &[
            "Unable to generate statements at global scope (BITAND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandIf",
        "int",
        "(-1i & f)",
        &[
            "Unable to generate statements at global scope (BITAND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandIf",
        "int",
        "(2i & f)",
        &[
            "Unable to generate statements at global scope (BITAND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandIf",
        "int",
        "(i & 0.0)",
        &[
            "Unable to generate statements at global scope (BITAND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandIf",
        "int",
        "(i & 1.0)",
        &[
            "Unable to generate statements at global scope (BITAND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitandIf",
        "int",
        "(i & 2.0)",
        &[
            "Unable to generate statements at global scope (BITAND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("AndF", "float", "(0.0 && f)", &[]),
    (
        "AndF",
        "float",
        "(1.0 && f)",
        &[
            "Unable to generate statements at global scope (AND_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndF",
        "float",
        "(2.0 && f)",
        &[
            "Unable to generate statements at global scope (AND_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("AndF", "float", "(f && 0.0)", &[]),
    (
        "AndF",
        "float",
        "(f && 1.0)",
        &[
            "Unable to generate statements at global scope (AND_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndF",
        "float",
        "(f && 2.0)",
        &[
            "Unable to generate statements at global scope (AND_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("AndFi", "int", "(0.0 && i)", &[]),
    (
        "AndFi",
        "int",
        "(1.0 && i)",
        &[
            "Unable to generate statements at global scope (AND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndFi",
        "int",
        "(2.0 && i)",
        &[
            "Unable to generate statements at global scope (AND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("AndFi", "int", "(f && 0i)", &[]),
    (
        "AndFi",
        "int",
        "(f && 1i)",
        &[
            "Unable to generate statements at global scope (AND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndFi",
        "int",
        "(f && -1i)",
        &[
            "Unable to generate statements at global scope (AND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndFi",
        "int",
        "(f && 2i)",
        &[
            "Unable to generate statements at global scope (AND_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("AndI", "int", "(0i && i)", &[]),
    (
        "AndI",
        "int",
        "(1i && i)",
        &[
            "Unable to generate statements at global scope (AND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndI",
        "int",
        "(-1i && i)",
        &[
            "Unable to generate statements at global scope (AND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndI",
        "int",
        "(2i && i)",
        &[
            "Unable to generate statements at global scope (AND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("AndI", "int", "(i && 0i)", &[]),
    (
        "AndI",
        "int",
        "(i && 1i)",
        &[
            "Unable to generate statements at global scope (AND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndI",
        "int",
        "(i && -1i)",
        &[
            "Unable to generate statements at global scope (AND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndI",
        "int",
        "(i && 2i)",
        &[
            "Unable to generate statements at global scope (AND_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("AndIf", "int", "(0i && f)", &[]),
    (
        "AndIf",
        "int",
        "(1i && f)",
        &[
            "Unable to generate statements at global scope (AND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndIf",
        "int",
        "(-1i && f)",
        &[
            "Unable to generate statements at global scope (AND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndIf",
        "int",
        "(2i && f)",
        &[
            "Unable to generate statements at global scope (AND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("AndIf", "int", "(i && 0.0)", &[]),
    (
        "AndIf",
        "int",
        "(i && 1.0)",
        &[
            "Unable to generate statements at global scope (AND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AndIf",
        "int",
        "(i && 2.0)",
        &[
            "Unable to generate statements at global scope (AND_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrF",
        "float",
        "(0.0 || f)",
        &[
            "Result of comparison is constant",
            "Unable to generate statements at global scope (OR_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("OrF", "float", "(1.0 || f)", &[]),
    ("OrF", "float", "(2.0 || f)", &[]),
    (
        "OrF",
        "float",
        "(f || 0.0)",
        &[
            "Result of comparison is constant",
            "Unable to generate statements at global scope (OR_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrF",
        "float",
        "(f || 1.0)",
        &[
            "Result of comparison is constant",
            "Unable to generate statements at global scope (OR_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrF",
        "float",
        "(f || 2.0)",
        &[
            "Result of comparison is constant",
            "Unable to generate statements at global scope (OR_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrFi",
        "int",
        "(0.0 || i)",
        &[
            "Unable to generate statements at global scope (OR_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("OrFi", "int", "(1.0 || i)", &[]),
    ("OrFi", "int", "(2.0 || i)", &[]),
    (
        "OrFi",
        "int",
        "(f || 0i)",
        &[
            "Unable to generate statements at global scope (OR_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrFi",
        "int",
        "(f || 1i)",
        &[
            "Unable to generate statements at global scope (OR_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrFi",
        "int",
        "(f || -1i)",
        &[
            "Unable to generate statements at global scope (OR_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrFi",
        "int",
        "(f || 2i)",
        &[
            "Unable to generate statements at global scope (OR_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrI",
        "int",
        "(0i || i)",
        &[
            "Unable to generate statements at global scope (OR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("OrI", "int", "(1i || i)", &[]),
    ("OrI", "int", "(-1i || i)", &[]),
    ("OrI", "int", "(2i || i)", &[]),
    (
        "OrI",
        "int",
        "(i || 0i)",
        &[
            "Unable to generate statements at global scope (OR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrI",
        "int",
        "(i || 1i)",
        &[
            "Unable to generate statements at global scope (OR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrI",
        "int",
        "(i || -1i)",
        &[
            "Unable to generate statements at global scope (OR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrI",
        "int",
        "(i || 2i)",
        &[
            "Unable to generate statements at global scope (OR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrIf",
        "int",
        "(0i || f)",
        &[
            "Unable to generate statements at global scope (OR_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    ("OrIf", "int", "(1i || f)", &[]),
    ("OrIf", "int", "(-1i || f)", &[]),
    ("OrIf", "int", "(2i || f)", &[]),
    (
        "OrIf",
        "int",
        "(i || 0.0)",
        &[
            "Unable to generate statements at global scope (OR_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrIf",
        "int",
        "(i || 1.0)",
        &[
            "Unable to generate statements at global scope (OR_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "OrIf",
        "int",
        "(i || 2.0)",
        &[
            "Unable to generate statements at global scope (OR_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorI",
        "int",
        "(0i | i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "BitorI",
        "int",
        "(1i | i)",
        &[
            "Unable to generate statements at global scope (BITOR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorI",
        "int",
        "(-1i | i)",
        &[
            "Unable to generate statements at global scope (BITOR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorI",
        "int",
        "(2i | i)",
        &[
            "Unable to generate statements at global scope (BITOR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorI",
        "int",
        "(i | 0i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "BitorI",
        "int",
        "(i | 1i)",
        &[
            "Unable to generate statements at global scope (BITOR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorI",
        "int",
        "(i | -1i)",
        &[
            "Unable to generate statements at global scope (BITOR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "BitorI",
        "int",
        "(i | 2i)",
        &[
            "Unable to generate statements at global scope (BITOR_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AddI",
        "int",
        "(0i + i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "AddI",
        "int",
        "(1i + i)",
        &[
            "Unable to generate statements at global scope (ADD_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AddI",
        "int",
        "(-1i + i)",
        &[
            "Unable to generate statements at global scope (ADD_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AddI",
        "int",
        "(2i + i)",
        &[
            "Unable to generate statements at global scope (ADD_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AddI",
        "int",
        "(i + 0i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "AddI",
        "int",
        "(i + 1i)",
        &[
            "Unable to generate statements at global scope (ADD_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AddI",
        "int",
        "(i + -1i)",
        &[
            "Unable to generate statements at global scope (ADD_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "AddI",
        "int",
        "(i + 2i)",
        &[
            "Unable to generate statements at global scope (ADD_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulI",
        "int",
        "(0i * i)",
        &[
            "Unable to generate statements at global scope (MUL_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulI",
        "int",
        "(1i * i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "MulI",
        "int",
        "(-1i * i)",
        &[
            "Unable to generate statements at global scope (MUL_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulI",
        "int",
        "(2i * i)",
        &[
            "Unable to generate statements at global scope (MUL_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulI",
        "int",
        "(i * 0i)",
        &[
            "Unable to generate statements at global scope (MUL_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulI",
        "int",
        "(i * 1i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "MulI",
        "int",
        "(i * -1i)",
        &[
            "Unable to generate statements at global scope (MUL_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulI",
        "int",
        "(i * 2i)",
        &[
            "Unable to generate statements at global scope (MUL_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubF",
        "float",
        "(0.0 - f)",
        &[
            "Unable to generate statements at global scope (SUB_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubF",
        "float",
        "(1.0 - f)",
        &[
            "Unable to generate statements at global scope (SUB_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubF",
        "float",
        "(2.0 - f)",
        &[
            "Unable to generate statements at global scope (SUB_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubF",
        "float",
        "(f - 0.0)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "SubF",
        "float",
        "(f - 1.0)",
        &[
            "Unable to generate statements at global scope (SUB_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubF",
        "float",
        "(f - 2.0)",
        &[
            "Unable to generate statements at global scope (SUB_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivVf",
        "vector",
        "(v / 0.0)",
        &[
            "Unable to generate statements at global scope (DIV_VF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivVf",
        "vector",
        "(v / 1.0)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "DivVf",
        "vector",
        "(v / 2.0)",
        &[
            "Unable to generate statements at global scope (DIV_VF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivF",
        "float",
        "(0.0 / f)",
        &[
            "Unable to generate statements at global scope (DIV_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivF",
        "float",
        "(1.0 / f)",
        &[
            "Unable to generate statements at global scope (DIV_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivF",
        "float",
        "(2.0 / f)",
        &[
            "Unable to generate statements at global scope (DIV_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivF",
        "float",
        "(f / 0.0)",
        &[
            "Unable to generate statements at global scope (DIV_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivF",
        "float",
        "(f / 1.0)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "DivF",
        "float",
        "(f / 2.0)",
        &[
            "Unable to generate statements at global scope (DIV_F).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivIf",
        "float",
        "(0i / f)",
        &[
            "Unable to generate statements at global scope (DIV_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivIf",
        "float",
        "(1i / f)",
        &[
            "Unable to generate statements at global scope (DIV_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivIf",
        "float",
        "(-1i / f)",
        &[
            "Unable to generate statements at global scope (DIV_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivIf",
        "float",
        "(2i / f)",
        &[
            "Unable to generate statements at global scope (DIV_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivIf",
        "float",
        "(i / 0.0)",
        &[
            "Unable to generate statements at global scope (DIV_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivIf",
        "float",
        "(i / 1.0)",
        &[
            "Unable to generate statements at global scope (CONV_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivIf",
        "float",
        "(i / 2.0)",
        &[
            "Unable to generate statements at global scope (DIV_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulIf",
        "float",
        "(0i * f)",
        &[
            "Unable to generate statements at global scope (MUL_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulIf",
        "float",
        "(1i * f)",
        &[
            "Unable to generate statements at global scope (MUL_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulIf",
        "float",
        "(-1i * f)",
        &[
            "Unable to generate statements at global scope (MUL_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulIf",
        "float",
        "(2i * f)",
        &[
            "Unable to generate statements at global scope (MUL_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulIf",
        "float",
        "(i * 0.0)",
        &[
            "Unable to generate statements at global scope (MUL_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulIf",
        "float",
        "(i * 1.0)",
        &[
            "Unable to generate statements at global scope (CONV_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulIf",
        "float",
        "(i * 2.0)",
        &[
            "Unable to generate statements at global scope (MUL_IF).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubI",
        "int",
        "(0i - i)",
        &[
            "Unable to generate statements at global scope (SUB_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubI",
        "int",
        "(1i - i)",
        &[
            "Unable to generate statements at global scope (SUB_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubI",
        "int",
        "(-1i - i)",
        &[
            "Unable to generate statements at global scope (SUB_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubI",
        "int",
        "(2i - i)",
        &[
            "Unable to generate statements at global scope (SUB_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubI",
        "int",
        "(i - 0i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "SubI",
        "int",
        "(i - 1i)",
        &[
            "Unable to generate statements at global scope (SUB_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubI",
        "int",
        "(i - -1i)",
        &[
            "Unable to generate statements at global scope (SUB_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "SubI",
        "int",
        "(i - 2i)",
        &[
            "Unable to generate statements at global scope (SUB_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivI",
        "int",
        "(0i / i)",
        &[
            "Unable to generate statements at global scope (DIV_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivI",
        "int",
        "(1i / i)",
        &[
            "Unable to generate statements at global scope (DIV_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivI",
        "int",
        "(-1i / i)",
        &[
            "Unable to generate statements at global scope (DIV_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivI",
        "int",
        "(2i / i)",
        &[
            "Unable to generate statements at global scope (DIV_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivI",
        "int",
        "(i / 0i)",
        &[
            "Unable to generate statements at global scope (DIV_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivI",
        "int",
        "(i / 1i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "DivI",
        "int",
        "(i / -1i)",
        &[
            "Unable to generate statements at global scope (DIV_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivI",
        "int",
        "(i / 2i)",
        &[
            "Unable to generate statements at global scope (DIV_I).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivU",
        "__uint",
        "((__uint)0i / u)",
        &[
            "Unable to generate statements at global scope (DIV_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivU",
        "__uint",
        "((__uint)1i / u)",
        &[
            "Unable to generate statements at global scope (DIV_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivU",
        "__uint",
        "((__uint)2i / u)",
        &[
            "Unable to generate statements at global scope (DIV_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivU",
        "__uint",
        "(u / (__uint)0i)",
        &[
            "Unable to generate statements at global scope (DIV_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivU",
        "__uint",
        "(u / (__uint)1i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "DivU",
        "__uint",
        "(u / (__uint)2i)",
        &[
            "Unable to generate statements at global scope (DIV_U).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivFi",
        "float",
        "(0.0 / i)",
        &[
            "Unable to generate statements at global scope (DIV_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivFi",
        "float",
        "(1.0 / i)",
        &[
            "Unable to generate statements at global scope (DIV_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivFi",
        "float",
        "(2.0 / i)",
        &[
            "Unable to generate statements at global scope (DIV_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivFi",
        "float",
        "(f / 0i)",
        &[
            "Unable to generate statements at global scope (DIV_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivFi",
        "float",
        "(f / 1i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "DivFi",
        "float",
        "(f / -1i)",
        &[
            "Unable to generate statements at global scope (DIV_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "DivFi",
        "float",
        "(f / 2i)",
        &[
            "Unable to generate statements at global scope (DIV_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulFi",
        "float",
        "(0.0 * i)",
        &[
            "Unable to generate statements at global scope (MUL_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulFi",
        "float",
        "(1.0 * i)",
        &[
            "Unable to generate statements at global scope (MUL_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulFi",
        "float",
        "(2.0 * i)",
        &[
            "Unable to generate statements at global scope (MUL_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulFi",
        "float",
        "(f * 0i)",
        &[
            "Unable to generate statements at global scope (MUL_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulFi",
        "float",
        "(f * 1i)",
        &["initializer for 'y' is not constant"],
    ),
    (
        "MulFi",
        "float",
        "(f * -1i)",
        &[
            "Unable to generate statements at global scope (MUL_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
    (
        "MulFi",
        "float",
        "(f * 2i)",
        &[
            "Unable to generate statements at global scope (MUL_FI).",
            "initializer for 'y' is not constant",
        ],
    ),
];

// spec FTEF-26.3: with one operand constant, the opcodes that fold to it, to the other operand
// or to a constant do so, and the others are emitted
// spec FTEF-34.9: an opcode left at global scope is "Unable to generate statements at global
// scope"
#[test]
fn every_one_sided_fold_is_fteqccs() {
    for &(op, ty, expr, expected) in ONE_SIDED {
        let found: Vec<String> = diags(&format!(
            "#pragma target fte\nfloat f; int i; __uint u; vector v; {ty} y = {expr};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect();
        assert_eq!(found, expected, "{op} {expr}");
    }
}

// spec FTEF-26.7: a folded float to int is `QCC_MakeIntConst` of a `long long`, truncated with a
// warning when it does not fit
// spec FTEF-26.2: an overflow prints its float as C's `%f`, `nan` for a NaN
#[test]
fn a_float_made_an_int_is_truncated_to_32_bits() {
    let messages = |statement: &str| -> Vec<String> {
        diags(&format!("int i; void() g = {{ {statement} }};\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    assert_eq!(
        messages("i = 1i / 0;"),
        [
            "Numerical overflow. inf will be rounded to 2147483647",
            "Constant int operand 9223372036854775807 will be truncated to -1",
        ]
    );
    assert_eq!(
        messages("i = -1i / 0;"),
        [
            "Numerical overflow. -inf will be rounded to -2147483648",
            "Constant int operand 9223372036854775808 will be truncated to 0",
        ]
    );
    assert_eq!(
        messages("i = 0i / 0.0;"),
        ["Numerical overflow. nan will be rounded to 0"]
    );
}

// spec FTEF-26.8: a 64-bit constant made an int keeps its low word, warning of the truncation;
// under `ILP32` in the body an `l` literal is already 32 bits
#[test]
fn a_64_bit_constant_made_an_int_is_truncated() {
    let messages = |prefix: &str, statement: &str| -> Vec<String> {
        diags(&format!("{prefix}int i; void() g = {{ {statement} }};\n"))
            .into_iter()
            .map(|(_, message)| message)
            .collect()
    };
    assert_eq!(
        messages("", "i = 5000000000l;"),
        ["Numerical truncation of 12a05f200 to 0x2a05f200."]
    );
    assert_eq!(
        messages("", "i = -5000000000l;"),
        ["Numerical truncation of fffffffed5fa0e00 to 0xd5fa0e00."]
    );
    assert_eq!(
        messages("", "i = 4294967296l;"),
        ["Numerical truncation of 100000000 to 0."]
    );
    assert_eq!(messages("", "i = -1l;"), Vec::<String>::new());
    assert_eq!(
        messages("#pragma flag enable ILP32\n", "i = 5000000000l;"),
        ["numerical overflow"]
    );
}

// spec FTEF-18.12: `sizeof` of a name reads it as a type when one has the name
#[test]
fn sizeof_a_type_name() {
    let index = |source: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\n{source} float arr[1]; void() g = {{ arr[sizeof(T)] = 1; }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    assert_eq!(
        index("typedef vector T;"),
        ["(constant) array index out of bounds (0 <= 12 < 1)"]
    );
    assert_eq!(
        index("struct T { float a; vector b; };"),
        ["(constant) array index out of bounds (0 <= 16 < 1)"]
    );
    // A type written with its keyword names no value.
    assert_eq!(
        diags(
            "#pragma target fte\nstruct S { float a; }; S *p; void *q; float arr[1]; void() g = { \
             p = (struct S *)q; arr[sizeof(struct S)] = 1; };\n"
        )
        .into_iter()
        .map(|(_, message)| message)
        .collect::<Vec<_>>(),
        ["(constant) array index out of bounds (0 <= 4 < 1)"]
    );
}

// spec FTEF-19.52: an item of an enum of an integer base is a constant of that type, a float
// value converted with the conversion's warnings
#[test]
fn integer_enum_items_are_constants() {
    let messages = |source: &str| -> Vec<String> {
        diags(&format!(
            "#pragma target fte\n{source} float arr[1]; void() g = {{ arr[X] = 1; }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect()
    };
    assert_eq!(
        messages("enum E2 : int { C2 = 2, X };"),
        ["(constant) array index out of bounds (0 <= 3 < 1)"]
    );
    assert_eq!(
        messages("enum E5 : int { F = 2.5, X };"),
        [
            "Numerical overflow. 2.500000 will be rounded to 2",
            "(constant) array index out of bounds (0 <= 3 < 1)"
        ]
    );
    assert_eq!(
        messages("enum E6 : __uint32 { X = 5 };"),
        ["(constant) array index out of bounds (0 <= 5 < 1)"]
    );
    assert_eq!(
        messages("enum E3 : __uint32 { X = -1.5 };"),
        ["Numerical overflow. -1.500000 will be rounded to 0"]
    );
}

/// Constant expressions whose value the folding of one opcode feeds to the next, read as an
/// index: the pinned FTEQCC's diagnostics for each on the FTE target (2026-10-27).
const CHAINS: &[(&str, &[&str])] = &[
    (
        "('1 2 3' ^ '4 0 0')[0]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "('7 2 3' ^ '4 0 0')[0]",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "('1 2 3' | '4 0 0')[0]",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "('7 2 3' & '4 0 0')[0]",
        &["(constant) array index out of bounds (0 <= 4 < 1)"],
    ),
    (
        "5i / -1i",
        &["(constant) array index out of bounds (0 <= -5 < 1)"],
    ),
    (
        "-6i / -1i",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "13i % 5i",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "-13i % 5i",
        &["(constant) array index out of bounds (0 <= -3 < 1)"],
    ),
    (
        "13u % 5u",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "13u % 0u",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "13i % 0i",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "(2i && 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(2i && 0i)", &[]),
    ("(0i && 3i)", &[]),
    (
        "(2i || 0i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(0i || 0i)", &[]),
    (
        "(0i || 3i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(2i < 2.0)", &[]),
    ("(2i > 2.0)", &[]),
    ("(2.0 < 2i)", &[]),
    ("(2.0 > 2i)", &[]),
    (
        "(2i <= 2.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "(2i >= 2.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "(2i < 3.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "(3.0 > 2i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "(2i == 2.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(2i == 3.0)", &[]),
    (
        "(2.0 == 2i)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(3.0 == 2i)", &[]),
    (
        "(4i / 2.0)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "(4i / 0.0)",
        &["(constant) array index out of bounds (0 <= -1 < 1)"],
    ),
    (
        "(4.0 / 2i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "(4.0 / 0i)",
        &["(constant) array index out of bounds (0 <= -1 < 1)"],
    ),
    (
        "(8.0 / 4i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "(int)(-6l / -1l)",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "(int)(6l / 2l)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "(int)(6l / -1l)",
        &["(constant) array index out of bounds (0 <= -6 < 1)"],
    ),
    (
        "(1ul < 2ul)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(3ul < 2ul)", &[]),
    ("(1ul <= 1ul)", &["Result of comparison is constant"]),
    ("(2ul <= 1ul)", &["Result of comparison is constant"]),
    (
        "(int)~5.0",
        &["(constant) array index out of bounds (0 <= -6 < 1)"],
    ),
    (
        "~5.5",
        &[
            "Constant float operand 5.500000 will be truncated to 5",
            "(constant) array index out of bounds (0 <= -6 < 1)",
        ],
    ),
    (
        "(~'5 0 0')[0]",
        &["(constant) array index out of bounds (0 <= -6 < 1)"],
    ),
    (
        "(int)~'5 1 1'[1]",
        &["(constant) array index out of bounds (0 <= -2 < 1)"],
    ),
    (
        "(__uint)7.5",
        &[
            "Numerical overflow. 7.500000 will be rounded to 7",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "(__uint)-3.0",
        &["Numerical overflow. -3.000000 will be rounded to 0"],
    ),
    (
        "(__double)2.5 * 2",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(int)(float)(__double)7.25",
        &[
            "Numerical overflow. 7.250000 will be rounded to 7",
            "(constant) array index out of bounds (0 <= 7 < 1)",
        ],
    ),
    (
        "(float)(__double)3.75",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "(int)(__double)9.5",
        &[
            "Numerical overflow. 9.500000 will be rounded to 9",
            "(constant) array index out of bounds (0 <= 9 < 1)",
        ],
    ),
    (
        "(int)(__uint64)7u",
        &["(constant) array index out of bounds (0 <= 7 < 1)"],
    ),
    (
        "(int)(__int64)-3i",
        &["(constant) array index out of bounds (0 <= -3 < 1)"],
    ),
    (
        "(__int64)2.5",
        &[
            "Numerical overflow. 2.500000 will be rounded to 2",
            "(constant) array index out of bounds (0 <= 2 < 1)",
        ],
    ),
    (
        "(__int64)-7.5",
        &[
            "Numerical overflow. -7.500000 will be rounded to -7",
            "(constant) array index out of bounds (0 <= -7 < 1)",
        ],
    ),
    (
        "(int)(__uint64)8.5",
        &[
            "Numerical overflow. 8.500000 will be rounded to 8",
            "(constant) array index out of bounds (0 <= 8 < 1)",
        ],
    ),
    (
        "(int)(__uint64)-2.0",
        &["Numerical overflow. -2.000000 will be rounded to 0"],
    ),
    (
        "(float)(__int64)5",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(float)(__uint64)9ul",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "(float)9223372036854775807l",
        &["(constant) array index out of bounds (0 <= -1 < 1)"],
    ),
    (
        "(float)18446744073709551615ul",
        &["(constant) array index out of bounds (0 <= -1 < 1)"],
    ),
    (
        "(int)(__double)(__int64)5",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(int)(__double)18446744073709551615ul",
        &[
            "Numerical truncation of 0xffffffffffffffff to 0xffffffffffffffff.",
            "Numerical overflow. 18446744073709551616.000000 will be rounded to 9223372036854775807",
            "Numerical truncation of 7fffffffffffffff to 0xffffffff.",
            "(constant) array index out of bounds (0 <= -1 < 1)",
        ],
    ),
    (
        "(int)(float)(__uint64)5ul",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "3 + (int)0.0",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "(int)((__double)2.5 < (__double)3)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(int)((__double)3 < (__double)3)", &[]),
    (
        "(int)((__double)3 <= (__double)3)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(int)((__double)4 <= (__double)3)", &[]),
    (
        "(int)((__double)3 == (__double)3)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(int)((__double)3 != (__double)3)", &[]),
    ("(int)((__double)2 == (__double)3)", &[]),
    (
        "(int)((__double)2 != (__double)3)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "(int)((__double)3 > (__double)2)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(int)((__double)3 >= (__double)4)", &[]),
    (
        "(int)((__double)3 << 2i)",
        &["(constant) array index out of bounds (0 <= 12 < 1)"],
    ),
    (
        "(int)((__double)13 >> 2i)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "(int)((__double)-13 >> 1i)",
        &["(constant) array index out of bounds (0 <= -7 < 1)"],
    ),
    (
        "(int)((__double)12.5 & (__double)6)",
        &["(constant) array index out of bounds (0 <= 4 < 1)"],
    ),
    (
        "(int)((__double)12 | (__double)3)",
        &["(constant) array index out of bounds (0 <= 15 < 1)"],
    ),
    (
        "(int)((__double)12 ^ (__double)10)",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "(int)((__double)7 - (__double)9.5)",
        &[
            "Numerical overflow. -2.500000 will be rounded to -2",
            "(constant) array index out of bounds (0 <= -2 < 1)",
        ],
    ),
    (
        "(int)((__double)7 / (__double)2)",
        &[
            "Numerical overflow. 3.500000 will be rounded to 3",
            "(constant) array index out of bounds (0 <= 3 < 1)",
        ],
    ),
    (
        "(int)((__double)7 / (__double)0)",
        &[
            "Numerical overflow. inf will be rounded to 9223372036854775807",
            "Numerical truncation of 7fffffffffffffff to 0xffffffff.",
            "(constant) array index out of bounds (0 <= -1 < 1)",
        ],
    ),
    (
        "(int)(16ul >> 2i)",
        &["(constant) array index out of bounds (0 <= 4 < 1)"],
    ),
    (
        "(int)(18446744073709551615ul >> 60i)",
        &["(constant) array index out of bounds (0 <= 15 < 1)"],
    ),
    (
        "(int)(-16l >> 2i)",
        &["(constant) array index out of bounds (0 <= -4 < 1)"],
    ),
    (
        "(int)(5l << 3i)",
        &["(constant) array index out of bounds (0 <= 40 < 1)"],
    ),
    (
        "(int)(13ul / 4ul)",
        &[
            "Numerical overflow. 3.250000 will be rounded to 3",
            "(constant) array index out of bounds (0 <= 3 < 1)",
        ],
    ),
    (
        "(int)(13ul / 0ul)",
        &[
            "Numerical overflow. inf will be rounded to 2147483647",
            "Constant int operand 9223372036854775807 will be truncated to -1",
            "(constant) array index out of bounds (0 <= -1 < 1)",
        ],
    ),
    ("(int)(13l / 0l)", &[]),
    (
        "(int)(-9223372036854775807l / -1l)",
        &[
            "Numerical truncation of 7fffffffffffffff to 0xffffffff.",
            "(constant) array index out of bounds (0 <= -1 < 1)",
        ],
    ),
    (
        "(int)((__double)2 * 3i)",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "(int)(3i * (__double)2)",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "(int)(2.5 * (__double)2)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(int)(1l + 2.5)",
        &[
            "Numerical overflow. 3.500000 will be rounded to 3",
            "(constant) array index out of bounds (0 <= 3 < 1)",
        ],
    ),
    (
        "(int)(2.5 + 1l)",
        &[
            "Numerical overflow. 3.500000 will be rounded to 3",
            "(constant) array index out of bounds (0 <= 3 < 1)",
        ],
    ),
    (
        "(int)(7ul - 2u)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(int)(7u + 2l)",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "(int)((__double)2 + (__double)3)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(int)((__double)2 + (__double)-7)",
        &["(constant) array index out of bounds (0 <= -5 < 1)"],
    ),
    (
        "(int)(1ul < 2ul)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(int)(3ul < 2ul)", &[]),
    ("(int)(1ul <= 2ul)", &["Result of comparison is constant"]),
    ("(int)(3ul <= 2ul)", &["Result of comparison is constant"]),
    (
        "(int)(2ul > 1ul)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(int)(18446744073709551615ul < 1ul)", &[]),
    (
        "(int)(('3 0 0' ^ '5 0 0') * '1 0 0')",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "(int)(('3 6 0' ^ '5 3 0') * '0 1 0')",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(int)((__double)3 << 2i)",
        &["(constant) array index out of bounds (0 <= 12 < 1)"],
    ),
    (
        "(int)((__double)3 << 3i)",
        &["(constant) array index out of bounds (0 <= 24 < 1)"],
    ),
    (
        "(int)((__double)24 >> 2i)",
        &["(constant) array index out of bounds (0 <= 6 < 1)"],
    ),
    (
        "(int)((__double)24 >> 3i)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "(int)((__double)12 | (__double)5)",
        &["(constant) array index out of bounds (0 <= 13 < 1)"],
    ),
    (
        "(int)((__double)8 | (__double)1)",
        &["(constant) array index out of bounds (0 <= 9 < 1)"],
    ),
    (
        "(int)(5i % 0i)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(int)(5i % 3i)",
        &["(constant) array index out of bounds (0 <= 2 < 1)"],
    ),
    (
        "(int)(5u % 0u)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(int)(7u % 4u)",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    ("(int)(0 || 0)", &[]),
    (
        "(int)(0 || 2)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "(int)(2 || 0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "(int)(2 || 3)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(int)(0.0 || 0.0)", &[]),
    (
        "(int)(1.5 || 0.0)",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    (
        "(int)(5i / 0.0)",
        &[
            "Numerical overflow. inf will be rounded to 2147483647",
            "Constant int operand 9223372036854775807 will be truncated to -1",
            "(constant) array index out of bounds (0 <= -1 < 1)",
        ],
    ),
    (
        "(int)(5i / 2.0)",
        &[
            "Numerical overflow. 2.500000 will be rounded to 2",
            "(constant) array index out of bounds (0 <= 2 < 1)",
        ],
    ),
    (
        "(int)(5.0 / 0i)",
        &[
            "Numerical overflow. inf will be rounded to 2147483647",
            "Constant int operand 9223372036854775807 will be truncated to -1",
            "(constant) array index out of bounds (0 <= -1 < 1)",
        ],
    ),
    (
        "(int)(5.0 / 2i)",
        &[
            "Numerical overflow. 2.500000 will be rounded to 2",
            "(constant) array index out of bounds (0 <= 2 < 1)",
        ],
    ),
    (
        "(int)((-9223372036854775807l - 1l) / -1l)",
        &[
            "Numerical truncation of 7fffffffffffffff to 0xffffffff.",
            "(constant) array index out of bounds (0 <= -1 < 1)",
        ],
    ),
    (
        "(int)((-9223372036854775807l - 1l) / 2l)",
        &["Numerical truncation of c000000000000000 to 0."],
    ),
    (
        "(int)((-9223372036854775807l - 1l) / 1l)",
        &["Numerical truncation of 8000000000000000 to 0."],
    ),
    (
        "(int)(5l / -1l)",
        &["(constant) array index out of bounds (0 <= -5 < 1)"],
    ),
    (
        "(int)(5l / 1l)",
        &["(constant) array index out of bounds (0 <= 5 < 1)"],
    ),
    (
        "(int)(!'0 0 0')",
        &["(constant) array index out of bounds (0 <= 1 < 1)"],
    ),
    ("(int)(!'1 0 0')", &[]),
    ("(int)(!'0 0 2')", &[]),
    (
        "(int)(~5i)",
        &["(constant) array index out of bounds (0 <= -6 < 1)"],
    ),
    (
        "(int)(~0i)",
        &["(constant) array index out of bounds (0 <= -1 < 1)"],
    ),
    ("(int)(~-1i)", &[]),
    (
        "(int)(__int64)2.5",
        &[
            "Numerical overflow. 2.500000 will be rounded to 2",
            "(constant) array index out of bounds (0 <= 2 < 1)",
        ],
    ),
    (
        "(int)(__int64)3.0",
        &["(constant) array index out of bounds (0 <= 3 < 1)"],
    ),
    (
        "(int)(__int64)-2.5",
        &[
            "Numerical overflow. -2.500000 will be rounded to -2",
            "(constant) array index out of bounds (0 <= -2 < 1)",
        ],
    ),
    (
        "(int)(-1.0/0.0)",
        &[
            "Numerical overflow. -inf will be rounded to -2147483648",
            "Constant int operand 9223372036854775808 will be truncated to 0",
        ],
    ),
    (
        "(int)(1.0/0.0)",
        &[
            "Numerical overflow. inf will be rounded to 2147483647",
            "Constant int operand 9223372036854775807 will be truncated to -1",
            "(constant) array index out of bounds (0 <= -1 < 1)",
        ],
    ),
];

// spec FTEF-26.2: each fold gives C's value, which the next opcode folds in turn
// spec FTEF-26.3: the conversions of a constant fold, warning as FTEQCC does
// spec FTEF-26.9: two constant doubles fold as C computes them
// spec FTEF-26.10: a constant operand an opcode converts is the constant its conversion gives
#[test]
fn chained_folds_give_fteqccs_values() {
    for &(expr, expected) in CHAINS {
        let found: Vec<String> = diags(&format!(
            "#pragma target fte\nfloat arr[1]; float x;\nvoid() g = {{ x = arr[{expr}]; }};\n"
        ))
        .into_iter()
        .map(|(_, message)| message)
        .collect();
        assert_eq!(found, expected, "{expr}");
    }
}
