// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2016      Spike
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from Novum/vkQuake@27789d7333a46674474d871137bb71e056719628: Quake/pr_ext.c, PF_Sin, PF_pow, PF_mod, PF_str2chr, PF_chr2str, qc_isascii, PF_strpad, PF_strreplace, PF_uri_escape, PF_uri_unescape

//! The text and arithmetic of vkQuake's string and math extension builtins (BUILTIN-18.31 to
//! BUILTIN-18.35): each function is the builtin's body without its parameter reads.

use qc_vm::convert::{double_to_float, float_to_int, int_to_float};

/// vkQuake's `STRINGTEMP_LENGTH`, a temporary string's bytes, its zero among them.
const STRINGTEMP_LENGTH: usize = 1024;
/// 2^63's exponent: the first float past an `i64` (BUILTIN-18.32).
const I64_LIMIT_EXPONENT: i32 = 63;
/// `chr2str` stops 6 bytes short of a temporary string, room for one UTF-8 character and the
/// terminator (BUILTIN-18.32).
const CHR2STR_ROOM: usize = 6;
/// The Quake characters' private-use codes, `0xe000` to `0xe0ff`, and the codes below them that
/// are bytes (BUILTIN-18.32).
const QUAKE_CHARS: core::ops::Range<u32> = 0xe000..0xe100;
const BYTE_CODES: u32 = 0x100;
/// `uri_escape` stops 4 bytes short of a temporary string, room for one `%XX` and the
/// terminator; `uri_unescape` reads `%` and two digits (BUILTIN-18.32).
const URI_ESCAPE_ROOM: usize = 4;
const URI_ESCAPE_LENGTH: usize = 3;
/// A hexadecimal digit's bits, and its low digit's mask.
const NIBBLE_BITS: u32 = 4;
const NIBBLE_MASK: u8 = 0xf;
/// The value of the hexadecimal digit `A`.
const HEX_TEN: u8 = 10;

/// `PF_mod`: 0 for a zero divisor, else "a - (n * (int)(a / n))" in `float`.
#[must_use]
pub fn modulo(a: f32, n: f32) -> f32 {
    if n == 0.0 {
        return 0.0;
    }
    a - n * int_to_float(float_to_int(a / n))
}

/// `PF_Sin`: C's `sin` of the float as a `double`, rounded to a float.
#[must_use]
pub fn sin(value: f32) -> f32 {
    double_to_float(f64::from(value).sin())
}

/// `PF_pow`: C's `pow` of the two floats as `double`s, rounded to a float.
#[must_use]
pub fn pow(base: f32, exponent: f32) -> f32 {
    double_to_float(f64::from(base).powf(f64::from(exponent)))
}

/// `PF_str2chr`: the byte at `ofs` of `text`, from its end when negative, 0 past the end.
#[must_use]
pub fn str2chr(text: &[u8], ofs: i32) -> f32 {
    let len = i32::try_from(text.len()).unwrap_or(i32::MAX);
    let ofs = if ofs < 0 { len.wrapping_add(ofs) } else { ofs };
    if ofs != 0 && (ofs < 0 || ofs > len) {
        return 0.0;
    }
    let byte = usize::try_from(ofs)
        .ok()
        .and_then(|at| text.get(at))
        .copied()
        .unwrap_or(0);
    f32::from(byte)
}

/// C's `(unsigned int)` of a float on x86-64, `cvttss2si` to 64 bits then the low 32: a value
/// out of a 64-bit integer's range, or a NaN, is 0.
#[must_use]
pub fn float_to_unsigned(value: f32) -> u32 {
    // 2^63, the first float past an i64.
    let limit = 2.0_f32.powi(I64_LIMIT_EXPONENT);
    if value.is_nan() || value.abs() >= limit {
        return 0;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec BUILTIN-18.32: in range, the conversion truncates toward zero"
    )]
    let wide = value as i64;
    let [a, b, c, d, ..] = wide.to_le_bytes();
    u32::from_le_bytes([a, b, c, d])
}

/// `PF_chr2str`: one byte per code, `0xe000` to `0xe0ff` the Quake character of its low byte,
/// a code below 256 itself and any other `?`, while the text is shorter than 1018 bytes; the
/// text ends at a zero code.
#[must_use]
pub fn chr2str(codes: &[u32]) -> Vec<u8> {
    let mut out = Vec::new();
    for &code in codes {
        if out.len() >= STRINGTEMP_LENGTH.saturating_sub(CHR2STR_ROOM) {
            break;
        }
        let [low, ..] = code.to_le_bytes();
        let byte = if QUAKE_CHARS.contains(&code) || code < BYTE_CODES {
            low
        } else {
            b'?'
        };
        if byte == 0 {
            break;
        }
        out.push(byte);
    }
    out
}

