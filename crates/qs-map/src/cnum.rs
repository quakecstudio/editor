// SPDX-License-Identifier: GPL-3.0-or-later

//! The C library number conversions qbsp reads `.map` numbers with: `atoi` (MAP-3.3, MAP-4.1),
//! `atof` (MAP-4.1) and `sscanf`'s `%lf` (MAP-2.7), as C89 defines them, which is what the
//! original x86 build had: decimal numbers only, with no hexadecimal, infinity or NaN forms.

/// Whether `byte` is C's `isspace` in the C locale.
const fn is_c_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | 0x0B | 0x0C | b'\r')
}

/// The number of leading bytes of `bytes` that are C whitespace.
fn space_len(bytes: &[u8]) -> usize {
    bytes.iter().take_while(|&&byte| is_c_space(byte)).count()
}

/// C's `atoi`: optional whitespace, an optional sign and decimal digits; anything else ends
/// the number, and no digits give 0. A value outside `int` saturates, as the original
/// x86 build's `strtol` with its 32-bit `long` does.
#[must_use]
pub fn c_atoi(bytes: &[u8]) -> i32 {
    let rest = bytes.get(space_len(bytes)..).unwrap_or_default();
    let (negative, digits) = match rest.split_first() {
        Some((b'-', digits)) => (true, digits),
        Some((b'+', digits)) => (false, digits),
        _ => (false, rest),
    };
    let mut value: i64 = 0;
    for &byte in digits.iter().take_while(|byte| byte.is_ascii_digit()) {
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(byte.wrapping_sub(b'0')));
    }
    if negative {
        value = value.saturating_neg();
    }
    i32::try_from(value).unwrap_or(if negative { i32::MIN } else { i32::MAX })
}

/// C89 `strtod`: the value of the longest decimal number at the start of `bytes` after
/// optional whitespace, and the number of bytes it used, 0 when there is none.
#[must_use]
pub fn c_strtod(bytes: &[u8]) -> (f64, usize) {
    let start = space_len(bytes);
    let rest = bytes.get(start..).unwrap_or_default();
    let mut len = usize::from(matches!(rest.first(), Some(b'+' | b'-')));
    let digits = |from: usize| {
        rest.get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let whole = digits(len);
    len = len.saturating_add(whole);
    let mut fraction = 0;
    if rest.get(len) == Some(&b'.') {
        fraction = digits(len.saturating_add(1));
        if whole > 0 || fraction > 0 {
            len = len.saturating_add(1).saturating_add(fraction);
        }
    }
    if whole == 0 && fraction == 0 {
        return (0.0, 0);
    }
    if matches!(rest.get(len), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(rest.get(len.saturating_add(1)), Some(b'+' | b'-')));
        let exponent = digits(len.saturating_add(1).saturating_add(sign));
        if exponent > 0 {
            len = len
                .saturating_add(1)
                .saturating_add(sign)
                .saturating_add(exponent);
        }
    }
    let value = rest
        .get(..len)
        .and_then(|number| core::str::from_utf8(number).ok())
        .and_then(|number| number.parse::<f64>().ok())
        .unwrap_or(0.0);
    (value, start.saturating_add(len))
}

/// C's `atof`: [`c_strtod`]'s value.
#[must_use]
pub fn c_atof(bytes: &[u8]) -> f64 {
    c_strtod(bytes).0
}

/// `sscanf(text, "%lf %lf %lf", ...)` into three values that start at 0: each conversion
/// that fails leaves its value and every later one at 0 (MAP-2.7).
#[must_use]
pub fn scan_vector(bytes: &[u8]) -> [f64; 3] {
    let mut values = [0.0; 3];
    let mut rest = bytes;
    for value in &mut values {
        let (number, used) = c_strtod(rest);
        if used == 0 {
            break;
        }
        *value = number;
        rest = rest.get(used..).unwrap_or_default();
    }
    values
}
