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
];

// spec FTEF-26.2: each opcode with both operands constant folds to C's result of its operands
// spec FTEF-26.3: and warns of a division by 0 or an overflow as FTEQCC does
// spec FTEF-26.6: an unsigned division by 0 folds to 0, and a remainder by 0 folds through its
// emulation
// spec FTEF-31.8: a constant index is `QCC_Eval_Int` of it, its low word
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
