// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::{Frame, base64, encode};

// RFC 4648, section 10, "Test Vectors"
#[test]
fn base64_matches_the_rfc_vectors() {
    let vectors = [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ];
    for (input, output) in vectors {
        assert_eq!(base64(input.as_bytes()), output, "{input:?}");
    }
    assert_eq!(base64(&[0xFF, 0xEF, 0xBE]), "/+++");
}

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
    let (png, width, height) = encode(frame(4, 2), None).unwrap();
    assert_eq!((width, height), (4, 2));
    assert!(png.starts_with("iVBORw0KGgo"), "the PNG signature: {png}");
    let (_, width, height) = encode(frame(4, 2), Some((8, 6))).unwrap();
    assert_eq!((width, height), (8, 6));
    let short = Frame {
        width: 4,
        height: 2,
        rgba: vec![0; 3],
    };
    assert!(encode(short, None).is_err());
}