/// `PF_strpad`: `text` padded with spaces to `pad` bytes, on the left when `pad` is negative,
/// never past a temporary string.
#[must_use]
pub fn strpad(pad: i32, text: &[u8]) -> Vec<u8> {
    let most = i32::try_from(STRINGTEMP_LENGTH.saturating_sub(1)).unwrap_or(i32::MAX);
    let len = i32::try_from(text.len()).unwrap_or(i32::MAX);
    let room = STRINGTEMP_LENGTH.saturating_sub(1);
    if pad < 0 {
        let spaces = pad.wrapping_neg().wrapping_sub(len).clamp(0, most);
        let spaces = usize::try_from(spaces).unwrap_or(0);
        // `q_strlcpy (dest + pad, src, STRINGTEMP_LENGTH - pad)`.
        let kept = text.len().min(room.saturating_sub(spaces));
        let mut out = vec![b' '; spaces];
        out.extend_from_slice(text.get(..kept).unwrap_or_default());
        out
    } else {
        let spaces = pad.min(most).wrapping_sub(len).max(0);
        let mut out = text
            .get(..text.len().min(room))
            .unwrap_or_default()
            .to_vec();
        out.resize(
            out.len()
                .saturating_add(usize::try_from(spaces).unwrap_or(0)),
            b' ',
        );
        out
    }
}

/// `PF_strreplace`: each `search` in `subject` replaced by `replace`, left to right, while the
/// result is shorter than `STRINGTEMP_LENGTH - replacelen - 2` bytes; `subject` itself for an
/// empty `search`.
#[must_use]
pub fn strreplace(search: &[u8], replace: &[u8], subject: &[u8]) -> Vec<u8> {
    if search.is_empty() {
        return subject.to_vec();
    }
    let limit = i64::try_from(STRINGTEMP_LENGTH)
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::try_from(replace.len()).unwrap_or(i64::MAX))
        .saturating_sub(2);
    let mut out = Vec::new();
    let mut at = 0;
    while at < subject.len() && i64::try_from(out.len()).unwrap_or(i64::MAX) < limit {
        let rest = subject.get(at..).unwrap_or_default();
        if rest.starts_with(search) {
            out.extend_from_slice(replace);
            at = at.saturating_add(search.len());
        } else {
            out.extend_from_slice(rest.get(..1).unwrap_or_default());
            at = at.saturating_add(1);
        }
    }
    out
}

/// `PF_uri_escape`: letters, digits, `.`, `-` and `_` kept and every other byte `%XX`, while
/// the result is shorter than 1020 bytes.
#[must_use]
pub fn uri_escape(text: &[u8]) -> Vec<u8> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Vec::new();
    for &byte in text {
        if out.len() >= STRINGTEMP_LENGTH.saturating_sub(URI_ESCAPE_ROOM) {
            break;
        }
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_') {
            out.push(byte);
        } else {
            out.push(b'%');
            out.extend(
                [byte >> NIBBLE_BITS, byte & NIBBLE_MASK]
                    .iter()
                    .filter_map(|&n| HEX.get(usize::from(n))),
            );
        }
    }
    out
}

/// The value of one hexadecimal digit.
fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'F' => Some(byte.wrapping_sub(b'A').wrapping_add(HEX_TEN)),
        b'a'..=b'f' => Some(byte.wrapping_sub(b'a').wrapping_add(HEX_TEN)),
        b'0'..=b'9' => Some(byte.wrapping_sub(b'0')),
        _ => None,
    }
}

/// `PF_uri_unescape`: each `%` and two hexadecimal digits the byte they name, any other byte
/// kept, while the result is shorter than 1022 bytes; the text ends at a `%00`.
#[must_use]
pub fn uri_unescape(text: &[u8]) -> Vec<u8> {
    let byte = |at: usize| text.get(at).copied().unwrap_or(0);
    let mut out = Vec::new();
    let mut at = 0;
    while byte(at) != 0 && out.len() < STRINGTEMP_LENGTH.saturating_sub(2) {
        let pair = (byte(at) == b'%')
            .then(|| {
                let high = hex_digit(byte(at.saturating_add(1)))?;
                let low = hex_digit(byte(at.saturating_add(2)))?;
                Some(high.wrapping_shl(NIBBLE_BITS) | low)
            })
            .flatten();
        match pair {
            Some(0) => break,
            Some(value) => {
                out.push(value);
                at = at.saturating_add(URI_ESCAPE_LENGTH);
            }
            None => {
                out.push(byte(at));
                at = at.saturating_add(1);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
