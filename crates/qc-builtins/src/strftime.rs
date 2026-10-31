// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! C's `strftime` of the mock's one instant, 1970-01-01 00:00:00 UTC, a Thursday, for vkQuake's
//! `strftime` builtin (BUILTIN-18.7).

/// The text each conversion gives at the mock's instant, as C's `strftime` writes it.
const CONVERSIONS: [(u8, &str); 39] = [
    (b'a', "Thu"),
    (b'A', "Thursday"),
    (b'b', "Jan"),
    (b'B', "January"),
    (b'c', "Thu Jan  1 00:00:00 1970"),
    (b'C', "19"),
    (b'd', "01"),
    (b'D', "01/01/70"),
    (b'e', " 1"),
    (b'F', "1970-01-01"),
    (b'g', "70"),
    (b'G', "1970"),
    (b'h', "Jan"),
    (b'H', "00"),
    (b'I', "12"),
    (b'j', "001"),
    (b'k', " 0"),
    (b'l', "12"),
    (b'm', "01"),
    (b'M', "00"),
    (b'n', "\n"),
    (b'p', "AM"),
    (b'r', "12:00:00 AM"),
    (b'R', "00:00"),
    (b's', "0"),
    (b'S', "00"),
    (b't', "\t"),
    (b'T', "00:00:00"),
    (b'u', "4"),
    (b'U', "00"),
    (b'V', "01"),
    (b'w', "4"),
    (b'W', "00"),
    (b'x', "01/01/70"),
    (b'X', "00:00:00"),
    (b'y', "70"),
    (b'Y', "1970"),
    (b'z', "+0000"),
    (b'Z', "GMT"),
];

/// vkQuake's `STRINGTEMP_LENGTH`, the buffer `strftime` writes into.
const STRINGTEMP_LENGTH: usize = 1024;

/// `format` written as C's `strftime` writes it at the mock's instant (BUILTIN-18.7): each
/// conversion its text, `%%` a `%`, any other `%` sequence as written; empty when the text does
/// not fit `STRINGTEMP_LENGTH` with its terminator, as `strftime` then returns 0.
pub(crate) fn strftime(format: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut bytes = format.iter().copied();
    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            out.push(byte);
            continue;
        }
        match bytes.next() {
            Some(b'%') => out.push(b'%'),
            Some(conversion) => match CONVERSIONS.iter().find(|(c, _)| *c == conversion) {
                Some((_, text)) => out.extend_from_slice(text.as_bytes()),
                None => out.extend_from_slice(&[b'%', conversion]),
            },
            None => out.push(b'%'),
        }
    }
    if out.len() >= STRINGTEMP_LENGTH {
        return Vec::new();
    }
    out
}
