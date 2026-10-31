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
use qc_vm::convert::float_to_int;

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

// spec BUILTIN-6.1: makevectors rounds the angle to f32 before sine and cosine, as id's
// AngleVectors stores it in a float; a 360-degree yaw gives id's f32-rounded sine, bits
// 0x343bbd2e (about 1.748e-7), not the f64 value bits 0xa58d3132 (about -2.449e-16).
#[test]
fn makevectors_full_yaw_rounds_to_f32() {
    let (forward, right, up) = math::makevectors([0.0, 360.0, 0.0]);
    // forward = (cp*cy, cp*sy, -sp): cy ~ 1, sy is the f32-rounded sine of 2*PI.
    assert_eq!(forward[0].to_bits(), 0x3f80_0000); // 1.0
    assert_eq!(forward[1].to_bits(), 0x343b_bd2e); // ~1.748e-7, not ~-2.449e-16
    assert_eq!(forward[2].to_bits(), 0x8000_0000); // -0.0
    assert_eq!(right[0].to_bits(), 0x343b_bd2e);
    assert_eq!(right[1].to_bits(), 0xbf80_0000); // -1.0
    assert_eq!(right[2].to_bits(), 0x8000_0000); // -0.0
    assert!(vec_same(up, [0.0, 0.0, 1.0]));
}

// spec BUILTIN-6.1: makevectors of a representative non-axis yaw matches id's AngleVectors
// bit for bit (yaw 45: sin and cos are 0x3f3504f3, about 0.70710677).
#[test]
fn makevectors_non_axis_yaw() {
    let (forward, right, up) = math::makevectors([0.0, 45.0, 0.0]);
    assert_eq!(forward[0].to_bits(), 0x3f35_04f3);
    assert_eq!(forward[1].to_bits(), 0x3f35_04f3);
    assert_eq!(forward[2].to_bits(), 0x8000_0000); // -0.0
    assert_eq!(right[0].to_bits(), 0x3f35_04f3);
    assert_eq!(right[1].to_bits(), 0xbf35_04f3);
    assert_eq!(right[2].to_bits(), 0x8000_0000); // -0.0
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
    assert_eq!(float_to_int(1.9), 1);
    assert_eq!(float_to_int(-1.9), -1);
    assert_eq!(float_to_int(0.0), 0);
}

// spec BUILTIN-2.2: an out-of-range float or a NaN converts to 0x80000000.
#[test]
fn float_to_int_saturates() {
    assert_eq!(float_to_int(f32::NAN), i32::MIN);
    assert_eq!(float_to_int(1e30), i32::MIN);
    assert_eq!(float_to_int(-1e30), i32::MIN);
}

// spec BUILTIN-6.6: random of a 15-bit value is (r & 0x7fff) / (f32)0x7fff.
#[test]
fn random_bounds() {
    assert_eq!(math::random(32767), 1.0);
    assert_eq!(math::random(1), 3.051_851e-5);
    assert_eq!(math::random(0), 0.0);
}

// spec BUILTIN-6.1: makevectors with a non-zero pitch, yaw and roll exercises every product
// and sum of AngleVectors; the three vectors match id's form bit for bit (pitch 30, yaw 60,
// roll 45), so a changed operator in forward, right or up is caught.
#[test]
fn makevectors_pitch_yaw_roll() {
    let (forward, right, up) = math::makevectors([30.0, 60.0, 45.0]);
    assert_eq!(forward[0].to_bits(), 0x3edd_b3d6);
    assert_eq!(forward[1].to_bits(), 0x3f40_0000);
    assert_eq!(forward[2].to_bits(), 0xbf00_0000);
    assert_eq!(right[0].to_bits(), 0x3edf_0669);
    assert_eq!(right[1].to_bits(), 0xbf28_e4b2);
    assert_eq!(right[2].to_bits(), 0xbf1c_c470);
    assert_eq!(up[0].to_bits(), 0x3f4a_05ae);
    assert_eq!(up[1].to_bits(), 0xbd42_0408);
    assert_eq!(up[2].to_bits(), 0x3f1c_c470);
}

// spec BUILTIN-6.5: vectoangles of a straight-up vector gives pitch 90, and a straight-down
// one gives 270 (the v[2] > 0 branch), so the sign test is pinned.
#[test]
fn vectoangles_vertical_pitch() {
    assert!(vec_same(
        math::vectoangles([0.0, 0.0, 2.0]),
        [90.0, 0.0, 0.0]
    ));
    assert!(vec_same(
        math::vectoangles([0.0, 0.0, -2.0]),
        [270.0, 0.0, 0.0]
    ));
    // Exactly zero is not greater than zero, so it takes the 270 branch (BUILTIN-6.5).
    assert!(vec_same(
        math::vectoangles([0.0, 0.0, 0.0]),
        [270.0, 0.0, 0.0]
    ));
    // A purely horizontal vector has pitch atan2(0, fwd) = 0 exactly, which is not negative,
    // so it is not wrapped to 360 (pins the `p < 0` pitch-wrap test in vectoangles).
    assert!(vec_same(
        math::vectoangles([1.0, 0.0, 0.0]),
        [0.0, 0.0, 0.0]
    ));
}

// spec BUILTIN-6.7: rint of a strictly positive value adds 0.5 then truncates, so 0.5 rounds
// to 1 but a value just below 0 takes the subtract branch; the `f > 0.0` test is pinned.
#[test]
fn rint_positive_and_negative_branch() {
    assert_eq!(math::rint(0.5), 1.0);
    assert_eq!(math::rint(0.4), 0.0);
    // A strictly negative value subtracts 0.5: -0.5 - 0.5 = -1 truncated is -1.
    assert_eq!(math::rint(-0.5), -1.0);
    assert_eq!(math::rint(-0.4), 0.0);
}

// spec BUILTIN-7.1: ftos uses the integer form only when f equals its truncation exactly; a
// fraction uses %5.1f. A value just below a whole number must not read as whole.
#[test]
fn ftos_whole_vs_fraction_boundary() {
    assert_eq!(math::ftos(2.0), "2");
    assert_eq!(math::ftos(1.999_999_9), "  2.0");
    assert_eq!(math::ftos(2.000_000_5), "  2.0");
}

// spec BUILTIN-7.2: a vtos component just below a whole number prints %5.1f, not the integer
// form; pins the component rounding boundary.
#[test]
fn vtos_component_boundary() {
    assert_eq!(math::vtos([1.999_999_9, 0.0, 0.0]), "'  2.0   0.0   0.0'");
}

// spec BUILTIN-11.7: Q_atof reads a leading 0x hexadecimal integer, with an optional sign.
#[test]
fn q_atof_hexadecimal() {
    assert_eq!(math::q_atof("0xff"), 255.0);
    assert_eq!(math::q_atof("0X10"), 16.0);
    assert_eq!(math::q_atof("-0x10"), -16.0);
    assert_eq!(math::q_atof("0xAb"), 171.0);
}

// spec BUILTIN-11.7: Q_atof of a leading quote takes the code of the next character.
#[test]
fn q_atof_character_code() {
    assert_eq!(math::q_atof("'A"), 65.0);
    assert_eq!(math::q_atof("-'A"), -65.0);
}

// spec BUILTIN-11.7: Q_atof reads decimal digits with at most one dot, stops at the first
// other character, skips no leading spaces, and applies the sign.
#[test]
fn q_atof_decimal() {
    assert_eq!(math::q_atof("12.5"), 12.5);
    assert_eq!(math::q_atof("-3.25"), -3.25);
    // A second dot stops the parse at 12.5.
    assert_eq!(math::q_atof("12.5.5"), 12.5);
    // A non-digit stops the parse.
    assert_eq!(math::q_atof("7x"), 7.0);
    // A leading space is not skipped, so the whole thing reads 0.
    assert_eq!(math::q_atof(" 5"), 0.0);
    assert_eq!(math::q_atof(""), 0.0);
}

// spec BUILTIN-11.7: the spec's own examples and the three empty-prefix edges. The text
// states Q_atof has "no exponent" and gives "`\"1e3\"` is 1"; it also reads a hexadecimal
// number after `0x` and the code of the character after a `'`. An adversarial reviewer pins
// the normative example and the empty tails those two prefixes allow: "0x" with no hex digit
// and a lone "'" with no following character both read 0.
#[test]
fn q_atof_spec_examples_and_empty_prefixes() {
    // The spec's explicit example: the exponent is not read, so "1e3" stops at 'e' and is 1.
    assert_eq!(math::q_atof("1e3"), 1.0);
    // "0x" with no following hex digit reads 0 (the hex loop finds no digit).
    assert_eq!(math::q_atof("0x"), 0.0);
    // A lone "'" takes the code of the next character, which is absent, so it reads 0.
    assert_eq!(math::q_atof("'"), 0.0);
    // A leading dot with no second dot still parses: ".5" is 0.5.
    assert_eq!(math::q_atof(".5"), 0.5);
}

// spec BUILTIN-6.8: fabs clears the sign bit of a negative value (pins fabs against a 0.0
// mutant: fabs(-3) is 3, not 0).
#[test]
fn fabs_of_negative() {
    assert_eq!(math::fabs(-3.5), 3.5);
    assert_eq!(math::fabs(3.5), 3.5);
}

// ===== the mock engine fixture (DIFF-6, EDICT-13) =====

mod mock_engine {
    //! Unit tests of the deterministic mock engine's public methods (DIFF-6, DIFF-7),
    //! exercised directly rather than through a builtin so each rule is pinned on its own.

    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use crate::mock::{
        BUILTIN_SIGNON_SIZE, Buffers, Cvars, EngineStrings, LIMIT_MAX_DATAGRAM, MessageDest,
        MockEngine, ModelBounds, Overflow, Precache, Rng,
    };
    use crate::{BuiltinError, Class, FatalError, RuntimeError, table};
    use qc_edict::system::{SysField, SysGlobal};

    // spec DIFF-2.4: a known setting reads and sets its text; an unknown one reads None and
    // set leaves it unchanged, reporting whether it existed.
    #[test]
    fn cvars_known_and_unknown() {
        let mut cvars = Cvars::default();
        assert_eq!(cvars.text("skill"), Some("1"));
        assert!(cvars.set("skill", "2"));
        assert_eq!(cvars.text("skill"), Some("2"));
        assert!(!cvars.set("nope", "5"));
        assert_eq!(cvars.text("nope"), None);
    }

    // spec DIFF-6.2: the random source advances state = state*mult + inc, returning
    // (state >> 16) & 0x7fff; the shift and mask are pinned by a seeded draw.
    #[test]
    fn rng_is_the_diff_sequence() {
        let mut rng = Rng::new(1);
        // state = 1*1103515245 + 12345 = 1103527590; (>>16)&0x7fff = 0x41c1 & 0x7fff.
        let first = rng.next_rand();
        assert_eq!(first, (1_103_527_590u32 >> 16) & 0x7fff);
        // A second draw advances again and differs from the first.
        let second = rng.next_rand();
        assert_ne!(first, second);
        // A mask keeps the result in 0..=0x7fff.
        assert!(second <= 0x7fff);
    }

    // spec EDICT-13.1, EDICT-13.3: make allocates a fresh negative value per string and text
    // returns each one's bytes; next_value counts down from -1.
    #[test]
    fn engine_strings_make_and_text() {
        let mut strings = EngineStrings::default();
        let a = strings.make(b"one".to_vec());
        let b = strings.make(b"two".to_vec());
        assert_eq!(a, -1);
        assert_eq!(b, -2);
        assert_eq!(strings.text(a), Some(b"one".as_slice()));
        assert_eq!(strings.text(b), Some(b"two".as_slice()));
        assert_eq!(strings.text(-3), None);
        assert_eq!(strings.text(5), None);
    }

    // spec BUILTIN-7.4: the ftos/vtos temp buffer is one value; a second set overwrites its
    // text and returns the same value, so an earlier result's text changes.
    #[test]
    fn engine_strings_temp_is_one_value() {
        let mut strings = EngineStrings::default();
        let first = strings.set_temp(b"1".to_vec());
        let second = strings.set_temp(b"2".to_vec());
        assert_eq!(first, second);
        assert_eq!(first, -1);
        assert_eq!(strings.text(first), Some(b"2".as_slice()));
    }

    // spec DIFF-3.9: the temp buffer's change is reported once with its current text when it is
    // allocated or given a different text; the same text again, or a made string, is none.
    #[test]
    fn engine_strings_take_temp_change() {
        let mut strings = EngineStrings::default();
        assert_eq!(strings.take_temp_change(), None);
        strings.make(b"map".to_vec());
        assert_eq!(strings.take_temp_change(), None);
        let temp = strings.set_temp(b"1".to_vec());
        strings.set_temp(b"2".to_vec());
        assert_eq!(strings.take_temp_change(), Some((temp, b"2".to_vec())));
        assert_eq!(strings.take_temp_change(), None);
        strings.set_temp(b"2".to_vec());
        assert_eq!(strings.take_temp_change(), None);
        strings.set_temp(b"1".to_vec());
        assert_eq!(strings.take_temp_change(), Some((temp, b"1".to_vec())));
    }

    // spec EDICT-13.3: a made string and the temp buffer take distinct values whichever comes
    // first, and changing the buffer's text never changes a made string's (BUILTIN-7.4)
    #[test]
    fn reviewer_made_strings_survive_the_temp_buffer() {
        let mut strings = EngineStrings::default();
        let map = strings.make(b"start".to_vec());
        let temp = strings.set_temp(b"1".to_vec());
        let name = strings.make(b"player".to_vec());
        assert_eq!((map, temp, name), (-1, -2, -3));
        for text in ["2", "  0.5", "'  1.0   2.0   3.0'"] {
            assert_eq!(strings.set_temp(text.as_bytes().to_vec()), temp);
        }
        assert_eq!(strings.text(map), Some(b"start".as_slice()));
        assert_eq!(strings.text(name), Some(b"player".as_slice()));
        assert_eq!(
            strings.take_temp_change(),
            Some((temp, b"'  1.0   2.0   3.0'".to_vec()))
        );
    }

    // spec DIFF-4.2: client_active is true only for a slot 1..=len with a player; slot 0 and a
    // slot past the end are never active.
    #[test]
    fn client_active_bounds() {
        let mut engine = MockEngine::new("start", 0, 1);
        engine.clients_active = vec![true, false];
        assert!(!engine.client_active(0));
        assert!(engine.client_active(1));
        assert!(!engine.client_active(2));
        assert!(!engine.client_active(3));
    }

    // spec BUILTIN-10.3, BUILTIN-12.2: precache adds a name once and sound_index/model_index
    // find it from entry 0; a missing name is None.
    #[test]
    fn precache_and_index() {
        let mut engine = MockEngine::new("start", 0, 1);
        assert_eq!(engine.precache_index(Precache::Sound, "x.wav"), None);
        assert_eq!(engine.precache(Precache::Sound, "x.wav"), Ok(()));
        assert_eq!(engine.precache_index(Precache::Sound, "x.wav"), Some(1));
        // A second precache of the same name does not add a second entry.
        assert_eq!(engine.precache(Precache::Sound, "x.wav"), Ok(()));
        assert_eq!(engine.precache_index(Precache::Sound, "x.wav"), Some(1));
        assert_eq!(engine.precache_index(Precache::Sound, ""), Some(0));
        // model_precache starts with "", "maps/start.bsp".
        assert_eq!(
            engine.precache_index(Precache::Model, "maps/start.bsp"),
            Some(1)
        );
        assert_eq!(engine.precache(Precache::Model, "progs/x.mdl"), Ok(()));
        assert_eq!(
            engine.precache_index(Precache::Model, "progs/x.mdl"),
            Some(2)
        );
    }

    // spec BUILTIN-13.3: append_message adds to the buffer up to its limit; an append that
    // would exceed the limit leaves the size unchanged and reports overflow. MSG_ONE is never
    // kept and never overflows.
    #[test]
    fn append_message_limit() {
        let mut engine = MockEngine::new("start", 0, 1);
        assert_eq!(engine.append_message(MessageDest::Broadcast, 10), Ok(()));
        assert_eq!(engine.buffers.unreliable, 10);
        // Exactly at the limit fits.
        assert_eq!(
            engine.append_message(MessageDest::Broadcast, LIMIT_MAX_DATAGRAM - 10),
            Ok(())
        );
        assert_eq!(engine.buffers.unreliable, LIMIT_MAX_DATAGRAM);
        // One more byte overflows and does not change the size.
        assert_eq!(
            engine.append_message(MessageDest::Broadcast, 1),
            Err(Overflow)
        );
        assert_eq!(engine.buffers.unreliable, LIMIT_MAX_DATAGRAM);
        // MSG_ONE is not kept and never overflows.
        assert_eq!(engine.append_message(MessageDest::One, 1_000_000), Ok(()));
        // The signon buffer has its own larger limit.
        assert_eq!(
            engine.append_message(MessageDest::Init, BUILTIN_SIGNON_SIZE),
            Ok(())
        );
        assert_eq!(engine.append_message(MessageDest::Init, 1), Err(Overflow));
    }

    // spec BUILTIN-11.5: append_command appends while the total stays below the buffer size;
    // a fill that would reach it drops the text and reports overflow.
    #[test]
    fn append_command_limit() {
        let mut engine = MockEngine::new("start", 0, 1);
        assert_eq!(engine.append_command(b"quit\n"), Ok(()));
        assert_eq!(engine.command_buffer, b"quit\n");
        // A text that would reach BUILTIN_CMD_TEXT_SIZE overflows and changes nothing.
        let big = vec![b'x'; 8192];
        assert_eq!(engine.append_command(&big), Err(Overflow));
        assert_eq!(engine.command_buffer, b"quit\n");
        // Exactly BUILTIN_CMD_TEXT_SIZE bytes into a fresh buffer reaches the size, so it is
        // dropped (the bound is strictly less-than); pins the `< BUILTIN_CMD_TEXT_SIZE` test.
        let mut fresh = MockEngine::new("start", 0, 1);
        let exact = vec![b'y'; 8192];
        assert_eq!(fresh.append_command(&exact), Err(Overflow));
        assert!(fresh.command_buffer.is_empty());
        // One byte under the size fits.
        let mut fresh2 = MockEngine::new("start", 0, 1);
        let under = vec![b'z'; 8191];
        assert_eq!(fresh2.append_command(&under), Ok(()));
        assert_eq!(fresh2.command_buffer.len(), 8191);
    }

