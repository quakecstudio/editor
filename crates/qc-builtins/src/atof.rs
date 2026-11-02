// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! C's `atof`, `strtod` of the longest prefix that is a number, which vkQuake's extension
//! builtins call (BUILTIN-18.19).

/// `atof(text)`: leading white space skipped, then an optional sign and a decimal number with
/// an optional exponent, a hexadecimal one (`0x`, an optional `p` exponent), `inf`, `infinity`
/// or `nan` in any case; 0 when none is there.
pub(crate) fn atof(text: &[u8]) -> f64 {
    let start = text
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
        .unwrap_or(text.len());
    let rest = text.get(start..).unwrap_or_default();
    let (negative, rest) = match rest.first() {
        Some(b'-') => (true, rest.get(1..).unwrap_or_default()),
        Some(b'+') => (false, rest.get(1..).unwrap_or_default()),
        _ => (false, rest),
    };
    let magnitude = special(rest)
        .or_else(|| hexadecimal(rest))
        .unwrap_or_else(|| decimal(rest));
    if negative { -magnitude } else { magnitude }
}

/// `inf`, `infinity` and `nan` in any case.
fn special(text: &[u8]) -> Option<f64> {
    let lower: Vec<u8> = text.iter().take(3).map(u8::to_ascii_lowercase).collect();
    match lower.as_slice() {
        b"inf" => Some(f64::INFINITY),
        b"nan" => Some(f64::NAN),
        _ => None,
    }
}

/// The decimal number at the start of `text`: digits, an optional point and digits, and an
/// exponent only when digits follow its `e` and sign; 0 with no digit.
fn decimal(text: &[u8]) -> f64 {
    let digits = |from: usize| {
        text.get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut end = digits(0);
    let mut any = end > 0;
    if text.get(end) == Some(&b'.') {
        let after = digits(end.saturating_add(1));
        any |= after > 0;
        end = end.saturating_add(1).saturating_add(after);
    }
    if !any {
        return 0.0;
    }
    if matches!(text.get(end), Some(b'e' | b'E')) {
        let mut at = end.saturating_add(1);
        if matches!(text.get(at), Some(b'+' | b'-')) {
            at = at.saturating_add(1);
        }
        let exp = digits(at);
        if exp > 0 {
            end = at.saturating_add(exp);
        }
    }
    let number = String::from_utf8_lossy(text.get(..end).unwrap_or_default()).into_owned();
    number.parse().unwrap_or(0.0)
}

/// A hexadecimal number `0x` digits, a point and digits, and a binary exponent after `p`;
/// `None` when `0x` is not followed by a hexadecimal digit, which C reads as the 0 before it.
fn hexadecimal(text: &[u8]) -> Option<f64> {
    let rest = match text {
        [b'0', b'x' | b'X', rest @ ..] => rest,
        _ => return None,
    };
    let mut value = 0.0_f64;
    let mut scale = 0_i32;
    let mut any = false;
    let mut at = 0;
    while let Some(digit) = rest.get(at).and_then(|b| char::from(*b).to_digit(16)) {
        value = value.mul_add(16.0, f64::from(digit));
        any = true;
        at = at.saturating_add(1);
    }
    if rest.get(at) == Some(&b'.') {
        at = at.saturating_add(1);
        while let Some(digit) = rest.get(at).and_then(|b| char::from(*b).to_digit(16)) {
            value = value.mul_add(16.0, f64::from(digit));
            scale = scale.saturating_sub(4);
            any = true;
            at = at.saturating_add(1);
        }
    }
    if !any {
        return None;
    }
    if matches!(rest.get(at), Some(b'p' | b'P')) {
        let mut sign = 1_i32;
        let mut digit_at = at.saturating_add(1);
        match rest.get(digit_at) {
            Some(b'-') => {
                sign = -1;
                digit_at = digit_at.saturating_add(1);
            }
            Some(b'+') => digit_at = digit_at.saturating_add(1),
            _ => {}
        }
        let mut exp = 0_i32;
        let mut seen = false;
        while let Some(d) = rest.get(digit_at).filter(|b| b.is_ascii_digit()) {
            exp = exp
                .saturating_mul(10)
                .saturating_add(i32::from(d.saturating_sub(b'0')));
            seen = true;
            digit_at = digit_at.saturating_add(1);
        }
        if seen {
            scale = scale.saturating_add(exp.saturating_mul(sign));
        }
    }
    Some(value * 2.0_f64.powi(scale))
}
