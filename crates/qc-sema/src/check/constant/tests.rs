// SPDX-License-Identifier: GPL-3.0-or-later

use qc_lexer::LexFlags;

use super::Constant;
use crate::cast::ConstType;

// spec FTEF-26.13: a constant's words read as each type `QCC_SRef_EvalConst` reads them
#[test]
fn a_constant_reads_as_its_words() {
    assert_eq!(Constant::double(2.5).d(), 2.5);
    assert_eq!(Constant::double(-7.0).d(), -7.0);
    assert_eq!(Constant::vector([1.5, -2.0, 3.0]).v(), [1.5, -2.0, 3.0]);
    assert_eq!(Constant::int64(-5).i64(), -5);
    assert_eq!(Constant::uint64(u64::MAX).u64(), u64::MAX);
}

// spec FTEF-26.3: `QCC_Eval_Truth`: a float, double, integer, 64-bit value or string not 0, a
// vector by its first component
#[test]
fn a_constant_is_true_when_it_is_not_zero() {
    let cases = [
        (Constant::float(0.0), false),
        (Constant::float(0.5), true),
        (Constant::double(0.0), false),
        (Constant::double(-0.25), true),
        (Constant::int(0), false),
        (Constant::int(-1), true),
        (Constant::uint(0), false),
        (Constant::uint(3), true),
        (Constant::int64(0), false),
        (Constant::int64(1_i64 << 40), true),
        (Constant::uint64(0), false),
        (Constant::uint64(1_u64 << 33), true),
        (Constant::vector([0.0, 1.0, 1.0]), false),
        (Constant::vector([1.0, 0.0, 0.0]), true),
        (Constant::string(true), false),
        (Constant::string(false), true),
    ];
    for (constant, truth) in cases {
        assert_eq!(constant.truth(), truth, "{constant:?}");
    }
}

// spec FTEF-26.1: `QCC_PR_LexNumber`'s value: octal after a `0`, hexadecimal after `0x`, digits
// of the base only, a fraction read whole, an integer made the literal's type
#[test]
fn a_number_is_read_by_its_base_and_type() {
    let flags = LexFlags::default();
    let number = |text: &str| Constant::of_number(text, flags);
    assert_eq!(number("010").f(), 8.0);
    assert_eq!(number("08").f(), 0.0);
    assert_eq!(number("0x10").f(), 16.0);
    assert_eq!(number("10").f(), 10.0);
    assert_eq!(number("-3").f(), -3.0);
    assert_eq!(number("2.5").f(), 2.5);
    assert_eq!(number("5i").i(), 5);
    let double = number("5d");
    assert_eq!(double.ty, ConstType::Double);
    assert_eq!(double.d(), 5.0);
    let fraction = number("2.5d");
    assert_eq!(fraction.ty, ConstType::Double);
    assert_eq!(fraction.d(), 2.5);
}
