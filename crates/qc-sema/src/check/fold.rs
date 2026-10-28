// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_StatementFlags, QCC_PR_RoundFloatConst

//! Constant arithmetic (FTEF-26): what `QCC_PR_StatementFlags` makes of an opcode whose
//! operands are constants before it emits anything, a constant or one of the operands, and the
//! warnings it gives on the way. C's arithmetic is written out: integers wrap, shift counts are
//! taken modulo the width, and a float out of an integer's range saturates, as on the arm64
//! build the differential is recorded on.

use qc_diag::Code;
use qc_preproc::Op;

use crate::check::constant::Constant;

/// What folding made of an opcode.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Folded {
    /// A constant.
    Constant(Constant),
    /// The left operand, unchanged: `0 + b` is `b`.
    Left,
    /// The right operand, unchanged: `a + 0` is `a`.
    Right,
}

/// A warning folding gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Notice {
    /// `W0233` for a division by 0 (`WARN_DIVISIONBY0`), `W0234` for an overflow
    /// (`WARN_OVERFLOW`).
    pub(crate) code: Code,
    /// FTEQCC's text.
    pub(crate) message: String,
}

/// The folding of `op` with the operands `a` and `b`, each its constant value if it has
/// one (FTEF-26.2 to FTEF-26.4); `None` when nothing is folded and the opcode is emitted.
pub(crate) fn fold(
    op: Op,
    a: Option<Constant>,
    b: Option<Constant>,
    notices: &mut Vec<Notice>,
) -> Option<Folded> {
    match (a, b) {
        (Some(a), Some(b)) => both(op, a, b, notices).map(Folded::Constant),
        (Some(a), None) => left(op, a, notices),
        (None, Some(b)) => right(op, b, notices),
        (None, None) => None,
    }
}

/// `QCC_MakeFloatConst` of a C truth value, `0.0` or `1.0`.
fn float_bool(value: bool) -> Constant {
    Constant::float(if value { 1.0 } else { 0.0 })
}

/// `QCC_MakeIntConst` of a C truth value.
fn int_bool(value: bool) -> Constant {
    Constant::int(i32::from(value))
}

