// SPDX-License-Identifier: GPL-3.0-or-later

//! The folding arms and guards the program tests in `tests/folding.rs` reach only through
//! values no program writes, or with no message to tell them apart.

use qc_preproc::Op;

use super::{Folded, Notice, fold};
use crate::check::constant::Constant;

/// The folding of `op` and the messages it gives.
fn folded(op: Op, a: Option<Constant>, b: Option<Constant>) -> (Option<Folded>, Vec<String>) {
    let mut notices: Vec<Notice> = Vec::new();
    let result = fold(op, a, b, &mut notices);
    (result, notices.into_iter().map(|n| n.message).collect())
}

fn constant(op: Op, a: Constant, b: Constant) -> Option<Constant> {
    match folded(op, Some(a), Some(b)).0 {
        Some(Folded::Constant(c)) => Some(c),
        _ => None,
    }
}

// spec FTEF-26.2: two constant operands fold by the opcode's C operation, each arm its own
#[test]
fn arms_of_two_constants() {
    let int = Constant::int;
    let double = Constant::double;
    assert_eq!(
        constant(
            Op::BitxorV,
            Constant::vector([1.0, 2.0, 3.0]),
            Constant::vector([3.0, 3.0, 3.0])
        ),
        Some(Constant::vector([2.0, 1.0, 0.0]))
    );
    assert_eq!(constant(Op::ModI, int(7), int(3)), Some(int(1)));
    assert_eq!(
        constant(Op::ModU, Constant::uint(7), Constant::uint(3)),
        Some(Constant::uint(1))
    );
    assert_eq!(
        constant(Op::LtI64, Constant::int64(1), Constant::int64(1)),
        Some(int(0))
    );
    assert_eq!(
        constant(Op::LtI64, Constant::int64(1), Constant::int64(2)),
        Some(int(1))
    );
    assert_eq!(
        constant(Op::LshiftDi, double(3.0), int(2)),
        Some(double(12.0))
    );
    assert_eq!(
        constant(Op::RshiftDi, double(12.0), int(2)),
        Some(double(3.0))
    );
    assert_eq!(
        constant(Op::BitorD, double(4.0), double(1.0)),
        Some(double(5.0))
    );
    // `(bitofs<<8)|bits`: four bits at bit 4 of 0xF0, signed and unsigned.
    let mask = Constant::uint((4 << 8) | 4);
    assert_eq!(constant(Op::BitextendI, int(0xF0), mask), Some(int(-1)));
    assert_eq!(
        constant(Op::BitextendU, int(0xF0), mask),
        Some(Constant::uint(15))
    );
}

// spec FTEF-26.2: a float divided by an integer 0, or an integer by a float 0, warns
#[test]
fn mixed_divisions_by_zero() {
    let (_, by_zero) = folded(
        Op::DivIf,
        Some(Constant::int(1)),
        Some(Constant::float(0.0)),
    );
    assert_eq!(by_zero, ["Division of 1 by 0"]);
    let (_, fine) = folded(
        Op::DivIf,
        Some(Constant::int(1)),
        Some(Constant::float(2.0)),
    );
    assert!(fine.is_empty());
    let (_, by_zero) = folded(
        Op::DivFi,
        Some(Constant::float(1.5)),
        Some(Constant::int(0)),
    );
    assert_eq!(by_zero, ["Division of 1.5 by 0"]);
    let (_, fine) = folded(
        Op::DivFi,
        Some(Constant::float(1.5)),
        Some(Constant::int(2)),
    );
    assert!(fine.is_empty());
}

// spec FTEF-26.3: one constant operand: `!` of an entity or a function is a float, and a
// conversion that changes the value warns, of overflow or of a denormalized float
#[test]
fn unary_folds_and_conversions() {
    let (not, _) = folded(Op::NotEnt, Some(Constant::int(0)), None);
    assert_eq!(not, Some(Folded::Constant(Constant::float(1.0))));
    let (not, _) = folded(Op::NotFnc, Some(Constant::int(5)), None);
    assert_eq!(not, Some(Folded::Constant(Constant::float(0.0))));
    let (_, changed) = folded(Op::ConvDu64, Some(Constant::double(1.5)), None);
    assert_eq!(changed.len(), 1);
    let (_, same) = folded(Op::ConvDu64, Some(Constant::double(2.0)), None);
    assert!(same.is_empty());
    let (_, overflow) = folded(Op::ConvFtoi, Some(Constant::float(1.5)), None);
    assert_eq!(
        overflow,
        ["Numerical overflow. 1.500000 will be rounded to 1"]
    );
    let (_, denormal) = folded(Op::ConvFtoi, Some(Constant::float(1e-40)), None);
    assert!(
        denormal
            .first()
            .is_some_and(|m| m.starts_with("Denormalized float ")),
        "{denormal:?}"
    );
}

// spec FTEF-26.4: only the right operand constant: a vector divided by a float 0 warns, and a
// division by 1 is the left operand
#[test]
fn right_operand_divisions() {
    let (_, by_zero) = folded(Op::DivVf, None, Some(Constant::float(0.0)));
    assert_eq!(by_zero, ["Division by 0"]);
    let (_, fine) = folded(Op::DivVf, None, Some(Constant::float(2.0)));
    assert!(fine.is_empty());
    assert_eq!(
        folded(Op::DivF, None, Some(Constant::float(1.0))).0,
        Some(Folded::Left)
    );
    assert_eq!(folded(Op::DivF, None, Some(Constant::float(2.0))).0, None);
    let (_, by_zero) = folded(Op::DivI, None, Some(Constant::int(0)));
    assert_eq!(by_zero, ["Division by 0"]);
    let (_, fine) = folded(Op::DivI, None, Some(Constant::int(2)));
    assert!(fine.is_empty());
}