    // spec BUILTIN-9.4, DIFF-6.3: model_bounds_for gives empty bounds for the empty name, the
    // directive's bounds for a named model, the .mdl default for a .mdl name, and empty bounds
    // for anything else.
    #[test]
    fn model_bounds_rule() {
        let mut engine = MockEngine::new("start", 0, 1);
        let zero = ModelBounds {
            mins: [0.0; 3],
            maxs: [0.0; 3],
        };
        assert_eq!(engine.model_bounds_for(""), zero);
        assert_eq!(
            engine.model_bounds_for("progs/x.mdl"),
            ModelBounds {
                mins: [-16.0; 3],
                maxs: [16.0; 3],
            }
        );
        assert_eq!(engine.model_bounds_for("*1"), zero);
        // A directive's bounds win over the .mdl default.
        let bounds = ModelBounds {
            mins: [-1.0; 3],
            maxs: [2.0; 3],
        };
        engine
            .model_bounds
            .push(("progs/y.mdl".to_string(), bounds));
        assert_eq!(engine.model_bounds_for("progs/y.mdl"), bounds);
    }

    // spec BUILTIN-12.6: set_lightstyle stores the value at its index.
    #[test]
    fn set_lightstyle_stores() {
        let mut engine = MockEngine::new("start", 0, 1);
        engine.set_lightstyle(5, b"abc".to_vec());
        assert_eq!(engine.lightstyles[5].as_deref(), Some("abc"));
        // An index past the array does nothing (no panic).
        engine.set_lightstyle(1000, b"z".to_vec());
    }

    // spec DIFF-6.1: the mock engine's random source is the DIFF-6.2 one, seeded as given
    #[test]
    fn mock_engine_draws_from_its_seeded_source() {
        let mut engine = MockEngine::new("start", 0, 7);
        let first = engine.rng.next_rand();
        // Same as a fresh Rng seeded with 7.
        let mut rng = Rng::new(7);
        assert_eq!(first, rng.next_rand());
    }

    // spec BUILTIN-4.4: the default Buffers start empty.
    #[test]
    fn buffers_default_empty() {
        let buffers = Buffers::default();
        assert_eq!(buffers.unreliable, 0);
        assert_eq!(buffers.reliable, 0);
        assert_eq!(buffers.signon, 0);
    }

    // spec BUILTIN-3.1, BUILTIN-3.2, BUILTIN-3.3: each BuiltinError kind reports its message
    #[test]
    fn builtin_error_messages() {
        assert_eq!(
            BuiltinError::from(RuntimeError::InvalidEntity).message(),
            "invalid entity"
        );
        assert_eq!(BuiltinError::Host("h".to_string()).message(), "h");
        assert_eq!(
            BuiltinError::from(FatalError::BreakStatement).message(),
            "break statement"
        );
        assert_eq!(BuiltinError::Host("h".to_string()).to_string(), "h");
    }