/// FTEF-26.2: both operands constant.
#[expect(
    clippy::too_many_lines,
    reason = "one arm for each opcode QCC_PR_StatementFlags folds"
)]
#[expect(
    clippy::float_cmp,
    reason = "spec FTEF-26.2: C's exact float comparisons of the folded operands"
)]
fn both(op: Op, a: Constant, b: Constant, notices: &mut Vec<Notice>) -> Option<Constant> {
    let (fa, fb, ia, ib) = (a.f(), b.f(), a.i(), b.i());
    Some(match op {
        Op::AndAny | Op::AndFi | Op::AndIf => float_bool(a.truth() && b.truth()),
        Op::OrAny | Op::OrFi | Op::OrIf => float_bool(a.truth() || b.truth()),
        Op::BitxorI => Constant::int(ia ^ ib),
        Op::RshiftI => Constant::int(ia.wrapping_shr(shift(ib))),
        Op::LshiftI => Constant::int(ia.wrapping_shl(shift(ib))),
        Op::BitxorF => Constant::float(int_to_f32(round(a, notices) ^ round(b, notices))),
        Op::BitxorV => Constant::vector(lanes(a, b, |x, y| f_to_i(x) ^ f_to_i(y))),
        Op::RshiftF => {
            let (x, y) = (round(a, notices), round(b, notices));
            Constant::float(int_to_f32(x.wrapping_shr(shift(y))))
        }
        Op::RshiftIf => Constant::int(ia.wrapping_shr(shift(round(b, notices)))),
        Op::RshiftFi => Constant::int(round(a, notices).wrapping_shr(shift(ib))),
        Op::LshiftF => {
            let (x, y) = (round(a, notices), round(b, notices));
            Constant::float(int_to_f32(x.wrapping_shl(shift(y))))
        }
        Op::LshiftIf => Constant::int(ia.wrapping_shl(shift(round(b, notices)))),
        Op::LshiftFi => Constant::int(round(a, notices).wrapping_shl(shift(ib))),
        Op::BitorF => Constant::float(int_to_f32(round(a, notices) | round(b, notices))),
        Op::BitorV => Constant::vector(lanes(a, b, |x, y| f_to_i(x) | f_to_i(y))),
        Op::BitandF => Constant::float(int_to_f32(round(a, notices) & round(b, notices))),
        Op::BitandV => Constant::vector(lanes(a, b, |x, y| f_to_i(x) & f_to_i(y))),
        Op::MulF => Constant::float(fa * fb),
        Op::DivF => {
            if fb == 0.0 {
                notice(
                    notices,
                    Code::W0233,
                    format!("Division of {} by 0", c_g(fa.into())),
                );
            }
            Constant::float(fa / fb)
        }
        Op::AddF => Constant::float(fa + fb),
        Op::SubF => Constant::float(fa - fb),
        Op::BitorI => Constant::int(ia | ib),
        Op::BitandI => Constant::int(ia & ib),
        Op::MulI => Constant::int(ia.wrapping_mul(ib)),
        Op::MulIf => Constant::float(int_to_f32(ia) * fb),
        Op::MulFi => Constant::float(fa * int_to_f32(ib)),
        Op::DivI => {
            if ib == 0 {
                notice(notices, Code::W0233, "Division by constant 0".to_owned());
                Constant::int(0)
            } else if ib == -1 && ia == i32::MIN {
                notice(
                    notices,
                    Code::W0233,
                    "Signed overflow on division by -1".to_owned(),
                );
                Constant::int(i32::MAX)
            } else {
                Constant::int(ia.checked_div(ib).unwrap_or(0))
            }
        }
        Op::AddI => Constant::int(ia.wrapping_add(ib)),
        Op::SubI => Constant::int(ia.wrapping_sub(ib)),
        Op::ModI if ib != 0 => Constant::int(ia.checked_rem(ib).unwrap_or(0)),
        Op::ModU if b.u() != 0 => Constant::uint(a.u().checked_rem(b.u()).unwrap_or(0)),
        Op::ModF if fb != 0.0 => Constant::float(fa - fb * int_to_f32(f_to_i(fa / fb))),
        Op::AddIf => Constant::float(int_to_f32(ia) + fb),
        Op::AddFi => Constant::float(fa + int_to_f32(ib)),
        Op::SubIf => Constant::float(int_to_f32(ia) - fb),
        Op::SubFi => Constant::float(fa - int_to_f32(ib)),
        Op::AndI => int_bool(ia != 0 && ib != 0),
        Op::OrI => int_bool(ia != 0 || ib != 0),
        Op::AndF => int_bool(fa != 0.0 && fb != 0.0),
        Op::OrF => int_bool(fa != 0.0 || fb != 0.0),
        Op::MulV => {
            let (x, y) = (a.v(), b.v());
            Constant::float(x[0] * y[0] + x[1] * y[1] + x[2] * y[2])
        }
        Op::MulFv => Constant::vector(b.v().map(|y| fa * y)),
        Op::MulVf => Constant::vector(a.v().map(|x| x * fb)),
        Op::AddV => Constant::vector(lanes_f(a, b, |x, y| x + y)),
        Op::SubV => Constant::vector(lanes_f(a, b, |x, y| x - y)),
        Op::LeI => int_bool(ia <= ib),
        Op::GeI => int_bool(ia >= ib),
        Op::LtI => int_bool(ia < ib),
        Op::GtI => int_bool(ia > ib),
        Op::LeIf => int_bool(int_to_f32(ia) <= fb),
        Op::GeIf => int_bool(int_to_f32(ia) >= fb),
        Op::LtIf => int_bool(int_to_f32(ia) < fb),
        Op::GtIf => int_bool(int_to_f32(ia) > fb),
        Op::LeFi => int_bool(fa <= int_to_f32(ib)),
        Op::GeFi => int_bool(fa >= int_to_f32(ib)),
        Op::LtFi => int_bool(fa < int_to_f32(ib)),
        Op::GtFi => int_bool(fa > int_to_f32(ib)),
        Op::EqF => float_bool(fa == fb),
        Op::EqI => int_bool(ia == ib),
        Op::EqIf => int_bool(int_to_f32(ia) == fb),
        Op::EqFi => int_bool(fa == int_to_f32(ib)),
        Op::MulVi => Constant::vector(a.v().map(|x| x * int_to_f32(ib))),
        Op::MulIv => Constant::vector(b.v().map(|y| int_to_f32(ia) * y)),
        Op::DivIf => {
            if fb == 0.0 {
                notice(notices, Code::W0233, format!("Division of {ia} by 0"));
            }
            Constant::float(int_to_f32(ia) / fb)
        }
        Op::DivFi => {
            if ib == 0 {
                notice(
                    notices,
                    Code::W0233,
                    format!("Division of {} by 0", c_g(fa.into())),
                );
            }
            Constant::float(fa / int_to_f32(ib))
        }
        Op::BitandIf => Constant::int(ia & round(b, notices)),
        Op::BitorIf => Constant::int(ia | round(b, notices)),
        Op::BitandFi => Constant::int(round(a, notices) & ib),
        Op::BitorFi => Constant::int(round(a, notices) | ib),
        Op::NeF => float_bool(fa != fb),
        Op::NeI => int_bool(ia != ib),
        Op::NeIf => int_bool(int_to_f32(ia) != fb),
        Op::NeFi => int_bool(fa != int_to_f32(ib)),
        Op::LtU => Constant::uint(u32::from(a.u() < b.u())),
        Op::LeU => Constant::uint(u32::from(a.u() <= b.u())),
        // C's `_uint / _uint` with no check; arm64's `udiv` by 0 is 0.
        Op::DivU => Constant::uint(a.u().checked_div(b.u()).unwrap_or(0)),
        Op::RshiftU => Constant::uint(a.u().wrapping_shr(shift(ib))),
        Op::AddI64 => Constant::int64(a.i64().wrapping_add(b.i64())),
        Op::SubI64 => Constant::int64(a.i64().wrapping_sub(b.i64())),
        Op::MulI64 => Constant::int64(a.i64().wrapping_mul(b.i64())),
        Op::DivI64 => {
            if b.i64() == 0 {
                notice(notices, Code::W0233, "Division by constant 0".to_owned());
                Constant::int64(0)
            } else if b.i64() == -1 && a.i64() == i64::MIN {
                notice(
                    notices,
                    Code::W0233,
                    "Signed overflow on division by -1".to_owned(),
                );
                Constant::int64(i64::MAX)
            } else {
                Constant::int64(a.i64().checked_div(b.i64()).unwrap_or(0))
            }
        }
        Op::LshiftI64i => Constant::int64(a.i64().wrapping_shl(shift(ib))),
        Op::RshiftI64i => Constant::int64(a.i64().wrapping_shr(shift(ib))),
        Op::BitandI64 => Constant::int64(a.i64() & b.i64()),
        Op::BitorI64 => Constant::int64(a.i64() | b.i64()),
        Op::BitxorI64 => Constant::int64(a.i64() ^ b.i64()),
        Op::LeI64 => int_bool(a.i64() <= b.i64()),
        Op::LtI64 => int_bool(a.i64() < b.i64()),
        Op::EqI64 => int_bool(a.i64() == b.i64()),
        Op::NeI64 => int_bool(a.i64() != b.i64()),
        Op::LtU64 => int_bool(a.u64() < b.u64()),
        Op::LeU64 => int_bool(a.u64() <= b.u64()),
        Op::DivU64 => {
            if b.u64() == 0 {
                notice(notices, Code::W0233, "Division by constant 0".to_owned());
                Constant::uint(0)
            } else {
                Constant::uint64(a.u64().checked_div(b.u64()).unwrap_or(0))
            }
        }
        // A unary opcode reads only `a`; the constant `b` FTEQCC was handed changes nothing.
        _ => {
            return unary(op, a, notices).and_then(|f| match f {
                Folded::Constant(c) => Some(c),
                Folded::Left | Folded::Right => None,
            });
        }
    })
}

