// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_cmdlib.c, QCC_LoadFile, QCC_SanitizeCharSet, decodeUTF
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, QCC_main

//! The text FTEQCC's file loader hands its lexer (`docs/spec/dialect-fteqcc.md` FTEF-14.54):
//! `QCC_LoadFile` reads the file's bytes, `QCC_SanitizeCharSet` decodes a file that starts with a
//! UTF-32 or UTF-16 byte order mark to UTF-8 and skips a UTF-8 one, and every NUL byte left is a
//! space. In a module's source `QCC_main` then passes over a first line that starts with `#!`
//! (`docs/spec/project-file.md` PRJ-4.8).

use std::borrow::Cow;

use crate::source::TextOrigin;

/// The UTF-8 byte order mark, which `QCC_SanitizeCharSet` skips (`mem += 3;`).
const UTF8_BOM: &[u8] = b"\xef\xbb\xbf";

/// The replacement character `decodeUTF` writes for a code point past U+10FFFF.
const REPLACEMENT: u32 = 0xfffd;

/// The wide encodings `QCC_SanitizeCharSet` decodes, each named by the byte order mark a file
/// starts with, tried in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wide {
    /// `ff fe 00 00`.
    Utf32Le,
    /// `00 00 fe ff`.
    Utf32Be,
    /// `ff fe`.
    Utf16Le,
    /// `fe ff`.
    Utf16Be,
}

impl Wide {
    /// Every encoding, in the order `QCC_SanitizeCharSet` tests their marks, so that a UTF-32LE
    /// mark, which starts with UTF-16LE's, is UTF-32LE.
    const ALL: [Self; 4] = [Self::Utf32Le, Self::Utf32Be, Self::Utf16Le, Self::Utf16Be];

    /// The byte order mark.
    const fn mark(self) -> &'static [u8] {
        match self {
            Self::Utf32Le => b"\xff\xfe\x00\x00",
            Self::Utf32Be => b"\x00\x00\xfe\xff",
            Self::Utf16Le => b"\xff\xfe",
            Self::Utf16Be => b"\xfe\xff",
        }
    }

    /// The encoding whose mark `bytes` starts with.
    fn of(bytes: &[u8]) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|wide| bytes.starts_with(wide.mark()))
    }

    /// The bytes of one code unit: `w` in `decodeUTF`.
    const fn width(self) -> usize {
        match self {
            Self::Utf32Le | Self::Utf32Be => 4,
            Self::Utf16Le | Self::Utf16Be => 2,
        }
    }

    /// The code unit `unit`, of [`Self::width`] bytes.
    fn value(self, unit: &[u8]) -> u32 {
        let mut value = 0_u32;
        let little = matches!(self, Self::Utf32Le | Self::Utf16Le);
        let mut push = |byte: &u8| value = (value << 8) | u32::from(*byte);
        if little {
            unit.iter().rev().for_each(&mut push);
        } else {
            unit.iter().for_each(&mut push);
        }
        value
    }
}

/// The low byte of `value`, as C's store of an `unsigned int` into a `char` keeps it.
fn low_byte(value: u32) -> u8 {
    let [low, ..] = value.to_le_bytes();
    low
}

/// `decodeUTF` over `bytes`, the code units after the mark of `wide`, whose first byte is at
/// `from` in the file: the UTF-8 it writes, and the file offset each of its bytes came from,
/// with the offset after the last unit read at the end.
///
/// A trailing part of a unit is not read (`chars = inbytes / w;`). A high surrogate before a low
/// one is one code point, any other surrogate is encoded as it is, in three bytes, and a UTF-32
/// value past U+10FFFF is U+FFFD (`if (inc > 0x10FFFF) inc = 0xFFFD;`).
fn decode_utf(wide: Wide, bytes: &[u8], from: usize) -> (Vec<u8>, Vec<usize>) {
    let width = wide.width();
    let units: Vec<u32> = bytes.chunks_exact(width).map(|u| wide.value(u)).collect();
    let mut text = Vec::with_capacity(bytes.len());
    let mut offsets = Vec::with_capacity(bytes.len().saturating_add(1));
    let mut index = 0;
    while let Some(&unit) = units.get(index) {
        let at = from.saturating_add(index.saturating_mul(width));
        let mut code = unit;
        if wide == Wide::Utf16Le || wide == Wide::Utf16Be {
            let low = units.get(index.saturating_add(1)).copied();
            if let Some(low) = low.filter(|l| (0xdc00..0xe000).contains(l))
                && (0xd800..0xdc00).contains(&unit)
            {
                code = (((unit & 0x3ff) << 10) | (low & 0x3ff)).saturating_add(0x1_0000);
                index = index.saturating_add(1);
            }
        }
        index = index.saturating_add(1);
        if code > 0x10_ffff {
            code = REPLACEMENT;
        }
        let written = encode(code, &mut text);
        offsets.extend(std::iter::repeat_n(at, written));
    }
    offsets.push(from.saturating_add(index.saturating_mul(width)));
    (text, offsets)
}

