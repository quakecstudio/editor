// SPDX-License-Identifier: GPL-3.0-or-later

//! C's `atof` on the text of a QuakeC number.

/// C `atof` of `text`, rounded to a 32-bit float, as `PR_LexNumber` computes a literal's
/// value.
///
/// Implements QC-4.1 and QC-4.3: leading C whitespace is skipped, then an optional sign,
/// digits, and at most one `.` with more digits; conversion stops at anything else, so
/// `1.2.3` is 1.2. Text with no digits is 0. A number token never holds an exponent, a hex
/// prefix or `inf` (QC-4.4), so those forms of `atof` are not read here.
#[must_use]
pub fn atof(text: &[u8]) -> f32 {
    let mut rest = text;
    while let Some((&first, tail)) = rest.split_first() {
        if !matches!(first, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r') {
            break;
        }
        rest = tail;
    }
    let mut negative = false;
    if let Some((&sign, tail)) = rest.split_first()
        && (sign == b'+' || sign == b'-')
    {
        negative = sign == b'-';
        rest = tail;
    }
    let mut decimal = String::from("0");
    let mut digits = false;
    let mut seen_dot = false;
    for &b in rest {
        if b.is_ascii_digit() {
            digits = true;
            decimal.push(char::from(b));
        } else if b == b'.' && !seen_dot {
            seen_dot = true;
            decimal.push('.');
        } else {
            break;
        }
    }
    if !digits {
        return 0.0;
    }
    let magnitude = decimal.parse::<f64>().unwrap_or(0.0);
    let value = if negative { -magnitude } else { magnitude };
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec QC-4.1: atof's double is rounded to the literal's 32-bit float"
    )]
    let rounded = value as f32;
    rounded
}
