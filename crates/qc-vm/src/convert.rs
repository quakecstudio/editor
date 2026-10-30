// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_exec.c, PR_ExecuteProgram

//! C's conversions between `int`, `float` and `double`, as id's x86 build performs them.
//!
//! Implements the conversions of VM-9.1 and VM-9.2, which the builtins share (BUILTIN-2.1,
//! BUILTIN-2.2, BUILTIN-6): the VM's `BITAND` and `BITOR` and every builtin that uses a float
//! as an integer or stores a double into a `float` go through these functions.

/// The x86 "integer indefinite" value, the result VM-9.2 and BUILTIN-2.2 fix for converting a
/// NaN, an infinity or an out-of-range value to `int`.
///
/// `VM_FLOAT_TO_INT_INVALID` in VM-9 (`docs/spec/vm.md`).
pub const VM_FLOAT_TO_INT_INVALID: i32 = -2_147_483_648;

/// `(int)value` for a `float`: truncation toward zero (VM-9.1, BUILTIN-2.1), and
/// [`VM_FLOAT_TO_INT_INVALID`] for a NaN, an infinity or a value outside the range of `int`
/// (VM-9.2, BUILTIN-2.2).
#[must_use]
pub fn float_to_int(value: f32) -> i32 {
    double_to_int(f64::from(value))
}

/// `(int)value` for a `double`, as [`float_to_int`] (BUILTIN-2.1, BUILTIN-2.2).
#[must_use]
pub fn double_to_int(value: f64) -> i32 {
    let truncated = value.trunc();
    // Both bounds are exact in f64, so the range test is exact; a NaN fails it.
    if truncated >= f64::from(i32::MIN) && truncated <= f64::from(i32::MAX) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec VM-9.1, BUILTIN-2.1: an integral value in range converts exactly"
        )]
        let int = truncated as i32;
        int
    } else {
        VM_FLOAT_TO_INT_INVALID
    }
}

/// `(float)value` for an `int`, rounded to nearest even, so a value above 2^24 can lose its
/// low bits (VM-9.1).
#[must_use]
pub fn int_to_float(value: i32) -> f32 {
    #[expect(
        clippy::cast_precision_loss,
        reason = "spec VM-9.1: C's (float)n rounds to nearest even, which `as f32` is"
    )]
    let float = value as f32;
    float
}

/// `(float)value` for a `double`, rounded to nearest, as C's store of a double expression into
/// a `float` (BUILTIN-6).
#[must_use]
pub fn double_to_float(value: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec BUILTIN-6: C's (float)x rounds a double to nearest, which `as f32` is"
    )]
    let float = value as f32;
    float
}

#[cfg(test)]
mod tests {
    use super::{
        VM_FLOAT_TO_INT_INVALID, double_to_float, double_to_int, float_to_int, int_to_float,
    };

    // spec VM-9.1: (int) truncates toward zero
    // spec BUILTIN-2.1: 1.9 is 1 and -1.9 is -1
    #[test]
    fn float_to_int_truncates_toward_zero() {
        assert_eq!(float_to_int(1.9), 1);
        assert_eq!(float_to_int(-1.9), -1);
        assert_eq!(float_to_int(-0.0), 0);
        assert_eq!(double_to_int(-2_147_483_648.9), i32::MIN);
        assert_eq!(double_to_int(2_147_483_647.9), i32::MAX);
    }

    // spec VM-9.2: a NaN, an infinity or a value past the int range is -2147483648
    // spec BUILTIN-2.2: the x86 answer, 0x80000000, for the float and double conversions
    #[test]
    fn out_of_range_conversions_are_integer_indefinite() {
        assert_eq!(VM_FLOAT_TO_INT_INVALID, i32::MIN);
        for value in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::from_bits(0x4F00_0000), // 2^31
            -3e9,
        ] {
            assert_eq!(float_to_int(value), VM_FLOAT_TO_INT_INVALID, "{value}");
        }
        for value in [f64::NAN, 2_147_483_648.0, -2_147_483_649.0] {
            assert_eq!(double_to_int(value), VM_FLOAT_TO_INT_INVALID, "{value}");
        }
    }

    // spec VM-9.1: (float)n rounds to nearest even above 2^24
    #[test]
    fn int_to_float_rounds_to_nearest_even() {
        assert_eq!(int_to_float(-7).to_bits(), (-7.0_f32).to_bits());
        assert_eq!(
            int_to_float(16_777_217).to_bits(),
            16_777_216.0_f32.to_bits()
        );
        assert_eq!(
            int_to_float(16_777_219).to_bits(),
            16_777_220.0_f32.to_bits()
        );
    }

    // spec BUILTIN-6.8: a double result stored into a float rounds to nearest
    #[test]
    fn double_to_float_rounds_to_nearest() {
        assert_eq!(double_to_float(0.1).to_bits(), 0.1_f32.to_bits());
        assert_eq!(
            double_to_float(1.0 + f64::EPSILON).to_bits(),
            1.0_f32.to_bits()
        );
        assert!(double_to_float(f64::MAX).is_infinite());
    }
}
