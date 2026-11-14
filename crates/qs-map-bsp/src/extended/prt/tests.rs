// SPDX-License-Identifier: GPL-3.0-or-later

use super::fmt_number;

// spec QBSP-15.51: numbers are written as `{fmt}` writes them, the shortest digits that read
// back, with a two-digit exponent below 1e-4 or from 1e16
#[test]
fn numbers_as_fmt_writes_them() {
    assert_eq!(fmt_number(16.0), "16");
    assert_eq!(fmt_number(-0.5), "-0.5");
    assert_eq!(fmt_number(0.0), "0");
    assert_eq!(fmt_number(0.0001), "0.0001");
    assert_eq!(fmt_number(0.00001), "1e-05");
    assert_eq!(
        fmt_number(-3.552_713_678_800_501e-15),
        "-3.552713678800501e-15"
    );
    assert_eq!(fmt_number(1e16), "1e+16");
    assert_eq!(fmt_number(123_456.75), "123456.75");
}
