// SPDX-License-Identifier: GPL-3.0-or-later

//! Byte offsets and protocol positions.

use crate::lines::{Encoding, LineIndex, Position};

const fn at(line: u32, character: u32) -> Position {
    Position { line, character }
}

// LSP 3.18, "Text Documents": "the protocol specifies the following end-of-line sequences: '\n',
// '\r\n' and '\r'."
#[test]
fn every_line_break() {
    let text = b"a\nb\r\nc\rd";
    let lines = LineIndex::new(text, Encoding::Utf16);
    assert_eq!(lines.position(0), at(0, 0));
    assert_eq!(lines.position(2), at(1, 0));
    assert_eq!(lines.position(5), at(2, 0));
    assert_eq!(lines.position(7), at(3, 0));
    assert_eq!(lines.offset(at(3, 0)), 7);
    // "Positions are line end character agnostic": the end of a line is before its break.
    assert_eq!(lines.position(4), at(1, 1));
    assert_eq!(lines.offset(at(1, 5)), 3);
}

// LSP 3.18, "Text Documents": "in a string of the form `a𐐀b` the character offset of the
// character `a` is 0, the character offset of `𐐀` is 1 and the character offset of b is 3 since
// `𐐀` is represented using two code units in UTF-16."
#[test]
fn utf16_and_utf8_columns() {
    let text = "a\u{10400}b".as_bytes();
    let utf16 = LineIndex::new(text, Encoding::Utf16);
    assert_eq!(utf16.position(1), at(0, 1));
    assert_eq!(utf16.position(5), at(0, 3));
    assert_eq!(utf16.offset(at(0, 3)), 5);
    // A position inside the surrogate pair is the character's start.
    assert_eq!(utf16.offset(at(0, 2)), 1);
    let utf8 = LineIndex::new(text, Encoding::Utf8);
    assert_eq!(utf8.position(5), at(0, 5));
    assert_eq!(utf8.offset(at(0, 5)), 5);
    assert_eq!(Encoding::Utf8.kind(), "utf-8");
    assert_eq!(Encoding::default().kind(), "utf-16");
}

// LSP 3.18, `Position.character`: "If the character value is greater than the line length it
// defaults back to the line length."
#[test]
fn positions_past_the_end() {
    let lines = LineIndex::new(b"ab\ncd", Encoding::Utf16);
    assert_eq!(lines.offset(at(0, 9)), 2);
    assert_eq!(lines.offset(at(5, 0)), 5);
    assert_eq!(lines.offset(at(u32::MAX, u32::MAX)), 5);
    assert_eq!(lines.position(99), at(1, 2));
    let empty = LineIndex::new(b"", Encoding::Utf16);
    assert_eq!(empty.position(0), at(0, 0));
    assert_eq!(empty.offset(at(0, 3)), 0);
}

// Quake's character set is not UTF-8: each sequence that is not, as `String::from_utf8_lossy`
// splits them, counts as one replacement character, as a client decoding the file as UTF-8
// shows it.
#[test]
fn bytes_that_are_not_utf8() {
    let text = b"\x80\x81x\xffy";
    let lines = LineIndex::new(text, Encoding::Utf16);
    assert_eq!(
        String::from_utf8_lossy(text).encode_utf16().count(),
        5,
        "two, then x, one, then y"
    );
    assert_eq!(lines.position(1), at(0, 1));
    assert_eq!(lines.position(2), at(0, 2));
    assert_eq!(lines.position(3), at(0, 3));
    assert_eq!(lines.position(4), at(0, 4));
    assert_eq!(lines.offset(at(0, 1)), 1);
    assert_eq!(lines.offset(at(0, 2)), 2);
    assert_eq!(lines.offset(at(0, 4)), 4);
    assert_eq!(lines.offset(at(0, 0)), 0);
    let range = lines.range(2, 4);
    assert_eq!((range.start, range.end), (at(0, 2), at(0, 4)));
}

// LSP 3.18, `PositionEncodingKind.UTF8`: "Character offsets count UTF-8 code units (i.e. bytes)"
// of the text the client has, in which a sequence that is not UTF-8 is a three-byte U+FFFD.
#[test]
fn bytes_that_are_not_utf8_in_utf8_positions() {
    let text = b"\x8dx";
    let lines = LineIndex::new(text, Encoding::Utf8);
    assert_eq!(lines.position(1), at(0, 3));
    assert_eq!(lines.position(2), at(0, 4));
    assert_eq!(lines.offset(at(0, 3)), 1);
    assert_eq!(lines.offset(at(0, 2)), 0);
    assert_eq!(lines.offset(at(0, 4)), 2);
}