/// `decodeUTF`'s encoding of the code point `code`, at most U+10FFFF, in UTF-8's one to four
/// bytes, a surrogate's in three as any other value of U+0800 to U+FFFF, written to `text`;
/// how many bytes it wrote.
fn encode(code: u32, text: &mut Vec<u8>) -> usize {
    let tail = |shift: u32| low_byte(((code >> shift) & 0x3f) | 0x80);
    let before = text.len();
    if code <= 0x7f {
        text.push(low_byte(code));
    } else if code <= 0x7ff {
        text.extend_from_slice(&[low_byte(((code >> 6) & 0x1f) | 0xc0), tail(0)]);
    } else if code <= 0xffff {
        text.extend_from_slice(&[low_byte(((code >> 12) & 0xf) | 0xe0), tail(6), tail(0)]);
    } else {
        let lead = low_byte(((code >> 18) & 0x07) | 0xf0);
        text.extend_from_slice(&[lead, tail(12), tail(6), tail(0)]);
    }
    text.len().saturating_sub(before)
}

/// The text `QCC_LoadFile` hands FTEQCC's lexer for a file of `bytes`, and where it stands in
/// them. Implements FTEF-14.54.
///
/// `QCC_SanitizeCharSet` decodes a file that starts with the byte order mark of UTF-32LE,
/// UTF-32BE, UTF-16LE or UTF-16BE, tested in that order, to UTF-8 (`decodeUTF`), skips a UTF-8
/// one, and leaves any other file as it is; then `QCC_LoadFile` makes every NUL byte of the text
/// a space (`else if (!mem[check])`, `mem[check] = ' ';`). It also writes a line break after the
/// text (`mem[len] = '\n';`), which this function does not add: the readers take the end of the
/// text for the end of its last line (FTEF-14.54).
pub fn qcc_load_file(bytes: Vec<u8>) -> (Vec<u8>, TextOrigin) {
    let (mut text, origin) = if let Some(wide) = Wide::of(&bytes) {
        let from = wide.mark().len();
        let (text, offsets) = decode_utf(wide, bytes.get(from..).unwrap_or_default(), from);
        (text, TextOrigin::Decoded(offsets))
    } else if bytes.starts_with(UTF8_BOM) {
        let mut text = bytes;
        text.drain(..UTF8_BOM.len());
        (text, TextOrigin::Shifted(UTF8_BOM.len()))
    } else {
        (bytes, TextOrigin::Shifted(0))
    };
    for byte in &mut text {
        if *byte == 0 {
            *byte = b' ';
        }
    }
    (text, origin)
}

/// [`qcc_load_file`]'s text for a file of `bytes`, kept at the file's own offsets for a reader
/// of the file as written, the syntax tree, and where that text starts. Implements FTEF-14.56.
///
/// The bytes of a UTF-8 byte order mark `QCC_SanitizeCharSet` skips stay in front, so the text
/// starts after them, and each NUL byte is a space; a file with no NUL is borrowed as it is.
/// `None` for a file the loader decodes from UTF-16 or UTF-32, whose text is at other offsets
/// than its bytes.
#[must_use]
pub fn qcc_load_in_place(bytes: &[u8]) -> Option<(Cow<'_, [u8]>, usize)> {
    if Wide::of(bytes).is_some() {
        return None;
    }
    let start = if bytes.starts_with(UTF8_BOM) {
        UTF8_BOM.len()
    } else {
        0
    };
    if !bytes.contains(&0) {
        return Some((Cow::Borrowed(bytes), start));
    }
    let text = bytes
        .iter()
        .map(|&byte| if byte == 0 { b' ' } else { byte })
        .collect();
    Some((Cow::Owned(text), start))
}

/// `QCC_main`'s pass over the first line of a module's source, `text` as [`qcc_load_file`] gives
/// it, when that line starts with `#!`: "ignore the first line if it starts with a #! for unix
/// scripts" (`if (qccmsrc[0] == '#' && qccmsrc[1] == '!') qccmsrc = strchr(qccmsrc, '\n');`).
/// Implements PRJ-4.8.
///
/// The line's bytes before its line feed become spaces, so the text keeps its offsets and its
/// lines: FTEQCC reads on from the line feed, which ends the line the next one is counted from.
/// A text with no line feed is all that line, as `QCC_LoadFile` writes one after the text
/// (`mem[len] = '\n';`), and becomes all spaces.
pub fn skip_script_line(text: &mut [u8]) {
    if !text.starts_with(b"#!") {
        return;
    }
    for byte in text.iter_mut().take_while(|byte| **byte != b'\n') {
        *byte = b' ';
    }
}
