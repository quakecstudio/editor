// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! Unit tests for the vanilla builtins and the table.
//!
//! The math and string vectors are the test tables of `docs/spec/builtins.md`; each test cites
//! the statement it pins. Floats are compared bit for bit, as the spec gives the exact f32.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed lookup is a test failure"
)]

use crate::math;
use crate::table::{self, BUILTIN_MAX_NUMBER, Class};

/// Compares two f32 by bits, so NaNs and signed zeros are distinguished (DIFF-9.4).
fn same_bits(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits()
}

fn vec_same(a: [f32; 3], b: [f32; 3]) -> bool {
    same_bits(a[0], b[0]) && same_bits(a[1], b[1]) && same_bits(a[2], b[2])
}

// spec BUILTIN-5.1: the table has builtins 1 to 78, with the PF_Fixme gaps absent.
#[test]
fn table_has_the_vanilla_numbers() {
    // The present numbers: 1 to 78 minus the gaps (BUILTIN-1.2).
    let gaps = [5u32, 33, 39, 42, 50, 60, 61, 62, 63, 64, 65, 66, 71];
    for number in 1..=BUILTIN_MAX_NUMBER {
        let present = table::lookup(number).is_some();
        if gaps.contains(&number) {
            assert!(!present, "number {number} should be a PF_Fixme gap");
            assert!(table::is_fixme(number), "{number} should be is_fixme");
        } else {
            assert!(present, "number {number} should be a builtin");
            assert!(!table::is_fixme(number), "{number} should not be is_fixme");
        }
    }
}

// spec BUILTIN-5.2: three numbers share a C function under two names.
#[test]
fn precache_variants_share_a_c_function() {
    assert_eq!(table::lookup(75).unwrap().c_function, "PF_precache_model");
    assert_eq!(table::lookup(76).unwrap().c_function, "PF_precache_sound");
    assert_eq!(table::lookup(77).unwrap().c_function, "PF_precache_file");
    assert_eq!(table::lookup(75).unwrap().name, "precache_model2");
}

// spec BUILTIN-4.4: the class world builtins are the ones a mock cannot reproduce.
#[test]
fn world_builtins_have_the_world_class() {
    for number in [16u32, 17, 32, 34, 40, 41, 44, 67] {
        assert_eq!(table::lookup(number).unwrap().class, Class::World);
    }
}

// spec BUILTIN-7.1: ftos of a whole value is "%d"; a fraction is "%5.1f".
#[test]
fn ftos_whole_and_fraction() {
    assert_eq!(math::ftos(0.0), "0");
    assert_eq!(math::ftos(-0.0), "0");
    assert_eq!(math::ftos(1.0), "1");
    assert_eq!(math::ftos(-5.0), "-5");
    assert_eq!(math::ftos(0.5), "  0.5");
    assert_eq!(math::ftos(1.25), "  1.2");
    assert_eq!(math::ftos(1.35), "  1.4");
    assert_eq!(math::ftos(0.05), "  0.1");
    assert_eq!(math::ftos(-0.04), " -0.0");
}

// spec BUILTIN-7.6: ftos of a NaN or infinity is fixed.
#[test]
fn ftos_nan_and_infinity() {
    assert_eq!(math::ftos(f32::NAN), "  nan");
    assert_eq!(math::ftos(f32::INFINITY), "  inf");
    assert_eq!(math::ftos(f32::NEG_INFINITY), " -inf");
}

// spec BUILTIN-7.2: vtos is '%5.1f %5.1f %5.1f' with the single quotes.
#[test]
fn vtos_formats_each_component() {
    assert_eq!(math::vtos([0.0, 0.0, 0.0]), "'  0.0   0.0   0.0'");
    assert_eq!(math::vtos([1.0, -2.0, 3.5]), "'  1.0  -2.0   3.5'");
}

// spec BUILTIN-7.7: vtos of NaN, infinity, minus infinity.
#[test]
fn vtos_nan_and_infinity() {
    assert_eq!(
        math::vtos([f32::NAN, f32::INFINITY, f32::NEG_INFINITY]),
        "'  nan   inf  -inf'"
    );
}

// spec BUILTIN-6.7: rint rounds halves away from zero; 0 and -0 give +0.
#[test]
fn rint_rounds_halves_away_from_zero() {
    assert!(same_bits(math::rint(0.0), 0.0));
    assert!(same_bits(math::rint(-0.0), 0.0));
    assert_eq!(math::rint(0.5), 1.0);
    assert_eq!(math::rint(-0.5), -1.0);
    assert_eq!(math::rint(1.5), 2.0);
    assert_eq!(math::rint(2.5), 3.0);
    assert_eq!(math::rint(-2.5), -3.0);
    assert_eq!(math::rint(0.499_999_97), 0.0);
    assert_eq!(math::rint(-1.7), -2.0);
    assert_eq!(math::rint(3.2), 3.0);
}

