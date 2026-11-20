// SPDX-License-Identifier: GPL-3.0-or-later

use super::{LightStyles, MAX_LIGHTSTYLES, StyleLerp};

/// Style `style`'s string is `string`; the others have none.
fn one(style: usize, string: &'static [u8]) -> impl Fn(usize) -> Option<&'static [u8]> {
    move |s| (s == style).then_some(string)
}

// spec REND-5.1: there are 64 styles; a style number past them reads as 0, which no face
// lists, since 255 ends a face's styles
#[test]
fn there_are_64_styles() {
    assert_eq!(MAX_LIGHTSTYLES, 64);
    let styles = LightStyles::at(|_| Some(b"z".as_slice()), 0.0, StyleLerp::Off);
    assert_eq!(styles.value(63), 550);
    assert_eq!(styles.value(64), 0);
    assert_eq!(styles.value(255), 0);
}

// spec REND-5.2: a style with no string, or an empty one, is 256
#[test]
fn a_style_with_no_string_is_256() {
    assert_eq!(LightStyles::unset().value(0), 256);
    assert_eq!(LightStyles::default(), LightStyles::unset());
    assert_eq!(LightStyles::at(|_| None, 3.0, StyleLerp::Off).value(7), 256);
    assert_eq!(
        LightStyles::at(one(7, b""), 3.0, StyleLerp::Off).value(7),
        256
    );
}

// spec REND-5.3: a style is (letter - 'a') * 22 for the letter of frame (int)(time*10) mod
// its length: a is 0, m 264, z 550; a letter before a is negative; a negative time reads the
// first letter
#[test]
fn a_style_is_its_letter_at_ten_a_second() {
    let flicker = one(1, b"amz");
    assert_eq!(LightStyles::at(&flicker, 0.0, StyleLerp::Off).value(1), 0);
    assert_eq!(
        LightStyles::at(&flicker, 0.15, StyleLerp::Off).value(1),
        264,
        "frame 1"
    );
    assert_eq!(
        LightStyles::at(&flicker, 0.29, StyleLerp::Off).value(1),
        550,
        "frame 2, truncated"
    );
    assert_eq!(
        LightStyles::at(&flicker, 0.31, StyleLerp::Off).value(1),
        0,
        "frame 3 wraps to 0"
    );
    assert_eq!(
        LightStyles::at(&flicker, 12.0, StyleLerp::Off).value(1),
        0,
        "frame 120, a multiple of 3"
    );
    assert_eq!(
        LightStyles::at(&flicker, 0.0, StyleLerp::Off).value(0),
        256,
        "another style"
    );
    assert_eq!(
        LightStyles::at(one(2, b"0"), 0.0, StyleLerp::Off).value(2),
        -1078,
        "'0' - 'a' is -49"
    );
    assert_eq!(
        LightStyles::at(&flicker, -0.15, StyleLerp::Off).value(1),
        0,
        "a negative time"
    );
    assert_eq!(
        LightStyles::at(one(1, b"zm"), -0.15, StyleLerp::Off).value(1),
        550,
        "the first letter"
    );
}

// spec REND-5.4: R_BuildLightMap multiplies by the value as an unsigned
#[test]
fn the_scale_is_the_value_as_unsigned() {
    let styles = LightStyles::at(one(2, b"0"), 0.0, StyleLerp::Off);
    assert_eq!(styles.scale(2), (-1078_i32).cast_unsigned());
    assert_eq!(styles.scale(0), 256);
}

// spec REND-5.8: Ironwail's r_lerplightstyles 1, the default, blends toward the next letter by
// the fraction of the tenth of a second gone, wrapping to the first letter, truncated (times
// whose tenths are exact in a float)
#[test]
fn a_style_blends_toward_its_next_letter() {
    let ramp = one(1, b"mn");
    assert_eq!(
        LightStyles::at(&ramp, 0.0, StyleLerp::default()).value(1),
        264
    );
    // m is 12, n 13: 12*22 + 1*22*0.5 = 275.
    assert_eq!(
        LightStyles::at(&ramp, 0.25, StyleLerp::SkipAbrupt).value(1),
        275
    );
    // Frame 1 is n, blending to m again: 286 - 22*0.25 = 280.5, truncated.
    assert_eq!(
        LightStyles::at(&ramp, 0.125, StyleLerp::SkipAbrupt).value(1),
        280
    );
    // A style with no string stays 256.
    assert_eq!(
        LightStyles::at(&ramp, 0.25, StyleLerp::SkipAbrupt).value(0),
        256
    );
    // id's: the letter held for its tenth.
    assert_eq!(LightStyles::at(&ramp, 0.25, StyleLerp::Off).value(1), 264);
    assert_eq!(LightStyles::at(&ramp, 0.125, StyleLerp::Off).value(1), 286);
}

// spec REND-5.8: r_lerplightstyles 1 holds a change of ('m' - 'a') / 2, 6 letters, or more,
// as id does; 2 blends it too
#[test]
fn an_abrupt_change_is_held_unless_always() {
    // a to g is 6: held; a to f is 5: blended.
    let six = one(1, b"ag");
    let five = one(1, b"af");
    assert_eq!(
        LightStyles::at(&six, 0.25, StyleLerp::SkipAbrupt).value(1),
        0
    );
    assert_eq!(
        LightStyles::at(&five, 0.25, StyleLerp::SkipAbrupt).value(1),
        55
    );
    assert_eq!(LightStyles::at(&six, 0.25, StyleLerp::Always).value(1), 66);
    // Downward too: g to a.
    let down = one(1, b"ga");
    assert_eq!(
        LightStyles::at(&down, 0.25, StyleLerp::SkipAbrupt).value(1),
        132
    );
    assert_eq!(LightStyles::at(&down, 0.25, StyleLerp::Always).value(1), 66);
}

// spec REND-5.8: the frame is floor(time*10); a negative time reads the first letter, as
// REND-5.3's id path does
#[test]
fn a_blended_style_at_a_negative_time() {
    let ramp = one(1, b"mn");
    // floor(-2.5) is -3: k = -3 % 2 = -1 reads the first letter, m; n = 0 is m too.
    assert_eq!(
        LightStyles::at(&ramp, -0.25, StyleLerp::Always).value(1),
        264
    );
    let one_letter = one(1, b"z");
    assert_eq!(
        LightStyles::at(&one_letter, 7.33, StyleLerp::Always).value(1),
        550
    );
}