/// FTEF-26.3: only the left operand constant, which a unary opcode, a conversion among them,
/// reads alone.
#[expect(
    clippy::float_cmp,
    reason = "spec FTEF-26.3: C's exact comparisons of the operand with 0 and 1"
)]
fn left(op: Op, a: Constant, notices: &mut Vec<Notice>) -> Option<Folded> {
    if let Some(folded) = unary(op, a, notices) {
        return Some(folded);
    }
    let (fa, ia) = (a.f(), a.i());
    match op {
        Op::BitorF | Op::AddF if fa == 0.0 => Some(Folded::Right),
        Op::MulF if fa == 1.0 => Some(Folded::Right),
        Op::BitandF | Op::BitandFi if round(a, notices) == 0 => {
            Some(Folded::Constant(Constant::float(0.0)))
        }
        Op::LshiftU | Op::RshiftU if a.u() == 0 => Some(Folded::Constant(Constant::uint(0))),
        Op::LshiftI | Op::RshiftI | Op::BitandI | Op::BitandIf if ia == 0 => {
            Some(Folded::Constant(Constant::int(0)))
        }
        Op::AndAny | Op::AndF | Op::AndFi | Op::AndI | Op::AndIf if !a.truth() => {
            Some(Folded::Constant(Constant::float(0.0)))
        }
        Op::OrAny | Op::OrF | Op::OrFi | Op::OrI | Op::OrIf if a.truth() => {
            Some(Folded::Constant(Constant::float(1.0)))
        }
        Op::BitorI | Op::AddI if ia == 0 => Some(Folded::Right),
        Op::MulI if ia == 1 => Some(Folded::Right),
        _ => None,
    }
}

