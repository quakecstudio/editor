// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::Frame;

fn frame(width: u32, height: u32) -> Frame {
    let pixels = usize::try_from(width * height).unwrap();
    Frame {
        width,
        height,
        rgba: [10, 20, 30, 255].repeat(pixels),
    }
}

// spec BRIDGE-4.9: a screenshot is a PNG in base64, at the size drawn or the size asked
#[test]
fn a_frame_encodes_as_a_png_at_the_size_asked() {
    let (png, width, height) = frame(4, 2).encode(None).unwrap();
    assert_eq!((width, height), (4, 2));
    assert!(png.starts_with("iVBORw0KGgo"), "the PNG signature: {png}");
    let (_, width, height) = frame(4, 2).encode(Some((8, 6))).unwrap();
    assert_eq!((width, height), (8, 6));
    let short = Frame {
        width: 4,
        height: 2,
        rgba: vec![0; 3],
    };
    assert!(short.encode(None).is_err());
}
