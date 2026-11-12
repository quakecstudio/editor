// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::collections::BTreeSet;

use super::{Field, Scale, color, color_text};

// No spec statement: P5-13, MAP-27: the field follows the key and its type: angle, a
// colour, a float the map gives several values, else text
#[test]
fn each_key_gets_its_field() {
    let values = BTreeSet::from(["1".to_owned(), "3".to_owned()]);
    let one = BTreeSet::from(["1".to_owned()]);
    assert_eq!(Field::of("angle", Some("float"), None), Field::Angle);
    assert_eq!(Field::of("_color", None, None), Field::Color);
    assert_eq!(Field::of("Color", Some("vector"), None), Field::Color);
    assert_eq!(
        Field::of("sounds", Some("float"), Some(&values)),
        Field::Choice(&values)
    );
    assert_eq!(Field::of("sounds", Some("float"), Some(&one)), Field::Text);
    assert_eq!(
        Field::of("message", Some("string"), Some(&values)),
        Field::Text
    );
    assert_eq!(
        Field::of("spawnflags", Some("float"), Some(&values)),
        Field::Text
    );
    assert_eq!(Field::of("speed", None, Some(&values)), Field::Text);
}

// No spec statement: P5-13: a colour keeps the scale it was written in, 0 to 255 or 0 to 1
#[test]
fn a_colour_keeps_its_scale() {
    let (rgb, scale) = color("255 128 0");
    assert_eq!(scale, Scale::Byte);
    assert_eq!(color_text(rgb, scale), "255 128 0");
    let (rgb, scale) = color("1 0.5 0.25");
    assert_eq!(scale, Scale::Unit);
    assert_eq!(color_text(rgb, scale), "1 0.5 0.25");
    assert_eq!(color("not a colour"), ([0.0; 3], Scale::Unit));
    assert_eq!(color("1 2"), ([0.0; 3], Scale::Unit));
    let (rgb, _) = color("2 0 0");
    assert!((rgb.first().unwrap() - 2.0 / 255.0).abs() < f32::EPSILON);
}