/// FTEF-26.3: the unary opcodes and conversions of a constant.
#[expect(
    clippy::float_cmp,
    reason = "spec FTEF-26.3: C compares a converted value with the original exactly"
)]
fn unary(op: Op, a: Constant, notices: &mut Vec<Notice>) -> Option<Folded> {
    let (fa, ia, ua) = (a.f(), a.i(), a.u());
    let c = match op {
        Op::BitnotF => Constant::float(int_to_f32(!round(a, notices))),
        Op::BitnotI => Constant::int(!ia),
        Op::BitnotV => Constant::vector(a.v().map(|x| int_to_f32(!f_to_i(x)))),
        Op::ConvFtoi => {
            let i = f_to_i(fa);
            if int_to_f32(i) != fa {
                overflow(notices, fa.into(), &i.to_string(), ia);
            }
            Constant::int(make_int(f_to_i64(fa), notices))
        }
        Op::ConvItof => {
            let f = int_to_f32(ia);
            if f_to_i(f) != ia {
                let message = format!(
                    "Numerical truncation of {} to {}.",
                    c_hash_x(u64::from(ua)),
                    c_hash_x(u64::from(u32::from_le_bytes(f_to_i(f).to_le_bytes())))
                );
                notice(notices, Code::W0234, message);
            }
            Constant::float(f)
        }
        Op::ConvFu => {
            let u = f_to_u(fa);
            if u_to_f32(u) != fa {
                overflow(notices, fa.into(), &u.to_string(), ia);
            }
            Constant::uint(u)
        }
        Op::ConvUf => {
            let f = u_to_f32(ua);
            if f_to_u(f) != ua {
                let message = format!(
                    "Numerical truncation of {} to {}.",
                    c_hash_x(u64::from(ua)),
                    c_hash_x(u64::from(f_to_u(f)))
                );
                notice(notices, Code::W0234, message);
            }
            Constant::float(f)
        }
        Op::ConvFd => Constant::double(fa.into()),
        Op::ConvDf => Constant::float(d_to_f32(a.d())),
        Op::ConvIi64 => Constant::int64(ia.into()),
        Op::ConvUi64 => Constant::uint64(ua.into()),
        Op::ConvI64i => {
            let wide = a.i64();
            let narrow = low(wide);
            if i64::from(narrow) != wide {
                let message = format!(
                    "Numerical truncation of {:x} to {}.",
                    u64::from_le_bytes(wide.to_le_bytes()),
                    c_hash_x(u64::from(u32::from_le_bytes(narrow.to_le_bytes())))
                );
                notice(notices, Code::W0234, message);
            }
            Constant::int(narrow)
        }
        Op::ConvFi64 | Op::ConvDi64 | Op::ConvFu64 | Op::ConvDu64 => wide_from_real(op, a, notices),
        Op::ConvI64f | Op::ConvU64f | Op::ConvI64d | Op::ConvU64d => real_from_wide(op, a, notices),
        _ => return None,
    };
    Some(Folded::Constant(c))
}

