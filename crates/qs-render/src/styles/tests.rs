// SPDX-License-Identifier: GPL-3.0-or-later

use super::{LightStyles, MAX_LIGHTSTYLES};

/// Style `style`'s string is `string`; the others have none.
fn one(style: usize, string: &'static [u8]) -> impl Fn(usize) -> Option<&'static [u8]> {
    move |s| (s == style).then_some(string)
}

// spec REND-5.1: there are 64 styles; a style number past them reads as 0, which no face
// lists, since 255 ends a face's styles
#[test]
fn there_are_64_styles() {
    assert_eq!(MAX_LIGHTSTYLES, 64);
    let styles = LightStyles::at(|_| Some(b"z".as_slice()), 0.0);
    assert_eq!(styles.value(63), 550);
    assert_eq!(styles.value(64), 0);
    assert_eq!(styles.value(255), 0);
}

// spec REND-5.2: a style with no string, or an empty one, is 256
#[test]
fn a_style_with_no_string_is_256() {
    assert_eq!(LightStyles::unset().value(0), 256);
    assert_eq!(LightStyles::default(), LightStyles::unset());
    assert_eq!(LightStyles::at(|_| None, 3.0).value(7), 256);
    assert_eq!(LightStyles::at(one(7, b""), 3.0).value(7), 256);
}

// spec REND-5.3: a style is (letter - 'a') * 22 for the letter of frame (int)(time*10) mod
// its length: a is 0, m 264, z 550; a letter before a is negative; a negative time reads the
// first letter
#[test]
fn a_style_is_its_letter_at_ten_a_second() {
    let flicker = one(1, b"amz");
    assert_eq!(LightStyles::at(&flicker, 0.0).value(1), 0);
    assert_eq!(LightStyles::at(&flicker, 0.15).value(1), 264, "frame 1");
    assert_eq!(
        LightStyles::at(&flicker, 0.29).value(1),
        550,
        "frame 2, truncated"
    );
    assert_eq!(
        LightStyles::at(&flicker, 0.31).value(1),
        0,
        "frame 3 wraps to 0"
    );
    assert_eq!(
        LightStyles::at(&flicker, 12.0).value(1),
        0,
        "frame 120, a multiple of 3"
    );
    assert_eq!(
        LightStyles::at(&flicker, 0.0).value(0),
        256,
        "another style"
    );
    assert_eq!(
        LightStyles::at(one(2, b"0"), 0.0).value(2),
        -1078,
        "'0' - 'a' is -49"
    );
    assert_eq!(
        LightStyles::at(&flicker, -0.15).value(1),
        0,
        "a negative time"
    );
    assert_eq!(
        LightStyles::at(one(1, b"zm"), -0.15).value(1),
        550,
        "the first letter"
    );
}

// spec REND-5.4: R_BuildLightMap multiplies by the value as an unsigned
#[test]
fn the_scale_is_the_value_as_unsigned() {
    let styles = LightStyles::at(one(2, b"0"), 0.0);
    assert_eq!(styles.scale(2), (-1078_i32).cast_unsigned());
    assert_eq!(styles.scale(0), 256);
}