// spec BUILTIN-6.8: floor, ceil keep a zero's sign; fabs clears the sign bit.
#[test]
fn floor_ceil_fabs() {
    assert_eq!(math::floor(-0.5), -1.0);
    assert!(same_bits(math::ceil(-0.5), -0.0));
    assert_eq!(math::ceil(1.2), 2.0);
    assert_eq!(math::floor(1.8), 1.0);
    assert!(same_bits(math::fabs(-0.0), 0.0));
}

// spec BUILTIN-6.2: vlen is (f32)sqrt(x*x + y*y + z*z).
#[test]
fn vlen_values() {
    assert_eq!(math::vlen([3.0, 4.0, 0.0]), 5.0);
    assert_eq!(math::vlen([1.0, 1.0, 1.0]), 1.732_050_8);
    assert_eq!(math::vlen([0.0, 0.0, 0.0]), 0.0);
}

// spec BUILTIN-6.3: normalize is '0 0 0' at length 0, else v * (f32)(1/len).
#[test]
fn normalize_values() {
    assert!(vec_same(math::normalize([3.0, 4.0, 0.0]), [0.6, 0.8, 0.0]));
    assert!(vec_same(
        math::normalize([0.0, 0.0, -2.0]),
        [0.0, 0.0, -1.0]
    ));
    assert!(vec_same(math::normalize([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]));
}

// spec BUILTIN-6.4: vectoyaw is a whole yaw from 0 to 359.
#[test]
fn vectoyaw_values() {
    assert_eq!(math::vectoyaw([1.0, 0.0, 0.0]), 0.0);
    assert_eq!(math::vectoyaw([0.0, 1.0, 0.0]), 90.0);
    assert_eq!(math::vectoyaw([-1.0, 0.0, 0.0]), 180.0);
    assert_eq!(math::vectoyaw([0.0, -1.0, 0.0]), 270.0);
    assert_eq!(math::vectoyaw([1.0, -1.0, 0.0]), 315.0);
    assert_eq!(math::vectoyaw([1.0, 0.01, 0.0]), 0.0);
    assert_eq!(math::vectoyaw([-1.0, -0.01, 0.0]), 181.0);
    assert_eq!(math::vectoyaw([0.0, 0.0, 5.0]), 0.0);
}

// spec BUILTIN-6.5: vectoangles is 'pitch yaw 0', with '0 0 0' giving '270 0 0'.
#[test]
fn vectoangles_values() {
    assert!(vec_same(
        math::vectoangles([0.0, 0.0, 1.0]),
        [90.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([0.0, 0.0, -1.0]),
        [270.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([0.0, 0.0, 0.0]),
        [270.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([1.0, 0.0, 1.0]),
        [45.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([1.0, 0.0, -1.0]),
        [315.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([-3.0, 4.0, 5.0]),
        [45.0, 126.0, 0.0]
    ));
}

// spec BUILTIN-6.1: makevectors of zero angles gives forward '1 0 -0'.
#[test]
fn makevectors_zero_angles() {
    let (forward, right, up) = math::makevectors([0.0, 0.0, 0.0]);
    assert!(vec_same(forward, [1.0, 0.0, -0.0]));
    assert!(vec_same(right, [0.0, -1.0, -0.0]));
    assert!(vec_same(up, [0.0, 0.0, 1.0]));
}

// spec BUILTIN-15.5: anglemod wraps to 0 up to 360 and truncates to a multiple of 360/65536.
#[test]
fn anglemod_values() {
    assert_eq!(math::anglemod(0.0), 0.0);
    assert_eq!(math::anglemod(20.0), 19.995_117);
    assert_eq!(math::anglemod(-90.0), 270.0);
    assert_eq!(math::anglemod(360.0), 0.0);
    assert_eq!(math::anglemod(45.5), 45.499_878);
    assert_eq!(math::anglemod(720.5), 0.499_877_93);
}

// spec BUILTIN-2.1: a float used as an integer truncates toward zero.
#[test]
fn float_to_int_truncates() {
    assert_eq!(math::float_to_int(1.9), 1);
    assert_eq!(math::float_to_int(-1.9), -1);
    assert_eq!(math::float_to_int(0.0), 0);
}

// spec BUILTIN-2.2: an out-of-range float or a NaN converts to 0x80000000.
#[test]
fn float_to_int_saturates() {
    assert_eq!(math::float_to_int(f32::NAN), i32::MIN);
    assert_eq!(math::float_to_int(1e30), i32::MIN);
    assert_eq!(math::float_to_int(-1e30), i32::MIN);
}

// spec BUILTIN-6.6: random of a 15-bit value is (r & 0x7fff) / (f32)0x7fff.
#[test]
fn random_bounds() {
    assert_eq!(math::random(32767), 1.0);
    assert_eq!(math::random(1), 3.051_851e-5);
    assert_eq!(math::random(0), 0.0);
}