/// FTEF-26.3: `OP_CONV_FI64`, `OP_CONV_DI64`, `OP_CONV_FU64` and `OP_CONV_DU64` of a constant:
/// the value made a 64-bit integer, with "Numerical overflow. F will be rounded to N" when
/// `(pint64_t)value != value` (`(puint64_t)` for `OP_CONV_FU64`), N printed as the conversion
/// gives it.
#[expect(
    clippy::float_cmp,
    reason = "spec FTEF-26.3: C compares the converted value with the float exactly"
)]
fn wide_from_real(op: Op, a: Constant, notices: &mut Vec<Notice>) -> Constant {
    let value = if matches!(op, Op::ConvFi64 | Op::ConvFu64) {
        f64::from(a.f())
    } else {
        a.d()
    };
    let (changed, rounded, constant) = match op {
        Op::ConvFi64 | Op::ConvDi64 => {
            let i = real_to_i64(value);
            (
                i64_to_real(i, op) != value,
                i.to_string(),
                Constant::int64(i),
            )
        }
        Op::ConvFu64 => {
            let u = real_to_u64(value);
            (
                u64_to_real(u, op) != value,
                u.to_string(),
                Constant::uint64(u),
            )
        }
        _ => {
            let u = real_to_u64(value);
            let signed = i64::from_le_bytes(u.to_le_bytes());
            (
                i64_to_real(real_to_i64(value), op) != value,
                signed.to_string(),
                Constant::uint64(u),
            )
        }
    };
    if changed {
        let message = format!(
            "Numerical overflow. {} will be rounded to {rounded}",
            c_f(value)
        );
        notice(notices, Code::W0234, message);
    }
    constant
}

/// FTEF-26.3: `OP_CONV_I64F`, `OP_CONV_U64F`, `OP_CONV_I64D` and `OP_CONV_U64D` of a constant:
/// the 64-bit integer made a float or double, with "Numerical truncation of A to B." when the
/// result converted back differs, as `%#llx`.
fn real_from_wide(op: Op, a: Constant, notices: &mut Vec<Notice>) -> Constant {
    let unsigned = matches!(op, Op::ConvU64f | Op::ConvU64d);
    let bits = a.u64();
    let value = if unsigned {
        u64_to_real(bits, op)
    } else {
        i64_to_real(a.i64(), op)
    };
    // `(pint64_t)d != eval_a->u64` compares the unsigned value with the signed conversion.
    let back = if unsigned && op == Op::ConvU64f {
        real_to_u64(value)
    } else {
        u64::from_le_bytes(real_to_i64(value).to_le_bytes())
    };
    if back != bits {
        let shown = if unsigned { real_to_u64(value) } else { back };
        let message = format!(
            "Numerical truncation of {} to {}.",
            c_hash_x(bits),
            c_hash_x(shown)
        );
        notice(notices, Code::W0234, message);
    }
    if matches!(op, Op::ConvI64f | Op::ConvU64f) {
        Constant::float(d_to_f32(value))
    } else {
        Constant::double(value)
    }
}

/// FTEF-26.4: only the right operand constant.
#[expect(
    clippy::float_cmp,
    reason = "spec FTEF-26.4: C's exact comparisons of the operand with 0 and 1"
)]
fn right(op: Op, b: Constant, notices: &mut Vec<Notice>) -> Option<Folded> {
    let (fb, ib) = (b.f(), b.i());
    match op {
        // `QCC_Eval_Truth(eval_a, ...)` with `eval_a` NULL is `false`: `||` never folds here.
        Op::AndAny | Op::AndF | Op::AndFi | Op::AndI | Op::AndIf if !b.truth() => {
            Some(Folded::Constant(Constant::float(0.0)))
        }
        Op::BitorF | Op::SubF | Op::AddF if fb == 0.0 => Some(Folded::Left),
        Op::DivVf => {
            if fb == 0.0 {
                notice(notices, Code::W0233, "Division by 0".to_owned());
            }
            (fb == 1.0).then_some(Folded::Left)
        }
        Op::DivF | Op::DivIf => {
            if fb == 0.0 {
                notice(notices, Code::W0233, "Division by 0".to_owned());
            }
            (fb == 1.0).then_some(Folded::Left)
        }
        Op::MulF | Op::MulIf if fb == 1.0 => Some(Folded::Left),
        Op::BitorI | Op::SubI | Op::AddI | Op::AddPiw if ib == 0 => Some(Folded::Left),
        Op::DivI | Op::DivU | Op::DivFi => {
            if ib == 0 {
                notice(notices, Code::W0233, "Division by 0".to_owned());
            }
            (ib == 1).then_some(Folded::Left)
        }
        Op::MulI | Op::MulFi if ib == 1 => Some(Folded::Left),
        Op::BitandI if ib == !0 => Some(Folded::Left),
        _ => None,
    }
}