    // spec DIFF-8.2: every runtime error's message, byte for byte, as the trace records it
    // spec BUILTIN-1.2: a PF_Fixme number's message keeps id's spelling
    // spec LIMIT-6.2: a full precache list is PF_precache_sound or PF_precache_model: overflow
    #[test]
    fn runtime_error_messages() {
        let cases = [
            (RuntimeError::Unimplemented, "unimplemented bulitin"),
            (RuntimeError::InvalidEntity, "invalid entity"),
            (RuntimeError::InvalidString, "invalid string"),
            (RuntimeError::InvalidField, "invalid field"),
            (RuntimeError::InvalidLightstyle, "invalid lightstyle"),
            (RuntimeError::BackwardsBounds, "backwards mins/maxs"),
            (
                RuntimeError::NoPrecache {
                    name: "progs/x.mdl".to_string(),
                },
                "no precache: progs/x.mdl\n",
            ),
            (
                RuntimeError::PrecacheOutsideSpawn,
                "PF_Precache_*: Precache can only be done in spawn functions",
            ),
            (RuntimeError::BadString, "Bad string"),
            (
                RuntimeError::SoundPrecacheOverflow,
                "PF_precache_sound: overflow",
            ),
            (
                RuntimeError::ModelPrecacheOverflow,
                "PF_precache_model: overflow",
            ),
            (RuntimeError::EntityNotClient, "Entity is not a client"),
            (RuntimeError::ParmNotClient, "Parm 0 not a client"),
            (RuntimeError::WriteDestNotClient, "WriteDest: not a client"),
            (RuntimeError::WriteDestBad, "WriteDest: bad destination"),
            (RuntimeError::NoParameter { number: 8 }, "no parameter 8"),
            (
                RuntimeError::ParameterOutOfRange,
                "builtin parameter out of range",
            ),
            (RuntimeError::ReturnOutOfRange, "return slot out of range"),
            (
                RuntimeError::NoSystemGlobal(SysGlobal::VForward),
                "no system global v_forward",
            ),
            (RuntimeError::NoField(SysField::Health), "no field health"),
            (
                RuntimeError::NoVectorField(SysField::Origin),
                "no vector field origin",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(BuiltinError::from(error).to_string(), text);
        }
    }

    // spec DIFF-8.2: every fatal error's message, byte for byte, as Sys_Error formats it
    // spec BUILTIN-12.1: the sound errors print the volume and channel as %i and the
    // attenuation as %f
    #[test]
    fn fatal_error_messages() {
        let cases = [
            (FatalError::BreakStatement, "break statement"),
            (FatalError::BadPointer, "NUM_FOR_EDICT: bad pointer"),
            (FatalError::NoFreeEdicts, "ED_Alloc: no free edicts"),
            (
                FatalError::ModelNotPrecached {
                    name: "progs/x.mdl".to_string(),
                },
                "SV_ModelIndex: model progs/x.mdl not precached",
            ),
            (
                FatalError::SoundVolume { volume: 256 },
                "SV_StartSound: volume = 256",
            ),
            (
                FatalError::SoundAttenuation { atten: 4.5 },
                "SV_StartSound: attenuation = 4.500000",
            ),
            (
                FatalError::SoundAttenuation {
                    atten: f32::INFINITY,
                },
                "SV_StartSound: attenuation = inf",
            ),
            (
                FatalError::SoundChannel { channel: -1 },
                "SV_StartSound: channel = -1",
            ),
            (
                FatalError::MessageOverflow,
                "SZ_GetSpace: overflow without allowoverflow set",
            ),
        ];
        for (error, text) in cases {
            assert_eq!(BuiltinError::from(error).to_string(), text);
        }
    }

    // spec BUILTIN-5.1, BUILTIN-5.2: lookup finds a present number with its class and name, and
    // is None for a PF_Fixme gap.
    #[test]
    fn lookup_present_and_gap() {
        let makevectors = table::lookup(1).unwrap();
        assert_eq!(makevectors.name, "makevectors");
        assert_eq!(makevectors.class, Class::Pure);
        assert!(table::lookup(5).is_none());
    }
}

// ===== the state, output and world builtin bodies (BUILTIN-8 to BUILTIN-16, DIFF-7) =====

mod bodies {
    //! Tests of the class state, output and world builtin bodies against the mock fixture.
    //!
    //! Each builds a progs whose entry function calls one builtin, runs it through a
    //! [`qc_vm::Machine`] with [`MockBuiltins`] over a [`MockEngine`], and checks the result,
    //! the edicts and the engine fixture. Each test cites the `builtins.md` or `differential.md`
    //! statement it pins.

    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::expect_used,
        clippy::arithmetic_side_effects,
        clippy::unwrap_in_result,
        reason = "test code: a failed unwrap, index, expect or overflow is a failed test"
    )]

    use crate::mock::{EngineProfile, MessageDest, MockEngine, Precache};
    use crate::{MockBuiltins, math};
    use qc_edict::system::{SYSTEM_GLOBALS, field_by_name, global_by_name};
    use qc_edict::{EDICT_SYS_GLOBALS_END, EdictNumber};
    use qc_progs::opcode::Opcode;
    use qc_progs::{Function, Progs, ProgsFile, Statement, write};
    use qc_types::{Etype, PROGS_OFS_PARM0, PROGS_OFS_RETURN, PROGS_PARM_SLOTS, Slot};
    use qc_vm::{Builtins as _, ErrorKind, Machine, NoObserver, RunError, ServerState};

    /// A global slot holding the function value the entry's `CALL` calls: the first slot past
    /// the system globals (EDICT-2.1), so no builtin that writes one, such as `makevectors`
    /// writing `v_forward` (slots 59 to 61), overwrites it.
    const CALL_TARGET_SLOT: i32 = 92;
    /// The entity field slots the progs has (EDICT-3.3 minimum); the mock adds none.
    const ENTITY_FIELDS: i32 = 105;

    /// A fixture that runs one builtin and gives access to the machine and engine after.
    struct Fixture {
        machine: Machine,
        builtins: MockBuiltins,
        progs_bytes: Vec<u8>,
    }

    /// Builds a progs whose entry (function 1) calls builtin `number` with `argc` arguments,
    /// then is done. Function 2 is the builtin (`first_statement = -number`). `strings` is the
    /// string table bytes (a leading NUL for the empty string); `numglobals` is at least 92.
    fn build_progs(number: u32, argc: u16, strings: Vec<u8>) -> Vec<u8> {
        build_named_progs(number, argc, strings, 0)
    }

    /// [`build_progs`] with the builtin's function named by the string at `s_name`.
    fn build_named_progs(number: u32, argc: u16, strings: Vec<u8>, s_name: i32) -> Vec<u8> {
        let mut globals = vec![Slot::ZERO; 128];
        globals[usize::try_from(CALL_TARGET_SLOT).unwrap()] = Slot::from_i32(2);

        let call_number = 51_u32.saturating_add(u32::from(argc));
        let call_op = u32::from(Opcode::from_number(call_number).unwrap().number());
        let statements = vec![
            Statement {
                op: call_op,
                a: CALL_TARGET_SLOT,
                b: 0,
                c: 0,
            },
            Statement {
                op: u32::from(Opcode::Done.number()),
                a: i32::from(PROGS_OFS_RETURN),
                b: 0,
                c: 0,
            },
        ];
        let builtin_first = i32::try_from(number).unwrap().checked_neg().unwrap();
        let functions = vec![
            Function::default(),
            Function {
                first_statement: 0,
                ..Function::default()
            },
            Function {
                first_statement: builtin_first,
                s_name,
                ..Function::default()
            },
        ];
        let file = ProgsFile {
            entityfields: ENTITY_FIELDS,
            statements,
            functions,
            globals,
            strings,
            ..ProgsFile::default()
        };
        write(&file).unwrap()
    }

    // No spec statement: this pins the fixture, whose call target must lie outside every
    // system global (EDICT-2.1 lists them) so that a builtin's writes cannot redirect the call.
    #[test]
    fn the_call_target_slot_is_past_the_system_globals() {
        let slot = usize::try_from(CALL_TARGET_SLOT).unwrap();
        assert!(slot >= EDICT_SYS_GLOBALS_END, "{slot}");
        for var in SYSTEM_GLOBALS {
            let size = if var.etype == Etype::Vector { 3 } else { 1 };
            assert!(slot < var.slot || slot >= var.slot + size, "{}", var.name);
        }
    }

    /// A string table from a list of strings; returns the bytes and the offset of each.
    fn string_table(items: &[&str]) -> (Vec<u8>, Vec<i32>) {
        let mut bytes = vec![0_u8];
        let mut offsets = Vec::new();
        for item in items {
            offsets.push(i32::try_from(bytes.len()).unwrap());
            bytes.extend_from_slice(item.as_bytes());
            bytes.push(0);
        }
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        (bytes, offsets)
    }

    impl Fixture {
        /// A fixture for builtin `number`, `argc` args, a string table, and `maxclients`.
        fn new(number: u32, argc: u16, strings: Vec<u8>, maxclients: usize) -> Self {
            let progs_bytes = build_progs(number, argc, strings);
            let progs = Progs::read(&progs_bytes).unwrap();
            let machine = Machine::new(&progs, maxclients).unwrap();
            let mut engine = MockEngine::new("test", 0, 1);
            engine.clients_active = vec![true; maxclients];
            Self {
                machine,
                builtins: MockBuiltins::with_engine(engine),
                progs_bytes,
            }
        }

        /// Sets the `n`th parameter slot (`OFS_PARM0 + 3n`) to `value`.
        fn set_parm(&mut self, n: u16, value: Slot) {
            let slot = i32::from(PROGS_OFS_PARM0) + i32::from(PROGS_PARM_SLOTS) * i32::from(n);
            self.machine.set_global(slot, value).unwrap();
        }

        /// Sets the `n`th parameter to a float.
        fn set_parm_f(&mut self, n: u16, value: f32) {
            self.set_parm(n, Slot::from_f32(value));
        }

        /// Sets the `n`th parameter to a vector (three slots).
        fn set_parm_v(&mut self, n: u16, value: [f32; 3]) {
            let base = i32::from(PROGS_OFS_PARM0) + i32::from(PROGS_PARM_SLOTS) * i32::from(n);
            for (i, c) in value.into_iter().enumerate() {
                self.machine
                    .set_global(base + i32::try_from(i).unwrap(), Slot::from_f32(c))
                    .unwrap();
            }
        }

        /// Sets the `n`th parameter to a raw i32 (a string, entity or field value).
        fn set_parm_i(&mut self, n: u16, value: i32) {
            self.set_parm(n, Slot::from_i32(value));
        }

        /// Sets an entity-value system global (`self`, `msg_entity`) to edict `number`.
        fn set_global_entity(&mut self, name: &str, number: i32) {
            let value = self.machine.edicts().edict_num(number).unwrap();
            let slot = i32::try_from(global_by_name(name).unwrap().slot).unwrap();
            self.machine.set_global(slot, Slot::from(value)).unwrap();
        }

        /// Sets a scalar field of edict `number` to a float.
        fn set_field_f(&mut self, number: usize, name: &str, value: f32) {
            let slot = field_by_name(name).unwrap().slot;
            self.machine
                .edicts_mut()
                .set_field(EdictNumber::new(number), slot, Slot::from_f32(value))
                .unwrap();
        }

        /// Sets a vector field of edict `number`.
        fn set_field_v(&mut self, number: usize, name: &str, value: [f32; 3]) {
            let slot = field_by_name(name).unwrap().slot;
            for (i, c) in value.into_iter().enumerate() {
                self.machine
                    .edicts_mut()
                    .set_field(EdictNumber::new(number), slot + i, Slot::from_f32(c))
                    .unwrap();
            }
        }

        /// Reads a scalar field of edict `number` as a float.
        fn field_f(&self, number: usize, name: &str) -> f32 {
            let slot = field_by_name(name).unwrap().slot;
            self.machine
                .edicts()
                .field(EdictNumber::new(number), slot)
                .unwrap()
                .as_f32()
        }

        /// Reads a scalar field of edict `number` as an i32.
        fn field_i(&self, number: usize, name: &str) -> i32 {
            let slot = field_by_name(name).unwrap().slot;
            self.machine
                .edicts()
                .field(EdictNumber::new(number), slot)
                .unwrap()
                .as_i32()
        }

        /// Reads a vector field of edict `number`.
        fn field_v(&self, number: usize, name: &str) -> [f32; 3] {
            let slot = field_by_name(name).unwrap().slot;
            [
                self.machine
                    .edicts()
                    .field(EdictNumber::new(number), slot)
                    .unwrap()
                    .as_f32(),
                self.machine
                    .edicts()
                    .field(EdictNumber::new(number), slot + 1)
                    .unwrap()
                    .as_f32(),
                self.machine
                    .edicts()
                    .field(EdictNumber::new(number), slot + 2)
                    .unwrap()
                    .as_f32(),
            ]
        }

        /// The return slot as a float.
        fn return_f(&self) -> f32 {
            self.machine
                .global(i32::from(PROGS_OFS_RETURN))
                .unwrap()
                .as_f32()
        }

        /// The return slot as an i32.
        fn return_i(&self) -> i32 {
            self.machine
                .global(i32::from(PROGS_OFS_RETURN))
                .unwrap()
                .as_i32()
        }

        /// Runs the builtin; returns `Ok` or the run error.
        fn run(&mut self, server: ServerState) -> Result<(), RunError> {
            let progs = Progs::read(&self.progs_bytes).unwrap();
            self.machine
                .execute_program(&progs, server, &mut self.builtins, &mut NoObserver, 1)
        }

        /// Runs the builtin, expecting success.
        fn run_ok(&mut self, server: ServerState) {
            self.run(server).expect("builtin succeeded");
        }

        /// Runs the builtin, expecting a failure with this message.
        fn run_err(&mut self, server: ServerState, message: &str) {
            let error = self.run(server).expect_err("builtin failed");
            assert_eq!(error.message(), message);
        }

        /// The mock engine after the run.
        fn engine(&self) -> &MockEngine {
            self.builtins.engine()
        }

        /// The return slot as a vector (three slots).
        fn return_v(&self) -> [f32; 3] {
            let base = i32::from(PROGS_OFS_RETURN);
            [
                self.machine.global(base).unwrap().as_f32(),
                self.machine.global(base + 1).unwrap().as_f32(),
                self.machine.global(base + 2).unwrap().as_f32(),
            ]
        }

        /// A vector system global by name.
        fn sysglobal_v(&self, name: &str) -> [f32; 3] {
            let slot = i32::try_from(global_by_name(name).unwrap().slot).unwrap();
            [
                self.machine.global(slot).unwrap().as_f32(),
                self.machine.global(slot + 1).unwrap().as_f32(),
                self.machine.global(slot + 2).unwrap().as_f32(),
            ]
        }

        /// Sets an integer field of edict `number`.
        fn set_field_i(&mut self, number: usize, name: &str, value: i32) {
            let slot = field_by_name(name).unwrap().slot;
            self.machine
                .edicts_mut()
                .set_field(EdictNumber::new(number), slot, Slot::from_i32(value))
                .unwrap();
        }
    }

    /// Spawns edict `number` (clears its free flag) so field access and links work.
    fn in_use(fixture: &mut Fixture, number: usize) {
        fixture
            .machine
            .edicts_mut()
            .clear_edict(EdictNumber::new(number));
    }

    // ----- BUILTIN-8: entities -----

    // spec BUILTIN-8.1: spawn allocates an edict and returns it.
    #[test]
    fn spawn_allocates_an_edict() {
        let mut f = Fixture::new(14, 0, vec![0], 0);
        let before = f.machine.edicts().num_edicts();
        f.run_ok(ServerState::Active);
        assert_eq!(f.machine.edicts().num_edicts(), before + 1);
        // The returned value names the new edict (number `before`).
        let value = f
            .machine
            .edicts()
            .edict_num(i32::try_from(before).unwrap())
            .unwrap();
        assert_eq!(f.return_i(), value.bits());
        assert_eq!(
            f.machine.edicts().is_free(EdictNumber::new(before)),
            Some(false)
        );
    }

    // spec BUILTIN-8.2: remove frees the edict, refusing nothing.
    #[test]
    fn remove_frees_the_edict() {
        let mut f = Fixture::new(15, 1, vec![0], 0);
        // num_edicts starts at 1 (world); grow it so edict 1 is in use.
        f.machine.edicts_mut().alloc(1.0).unwrap();
        in_use(&mut f, 1);
        let value = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, value.bits());
        f.run_ok(ServerState::Active);
        assert_eq!(f.machine.edicts().is_free(EdictNumber::new(1)), Some(true));
    }

    // spec BUILTIN-8.4: nextent returns the first non-free edict after e, else the world.
    #[test]
    fn nextent_skips_free_edicts() {
        let mut f = Fixture::new(47, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1, in use
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2, in use
        f.machine.edicts_mut().free(EdictNumber::new(1), 1.0); // free edict 1
        // nextent(world) starts at edict 1, which is free, so it reaches edict 2.
        f.set_parm_i(0, 0);
        f.run_ok(ServerState::Active);
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        assert_eq!(f.return_i(), e2.bits());
    }

    // spec BUILTIN-8.3: find returns the first edict whose string field equals match.
    #[test]
    fn find_matches_a_string_field() {
        let (strings, offsets) = string_table(&["ab"]);
        let mut f = Fixture::new(18, 3, strings, 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2
        // classname (field slot 28) of edict 2 is "ab".
        let classname = field_by_name("classname").unwrap().slot;
        f.machine
            .edicts_mut()
            .set_field(EdictNumber::new(2), classname, Slot::from_i32(offsets[0]))
            .unwrap();
        let world = f.machine.edicts().edict_num(0).unwrap();
        f.set_parm_i(0, world.bits()); // start
        f.set_parm_i(1, i32::try_from(classname).unwrap()); // field
        f.set_parm_i(2, offsets[0]); // match "ab"
        f.run_ok(ServerState::Active);
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        assert_eq!(f.return_i(), e2.bits());
    }

    // spec BUILTIN-8.6: findradius chains the included edicts, newest first.
    #[test]
    fn findradius_chains_included_edicts() {
        let mut f = Fixture::new(22, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2
        for n in [1_usize, 2] {
            f.set_field_f(n, "solid", 1.0);
            f.set_field_v(n, "origin", [0.0, 0.0, 0.0]);
        }
        f.set_parm_v(0, [0.0, 0.0, 0.0]);
        f.set_parm_f(1, 100.0);
        f.run_ok(ServerState::Active);
        // The head is edict 2 (last included); its chain is edict 1; edict 1's chain is world.
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        assert_eq!(f.return_i(), e2.bits());
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        let chain = field_by_name("chain").unwrap().slot;
        assert_eq!(
            f.machine
                .edicts()
                .field(EdictNumber::new(2), chain)
                .unwrap()
                .as_i32(),
            e1.bits()
        );
        assert_eq!(
            f.machine
                .edicts()
                .field(EdictNumber::new(1), chain)
                .unwrap()
                .as_i32(),
            0
        );
    }

    // spec BUILTIN-8.7: setspawnparms of an edict outside 1..maxclients is a runtime error.
    #[test]
    fn setspawnparms_non_client_errors() {
        let mut f = Fixture::new(78, 1, vec![0], 1);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2, not a client slot
        in_use(&mut f, 2);
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        f.set_parm_i(0, e2.bits());
        f.run_err(ServerState::Active, "Entity is not a client");
    }

    // ----- BUILTIN-9: placing -----

    // spec BUILTIN-9.1: setorigin sets origin and links (absmin = origin + mins - 1).
    #[test]
    fn setorigin_sets_and_links() {
        let mut f = Fixture::new(2, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_v(1, [1.0, 2.0, 3.0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_v(1, "origin"), [1.0, 2.0, 3.0]);
        // mins are 0, widen 1: absmin = origin - 1.
        assert_eq!(f.field_v(1, "absmin"), [0.0, 1.0, 2.0]);
    }

    // spec BUILTIN-9.2: setsize of backwards mins/maxs is a runtime error before writing.
    #[test]
    fn setsize_backwards_errors() {
        let mut f = Fixture::new(4, 3, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_v(1, [1.0, 0.0, 0.0]); // min
        f.set_parm_v(2, [0.0, 0.0, 0.0]); // max, backwards on x
        f.run_err(ServerState::Active, "backwards mins/maxs");
        assert_eq!(f.field_v(1, "mins"), [0.0, 0.0, 0.0]);
    }

    // spec BUILTIN-9.2: setsize sets mins, maxs, size and links.
    #[test]
    fn setsize_sets_fields() {
        let mut f = Fixture::new(4, 3, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_v(1, [-16.0, -16.0, -24.0]);
        f.set_parm_v(2, [16.0, 16.0, 32.0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_v(1, "mins"), [-16.0, -16.0, -24.0]);
        assert_eq!(f.field_v(1, "size"), [32.0, 32.0, 56.0]);
    }

    // spec BUILTIN-9.3: setmodel of a name not precached is a runtime error.
    #[test]
    fn setmodel_not_precached_errors() {
        let (strings, offsets) = string_table(&["progs/x.mdl"]);
        let mut f = Fixture::new(3, 2, strings, 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_i(1, offsets[0]);
        f.run_err(ServerState::Active, "no precache: progs/x.mdl\n");
    }

    // spec BUILTIN-9.4: setmodel sets model, modelindex and the .mdl bounds.
    #[test]
    fn setmodel_sets_bounds() {
        let (strings, offsets) = string_table(&["progs/x.mdl"]);
        let mut f = Fixture::new(3, 2, strings, 0);
        f.builtins
            .engine_mut()
            .precache(Precache::Model, "progs/x.mdl")
            .unwrap();
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_i(1, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_i(1, "model"), offsets[0]);
        assert_eq!(f.field_v(1, "mins"), [-16.0, -16.0, -16.0]);
        assert_eq!(f.field_v(1, "maxs"), [16.0, 16.0, 16.0]);
    }

    // ----- BUILTIN-10: precaching -----

    // spec BUILTIN-10.1: precache outside a spawn function (server active) is a runtime error.
    #[test]
    fn precache_sound_not_loading_errors() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut f = Fixture::new(19, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_err(
            ServerState::Active,
            "PF_Precache_*: Precache can only be done in spawn functions",
        );
    }

    // spec BUILTIN-10.2, BUILTIN-10.3: precache_sound returns s and adds it to the list.
    #[test]
    fn precache_sound_adds_and_returns() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut f = Fixture::new(19, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Loading);
        assert_eq!(f.return_i(), offsets[0]);
        assert_eq!(
            f.engine().precache_index(Precache::Sound, "misc/x.wav"),
            Some(1)
        );
    }

    // spec BUILTIN-10.2: an empty string precache is the runtime error Bad string.
    #[test]
    fn precache_sound_bad_string() {
        let mut f = Fixture::new(19, 1, vec![0], 0);
        f.set_parm_i(0, 0); // the empty string at offset 0
        f.run_err(ServerState::Loading, "Bad string");
    }

    // spec LIMIT-6.2: precaching a new name into a full list is the runtime error
    // PF_precache_sound: overflow or PF_precache_model: overflow, id's PR_RunError
    #[test]
    fn a_full_precache_list_is_a_runtime_error() {
        for (number, message) in [
            (19, "PF_precache_sound: overflow"),
            (20, "PF_precache_model: overflow"),
        ] {
            let (strings, offsets) = string_table(&["misc/new"]);
            let mut f = Fixture::new(number, 1, strings, 0);
            let engine = f.builtins.engine_mut();
            let list = if number == 19 {
                &mut engine.sound_precache
            } else {
                &mut engine.model_precache
            };
            list.resize_with(256, || "taken".to_string());
            f.set_parm_i(0, offsets[0]);
            let error = f.run(ServerState::Loading).unwrap_err();
            assert_eq!(error.message(), message);
            assert!(matches!(error.kind, ErrorKind::Builtin { .. }), "{error:?}");
        }
    }

    // spec BUILTIN-10.5: precache_file returns s and does nothing, at any time.
    #[test]
    fn precache_file_returns_s() {
        let (strings, offsets) = string_table(&["maps/x.bsp"]);
        let mut f = Fixture::new(68, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_i(), offsets[0]);
    }

    // spec BUILTIN-10.6: precache_model2 behaves exactly as precache_model.
    #[test]
    fn precache_model2_adds() {
        let (strings, offsets) = string_table(&["progs/x.mdl"]);
        let mut f = Fixture::new(75, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Loading);
        assert_eq!(
            f.engine().precache_index(Precache::Model, "progs/x.mdl"),
            Some(2)
        );
    }

    // ----- BUILTIN-11: printing, commands, settings -----

    // spec BUILTIN-11.4: stuffcmd to a non-client is the runtime error Parm 0 not a client.
    #[test]
    fn stuffcmd_non_client_errors() {
        let (strings, offsets) = string_table(&["cmd\n"]);
        let mut f = Fixture::new(21, 2, strings, 1);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2, not a client
        in_use(&mut f, 2);
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        f.set_parm_i(0, e2.bits());
        f.set_parm_i(1, offsets[0]);
        f.run_err(ServerState::Active, "Parm 0 not a client");
    }

    // spec BUILTIN-11.5: localcmd appends s to the command buffer.
    #[test]
    fn localcmd_appends() {
        let (strings, offsets) = string_table(&["quit\n"]);
        let mut f = Fixture::new(46, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().command_buffer, b"quit\n");
    }

    // spec BUILTIN-11.6: changelevel records it and appends; a second one does nothing.
    #[test]
    fn changelevel_once() {
        let (strings, offsets) = string_table(&["e1m2"]);
        let mut f = Fixture::new(70, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Active);
        assert!(f.engine().changelevel_issued);
        assert_eq!(f.engine().command_buffer, b"changelevel e1m2\n");
    }

    // spec BUILTIN-11.7: cvar returns the setting's value, or 0 for an unknown name.
    #[test]
    fn cvar_reads_a_setting() {
        let (strings, offsets) = string_table(&["skill", "nope"]);
        let mut f = Fixture::new(45, 1, strings, 0);
        f.builtins.engine_mut().cvars.set("skill", "2.5");
        f.set_parm_i(0, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 2.5);
        // An unknown name reads 0.
        let mut f2 = Fixture::new(45, 1, string_table(&["nope"]).0, 0);
        f2.set_parm_i(0, string_table(&["nope"]).1[0]);
        f2.run_ok(ServerState::Active);
        assert_eq!(f2.return_f(), 0.0);
    }

    // spec BUILTIN-11.8: cvar_set sets a known setting and leaves an unknown one unchanged.
    #[test]
    fn cvar_set_sets_a_setting() {
        let (strings, offsets) = string_table(&["skill", "3"]);
        let mut f = Fixture::new(72, 2, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.set_parm_i(1, offsets[1]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().cvars.text("skill"), Some("3"));
    }

    // ----- BUILTIN-12: sounds, particles, lightstyles, statics -----

    // spec BUILTIN-12.1: a volume outside 0..255 is a fatal error.
    #[test]
    fn sound_bad_volume() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut f = Fixture::new(8, 5, strings, 0);
        f.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        f.set_parm_f(1, 0.0); // channel
        f.set_parm_i(2, offsets[0]); // sample
        f.set_parm_f(3, 2.0); // volume * 255 = 510, out of range
        f.set_parm_f(4, 1.0); // attenuation
        f.run_err(ServerState::Active, "SV_StartSound: volume = 510");
    }

    // spec BUILTIN-12.3: a valid sound appends bytes to the unreliable datagram.
    #[test]
    fn sound_appends_to_datagram() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut f = Fixture::new(8, 5, strings, 0);
        f.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_f(1, 0.0);
        f.set_parm_i(2, offsets[0]);
        f.set_parm_f(3, 1.0);
        f.set_parm_f(4, 1.0);
        f.run_ok(ServerState::Active);
        assert!(f.engine().buffers.unreliable > 0);
    }

    // spec BUILTIN-12.6: lightstyle of a style outside 0..63 is invalid lightstyle.
    #[test]
    fn lightstyle_out_of_range() {
        let (strings, offsets) = string_table(&["a"]);
        let mut f = Fixture::new(35, 2, strings, 0);
        f.set_parm_f(0, 64.0);
        f.set_parm_i(1, offsets[0]);
        f.run_err(ServerState::Active, "invalid lightstyle");
    }

    // spec BUILTIN-12.6: lightstyle stores the value.
    #[test]
    fn lightstyle_stores() {
        let (strings, offsets) = string_table(&["mmnn"]);
        let mut f = Fixture::new(35, 2, strings, 0);
        f.set_parm_f(0, 3.0);
        f.set_parm_i(1, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().lightstyles[3].as_deref(), Some("mmnn"));
    }

    // spec BUILTIN-12.8: makestatic appends to the signon buffer and frees e.
    #[test]
    fn makestatic_frees_e() {
        let mut f = Fixture::new(69, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        // model field 0 means the empty model (index 0), which is allowed.
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.run_ok(ServerState::Active);
        assert!(f.engine().buffers.signon > 0);
        assert_eq!(f.machine.edicts().is_free(EdictNumber::new(1)), Some(true));
    }

    // ----- BUILTIN-13: network messages -----

    // spec BUILTIN-13.1: a bad destination is WriteDest: bad destination.
    #[test]
    fn write_bad_destination() {
        let mut f = Fixture::new(52, 2, vec![0], 0);
        f.set_parm_f(0, 5.0); // no such destination
        f.set_parm_f(1, 1.0);
        f.run_err(ServerState::Active, "WriteDest: bad destination");
    }

    // spec BUILTIN-13.2: WriteLong appends four bytes to the chosen buffer.
    #[test]
    fn write_long_four_bytes() {
        let mut f = Fixture::new(55, 2, vec![0], 0);
        f.set_parm_f(0, 0.0); // MSG_BROADCAST
        f.set_parm_f(1, 1.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.unreliable, 4);
    }

    // spec BUILTIN-13.1: MSG_ONE to a non-client is WriteDest: not a client.
    #[test]
    fn write_one_non_client() {
        let mut f = Fixture::new(52, 2, vec![0], 1);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2
        in_use(&mut f, 2);
        f.set_global_entity("msg_entity", 2); // not a client slot
        f.set_parm_f(0, 1.0); // MSG_ONE
        f.set_parm_f(1, 1.0);
        f.run_err(ServerState::Active, "WriteDest: not a client");
    }

    // spec BUILTIN-2.10: WriteString reads the destination before the string.
    #[test]
    fn write_string_bad_destination_first() {
        let mut f = Fixture::new(58, 2, vec![0], 0);
        f.set_parm_f(0, 5.0); // bad destination
        f.set_parm_i(1, 0x0100_0000); // also a bad string
        f.run_err(ServerState::Active, "WriteDest: bad destination");
    }

    // ----- BUILTIN-16: errors -----

    // spec BUILTIN-16.1: error raises a failure carrying the joined text.
    #[test]
    fn error_carries_text() {
        let (strings, offsets) = string_table(&["boom"]);
        let mut f = Fixture::new(10, 1, strings, 0);
        f.set_parm_i(0, offsets[0]);
        f.run_err(ServerState::Active, "boom");
    }

    // spec BUILTIN-16.2: objerror frees self and carries the joined text.
    #[test]
    fn objerror_frees_self() {
        let (strings, offsets) = string_table(&["oops"]);
        let mut f = Fixture::new(11, 1, strings, 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        f.set_global_entity("self", 1);
        f.set_parm_i(0, offsets[0]);
        f.run_err(ServerState::Active, "oops");
        assert_eq!(f.machine.edicts().is_free(EdictNumber::new(1)), Some(true));
    }

    // spec BUILTIN-3.2: error and objerror are host errors: they reach the VM as one and leave
    // the call depth where they raised it, here the entry function's 1.
    #[test]
    fn error_and_objerror_are_host_errors_that_keep_the_depth() {
        for number in [10_u32, 11] {
            let (strings, offsets) = string_table(&["deep"]);
            let mut f = Fixture::new(number, 1, strings, 0);
            f.set_parm_i(0, offsets[0]);
            let error = f.run(ServerState::Active).expect_err("host error");
            assert_eq!(
                error.kind,
                qc_vm::ErrorKind::Host {
                    message: "deep".to_owned()
                }
            );
            assert_eq!(f.machine.depth(), 1, "builtin {number}");
        }
    }

    // spec BUILTIN-3.3: break is a fatal error, which reaches the VM as one and leaves the depth.
    // spec BUILTIN-3.1: a builtin's runtime error resets the depth to 0, as PR_RunError does.
    #[test]
    fn fatal_and_runtime_builtin_errors_keep_their_class() {
        let mut fatal = Fixture::new(6, 0, vec![0], 0);
        let error = fatal.run(ServerState::Active).expect_err("fatal error");
        assert!(error.kind.is_fatal());
        assert_eq!(fatal.machine.depth(), 1);

        let mut runtime = Fixture::new(5, 0, vec![0], 0);
        let error = runtime.run(ServerState::Active).expect_err("runtime error");
        assert_eq!(
            error.kind,
            qc_vm::ErrorKind::Builtin {
                message: "unimplemented bulitin".to_owned()
            }
        );
        assert_eq!(runtime.machine.depth(), 0);
    }

    // ----- BUILTIN-2: argument checks (the DIFF-6.6 messages) -----

    // spec BUILTIN-2.7: an entity value that names no edict is invalid entity.
    #[test]
    fn invalid_entity_message() {
        let mut f = Fixture::new(47, 1, vec![0], 0); // nextent
        f.set_parm_i(0, 3); // 3 is not a whole edict (edict_size is 516), names no edict
        f.run_err(ServerState::Active, "invalid entity");
    }

    // spec BUILTIN-2.5: a string value neither in the table nor an engine string is invalid
    // string.
    #[test]
    fn invalid_string_message() {
        let mut f = Fixture::new(23, 1, vec![0], 0); // bprint
        f.set_parm_i(0, 0x0100_0000); // far outside the one-byte string table
        f.run_err(ServerState::Active, "invalid string");
    }

    // spec BUILTIN-8.3: a field index outside the progs' fields is invalid field.
    #[test]
    fn invalid_field_message() {
        let (strings, offsets) = string_table(&["x"]);
        let mut f = Fixture::new(18, 3, strings, 0); // find
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1, so find examines it
        let world = f.machine.edicts().edict_num(0).unwrap();
        f.set_parm_i(0, world.bits());
        f.set_parm_i(1, 500); // 500 >= entityfields (105): invalid field
        f.set_parm_i(2, offsets[0]);
        f.run_err(ServerState::Active, "invalid field");
    }

    // spec BUILTIN-2.4: an entity at or above num_edicts is NUM_FOR_EDICT: bad pointer.
    #[test]
    fn num_for_edict_bad_pointer() {
        let mut f = Fixture::new(47, 1, vec![0], 0); // nextent reads with G_EDICTNUM
        // edict 10 is a whole edict but at/above num_edicts (1): the fatal error.
        let value = 10 * f.machine.edicts().layout().edict_size();
        f.set_parm_i(0, value);
        f.run_err(ServerState::Active, "NUM_FOR_EDICT: bad pointer");
    }

    // ----- DIFF-7: world builtins -----

    // spec DIFF-7.1: traceline hits nothing; trace_endpos is a copy of PARM1.
    #[test]
    fn traceline_writes_trace_globals() {
        let mut f = Fixture::new(16, 4, vec![0], 0);
        f.set_parm_v(0, [1.0, 2.0, 3.0]);
        f.set_parm_v(1, [4.0, 5.0, 6.0]);
        f.set_parm_f(2, 0.0);
        f.set_parm_i(3, 0); // forent: the world
        f.run_ok(ServerState::Active);
        let endpos = global_by_name("trace_endpos").unwrap().slot;
        let slot = i32::try_from(endpos).unwrap();
        assert_eq!(f.machine.global(slot).unwrap().as_f32(), 4.0);
        assert_eq!(f.machine.global(slot + 1).unwrap().as_f32(), 5.0);
        let frac = global_by_name("trace_fraction").unwrap().slot;
        assert_eq!(
            f.machine
                .global(i32::try_from(frac).unwrap())
                .unwrap()
                .as_f32(),
            1.0
        );
    }

    // spec DIFF-7.2: pointcontents returns -1.
    #[test]
    fn pointcontents_returns_empty() {
        let mut f = Fixture::new(41, 1, vec![0], 0);
        f.set_parm_v(0, [0.0, 0.0, -1.0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), -1.0);
    }

    // spec DIFF-7.4: checkbottom returns 1; aim returns a copy of v_forward.
    #[test]
    fn checkbottom_returns_one() {
        let mut f = Fixture::new(40, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 1.0);
    }

    // spec DIFF-7.5: droptofloor links self, sets FL_ONGROUND and groundentity, returns 1.
    #[test]
    fn droptofloor_sets_onground() {
        let mut f = Fixture::new(34, 0, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        f.set_field_f(1, "flags", 1.0);
        f.set_field_v(1, "origin", [0.0, 0.0, 100.0]);
        f.set_global_entity("self", 1);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 1.0);
        // flags 1 | FL_ONGROUND (512) = 513.
        assert_eq!(f.field_f(1, "flags"), 513.0);
        assert_eq!(f.field_i(1, "groundentity"), 0);
        // origin unchanged; absmin z = 100 + 0 - 1 = 99.
        assert_eq!(f.field_v(1, "origin")[2], 100.0);
        assert_eq!(f.field_v(1, "absmin")[2], 99.0);
    }

    // spec DIFF-7.7: walkmove without a movement flag returns 0 and moves nothing.
    #[test]
    fn walkmove_without_flags() {
        let mut f = Fixture::new(32, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 4096.0); // no FL_ONGROUND/FL_FLY/FL_SWIM
        f.set_field_v(1, "origin", [10.0, 20.0, 30.0]);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 0.0);
        f.set_parm_f(1, 10.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 0.0);
        assert_eq!(f.field_v(1, "origin"), [10.0, 20.0, 30.0]);
    }

    // spec DIFF-7.6, DIFF-7.7: walkmove on the ground steps in x and y and returns 1.
    #[test]
    fn walkmove_steps() {
        let mut f = Fixture::new(32, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 512.0); // FL_ONGROUND
        f.set_field_v(1, "origin", [10.0, 20.0, 30.0]);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 180.0); // yaw
        f.set_parm_f(1, 10.0); // dist
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 1.0);
        // yaw 180 by 10 moves x by -10: from 10 to 0 (the sine's residue is below a float
        // step at y=20).
        assert_eq!(f.field_v(1, "origin")[0], 0.0);
        assert_eq!(f.field_v(1, "origin")[1], 20.0);
    }

    // spec DIFF-7.6: the walkmove step rounds its angle to f32 before cosine and sine, as id's
    // PF_walkmove keeps the yaw in a float. A 360-degree yaw by 10 gives id's f32-rounded
    // move_y, bits 0x35eaac7a (about 1.748e-6), not the f64 path's 0xa7307d7e (about -2.449e-15);
    // with origin_y 0 the new origin_y carries that residue bit for bit.
    #[test]
    fn walkmove_full_yaw_rounds_to_f32() {
        let mut f = Fixture::new(32, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 512.0); // FL_ONGROUND
        f.set_field_v(1, "origin", [10.0, 0.0, 30.0]);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 360.0); // yaw
        f.set_parm_f(1, 10.0); // dist
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 1.0);
        let origin = f.field_v(1, "origin");
        // move_x = (f32)(cos(f32-radians) * 10) = 10; origin_x 10 + 10 = 20.
        assert_eq!(origin[0].to_bits(), 0x41a0_0000); // 20.0
        // move_y = (f32)(sin(f32-radians) * 10), bits 0x35eaac7a; origin_y 0 + that.
        assert_eq!(origin[1].to_bits(), 0x35ea_ac7a);
        assert_eq!(origin[2], 30.0); // origin_z is not written.
    }

    // spec DIFF-7.8: movetogoal without a movement flag writes 0 to RETURN.
    #[test]
    fn movetogoal_without_flags() {
        let mut f = Fixture::new(67, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 0.0);
        f.set_field_v(1, "origin", [10.0, 20.0, 30.0]);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 8.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 0.0);
        assert_eq!(f.field_v(1, "origin"), [10.0, 20.0, 30.0]);
    }

    // spec BUILTIN-13.1, DIFF-6.5: each message destination counts into its own buffer:
    // MSG_BROADCAST the unreliable datagram, MSG_ALL the reliable one, MSG_INIT the signon
    // buffer, and MSG_ONE into none of them.
    // spec BUILTIN-4.4: a typed engine-only error reports its builtin's number and name.
    #[test]
    fn message_dest_variants() {
        let sizes = |dest: MessageDest| {
            let mut engine = MockEngine::new("start", 0, 1);
            assert_eq!(engine.append_message(dest, 3), Ok(()));
            let b = &engine.buffers;
            (b.unreliable, b.reliable, b.signon)
        };
        assert_eq!(sizes(MessageDest::Broadcast), (3, 0, 0));
        assert_eq!(sizes(MessageDest::All), (0, 3, 0));
        assert_eq!(sizes(MessageDest::Init), (0, 0, 3));
        assert_eq!(sizes(MessageDest::One), (0, 0, 0));
    }

    // ----- BUILTIN-6, BUILTIN-7: the pure math and string builtins through dispatch -----

    // spec BUILTIN-6.1: makevectors writes v_forward, v_right and v_up; zero angles give
    // forward '1 0 -0'. Driving it through the dispatcher pins the body against a no-op.
    #[test]
    fn makevectors_writes_system_vectors() {
        let mut f = Fixture::new(1, 1, vec![0], 0);
        f.set_parm_v(0, [0.0, 0.0, 0.0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.sysglobal_v("v_forward"), [1.0, 0.0, -0.0]);
        assert_eq!(f.sysglobal_v("v_right"), [0.0, -1.0, -0.0]);
        assert_eq!(f.sysglobal_v("v_up"), [0.0, 0.0, 1.0]);
    }

    // spec BUILTIN-6.6: random draws from the mock source and returns (r & 0x7fff)/0x7fff; the
    // returned value is the first draw of the seeded source, in 0..=1.
    #[test]
    fn random_returns_a_draw() {
        let mut f = Fixture::new(7, 0, vec![0], 0);
        f.run_ok(ServerState::Active);
        let r = f.return_f();
        assert!((0.0..=1.0).contains(&r));
        // A second machine with the same seed draws the same first value.
        let mut f2 = Fixture::new(7, 0, vec![0], 0);
        f2.run_ok(ServerState::Active);
        assert_eq!(r, f2.return_f());
    }

    // spec BUILTIN-6.2, BUILTIN-6.3: vlen returns the length; normalize returns the unit vector.
    #[test]
    fn vlen_and_normalize_through_dispatch() {
        let mut f = Fixture::new(12, 1, vec![0], 0);
        f.set_parm_v(0, [3.0, 4.0, 0.0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 5.0);

        let mut g = Fixture::new(9, 1, vec![0], 0);
        g.set_parm_v(0, [3.0, 4.0, 0.0]);
        g.run_ok(ServerState::Active);
        assert_eq!(g.return_v(), [0.6, 0.8, 0.0]);
    }

    // spec BUILTIN-6.4, BUILTIN-6.5: vectoyaw returns the yaw; vectoangles returns 'pitch yaw 0'.
    #[test]
    fn vectoyaw_and_vectoangles_through_dispatch() {
        let mut f = Fixture::new(13, 1, vec![0], 0);
        f.set_parm_v(0, [0.0, 1.0, 0.0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 90.0);

        let mut g = Fixture::new(51, 1, vec![0], 0);
        g.set_parm_v(0, [0.0, 0.0, 1.0]);
        g.run_ok(ServerState::Active);
        assert_eq!(g.return_v(), [90.0, 0.0, 0.0]);
    }

    // spec BUILTIN-6.7, BUILTIN-6.8: rint, floor, ceil and fabs each return their value, so a
    // body replaced with Ok(()) (leaving RETURN unchanged) is caught.
    #[test]
    fn rint_floor_ceil_fabs_through_dispatch() {
        let cases = [
            (36u32, 2.5f32, 3.0f32),
            (37, 2.7, 2.0),
            (38, 2.2, 3.0),
            (43, -4.5, 4.5),
        ];
        for (number, input, expected) in cases {
            let mut f = Fixture::new(number, 1, vec![0], 0);
            // Poison RETURN with a different value so an untouched slot fails the assertion.
            f.machine
                .set_global(i32::from(PROGS_OFS_RETURN), Slot::from_f32(-123.0))
                .unwrap();
            f.set_parm_f(0, input);
            f.run_ok(ServerState::Active);
            assert_eq!(f.return_f(), expected, "builtin {number}");
        }
    }

    // spec BUILTIN-7.1, BUILTIN-7.4: ftos writes its text to the engine temp buffer and returns
    // the buffer value; the resolved text is the decimal form. vtos does the same for a vector.
    #[test]
    fn ftos_and_vtos_through_dispatch() {
        let mut f = Fixture::new(26, 1, vec![0], 0);
        f.set_parm_f(0, 5.0);
        f.run_ok(ServerState::Active);
        let value = f.return_i();
        assert!(value < 0, "ftos returns an engine-string value");
        assert_eq!(f.engine().strings.text(value), Some(b"5".as_slice()));

        let mut g = Fixture::new(27, 1, vec![0], 0);
        g.set_parm_v(0, [1.0, 2.0, 3.5]);
        g.run_ok(ServerState::Active);
        let gv = g.return_i();
        assert_eq!(
            g.engine().strings.text(gv),
            Some("'  1.0   2.0   3.5'".as_bytes())
        );
    }

    // spec BUILTIN-16.4: traceon sets the VM's trace flag and traceoff clears it; the run
    // clears it only when it starts (VM-2.3), so it is still set after a traceon run
    #[test]
    fn traceon_sets_and_traceoff_clears_the_trace_flag() {
        let mut on = Fixture::new(29, 0, vec![0], 0);
        on.run_ok(ServerState::Active);
        assert!(on.machine.tracing());
        let mut off = Fixture::new(30, 0, vec![0], 0);
        off.machine.set_tracing(true);
        off.run_ok(ServerState::Active);
        assert!(!off.machine.tracing());
    }

    // spec BUILTIN-16.4: traceoff clears the flag traceon set earlier in the same run
    #[test]
    fn traceoff_clears_a_flag_traceon_set_in_the_run() {
        let mut globals = vec![Slot::ZERO; 128];
        globals[usize::try_from(CALL_TARGET_SLOT).unwrap()] = Slot::from_i32(2);
        globals[usize::try_from(CALL_TARGET_SLOT + 1).unwrap()] = Slot::from_i32(3);
        let call = |slot: i32| Statement {
            op: u32::from(Opcode::Call0.number()),
            a: slot,
            b: 0,
            c: 0,
        };
        let done = Statement {
            op: u32::from(Opcode::Done.number()),
            a: 0,
            b: 0,
            c: 0,
        };
        let builtin = |number: i32| Function {
            first_statement: -number,
            ..Function::default()
        };
        let file = ProgsFile {
            entityfields: ENTITY_FIELDS,
            statements: vec![call(CALL_TARGET_SLOT), call(CALL_TARGET_SLOT + 1), done],
            functions: vec![
                Function::default(),
                Function::default(),
                builtin(29),
                builtin(30),
            ],
            globals,
            strings: vec![0],
            ..ProgsFile::default()
        };
        let bytes = write(&file).unwrap();
        let progs = Progs::read(&bytes).unwrap();
        let mut machine = Machine::new(&progs, 1).unwrap();
        let mut builtins = MockBuiltins::default();
        machine
            .execute_program(
                &progs,
                ServerState::Active,
                &mut builtins,
                &mut NoObserver,
                1,
            )
            .unwrap();
        assert!(!machine.tracing());
    }

    // spec BUILTIN-2.7: G_EDICT accepts any whole edict value, so setorigin of edict 600, past
    // the edict array (EDICT-1.4), reaches the origin write, which has no edict to write
    #[test]
    fn a_vector_write_past_the_edicts_is_an_error() {
        let mut f = Fixture::new(2, 2, vec![0], 0);
        let past = 600 * f.machine.edicts().layout().edict_size();
        f.set_parm_i(0, past);
        f.set_parm_v(1, [1.0, 2.0, 3.0]);
        f.run_err(ServerState::Active, "no vector field origin");
    }

    // spec BUILTIN-16.5: coredump and eprint change nothing QuakeC observes, and eprint's e goes
    // through NUM_FOR_EDICT, so an edict at or above num_edicts is the fatal bad pointer
    #[test]
    fn coredump_and_eprint_print_only() {
        let mut dump = Fixture::new(28, 0, vec![0], 0);
        dump.run_ok(ServerState::Active);
        let mut print = Fixture::new(31, 1, vec![0], 0);
        print.set_parm_i(0, 0);
        print.run_ok(ServerState::Active);
        let mut past = Fixture::new(31, 1, vec![0], 0);
        let value = 10 * past.machine.edicts().layout().edict_size();
        past.set_parm_i(0, value);
        past.run_err(ServerState::Active, "NUM_FOR_EDICT: bad pointer");
    }

    /// A run of builtin `number`, called through a function named `name`, in a mock engine under
    /// `profile`.
    fn run_profiled(number: u32, name: &str, profile: EngineProfile) -> Result<(), RunError> {
        let (strings, offsets) = string_table(&[name]);
        let bytes = build_named_progs(number, 0, strings, offsets[0]);
        let progs = Progs::read(&bytes).unwrap();
        let mut machine = Machine::new(&progs, 0).unwrap();
        let mut engine = MockEngine::new("test", 0, 1);
        engine.profile = profile;
        let mut builtins = MockBuiltins::with_engine(engine);
        machine.execute_program(
            &progs,
            ServerState::Active,
            &mut builtins,
            &mut NoObserver,
            1,
        )
    }

    // spec BUILTIN-18.1: under vkQuake's profile every number reaches the dispatch; under
    // id's the table ends at LIMIT_NUM_BUILTINS
    #[test]
    fn the_profile_decides_which_numbers_exist() {
        let mut id = MockBuiltins::default();
        assert_eq!(id.limit(), Some(qc_vm::LIMIT_NUM_BUILTINS));
        id.engine_mut().profile = EngineProfile::VkQuake;
        assert_eq!(id.limit(), None);
        let bad = run_profiled(500, "ext", EngineProfile::Id).unwrap_err();
        assert_eq!(bad.kind, ErrorKind::BadBuiltin);
        // An id builtin keeps id's behaviour: `random` returns a value in [0, 1).
        assert!(run_profiled(7, "random", EngineProfile::VkQuake).is_ok());
    }

    // spec BUILTIN-18.2: a number with no builtin is PF_Fixme's error, naming the number and
    // the function called, id's gaps included
    #[test]
    fn vkquake_names_an_unimplemented_builtin() {
        let far = run_profiled(500, "frobnicate", EngineProfile::VkQuake).unwrap_err();
        assert_eq!(far.message(), "unimplemented builtin #500 - frobnicate");
        let gap = run_profiled(5, "gap", EngineProfile::VkQuake).unwrap_err();
        assert_eq!(gap.message(), "unimplemented builtin #5 - gap");
        let id = run_profiled(5, "gap", EngineProfile::Id).unwrap_err();
        assert_eq!(id.message(), "unimplemented bulitin");
    }

    // spec BUILTIN-18.4: forceinfokey on the world sets a serverinfo cvar and no other; on a
    // player it sets none, and a bad player is NUM_FOR_EDICT's error
    #[test]
    fn forceinfokey_sets_serverinfo_cvars_on_the_world() {
        let (strings, offsets) = string_table(&["teamplay", "3", "skill"]);
        let run = |player: i32, key: i32| {
            let mut f = Fixture::new(213, 3, strings.clone(), 1);
            f.builtins.engine_mut().profile = EngineProfile::VkQuake;
            f.set_parm_i(0, player);
            f.set_parm_i(1, key);
            f.set_parm_i(2, offsets[1]);
            let result = f.run(ServerState::Active);
            (result, f)
        };
        let (result, world) = run(0, offsets[0]);
        result.unwrap();
        assert_eq!(world.engine().cvars.text("teamplay"), Some("3"));
        let (result, other) = run(0, offsets[2]);
        result.unwrap();
        assert_eq!(other.engine().cvars.text("skill"), Some("1"));
        let edict = i32::try_from(other.machine.edicts().layout().edict_size()).unwrap();
        let (result, player) = run(edict, offsets[0]);
        result.unwrap();
        assert_eq!(player.engine().cvars.text("teamplay"), Some("0"));
        let (result, _) = run(edict * 1000, offsets[0]);
        assert_eq!(result.unwrap_err().message(), "NUM_FOR_EDICT: bad pointer");
    }

    // spec BUILTIN-18.7: strftime writes the mock's fixed instant, keeps an unknown conversion,
    // and gives an empty string for a text past STRINGTEMP_LENGTH
    #[test]
    fn strftime_writes_the_fixed_instant() {
        let long = "x".repeat(1100);
        let formats = ["%Y-%m-%dT%H:%M:%S%z", "%a %b %e %% %Q %", &long];
        let (strings, offsets) = string_table(&formats);
        for (format, expected) in [
            (0, "1970-01-01T00:00:00+0000"),
            (1, "Thu Jan  1 % %Q %"),
            (2, ""),
        ] {
            let mut f = Fixture::new(478, 2, strings.clone(), 0);
            f.builtins.engine_mut().profile = EngineProfile::VkQuake;
            f.set_parm_f(0, 1.0);
            f.set_parm_i(1, offsets[format]);
            f.run_ok(ServerState::Active);
            let value = f.return_i();
            assert_eq!(f.engine().strings.text(value), Some(expected.as_bytes()));
        }
    }

    // spec BUILTIN-18.6: registercvar makes a setting whose text is its name again (vkQuake
    // reads OFS_PARM0 twice), empty with one argument, and leaves an existing one
    #[test]
    fn registercvar_makes_a_setting_once() {
        let (strings, offsets) = string_table(&["mine", "5", "skill"]);
        for (argc, name, expected) in [(2, 0, "mine"), (1, 0, ""), (2, 2, "1")] {
            let mut f = Fixture::new(93, argc, strings.clone(), 0);
            f.builtins.engine_mut().profile = EngineProfile::VkQuake;
            f.set_parm_i(0, offsets[name]);
            f.set_parm_i(1, offsets[1]);
            f.run_ok(ServerState::Active);
            let key = ["mine", "5", "skill"][name];
            assert_eq!(f.engine().cvars.text(key), Some(expected), "{argc} {key}");
        }
    }

    // spec BUILTIN-18.5: clientstat and globalstat change nothing; globalstat's name must be a
    // string
    #[test]
    fn stats_change_nothing() {
        let (strings, offsets) = string_table(&["frags"]);
        for (number, name, ok) in [
            (232, 7, true),
            (233, offsets[0], true),
            (233, 999_999, false),
        ] {
            let mut f = Fixture::new(number, 3, strings.clone(), 1);
            f.builtins.engine_mut().profile = EngineProfile::VkQuake;
            let before = f.engine().clone();
            f.set_parm_f(0, 40.0);
            f.set_parm_f(1, 2.0);
            f.set_parm_i(2, name);
            let result = f.run(ServerState::Active);
            assert_eq!(result.is_ok(), ok, "{number}: {result:?}");
            assert_eq!(f.engine(), &before, "{number}");
        }
    }

    // spec BUILTIN-18.3: vkQuake's precache lists hold 2048 sounds and 8192 models, id's 256
    // each
    #[test]
    fn vkquake_precaches_more() {
        assert_eq!(EngineProfile::VkQuake.precache_limit(Precache::Sound), 2048);
        assert_eq!(EngineProfile::VkQuake.precache_limit(Precache::Model), 8192);
        assert_eq!(EngineProfile::Id.precache_limit(Precache::Sound), 256);
        assert_eq!(EngineProfile::Id.precache_limit(Precache::Model), 256);
        for (profile, limit) in [(EngineProfile::Id, 256), (EngineProfile::VkQuake, 2048)] {
            let mut engine = MockEngine::new("test", 0, 1);
            engine.profile = profile;
            for n in 1..limit {
                engine
                    .precache(Precache::Sound, &format!("s{n}.wav"))
                    .unwrap();
            }
            assert_eq!(engine.sound_precache.len(), limit);
            assert!(engine.precache(Precache::Sound, "full.wav").is_err());
        }
    }

    // spec BUILTIN-1.2: a PF_Fixme gap number is the runtime error with id's spelling.
    #[test]
    fn fixme_gap_errors() {
        let mut f = Fixture::new(5, 0, vec![0], 0);
        f.run_err(ServerState::Active, "unimplemented bulitin");
    }

    // spec BUILTIN-16.1: break is a fatal error carrying id's message.
    #[test]
    fn break_statement_errors() {
        let mut f = Fixture::new(6, 0, vec![0], 0);
        f.run_err(ServerState::Active, "break statement");
    }

    // ----- BUILTIN-9.4: setmodel sets modelindex and size from the bounds -----

    // spec BUILTIN-9.4: setmodel of a precached .mdl sets modelindex to its list entry and the
    // size to maxs - mins; pins the size subtraction in b_setmodel.
    #[test]
    fn setmodel_sets_modelindex_and_size() {
        let (strings, offsets) = string_table(&["progs/x.mdl"]);
        let mut f = Fixture::new(3, 2, strings, 0);
        f.builtins
            .engine_mut()
            .precache(Precache::Model, "progs/x.mdl")
            .unwrap();
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_i(1, offsets[0]);
        f.run_ok(ServerState::Active);
        // model_precache is "", "maps/<map>.bsp", then "progs/x.mdl" at index 2.
        assert_eq!(f.field_f(1, "modelindex"), 2.0);
        // size = maxs - mins = 16 - (-16) = 32 on every axis.
        assert_eq!(f.field_v(1, "size"), [32.0, 32.0, 32.0]);
    }

    // ----- BUILTIN-8.6: findradius delta and inclusion -----

    // spec BUILTIN-8.6: findradius includes an edict whose centre is within the radius and
    // excludes one outside it; pins the radius_delta arithmetic and the comparison.
    #[test]
    fn findradius_excludes_far_edicts() {
        let mut f = Fixture::new(22, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1, near
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2, far
        f.set_field_f(1, "solid", 1.0);
        f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
        f.set_field_f(2, "solid", 1.0);
        f.set_field_v(2, "origin", [1000.0, 0.0, 0.0]);
        f.set_parm_v(0, [0.0, 0.0, 0.0]);
        f.set_parm_f(1, 100.0);
        f.run_ok(ServerState::Active);
        // Only edict 1 is within 100 units; it is the chain head, its chain is the world.
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        assert_eq!(f.return_i(), e1.bits());
        let chain = field_by_name("chain").unwrap().slot;
        assert_eq!(
            f.machine
                .edicts()
                .field(EdictNumber::new(1), chain)
                .unwrap()
                .as_i32(),
            0
        );
    }

    // ----- BUILTIN-13: the remaining Write* builtins and their byte counts -----

    // spec BUILTIN-13.2: WriteChar, WriteByte and WriteAngle append one byte; WriteShort and
    // WriteCoord append two; WriteLong four; to the unreliable datagram (MSG_BROADCAST).
    #[test]
    fn write_builtins_byte_counts() {
        let cases = [(52u32, 1usize), (53, 1), (57, 1), (54, 2), (56, 2), (55, 4)];
        for (number, bytes) in cases {
            let mut f = Fixture::new(number, 2, vec![0], 0);
            f.set_parm_f(0, 0.0); // MSG_BROADCAST
            f.set_parm_f(1, 1.0);
            f.run_ok(ServerState::Active);
            assert_eq!(f.engine().buffers.unreliable, bytes, "builtin {number}");
        }
    }

    // spec BUILTIN-13.2: WriteString appends the string bytes plus a NUL to the datagram.
    #[test]
    fn write_string_appends_text_and_nul() {
        let (strings, offsets) = string_table(&["abc"]);
        let mut f = Fixture::new(58, 2, strings, 0);
        f.set_parm_f(0, 0.0); // MSG_BROADCAST
        f.set_parm_i(1, offsets[0]);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.unreliable, 4); // "abc" + NUL
    }

    // spec BUILTIN-13.2: WriteEntity reads the destination, then the entity, and appends two
    // bytes; a bad entity value is the fatal NUM_FOR_EDICT error.
    #[test]
    fn write_entity_appends_two_bytes() {
        let mut f = Fixture::new(59, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_f(0, 2.0); // MSG_ALL, the reliable datagram
        f.set_parm_i(1, e1.bits());
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.reliable, 2);
    }

    // spec BUILTIN-13.1: WriteByte to MSG_ALL uses the reliable datagram and to MSG_INIT the
    // signon buffer, so the write_dest match arms for 2 and 3 are pinned.
    #[test]
    fn write_dest_all_and_init() {
        let mut f = Fixture::new(52, 2, vec![0], 0);
        f.set_parm_f(0, 2.0); // MSG_ALL
        f.set_parm_f(1, 1.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.reliable, 1);
        assert_eq!(f.engine().buffers.unreliable, 0);

        let mut g = Fixture::new(52, 2, vec![0], 0);
        g.set_parm_f(0, 3.0); // MSG_INIT
        g.set_parm_f(1, 1.0);
        g.run_ok(ServerState::Active);
        assert_eq!(g.engine().buffers.signon, 1);
    }

    // spec BUILTIN-13.1: WriteByte to MSG_ONE of a valid client slot keeps nothing (DIFF-6.5)
    // and succeeds, so the MSG_ONE in-range branch of write_dest is pinned.
    #[test]
    fn write_one_valid_client_succeeds() {
        let mut f = Fixture::new(52, 2, vec![0], 1);
        f.set_global_entity("msg_entity", 1); // client slot 1
        f.set_parm_f(0, 1.0); // MSG_ONE
        f.set_parm_f(1, 1.0);
        f.run_ok(ServerState::Active);
        // Nothing is kept for a client's reliable message.
        assert_eq!(f.engine().buffers.unreliable, 0);
        assert_eq!(f.engine().buffers.reliable, 0);
    }

    // ----- BUILTIN-12: ambientsound, particle success paths -----

    // spec BUILTIN-12.4: ambientsound of a precached sound appends to the signon buffer.
    #[test]
    fn ambientsound_appends_to_signon() {
        let (strings, offsets) = string_table(&["ambient/x.wav"]);
        let mut f = Fixture::new(74, 4, strings, 0);
        f.builtins
            .engine_mut()
            .precache(Precache::Sound, "ambient/x.wav")
            .unwrap();
        f.set_parm_v(0, [1.0, 2.0, 3.0]);
        f.set_parm_i(1, offsets[0]);
        f.set_parm_f(2, 1.0);
        f.set_parm_f(3, 1.0);
        f.run_ok(ServerState::Active);
        assert!(f.engine().buffers.signon > 0);
    }

    // spec BUILTIN-12.4: ambientsound of a name not precached is a console message, no bytes.
    #[test]
    fn ambientsound_not_precached_is_silent() {
        let (strings, offsets) = string_table(&["none.wav"]);
        let mut f = Fixture::new(74, 4, strings, 0);
        f.set_parm_v(0, [1.0, 2.0, 3.0]);
        f.set_parm_i(1, offsets[0]);
        f.set_parm_f(2, 1.0);
        f.set_parm_f(3, 1.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.signon, 0);
    }

    // spec BUILTIN-12.5: particle appends its bytes to the unreliable datagram.
    #[test]
    fn particle_appends_to_datagram() {
        let mut f = Fixture::new(48, 4, vec![0], 0);
        f.set_parm_v(0, [0.0, 0.0, 0.0]);
        f.set_parm_v(1, [0.0, 0.0, 0.0]);
        f.set_parm_f(2, 7.0);
        f.set_parm_f(3, 10.0);
        f.run_ok(ServerState::Active);
        assert!(f.engine().buffers.unreliable > 0);
    }

    // spec BUILTIN-12.3: a sound with the default volume and attenuation 1 omits two bytes
    // (sound_flag_savings), so the datagram grows by fewer bytes than a non-default sound.
    #[test]
    fn sound_flag_savings_shrink_the_message() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        // Default volume (1.0 * 255 = 255) and attenuation 1.0: both flags saved.
        let mut f = Fixture::new(8, 5, strings.clone(), 0);
        f.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_f(1, 0.0);
        f.set_parm_i(2, offsets[0]);
        f.set_parm_f(3, 1.0); // default volume
        f.set_parm_f(4, 1.0); // attenuation 1
        f.run_ok(ServerState::Active);
        let saved_bytes = f.engine().buffers.unreliable;

        // A non-default volume and attenuation keep both bytes.
        let mut g = Fixture::new(8, 5, strings, 0);
        g.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        g.machine.edicts_mut().alloc(1.0).unwrap();
        let ge1 = g.machine.edicts().edict_num(1).unwrap();
        g.set_parm_i(0, ge1.bits());
        g.set_parm_f(1, 0.0);
        g.set_parm_i(2, offsets[0]);
        g.set_parm_f(3, 0.5); // non-default volume
        g.set_parm_f(4, 2.0); // non-default attenuation
        g.run_ok(ServerState::Active);
        let full_bytes = g.engine().buffers.unreliable;
        assert_eq!(full_bytes, saved_bytes + 2);
    }

    // ----- BUILTIN-11: sprint, centerprint and dprint success -----

    // spec BUILTIN-11.2, BUILTIN-11.3: sprint, centerprint to a valid client and dprint read
    // and check their text and succeed; a non-client sprint is a console message, not an error.
    #[test]
    fn print_builtins_succeed() {
        let (strings, offsets) = string_table(&["hi"]);
        // sprint (24) to client slot 1.
        let mut f = Fixture::new(24, 2, strings.clone(), 1);
        f.set_parm_i(0, f.machine.edicts().edict_num(1).unwrap().bits());
        f.set_parm_i(1, offsets[0]);
        f.run_ok(ServerState::Active);

        // sprint to a non-client (slot 0, the world): a console message, still Ok.
        let mut g = Fixture::new(24, 2, strings.clone(), 1);
        g.set_parm_i(0, g.machine.edicts().edict_num(0).unwrap().bits());
        g.set_parm_i(1, offsets[0]);
        g.run_ok(ServerState::Active);

        // dprint (25) reads and checks the text.
        let mut h = Fixture::new(25, 1, strings, 1);
        h.set_parm_i(0, offsets[0]);
        h.run_ok(ServerState::Active);
    }

    // spec BUILTIN-11.2: sprint of a bad string to a valid client is the invalid string error,
    // proving the client-in-range branch reads the text (not an early Ok).
    #[test]
    fn sprint_valid_client_checks_string() {
        let mut f = Fixture::new(24, 2, vec![0], 1);
        f.set_parm_i(0, f.machine.edicts().edict_num(1).unwrap().bits());
        f.set_parm_i(1, 0x0100_0000); // bad string value
        f.run_err(ServerState::Active, "invalid string");
    }

    // ----- DIFF-7: aim, checkclient, changeyaw, movetogoal -----

    // spec DIFF-7.4: aim returns a copy of v_forward.
    #[test]
    fn aim_returns_v_forward() {
        let mut f = Fixture::new(44, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        // Poison RETURN so a no-op body (leaving RETURN unchanged) is caught.
        f.set_parm_i(0, e1.bits());
        f.set_parm_f(1, 100.0);
        f.machine
            .set_global3(i32::from(PROGS_OFS_RETURN), [Slot::from_f32(-9.0); 3])
            .unwrap();
        let v_forward = global_by_name("v_forward").unwrap().slot;
        f.machine
            .set_global3(
                i32::try_from(v_forward).unwrap(),
                [0.5, -0.25, 1.0].map(Slot::from_f32),
            )
            .unwrap();
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_v(), [0.5, -0.25, 1.0]);
        // aim copies v_forward into RETURN, so the two agree whatever v_forward holds.
        assert_eq!(f.return_v(), f.sysglobal_v("v_forward"));
        // And it is not the poison value.
        assert_ne!(f.return_v(), [-9.0, -9.0, -9.0]);
    }

    // spec DIFF-7.3: checkclient with no clients returns the world (edict 0).
    #[test]
    fn checkclient_without_clients_returns_world() {
        let mut f = Fixture::new(17, 0, vec![0], 0);
        f.run_ok(ServerState::Active);
        let world = f.machine.edicts().edict_num(0).unwrap();
        assert_eq!(f.return_i(), world.bits());
    }

    // spec DIFF-7.3: checkclient chooses a healthy, non-notarget client in a slot and returns
    // it; pins pick_client's qualifying branch.
    #[test]
    fn checkclient_chooses_a_healthy_client() {
        let mut f = Fixture::new(17, 0, vec![0], 1);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1 is client slot 1
        in_use(&mut f, 1);
        f.set_field_f(1, "health", 100.0);
        f.set_field_f(1, "flags", 0.0);
        f.run_ok(ServerState::Active);
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        assert_eq!(f.return_i(), e1.bits());
    }

    // spec DIFF-7.3: a client with FL_NOTARGET does not qualify, so checkclient's search skips
    // it and chooses the next healthy client; pins pick_client's flag mask.
    #[test]
    fn checkclient_skips_notarget() {
        let mut f = Fixture::new(17, 0, vec![0], 2);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1 is client slot 1
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2 is client slot 2
        in_use(&mut f, 1);
        in_use(&mut f, 2);
        // The search starts from check+1 = 1; slot 1 has FL_NOTARGET, slot 2 qualifies.
        f.set_field_f(1, "health", 100.0);
        f.set_field_f(1, "flags", 128.0); // FL_NOTARGET
        f.set_field_f(2, "health", 100.0);
        f.set_field_f(2, "flags", 0.0);
        f.run_ok(ServerState::Active);
        let e2 = f.machine.edicts().edict_num(2).unwrap();
        assert_eq!(f.return_i(), e2.bits());
    }

    // spec BUILTIN-15.4: ChangeYaw turns self's yaw toward ideal_yaw by at most yaw_speed; a
    // 90-degree target with yaw_speed 20 moves angles_y to anglemod(20).
    #[test]
    fn changeyaw_turns_toward_ideal() {
        let mut f = Fixture::new(49, 0, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_v(1, "angles", [0.0, 0.0, 0.0]);
        f.set_field_f(1, "ideal_yaw", 90.0);
        f.set_field_f(1, "yaw_speed", 20.0);
        f.set_global_entity("self", 1);
        f.run_ok(ServerState::Active);
        // anglemod(20) is 19.995117.
        assert_eq!(f.field_v(1, "angles")[1], 19.995_117);
    }

    // spec BUILTIN-15.4: ChangeYaw of a yaw already at ideal does not move; pins the early
    // return.
    #[test]
    fn changeyaw_at_ideal_does_not_move() {
        let mut f = Fixture::new(49, 0, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_v(1, "angles", [0.0, 45.0, 0.0]);
        f.set_field_f(1, "ideal_yaw", 45.0);
        f.set_field_f(1, "yaw_speed", 20.0);
        f.set_global_entity("self", 1);
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_v(1, "angles")[1], 45.0);
    }

    /// BUILTIN-15.4 as its text gives it: the yaw ChangeYaw leaves for `angles_y`, `ideal` and
    /// `speed`.
    #[expect(
        clippy::float_cmp,
        reason = "spec BUILTIN-15.4: the reference compares exactly, as id's ChangeYaw does"
    )]
    fn spec_change_yaw(angles_y: f32, ideal: f32, speed: f32) -> f32 {
        let current = math::anglemod(angles_y);
        if current == ideal {
            return angles_y;
        }
        let mut mv = ideal - current;
        if ideal > current {
            if mv >= 180.0 {
                mv -= 360.0;
            }
        } else if mv <= -180.0 {
            mv += 360.0;
        }
        if mv > 0.0 {
            if mv > speed {
                mv = speed;
            }
        } else if mv < -speed {
            mv = -speed;
        }
        math::anglemod(current + mv)
    }

    // spec BUILTIN-15.4: ChangeYaw wraps a turn of 180 or more the short way and clamps it to
    // yaw_speed either way, exactly as the statement's text, at each boundary: a turn of
    // exactly +180 and -180, a turn equal to the speed, a 360-degree target from 0 that wraps
    // to no turn, a negative speed, and a NaN target, which id's comparisons leave as NaN
    #[test]
    fn changeyaw_matches_the_statement_at_its_boundaries() {
        let cases = [
            (0.0, 180.0, 10.0),
            (180.0, 0.0, 10.0),
            (0.0, 90.0, 90.0),
            (90.0, 0.0, 90.0),
            (0.0, 360.0, -5.0),
            (0.0, 360.0, 5.0),
            (10.0, 0.0, -5.0),
            (0.0, 200.0, 30.0),
            (200.0, 0.0, 30.0),
            (0.0, 30.0, f32::NAN),
            (30.0, f32::NAN, 10.0),
        ];
        for (angles_y, ideal, speed) in cases {
            let mut f = Fixture::new(49, 0, vec![0], 0);
            f.machine.edicts_mut().alloc(1.0).unwrap();
            f.set_field_v(1, "angles", [0.0, angles_y, 0.0]);
            f.set_field_f(1, "ideal_yaw", ideal);
            f.set_field_f(1, "yaw_speed", speed);
            f.set_global_entity("self", 1);
            f.run_ok(ServerState::Active);
            let want = spec_change_yaw(angles_y, ideal, speed);
            assert_eq!(
                f.field_v(1, "angles")[1].to_bits(),
                want.to_bits(),
                "angles_y {angles_y}, ideal {ideal}, speed {speed}: want {want}"
            );
        }
    }

    // spec DIFF-7.3: a client with health 0 does not qualify, so checkclient's search, which
    // starts at check + 1 (slot 2 here), skips it and wraps round to the healthy slot 1
    #[test]
    fn checkclient_skips_a_client_without_health() {
        let mut f = Fixture::new(17, 0, vec![0], 2);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.machine.edicts_mut().alloc(1.0).unwrap();
        in_use(&mut f, 1);
        in_use(&mut f, 2);
        f.set_field_f(1, "health", 100.0);
        f.set_field_f(1, "flags", 0.0);
        f.set_field_f(2, "health", 0.0);
        f.set_field_f(2, "flags", 0.0);
        f.run_ok(ServerState::Active);
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        assert_eq!(f.return_i(), e1.bits());
        assert_eq!(f.engine().checkclient.last_check, 1);
    }

    // spec DIFF-7.8: movetogoal on the ground turns toward ideal_yaw and steps when within 45
    // degrees of it; aligned (ideal 0) it moves origin_x by the step distance.
    #[test]
    fn movetogoal_steps_when_aligned() {
        let mut f = Fixture::new(67, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 512.0); // FL_ONGROUND
        f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
        f.set_field_v(1, "angles", [0.0, 0.0, 0.0]);
        f.set_field_f(1, "ideal_yaw", 0.0);
        f.set_field_f(1, "yaw_speed", 20.0);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 10.0); // step
        f.run_ok(ServerState::Active);
        // yaw 0 by 10 moves origin_x to 10.
        assert_eq!(f.field_v(1, "origin")[0], 10.0);
    }

    // spec DIFF-7.5: droptofloor sets groundentity to the world (0) and FL_ONGROUND; pins the
    // groundentity write and the flag or.
    #[test]
    fn droptofloor_sets_groundentity_world() {
        let mut f = Fixture::new(34, 0, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 0.0);
        f.set_field_v(1, "origin", [0.0, 0.0, 50.0]);
        f.set_field_i(1, "groundentity", 99); // a wrong value to be overwritten
        f.set_global_entity("self", 1);
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_f(1, "flags"), 512.0); // FL_ONGROUND
        assert_eq!(f.field_i(1, "groundentity"), 0);
        assert_eq!(f.return_f(), 1.0);
    }

    // ----- link_edict: absmin/absmax from origin, mins, maxs and the widen (EDICT-12.2) -----

    // spec BUILTIN-9.1, EDICT-12.2: setorigin links the edict, writing absmin = origin + mins
    // - widen and absmax = origin + maxs + widen; with non-zero origin, mins and maxs every
    // sum in link_edict is distinguishable. A non-item edict widens by 1.
    #[test]
    fn link_edict_absmin_absmax_non_item() {
        let mut f = Fixture::new(2, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_v(1, "mins", [-2.0, -3.0, -4.0]);
        f.set_field_v(1, "maxs", [5.0, 6.0, 7.0]);
        f.set_field_f(1, "flags", 0.0); // not an item: widen 1
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_v(1, [10.0, 20.0, 30.0]); // origin
        f.run_ok(ServerState::Active);
        // absmin = origin + mins - 1.
        assert_eq!(f.field_v(1, "absmin"), [7.0, 16.0, 25.0]);
        // absmax = origin + maxs + 1.
        assert_eq!(f.field_v(1, "absmax"), [16.0, 27.0, 38.0]);
    }

    // spec EDICT-12.2: an item edict (FL_ITEM) widens x and y by 15 and z by 0, so absmin and
    // absmax differ from the non-item widen; pins the widen selection.
    #[test]
    fn link_edict_item_widen() {
        let mut f = Fixture::new(2, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_v(1, "mins", [0.0, 0.0, 0.0]);
        f.set_field_v(1, "maxs", [0.0, 0.0, 0.0]);
        f.set_field_f(1, "flags", 256.0); // FL_ITEM
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_v(1, [100.0, 100.0, 100.0]);
        f.run_ok(ServerState::Active);
        // widen (15, 15, 0): absmin = origin - (15,15,0), absmax = origin + (15,15,0).
        assert_eq!(f.field_v(1, "absmin"), [85.0, 85.0, 100.0]);
        assert_eq!(f.field_v(1, "absmax"), [115.0, 115.0, 100.0]);
    }

    // ----- findradius radius_delta and inclusion boundary (BUILTIN-8.5) -----

    // spec BUILTIN-8.5: findradius measures org to the edict's centre, origin + (mins+maxs)/2;
    // an edict just inside the radius is included and one just outside is excluded, with
    // non-zero origin, mins and maxs so radius_delta's sums are pinned.
    #[test]
    fn findradius_centre_and_boundary() {
        // Edict centre at origin + (mins+maxs)/2 = (10,0,0) + ((-2+4)/2, 0, 0) = (11, 0, 0).
        // Distance from org (0,0,0) is 11.
        let mut inc = Fixture::new(22, 2, vec![0], 0);
        inc.machine.edicts_mut().alloc(1.0).unwrap();
        inc.set_field_f(1, "solid", 1.0);
        inc.set_field_v(1, "origin", [10.0, 0.0, 0.0]);
        inc.set_field_v(1, "mins", [-2.0, 0.0, 0.0]);
        inc.set_field_v(1, "maxs", [4.0, 0.0, 0.0]);
        inc.set_parm_v(0, [0.0, 0.0, 0.0]);
        inc.set_parm_f(1, 11.5); // radius 11.5 > 11: included
        inc.run_ok(ServerState::Active);
        let e1 = inc.machine.edicts().edict_num(1).unwrap();
        assert_eq!(inc.return_i(), e1.bits());

        let mut exc = Fixture::new(22, 2, vec![0], 0);
        exc.machine.edicts_mut().alloc(1.0).unwrap();
        exc.set_field_f(1, "solid", 1.0);
        exc.set_field_v(1, "origin", [10.0, 0.0, 0.0]);
        exc.set_field_v(1, "mins", [-2.0, 0.0, 0.0]);
        exc.set_field_v(1, "maxs", [4.0, 0.0, 0.0]);
        exc.set_parm_v(0, [0.0, 0.0, 0.0]);
        exc.set_parm_f(1, 10.5); // radius 10.5 < 11: excluded, chain stays world
        exc.run_ok(ServerState::Active);
        assert_eq!(exc.return_i(), 0);
    }

    // spec BUILTIN-8.5: a solid-but-SOLID_NOT (0) edict is skipped; pins the `!exact_eq(solid,
    // 0)` guard (an edict with solid 0 is never included even when within the radius).
    #[test]
    fn findradius_skips_solid_not() {
        let mut f = Fixture::new(22, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "solid", 0.0); // SOLID_NOT
        f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
        f.set_parm_v(0, [0.0, 0.0, 0.0]);
        f.set_parm_f(1, 100.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_i(), 0);
    }

    // ----- loop bounds: find, nextent do not examine edict num_edicts (BUILTIN-8.4, 8.8) -----

    // spec BUILTIN-8.4: nextent of the last in-use edict returns the world, not an edict at
    // num_edicts; pins the `number < num_edicts` loop bound.
    #[test]
    fn nextent_past_the_last_is_world() {
        let mut f = Fixture::new(47, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1, num_edicts now 2
        f.machine.edicts_mut().free(EdictNumber::new(1), 1.0); // free edict 1
        // nextent(world): slot 1 is free, so none qualifies below num_edicts (2) -> world.
        f.set_parm_i(0, 0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_i(), 0);
    }

    // spec BUILTIN-8.8: find that matches nothing below num_edicts returns the world; pins the
    // find loop bound and the string comparison.
    #[test]
    fn find_no_match_returns_world() {
        let (strings, offsets) = string_table(&["zzz", "classname_value"]);
        let mut f = Fixture::new(18, 3, strings, 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        let classname = field_by_name("classname").unwrap().slot;
        f.machine
            .edicts_mut()
            .set_field(EdictNumber::new(1), classname, Slot::from_i32(offsets[1]))
            .unwrap();
        let world = f.machine.edicts().edict_num(0).unwrap();
        f.set_parm_i(0, world.bits());
        f.set_parm_i(1, i32::try_from(classname).unwrap());
        f.set_parm_i(2, offsets[0]); // "zzz" matches nothing
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_i(), 0);
    }

    // ----- setspawnparms, stuffcmd, print_to_client client-range checks -----

    // spec BUILTIN-8.7: setspawnparms of a client slot succeeds; of the world (0) or a slot
    // above maxclients is the range error. Pins the `number < 1 || number > maxclients` test.
    #[test]
    fn setspawnparms_client_range() {
        // Slot 1 with maxclients 2 succeeds.
        let mut ok = Fixture::new(78, 1, vec![0], 2);
        ok.set_parm_i(0, ok.machine.edicts().edict_num(1).unwrap().bits());
        ok.run_ok(ServerState::Active);
        // The world (slot 0) is below 1: the error.
        let mut world = Fixture::new(78, 1, vec![0], 2);
        world.set_parm_i(0, world.machine.edicts().edict_num(0).unwrap().bits());
        world.run_err(ServerState::Active, "Entity is not a client");
    }

    // spec BUILTIN-11.4: stuffcmd to client slot 1 (in range) succeeds; pins the lower bound of
    // the client-range test (slot exactly 1 is valid).
    #[test]
    fn stuffcmd_valid_client_succeeds() {
        let (strings, offsets) = string_table(&["cmd\n"]);
        let mut f = Fixture::new(21, 2, strings, 1);
        f.set_parm_i(0, f.machine.edicts().edict_num(1).unwrap().bits());
        f.set_parm_i(1, offsets[0]);
        f.run_ok(ServerState::Active);
    }

    // spec BUILTIN-11.2: centerprint to a valid client reads and checks its text; a bad string
    // is the invalid string error (pins b_centerprint and print_to_client's in-range branch).
    #[test]
    fn centerprint_valid_client_checks_string() {
        let mut f = Fixture::new(73, 2, vec![0], 1);
        f.set_parm_i(0, f.machine.edicts().edict_num(1).unwrap().bits());
        f.set_parm_i(1, 0x0100_0000); // bad string
        f.run_err(ServerState::Active, "invalid string");
    }

    // spec BUILTIN-11.3: dprint reads and checks its text; a bad string is the invalid string
    // error, proving the body is not a no-op.
    #[test]
    fn dprint_checks_string() {
        let mut f = Fixture::new(25, 1, vec![0], 0);
        f.set_parm_i(0, 0x0100_0000); // bad string
        f.run_err(ServerState::Active, "invalid string");
    }

    // ----- exact message byte counts (BUILTIN-12) -----

    // spec BUILTIN-12.3: a sound with both flags omitted (non-default volume, non-1 attenuation)
    // occupies 13 bytes: svc, flags, volume, attenuation, a short channel, the sound byte and
    // three coordinates. Pins the BUILTIN_SOUND_MAX_BYTES sum.
    #[test]
    fn sound_full_message_is_thirteen_bytes() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut f = Fixture::new(8, 5, strings, 0);
        f.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_f(1, 0.0);
        f.set_parm_i(2, offsets[0]);
        f.set_parm_f(3, 0.5); // non-default volume keeps the volume byte
        f.set_parm_f(4, 2.0); // non-1 attenuation keeps the attenuation byte
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.unreliable, 13);
    }

    // spec BUILTIN-12.5: a particle message occupies 12 bytes: svc, three coords, three
    // direction chars, count and color. Pins BUILTIN_PARTICLE_BYTES.
    #[test]
    fn particle_message_is_twelve_bytes() {
        let mut f = Fixture::new(48, 4, vec![0], 0);
        f.set_parm_v(0, [0.0, 0.0, 0.0]);
        f.set_parm_v(1, [0.0, 0.0, 0.0]);
        f.set_parm_f(2, 7.0);
        f.set_parm_f(3, 10.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.unreliable, 12);
    }

    // spec BUILTIN-12.4: an ambientsound message occupies 10 bytes. Pins
    // BUILTIN_AMBIENTSOUND_BYTES.
    #[test]
    fn ambientsound_message_is_ten_bytes() {
        let (strings, offsets) = string_table(&["ambient/x.wav"]);
        let mut f = Fixture::new(74, 4, strings, 0);
        f.builtins
            .engine_mut()
            .precache(Precache::Sound, "ambient/x.wav")
            .unwrap();
        f.set_parm_v(0, [1.0, 2.0, 3.0]);
        f.set_parm_i(1, offsets[0]);
        f.set_parm_f(2, 1.0);
        f.set_parm_f(3, 1.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.signon, 10);
    }

    // spec BUILTIN-12.8: a makestatic message occupies 14 bytes and frees the edict. Pins
    // BUILTIN_MAKESTATIC_BYTES.
    #[test]
    fn makestatic_message_is_fourteen_bytes() {
        let mut f = Fixture::new(69, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.run_ok(ServerState::Active);
        assert_eq!(f.engine().buffers.signon, 14);
    }

    // spec BUILTIN-12.1: sound checks volume, then attenuation, then channel, in that order; a
    // bad attenuation after a valid volume is the attenuation error, and a bad channel after a
    // valid volume and attenuation is the channel error. Pins the order of the three checks.
    #[test]
    fn sound_checks_attenuation_then_channel() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        let mut atten = Fixture::new(8, 5, strings.clone(), 0);
        atten
            .builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        atten.set_parm_f(1, 0.0); // channel ok
        atten.set_parm_i(2, offsets[0]);
        atten.set_parm_f(3, 1.0); // volume 255 ok
        atten.set_parm_f(4, 5.0); // attenuation > 4: the attenuation error
        atten.run_err(ServerState::Active, "SV_StartSound: attenuation = 5.000000");

        let mut chan = Fixture::new(8, 5, strings, 0);
        chan.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        chan.set_parm_f(1, 9.0); // channel > 7: the channel error
        chan.set_parm_i(2, offsets[0]);
        chan.set_parm_f(3, 1.0); // volume ok
        chan.set_parm_f(4, 1.0); // attenuation ok
        chan.run_err(ServerState::Active, "SV_StartSound: channel = 9");
    }

    // ----- checkclient reconsider interval and the walkmove/movetogoal flag masks -----

    // spec DIFF-7.3: checkclient reconsiders only when the clock has advanced at least 0.1s
    // since the last choice; with the clock at the default 1.0 and last_check_time 1.0 it does
    // not reconsider, keeping last_check (world). Pins the interval test.
    #[test]
    fn checkclient_keeps_choice_within_interval() {
        let mut f = Fixture::new(17, 0, vec![0], 1);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        in_use(&mut f, 1);
        f.set_field_f(1, "health", 100.0);
        // The last check was already made at the current clock, so no reconsider; last_check
        // starts at 0 (world), which is returned.
        f.builtins.engine_mut().checkclient.last_check_time = 1.0;
        f.builtins.engine_mut().checkclient.last_check = 0;
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_i(), 0);
    }

    // spec DIFF-7.7: walkmove with only FL_FLY set (not FL_ONGROUND or FL_SWIM) still moves,
    // returning 1; pins the FL_ONGROUND|FL_FLY|FL_SWIM mask (a single flag passes).
    #[test]
    fn walkmove_with_fly_flag() {
        let mut f = Fixture::new(32, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 1.0); // FL_FLY
        f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 0.0);
        f.set_parm_f(1, 10.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_f(), 1.0);
        assert_eq!(f.field_v(1, "origin")[0], 10.0);
    }

    // spec DIFF-7.8: movetogoal with only FL_SWIM set moves; pins the movetogoal flag mask.
    #[test]
    fn movetogoal_with_swim_flag() {
        let mut f = Fixture::new(67, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 2.0); // FL_SWIM
        f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
        f.set_field_v(1, "angles", [0.0, 0.0, 0.0]);
        f.set_field_f(1, "ideal_yaw", 0.0);
        f.set_field_f(1, "yaw_speed", 20.0);
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 10.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_v(1, "origin")[0], 10.0);
    }

    // spec DIFF-7.8: movetogoal does not step when the yaw is more than 45 degrees from
    // ideal_yaw; with angles_y 0 and ideal_yaw 90 the delta is -90 (315 excluded range), so it
    // steps, but with ideal_yaw turned far it holds. Pins the 45/315 step window.
    #[test]
    fn movetogoal_holds_when_far_from_ideal() {
        let mut f = Fixture::new(67, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 512.0); // FL_ONGROUND
        f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
        f.set_field_v(1, "angles", [0.0, 100.0, 0.0]); // angles_y 100
        f.set_field_f(1, "ideal_yaw", 0.0); // delta = 100 - 0 = 100, in (45, 315): no step
        f.set_field_f(1, "yaw_speed", 0.0); // do not turn, so angles_y stays ~100
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 10.0);
        f.run_ok(ServerState::Active);
        // No step taken: origin_x unchanged.
        assert_eq!(f.field_v(1, "origin")[0], 0.0);
    }

    // ----- change_yaw: positive and negative turns, clamping and wrap-around (BUILTIN-15.4) -----

    // spec BUILTIN-15.4: a turn larger than yaw_speed is clamped to yaw_speed in both
    // directions, and a turn smaller than yaw_speed is taken in full; a target across the
    // 0/360 seam takes the short way. Each case pins a branch of the turn logic.
    #[test]
    fn changeyaw_cases() {
        let cases = [
            // (angles_y, ideal, yaw_speed, expected_angles_y)
            (0.0f32, 90.0f32, 20.0f32, 19.995_117f32), // positive, clamped
            (90.0, 0.0, 20.0, 69.999_39),              // negative, clamped
            (0.0, 10.0, 20.0, 9.997_559),              // positive, not clamped
            (10.0, 350.0, 30.0, 350.002_44),           // wrap down the short way
            (350.0, 10.0, 30.0, 9.997_559),            // wrap up the short way
            (20.0, 10.0, 30.0, 9.997_559),             // negative, not clamped
        ];
        for (angles_y, ideal, speed, expected) in cases {
            let mut f = Fixture::new(49, 0, vec![0], 0);
            f.machine.edicts_mut().alloc(1.0).unwrap();
            f.set_field_v(1, "angles", [0.0, angles_y, 0.0]);
            f.set_field_f(1, "ideal_yaw", ideal);
            f.set_field_f(1, "yaw_speed", speed);
            f.set_global_entity("self", 1);
            f.run_ok(ServerState::Active);
            assert_eq!(
                f.field_v(1, "angles")[1],
                expected,
                "angles_y {angles_y} ideal {ideal} speed {speed}"
            );
        }
    }

    // ----- field and argument helpers and guards -----

    // spec BUILTIN-8.3: a field slot exactly at entityfields is out of range (invalid field),
    // and one just below is valid; pins checked_field's `slot < fields` bound.
    #[test]
    fn checked_field_boundary() {
        // entityfields is 105 (ENTITY_FIELDS); slot 105 is invalid, 104 is valid.
        let (strings, offsets) = string_table(&["x"]);
        let mut bad = Fixture::new(18, 3, strings.clone(), 0); // find
        bad.machine.edicts_mut().alloc(1.0).unwrap();
        bad.set_parm_i(0, bad.machine.edicts().edict_num(0).unwrap().bits());
        bad.set_parm_i(1, 105); // == entityfields: invalid field
        bad.set_parm_i(2, offsets[0]);
        bad.run_err(ServerState::Active, "invalid field");

        // Slot 104 is a valid field; find examines edict 1 and finds no match, returns world.
        let mut ok = Fixture::new(18, 3, strings, 0);
        ok.machine.edicts_mut().alloc(1.0).unwrap();
        ok.set_parm_i(0, ok.machine.edicts().edict_num(0).unwrap().bits());
        ok.set_parm_i(1, 104);
        ok.set_parm_i(2, offsets[0]);
        ok.run_ok(ServerState::Active);
        assert_eq!(ok.return_i(), 0);
    }

    // spec BUILTIN-2.9: setorigin reads its edict with the G_EDICT check; a negative entity
    // value (names no edict) is invalid entity, pinning checked_edict's `number >= 0` guard
    // and read_parm_edict.
    #[test]
    fn setorigin_negative_entity_is_invalid() {
        let mut f = Fixture::new(2, 2, vec![0], 0);
        f.set_parm_i(0, -4); // a negative entity value
        f.set_parm_v(1, [1.0, 2.0, 3.0]);
        f.run_err(ServerState::Active, "invalid entity");
    }

    // spec BUILTIN-12.8: makestatic of an edict whose model field names an engine string that
    // is not a precached model is the fatal not-precached error, exercising read_field_int and
    // resolve_string on an engine-made value.
    #[test]
    fn makestatic_unprecached_model_errors() {
        let mut f = Fixture::new(69, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        // Make an engine string "progs/none.mdl" and store its value in the model field.
        let value = f
            .builtins
            .engine_mut()
            .strings
            .make(b"progs/none.mdl".to_vec());
        f.set_field_i(1, "model", value);
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.run_err(
            ServerState::Active,
            "SV_ModelIndex: model progs/none.mdl not precached",
        );
    }

    // spec BUILTIN-10.2: a precache string whose first byte is a space or below is Bad string;
    // a high-bit first byte is also Bad string. Pins bad_precache_string's `|| byte <= b' '`.
    #[test]
    fn precache_bad_string_variants() {
        // A leading space.
        let (strings, offsets) = string_table(&[" x.wav"]);
        let mut space = Fixture::new(19, 1, strings, 0);
        space.set_parm_i(0, offsets[0]);
        space.run_err(ServerState::Loading, "Bad string");

        // A high-bit first byte (0x80).
        let mut bytes = vec![0_u8];
        let off = i32::try_from(bytes.len()).unwrap();
        bytes.extend_from_slice(&[0x80, b'a', 0]);
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let mut high = Fixture::new(19, 1, bytes, 0);
        high.set_parm_i(0, off);
        high.run_err(ServerState::Loading, "Bad string");
    }

    // spec VM-7.3, EDICT-13.1: engine_string resolves an engine-allocated string value the
    // mock holds, so a setmodel of a model named by an engine string (e.g. the world's model)
    // resolves and finds the precached entry; pins MockBuiltins::engine_string.
    #[test]
    fn engine_string_resolves_in_setmodel() {
        let mut f = Fixture::new(3, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        // Precache a model and make an engine string naming it.
        f.builtins
            .engine_mut()
            .precache(Precache::Model, "progs/e.mdl")
            .unwrap();
        let value = f
            .builtins
            .engine_mut()
            .strings
            .make(b"progs/e.mdl".to_vec());
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_i(1, value); // the engine-string value
        f.run_ok(ServerState::Active);
        // model is the engine-string value itself; modelindex is the list entry.
        assert_eq!(f.field_i(1, "model"), value);
        assert!(f.field_f(1, "modelindex") > 0.0);
    }

    // spec VM-7.3, EDICT-13.1, EDICT-13.3: the engine_string trait method returns the text of
    // an engine-allocated string the mock holds and None for a value it does not, so the VM's
    // string opcodes can resolve one. Pins MockBuiltins::engine_string's bytes.
    #[test]
    fn engine_string_returns_the_held_text() {
        let mut builtins = MockBuiltins::default();
        let value = builtins.engine_mut().strings.make(b"netname".to_vec());
        assert_eq!(
            qc_vm::Builtins::engine_string(&builtins, value),
            Some(b"netname".as_slice())
        );
        // A value no engine string holds is None.
        assert_eq!(qc_vm::Builtins::engine_string(&builtins, -999), None);
        assert_eq!(qc_vm::Builtins::engine_string(&builtins, 5), None);
    }

    // spec BUILTIN-6.6: b_random writes a value to RETURN; poisoning RETURN first proves the
    // body writes it (a no-op body would leave the poison).
    #[test]
    fn random_writes_the_return_slot() {
        let mut f = Fixture::new(7, 0, vec![0], 0);
        f.machine
            .set_global(i32::from(PROGS_OFS_RETURN), Slot::from_f32(-42.0))
            .unwrap();
        f.run_ok(ServerState::Active);
        assert_ne!(f.return_f(), -42.0);
        assert!((0.0..=1.0).contains(&f.return_f()));
    }

    // spec BUILTIN-9.2: setsize of equal mins and maxs on an axis is not backwards and
    // succeeds (a flat box); pins the `a > b` test against `>=`/`==` (equal must pass).
    #[test]
    fn setsize_equal_bounds_succeed() {
        let mut f = Fixture::new(4, 3, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        f.set_parm_i(0, e1.bits());
        f.set_parm_v(1, [5.0, 0.0, -3.0]); // min
        f.set_parm_v(2, [5.0, 0.0, -3.0]); // max equal on every axis
        f.run_ok(ServerState::Active);
        assert_eq!(f.field_v(1, "size"), [0.0, 0.0, 0.0]);
    }

    // spec BUILTIN-12.1: a sound volume of exactly 0 or 255 is in range and succeeds; 256 (vol
    // > 1) is the fatal volume error. Pins the 0..=255 range boundaries.
    #[test]
    fn sound_volume_boundaries() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        for vol in [0.0f32, 1.0] {
            let mut f = Fixture::new(8, 5, strings.clone(), 0);
            f.builtins
                .engine_mut()
                .precache(Precache::Sound, "misc/x.wav")
                .unwrap();
            f.machine.edicts_mut().alloc(1.0).unwrap();
            let e1 = f.machine.edicts().edict_num(1).unwrap();
            f.set_parm_i(0, e1.bits());
            f.set_parm_f(1, 0.0);
            f.set_parm_i(2, offsets[0]);
            f.set_parm_f(3, vol); // 0 -> 0, 1.0 -> 255: both in range
            f.set_parm_f(4, 1.0);
            f.run_ok(ServerState::Active);
        }
        // Just above 255 (vol slightly over 1) is out of range.
        let mut over = Fixture::new(8, 5, strings, 0);
        over.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        over.set_parm_f(1, 0.0);
        over.set_parm_i(2, offsets[0]);
        over.set_parm_f(3, 256.0 / 255.0); // 256
        over.set_parm_f(4, 1.0);
        over.run_err(ServerState::Active, "SV_StartSound: volume = 256");
    }

    // spec BUILTIN-12.2: a sound or particle is dropped only when the datagram is strictly past
    // MAX_DATAGRAM - 16 (1008); at exactly 1008 it still appends. Pins the `> 1008` boundary.
    #[test]
    fn sound_datagram_boundary() {
        use crate::mock::MessageDest;
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        // Exactly 1008 bytes used: a sound still appends (1008 > 1008 is false).
        let mut at = Fixture::new(8, 5, strings.clone(), 0);
        at.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        at.builtins
            .engine_mut()
            .append_message(MessageDest::Broadcast, 1008)
            .unwrap();
        at.machine.edicts_mut().alloc(1.0).unwrap();
        let e1 = at.machine.edicts().edict_num(1).unwrap();
        at.set_parm_i(0, e1.bits());
        at.set_parm_f(1, 0.0);
        at.set_parm_i(2, offsets[0]);
        at.set_parm_f(3, 0.5);
        at.set_parm_f(4, 2.0);
        at.run_ok(ServerState::Active);
        assert!(at.engine().buffers.unreliable > 1008);

        // One past the threshold (1009): the sound is dropped, size unchanged.
        let mut past = Fixture::new(8, 5, strings, 0);
        past.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        past.builtins
            .engine_mut()
            .append_message(MessageDest::Broadcast, 1009)
            .unwrap();
        past.set_parm_i(0, past.machine.edicts().edict_num(0).unwrap().bits());
        past.set_parm_f(1, 0.0);
        past.set_parm_i(2, offsets[0]);
        past.set_parm_f(3, 0.5);
        past.set_parm_f(4, 2.0);
        past.run_ok(ServerState::Active);
        assert_eq!(past.engine().buffers.unreliable, 1009);
    }

    // spec BUILTIN-12.5: particle is dropped only when the datagram is strictly past 1008; at
    // exactly 1008 it still appends. Pins b_particle's `> 1008` boundary.
    #[test]
    fn particle_datagram_boundary() {
        use crate::mock::MessageDest;
        let mut at = Fixture::new(48, 4, vec![0], 0);
        at.builtins
            .engine_mut()
            .append_message(MessageDest::Broadcast, 1008)
            .unwrap();
        at.set_parm_v(0, [0.0, 0.0, 0.0]);
        at.set_parm_v(1, [0.0, 0.0, 0.0]);
        at.set_parm_f(2, 1.0);
        at.set_parm_f(3, 1.0);
        at.run_ok(ServerState::Active);
        assert!(at.engine().buffers.unreliable > 1008);

        let mut past = Fixture::new(48, 4, vec![0], 0);
        past.builtins
            .engine_mut()
            .append_message(MessageDest::Broadcast, 1009)
            .unwrap();
        past.set_parm_v(0, [0.0, 0.0, 0.0]);
        past.set_parm_v(1, [0.0, 0.0, 0.0]);
        past.set_parm_f(2, 1.0);
        past.set_parm_f(3, 1.0);
        past.run_ok(ServerState::Active);
        assert_eq!(past.engine().buffers.unreliable, 1009);
    }

    // spec BUILTIN-11.4: stuffcmd to the client slot exactly at maxclients succeeds (the upper
    // bound is inclusive); a slot one past it is the non-client error. Pins the `> maxclients`
    // upper bound.
    #[test]
    fn stuffcmd_upper_bound() {
        let (strings, offsets) = string_table(&["cmd\n"]);
        // maxclients 2, slot 2 is valid (exactly at the bound).
        let mut ok = Fixture::new(21, 2, strings.clone(), 2);
        ok.set_parm_i(0, ok.machine.edicts().edict_num(2).unwrap().bits());
        ok.set_parm_i(1, offsets[0]);
        ok.run_ok(ServerState::Active);
        // Slot 3 with maxclients 2 is one past the bound: the non-client error.
        let mut fail = Fixture::new(21, 2, strings, 2);
        fail.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2
        fail.machine.edicts_mut().alloc(1.0).unwrap(); // edict 3 (not a client slot)
        fail.set_parm_i(0, fail.machine.edicts().edict_num(3).unwrap().bits());
        fail.set_parm_i(1, offsets[0]);
        fail.run_err(ServerState::Active, "Parm 0 not a client");
    }

    // spec BUILTIN-11.2: sprint to the client slot exactly at maxclients reads its text (so a
    // bad string errors), proving the upper bound is inclusive; pins print_to_client's
    // `client > maxclients`.
    #[test]
    fn sprint_upper_bound_reads_text() {
        let mut f = Fixture::new(24, 2, vec![0], 2);
        f.set_parm_i(0, f.machine.edicts().edict_num(2).unwrap().bits()); // slot 2 == maxclients
        f.set_parm_i(1, 0x0100_0000); // bad string
        f.run_err(ServerState::Active, "invalid string");
    }

    // spec BUILTIN-13.1: a negative Write destination is a bad destination (the match's
    // default arm), pinning write_dest against a `to < 0` becoming `to > 0`.
    #[test]
    fn write_dest_negative_is_bad() {
        let mut f = Fixture::new(52, 2, vec![0], 0);
        f.set_parm_f(0, -1.0); // a negative destination
        f.set_parm_f(1, 1.0);
        f.run_err(ServerState::Active, "WriteDest: bad destination");
    }

    // spec BUILTIN-13.1: MSG_ONE whose msg_entity is the world (slot 0, below 1) is the
    // not-a-client error; pins write_dest's `number < 1` lower bound (the world is not a
    // client).
    #[test]
    fn write_one_to_world_is_not_a_client() {
        let mut f = Fixture::new(52, 2, vec![0], 2);
        f.set_global_entity("msg_entity", 0); // the world
        f.set_parm_f(0, 1.0); // MSG_ONE
        f.set_parm_f(1, 1.0);
        f.run_err(ServerState::Active, "WriteDest: not a client");
    }

    // spec DIFF-7.8: movetogoal steps when the yaw delta is exactly 45 (the window is strictly
    // greater than 45), so at angles_y 45, ideal 0 it still steps; pins the `> 45.0` bound.
    #[test]
    fn movetogoal_steps_at_exactly_45() {
        let mut f = Fixture::new(67, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 512.0); // FL_ONGROUND
        f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
        f.set_field_v(1, "angles", [0.0, 45.0, 0.0]); // delta = 45 - 0 = 45, not > 45: steps
        f.set_field_f(1, "ideal_yaw", 0.0);
        f.set_field_f(1, "yaw_speed", 0.0); // do not turn, keep angles_y 45
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 10.0);
        f.run_ok(ServerState::Active);
        // yaw 0 step 10 moves origin_x to 10.
        assert_eq!(f.field_v(1, "origin")[0], 10.0);
    }

    // spec BUILTIN-12.1: a sound attenuation of exactly 0 or 4 is in range and succeeds; a
    // value below 0 or above 4 is the fatal attenuation error. Pins the 0..=4 bounds.
    #[test]
    fn sound_attenuation_boundaries() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        for atten in [0.0f32, 4.0] {
            let mut f = Fixture::new(8, 5, strings.clone(), 0);
            f.builtins
                .engine_mut()
                .precache(Precache::Sound, "misc/x.wav")
                .unwrap();
            f.machine.edicts_mut().alloc(1.0).unwrap();
            let e1 = f.machine.edicts().edict_num(1).unwrap();
            f.set_parm_i(0, e1.bits());
            f.set_parm_f(1, 0.0);
            f.set_parm_i(2, offsets[0]);
            f.set_parm_f(3, 0.5);
            f.set_parm_f(4, atten);
            f.run_ok(ServerState::Active);
        }
        // Just below 0 is out of range.
        let mut low = Fixture::new(8, 5, strings, 0);
        low.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        low.set_parm_f(1, 0.0);
        low.set_parm_i(2, offsets[0]);
        low.set_parm_f(3, 0.5);
        low.set_parm_f(4, -0.5); // below 0
        low.run_err(
            ServerState::Active,
            "SV_StartSound: attenuation = -0.500000",
        );
    }

    // spec BUILTIN-12.1: a sound channel of exactly 0 or 7 is in range and succeeds; 8 is the
    // fatal channel error. Pins the 0..=7 bounds.
    #[test]
    fn sound_channel_boundaries() {
        let (strings, offsets) = string_table(&["misc/x.wav"]);
        for chan in [0.0f32, 7.0] {
            let mut f = Fixture::new(8, 5, strings.clone(), 0);
            f.builtins
                .engine_mut()
                .precache(Precache::Sound, "misc/x.wav")
                .unwrap();
            f.machine.edicts_mut().alloc(1.0).unwrap();
            let e1 = f.machine.edicts().edict_num(1).unwrap();
            f.set_parm_i(0, e1.bits());
            f.set_parm_f(1, chan);
            f.set_parm_i(2, offsets[0]);
            f.set_parm_f(3, 0.5);
            f.set_parm_f(4, 1.0);
            f.run_ok(ServerState::Active);
        }
        let mut over = Fixture::new(8, 5, strings, 0);
        over.builtins
            .engine_mut()
            .precache(Precache::Sound, "misc/x.wav")
            .unwrap();
        over.set_parm_f(1, 8.0); // channel 8 > 7
        over.set_parm_i(2, offsets[0]);
        over.set_parm_f(3, 0.5);
        over.set_parm_f(4, 1.0);
        over.run_err(ServerState::Active, "SV_StartSound: channel = 8");
    }

    // spec DIFF-7.5: droptofloor ORs FL_ONGROUND into the existing flags; an edict already on
    // the ground keeps the bit (OR, not XOR). Pins the `flags | FL_ONGROUND` against `^`.
    #[test]
    fn droptofloor_keeps_onground_when_set() {
        let mut f = Fixture::new(34, 0, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 512.0); // FL_ONGROUND already set
        f.set_field_v(1, "origin", [0.0, 0.0, 50.0]);
        f.set_global_entity("self", 1);
        f.run_ok(ServerState::Active);
        // 512 | 512 = 512; XOR would clear it to 0.
        assert_eq!(f.field_f(1, "flags"), 512.0);
    }

    // spec BUILTIN-8.5: findradius measures the signed delta org - centre; with a non-zero org
    // the sign matters, so an edict is included for the true delta but excluded if the
    // subtraction became an addition. Pins `o - centre` in radius_delta.
    #[test]
    fn findradius_delta_sign() {
        // org_x = 5, centre_x = origin 10 + (mins+maxs)/2 = 10 + 0 = 10; delta = 5 - 10 = -5,
        // distance 5. The addition 5 + 10 = 15 would be distance 15.
        let mut f = Fixture::new(22, 2, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "solid", 1.0);
        f.set_field_v(1, "origin", [10.0, 0.0, 0.0]);
        f.set_field_v(1, "mins", [0.0, 0.0, 0.0]);
        f.set_field_v(1, "maxs", [0.0, 0.0, 0.0]);
        f.set_parm_v(0, [5.0, 0.0, 0.0]); // org
        f.set_parm_f(1, 8.0); // radius 8: includes distance 5, excludes 15
        f.run_ok(ServerState::Active);
        let e1 = f.machine.edicts().edict_num(1).unwrap();
        assert_eq!(f.return_i(), e1.bits());
    }

    // spec BUILTIN-11.2: sprint to the world (slot 0, below 1) prints a console message and
    // returns without reading the text, so a bad string there is not an error; pins
    // print_to_client's `client < 1` lower bound (and its `||`).
    #[test]
    fn sprint_world_does_not_read_text() {
        let mut f = Fixture::new(24, 2, vec![0], 2);
        f.set_parm_i(0, f.machine.edicts().edict_num(0).unwrap().bits()); // the world
        f.set_parm_i(1, 0x0100_0000); // a bad string, which must not be read
        f.run_ok(ServerState::Active);
    }

    // spec BUILTIN-11.2: sprint to a client slot below maxclients reads its text, so a bad
    // string there is an error; pins print_to_client's `client > maxclients` upper bound
    // (slot 1 with maxclients 2 is valid and must read the text).
    #[test]
    fn sprint_slot_below_max_reads_text() {
        let mut f = Fixture::new(24, 2, vec![0], 2);
        f.set_parm_i(0, f.machine.edicts().edict_num(1).unwrap().bits()); // slot 1 < maxclients 2
        f.set_parm_i(1, 0x0100_0000); // bad string
        f.run_err(ServerState::Active, "invalid string");
    }

    // spec BUILTIN-8.7: setspawnparms of a slot exactly at maxclients succeeds; pins the upper
    // bound `number > maxclients` (slot maxclients is valid).
    #[test]
    fn setspawnparms_upper_bound() {
        let mut f = Fixture::new(78, 1, vec![0], 3);
        f.set_parm_i(0, f.machine.edicts().edict_num(3).unwrap().bits()); // slot 3 == maxclients
        f.run_ok(ServerState::Active);
    }

    // spec DIFF-7.8: movetogoal steps only when `angles_y - ideal_yaw` is not strictly between
    // 45 and 315, so at the window's upper edge a delta of exactly 315 steps and 314, just
    // inside, holds.
    #[test]
    fn movetogoal_steps_at_delta_315_and_holds_at_314() {
        let origin_x_after = |angles_y: f32| {
            let mut f = Fixture::new(67, 1, vec![0], 0);
            f.machine.edicts_mut().alloc(1.0).unwrap();
            f.set_field_f(1, "flags", 512.0); // FL_ONGROUND
            f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
            f.set_field_v(1, "angles", [0.0, angles_y, 0.0]); // delta = angles_y - 0
            f.set_field_f(1, "ideal_yaw", 0.0);
            f.set_field_f(1, "yaw_speed", 0.0); // keep angles_y
            f.set_global_entity("self", 1);
            f.set_parm_f(0, 10.0);
            f.run_ok(ServerState::Active);
            f.field_v(1, "origin")[0]
        };
        // 315 is not strictly less than 315: outside the window, so a step of 10 along yaw 0.
        assert_eq!(origin_x_after(315.0), 10.0);
        // 314 is inside the window: no step.
        assert_eq!(origin_x_after(314.0), 0.0);
    }

    // spec DIFF-7.8: movetogoal with angles_y 100 and ideal_yaw 10 has delta 90 (in the 45..315
    // window, no step); the subtraction becoming an addition would give delta 110 (also in the
    // window), so distinguish with angles_y 10, ideal_yaw 100: delta -90 (not in window,
    // steps), addition 110 (in window, no step). Pins `angles_y - yaw` in b_movetogoal.
    #[test]
    fn movetogoal_delta_is_a_difference() {
        let mut f = Fixture::new(67, 1, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        f.set_field_f(1, "flags", 512.0); // FL_ONGROUND
        f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
        f.set_field_v(1, "angles", [0.0, 10.0, 0.0]);
        f.set_field_f(1, "ideal_yaw", 100.0); // delta = 10 - 100 = -90: not in (45,315), steps
        f.set_field_f(1, "yaw_speed", 0.0); // no turn
        f.set_global_entity("self", 1);
        f.set_parm_f(0, 10.0);
        f.run_ok(ServerState::Active);
        // ideal_yaw 100 step 10: moves along yaw 100. With the addition (delta 110) it would be
        // in the window and not step, leaving origin at 0; the difference steps, so origin
        // moves off 0 on some axis.
        let origin = f.field_v(1, "origin");
        assert!(
            origin[0] != 0.0 || origin[1] != 0.0,
            "a step along yaw 100 moves the origin"
        );
    }

    // spec DIFF-7.3: pick_client does not choose a client whose health is 0 or less; a single
    // client with health exactly 0 leaves checkclient returning the world. Pins pick_client's
    // `health > 0.0` against `>=`.
    #[test]
    fn checkclient_skips_zero_health() {
        let mut f = Fixture::new(17, 0, vec![0], 1);
        f.machine.edicts_mut().alloc(1.0).unwrap();
        in_use(&mut f, 1);
        f.set_field_f(1, "health", 0.0); // exactly 0: does not qualify
        f.set_field_f(1, "flags", 0.0);
        f.run_ok(ServerState::Active);
        let world = f.machine.edicts().edict_num(0).unwrap();
        assert_eq!(f.return_i(), world.bits());
    }

    // spec BUILTIN-2.9: ChangeYaw reads `self` with the G_EDICT check and turns that edict, not
    // a fixed one; with self set to edict 2, edict 2's angles change and edict 1's do not. Pins
    // checked_self against a constant edict.
    #[test]
    fn changeyaw_uses_self_edict() {
        let mut f = Fixture::new(49, 0, vec![0], 0);
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 1
        f.machine.edicts_mut().alloc(1.0).unwrap(); // edict 2
        f.set_field_v(1, "angles", [0.0, 0.0, 0.0]);
        f.set_field_f(1, "ideal_yaw", 90.0);
        f.set_field_f(1, "yaw_speed", 20.0);
        f.set_field_v(2, "angles", [0.0, 0.0, 0.0]);
        f.set_field_f(2, "ideal_yaw", 90.0);
        f.set_field_f(2, "yaw_speed", 20.0);
        f.set_global_entity("self", 2); // turn edict 2
        f.run_ok(ServerState::Active);
        // Edict 2 turned; edict 1 (the constant a mutant would use) did not.
        assert_eq!(f.field_v(2, "angles")[1], 19.995_117);
        assert_eq!(f.field_v(1, "angles")[1], 0.0);
    }

    // spec BUILTIN-8.8: find examines only edicts below num_edicts; with no edict allocated
    // (num_edicts 1) a find whose target is the empty string must not match the in-use,
    // zero-filled edict at index num_edicts. Pins the `number < num_edicts` loop bound against
    // `<=` (which would examine edict 1 and match its empty classname).
    #[test]
    fn find_does_not_examine_edict_at_num_edicts() {
        let mut f = Fixture::new(18, 3, vec![0], 0); // num_edicts starts at 1 (maxclients 0)
        let classname = field_by_name("classname").unwrap().slot;
        let world = f.machine.edicts().edict_num(0).unwrap();
        f.set_parm_i(0, world.bits()); // start at the world; loop begins at edict 1
        f.set_parm_i(1, i32::try_from(classname).unwrap());
        f.set_parm_i(2, 0); // the empty string, which edict 1's zeroed classname would equal
        f.run_ok(ServerState::Active);
        // Under `<`, edict 1 (== num_edicts) is not examined, so no match: the world.
        assert_eq!(f.return_i(), 0);
    }

    // spec BUILTIN-8.5: findradius links only edicts below num_edicts; with no edict allocated
    // (num_edicts 1) it never examines the in-use, zero-filled edict at index num_edicts, so
    // the chain stays the world. Pins the `number < num_edicts` loop bound against `<=`.
    #[test]
    fn findradius_does_not_examine_edict_at_num_edicts() {
        let mut f = Fixture::new(22, 2, vec![0], 0); // num_edicts 1
        // Make edict 1 (the one at num_edicts) solid and at the origin, so a `<=` bound would
        // include it; a `<` bound never reaches it.
        f.set_field_f(1, "solid", 1.0);
        f.set_field_v(1, "origin", [0.0, 0.0, 0.0]);
        f.set_parm_v(0, [0.0, 0.0, 0.0]);
        f.set_parm_f(1, 100.0);
        f.run_ok(ServerState::Active);
        assert_eq!(f.return_i(), 0);
    }

    // spec BUILTIN-7.4: ftos and then vtos over one engine write the one buffer: vtos returns
    // the value ftos returned, the earlier result's text becomes vtos's, and the change is
    // reported once with vtos's text (DIFF-3.9)
    #[test]
    fn reviewer_ftos_then_vtos_share_one_buffer() {
        let mut f = Fixture::new(26, 1, vec![0], 0);
        f.set_parm_f(0, 5.0);
        f.run_ok(ServerState::Active);
        let value = f.return_i();
        assert_eq!(
            f.builtins.engine_mut().strings.take_temp_change(),
            Some((value, b"5".to_vec()))
        );

        let mut g = Fixture::new(27, 1, vec![0], 0);
        g.builtins = f.builtins;
        g.set_parm_v(0, [1.0, 2.0, 3.0]);
        g.run_ok(ServerState::Active);
        assert_eq!(g.return_i(), value);
        let text = b"'  1.0   2.0   3.0'".to_vec();
        assert_eq!(g.engine().strings.text(value), Some(text.as_slice()));
        assert_eq!(
            g.builtins.engine_mut().strings.take_temp_change(),
            Some((value, text))
        );
        assert_eq!(g.builtins.engine_mut().strings.take_temp_change(), None);
    }

    // spec DIFF-3.9: through dispatch, ftos of a value whose text the buffer already holds
    // reports no change, and a different text reports one
    #[test]
    fn reviewer_repeated_ftos_reports_a_change_only_for_new_text() {
        let mut f = Fixture::new(26, 1, vec![0], 0);
        let mut changes = Vec::new();
        for input in [5.0, 5.0, 6.0, 6.0, 5.0] {
            f.set_parm_f(0, input);
            f.run_ok(ServerState::Active);
            let change = f.builtins.engine_mut().strings.take_temp_change();
            changes.push(change.map(|(_, text)| text));
        }
        assert_eq!(
            changes,
            [
                Some(b"5".to_vec()),
                None,
                Some(b"6".to_vec()),
                None,
                Some(b"5".to_vec())
            ]
        );
    }
}