/// Adds the warning `code` with FTEQCC's text.
fn notice(notices: &mut Vec<Notice>, code: Code, message: String) {
    notices.push(Notice { code, message });
}

/// "Numerical overflow. %f will be rounded to N", or for a float whose exponent is 0
/// "Denormalized float %g will be rounded to N. This is probably not what you want."
fn overflow(notices: &mut Vec<Notice>, value: f64, rounded: &str, bits: i32) {
    let message = if bits & 0x7f80_0000 != 0 {
        format!(
            "Numerical overflow. {} will be rounded to {rounded}",
            c_f(value)
        )
    } else {
        format!(
            "Denormalized float {} will be rounded to {rounded}. This is probably not what you \
             want.",
            c_g(value)
        )
    };
    notice(notices, Code::W0234, message);
}

/// `QCC_PR_RoundFloatConst`: the float as an `int`, with "Constant float operand %f will be
/// truncated to %i" when that changes it.
#[expect(
    clippy::float_cmp,
    reason = "spec FTEF-26.2: `val != (float)ival` exactly"
)]
fn round(c: Constant, notices: &mut Vec<Notice>) -> i32 {
    let value = c.f();
    let rounded = f_to_i(value);
    if value != int_to_f32(rounded) {
        let message = format!(
            "Constant float operand {} will be truncated to {rounded}",
            c_f(f64::from(value))
        );
        notice(notices, Code::W0234, message);
    }
    rounded
}

/// The shift count C's `<<` and `>>` use on the builds FTEQCC runs on: the low bits of `count`.
fn shift(count: i32) -> u32 {
    u32::from_le_bytes(count.to_le_bytes())
}

/// The low 32 bits of `value`, C's `(int)` of a 64-bit integer.
fn low(value: i64) -> i32 {
    let [a, b, c, d, ..] = value.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}

/// Each component of two vectors through `f` as `int`s, `QCC_MakeVectorConst` of the results.
fn lanes(a: Constant, b: Constant, f: impl Fn(f32, f32) -> i32) -> [f32; 3] {
    let (x, y) = (a.v(), b.v());
    [
        int_to_f32(f(x[0], y[0])),
        int_to_f32(f(x[1], y[1])),
        int_to_f32(f(x[2], y[2])),
    ]
}

/// Each component of two vectors through `f`.
fn lanes_f(a: Constant, b: Constant, f: impl Fn(f32, f32) -> f32) -> [f32; 3] {
    let (x, y) = (a.v(), b.v());
    [f(x[0], y[0]), f(x[1], y[1]), f(x[2], y[2])]
}

/// C's `(int)` of a float: toward zero, saturating out of range as arm64 does.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec FTEF-26.2: C's float to int conversion, saturating on arm64"
)]
const fn f_to_i(value: f32) -> i32 {
    value as i32
}

/// C's `(long long)` of a float, saturating as arm64 does.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec FTEF-26.7: C's float to long long conversion, saturating on arm64"
)]
const fn f_to_i64(value: f32) -> i64 {
    value as i64
}

/// `QCC_MakeIntConst(llvalue)`: its low 32 bits, with "Constant int operand %llu will be
/// truncated to %i" when they are not the value (FTEF-26.7).
fn make_int(value: i64, notices: &mut Vec<Notice>) -> i32 {
    let [low @ .., _, _, _, _] = value.to_le_bytes();
    let truncated = i32::from_le_bytes(low);
    if i64::from(truncated) != value {
        let unsigned = u64::from_le_bytes(value.to_le_bytes());
        let message = format!("Constant int operand {unsigned} will be truncated to {truncated}");
        notice(notices, Code::W0234, message);
    }
    truncated
}

/// C's `(unsigned int)` of a float, saturating as arm64 does.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec FTEF-26.3: C's float to unsigned conversion, saturating on arm64"
)]
const fn f_to_u(value: f32) -> u32 {
    value as u32
}

/// C's `(float)` of an `int`, rounding to nearest.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec FTEF-26.2: C's int to float conversion rounds"
)]
const fn int_to_f32(value: i32) -> f32 {
    value as f32
}

/// C's `(float)` of an `unsigned int`, rounding to nearest.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec FTEF-26.3: C's unsigned to float conversion rounds"
)]
const fn u_to_f32(value: u32) -> f32 {
    value as f32
}

/// C's `(pint64_t)` of a float or double: toward zero, saturating as arm64 does.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec FTEF-26.3: C's double to 64-bit integer conversion, saturating on arm64"
)]
const fn real_to_i64(value: f64) -> i64 {
    value as i64
}

/// C's `(puint64_t)` of a float or double, saturating as arm64 does.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "spec FTEF-26.3: C's double to unsigned 64-bit conversion, saturating on arm64"
)]
const fn real_to_u64(value: f64) -> u64 {
    value as u64
}

/// C's `(float)` or `(double)` of a 64-bit integer, by the conversion `op` makes, as a double.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec FTEF-26.3: C's 64-bit integer to float and double conversions round"
)]
fn i64_to_real(value: i64, op: Op) -> f64 {
    if matches!(
        op,
        Op::ConvFi64 | Op::ConvFu64 | Op::ConvI64f | Op::ConvU64f
    ) {
        f64::from(value as f32)
    } else {
        value as f64
    }
}

/// C's `(float)` or `(double)` of an unsigned 64-bit integer, by the conversion `op` makes.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec FTEF-26.3: C's 64-bit integer to float and double conversions round"
)]
fn u64_to_real(value: u64, op: Op) -> f64 {
    if matches!(
        op,
        Op::ConvFi64 | Op::ConvFu64 | Op::ConvI64f | Op::ConvU64f
    ) {
        f64::from(value as f32)
    } else {
        value as f64
    }
}

/// C's `(float)` of a double, rounding to nearest.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec FTEF-26.3: C's double to float conversion rounds"
)]
const fn d_to_f32(value: f64) -> f32 {
    value as f32
}

/// C's `%#x`: `0x` and the hexadecimal digits, `0` alone for zero.
pub(crate) fn c_hash_x(value: u64) -> String {
    if value == 0 {
        "0".to_owned()
    } else {
        format!("{value:#x}")
    }
}

/// C's `%f` as the pinned FTEQCC's macOS build prints it: six decimals, `inf` and `-inf`, and
/// `nan` for any NaN.
fn c_f(value: f64) -> String {
    if value.is_nan() {
        "nan".to_owned()
    } else {
        format!("{value:.6}")
    }
}

/// C's `%g`: six significant digits, in `%e` form when the exponent is below -4 or at least 6,
/// trailing zeros removed.
pub(crate) fn c_g(value: f64) -> String {
    if value.is_nan() {
        return if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        }
        .to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    let scientific = format!("{value:.5e}");
    let exponent: i32 = scientific
        .split_once('e')
        .and_then(|(_, e)| e.parse().ok())
        .unwrap_or(0);
    if (-4..6).contains(&exponent) {
        let decimals = usize::try_from(5_i32.saturating_sub(exponent)).unwrap_or(0);
        trim_zeros(&format!("{value:.decimals$}"))
    } else {
        let (mantissa, _) = scientific.split_once('e').unwrap_or((&scientific, ""));
        let sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{}e{sign}{:02}",
            trim_zeros(mantissa),
            exponent.unsigned_abs()
        )
    }
}

/// `text` with the trailing zeros of its fraction, and a bare `.`, removed.
fn trim_zeros(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text.to_owned()
    }
}
