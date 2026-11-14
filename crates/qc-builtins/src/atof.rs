// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! C's `atof`, `strtod` of the longest prefix that is a number, which vkQuake's extension
//! builtins call (BUILTIN-18.25).

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
    let lower: Vec<u8> = text
        .iter()
        .take(SPECIAL_LENGTH)
        .map(u8::to_ascii_lowercase)
        .collect();
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

/// The hexadecimal number after `0x` at the start of `text`, correctly rounded as C99 requires
/// (N1256 7.20.1.3 paragraph 5: "If the subject sequence has the hexadecimal form and FLT_RADIX
/// is a power of 2, the value resulting from the conversion is correctly rounded."): the first
/// 60 significant bits of the digits kept exactly, every later digit only as whether it is
/// nonzero, and one rounding to the nearest `f64`, ties to even (BUILTIN-18.25). `None` with no
/// digit.
fn hexadecimal(text: &[u8]) -> Option<f64> {
    let rest = match text {
        [b'0', b'x' | b'X', rest @ ..] => rest,
        _ => return None,
    };
    let mut mantissa = Mantissa::default();
    let mut any = false;
    let mut at = 0;
    while let Some(digit) = rest.get(at).and_then(|b| char::from(*b).to_digit(HEX)) {
        mantissa.push(digit, Place::Whole);
        any = true;
        at = at.saturating_add(1);
    }
    if rest.get(at) == Some(&b'.') {
        at = at.saturating_add(1);
        while let Some(digit) = rest.get(at).and_then(|b| char::from(*b).to_digit(HEX)) {
            mantissa.push(digit, Place::Fraction);
            any = true;
            at = at.saturating_add(1);
        }
    }
    if !any {
        return None;
    }
    if matches!(rest.get(at), Some(b'p' | b'P')) {
        let mut negative = false;
        let mut digit_at = at.saturating_add(1);
        match rest.get(digit_at) {
            Some(b'-') => {
                negative = true;
                digit_at = digit_at.saturating_add(1);
            }
            Some(b'+') => digit_at = digit_at.saturating_add(1),
            _ => {}
        }
        let mut exp = 0_i64;
        let mut seen = false;
        while let Some(d) = rest.get(digit_at).filter(|b| b.is_ascii_digit()) {
            exp = exp
                .saturating_mul(DECIMAL)
                .saturating_add(i64::from(d.saturating_sub(b'0')));
            seen = true;
            digit_at = digit_at.saturating_add(1);
        }
        if seen {
            let exp = if negative { exp.saturating_neg() } else { exp };
            mantissa.scale = mantissa.scale.saturating_add(exp);
        }
    }
    Some(mantissa.rounded())
}

/// Where a hexadecimal digit is: before the point, or after it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Whole,
    Fraction,
}

/// A hexadecimal number read so far: `bits * 2^scale`, and whether a nonzero digit past the
/// bits kept was dropped.
#[derive(Default)]
struct Mantissa {
    bits: u64,
    scale: i64,
    sticky: bool,
}

/// The bits kept: 60, so that one more digit still fits in the `u64`.
const KEPT: u32 = 60;
/// A hexadecimal digit's base and bits.
const HEX: u32 = 16;
const HEX_BITS: i64 = 4;
/// A decimal exponent's base.
const DECIMAL: i64 = 10;
/// The letters `inf` and `nan` have.
const SPECIAL_LENGTH: usize = 3;
/// An `f64`'s mantissa bits below the implicit one, its exponent bias, its largest
/// exponent, the worth of its least subnormal bit, `2^-1074`, and its exponent field's
/// infinity.
const MANTISSA_BITS: i64 = 52;
const EXPONENT_BIAS: i64 = 1023;
const MAX_EXPONENT: i64 = 1023;
const LEAST_BIT: i64 = -1074;
const EXPONENT_INFINITE: i64 = 2047;
/// The implicit bit of a normal `f64`'s 53, and a carry past it.
const IMPLICIT: u64 = 1 << 52;
const CARRIED: u64 = 1 << 53;

impl Mantissa {
    /// Adds the next digit, at `place`.
    fn push(&mut self, digit: u32, place: Place) {
        if self.bits >> KEPT == 0 {
            self.bits = self
                .bits
                .saturating_mul(u64::from(HEX))
                .saturating_add(u64::from(digit));
            if place == Place::Fraction {
                self.scale = self.scale.saturating_sub(HEX_BITS);
            }
        } else {
            self.sticky |= digit != 0;
            if place == Place::Whole {
                self.scale = self.scale.saturating_add(HEX_BITS);
            }
        }
    }

    /// The nearest `f64`, ties to even: infinity past the largest, 0 below half the least.
    fn rounded(&self) -> f64 {
        if self.bits == 0 {
            return 0.0;
        }
        let length = i64::from(u64::BITS.saturating_sub(self.bits.leading_zeros()));
        // The value is 1.x times 2^exponent; its last kept bit is worth 2^last.
        let exponent = self.scale.saturating_add(length).saturating_sub(1);
        if exponent > MAX_EXPONENT {
            return f64::INFINITY;
        }
        let last = exponent.saturating_sub(MANTISSA_BITS).max(LEAST_BIT);
        let shift = self.scale.saturating_sub(last);
        let mut kept = if shift >= 0 {
            // Exact: the bits fit in the 53 the value keeps.
            u32::try_from(shift)
                .ok()
                .and_then(|shift| self.bits.checked_shl(shift))
                .unwrap_or(0)
        } else {
            let dropped = shift.unsigned_abs();
            let wide = u128::from(self.bits);
            let (kept, rest, half) = match u32::try_from(dropped) {
                Ok(n) if n < 128 => {
                    let half = 1_u128 << n.saturating_sub(1);
                    let rest = wide & ((1_u128 << n).saturating_sub(1));
                    (wide >> n, rest, half)
                }
                _ => (0, wide, u128::MAX),
            };
            let kept = u64::try_from(kept).unwrap_or(u64::MAX);
            let up = rest > half || (rest == half && (self.sticky || kept & 1 == 1));
            if up { kept.saturating_add(1) } else { kept }
        };
        let mut last = last;
        // A round-up that carried into the next power of two: 2^53 units are 2^52 of twice the
        // worth.
        if kept == CARRIED {
            kept = IMPLICIT;
            last = last.saturating_add(1);
        }
        if kept == 0 {
            return 0.0;
        }
        if kept < IMPLICIT {
            // A subnormal: its bits are the mantissa, its exponent field 0.
            return f64::from_bits(kept);
        }
        let biased = last
            .saturating_add(MANTISSA_BITS)
            .saturating_add(EXPONENT_BIAS);
        if biased >= EXPONENT_INFINITE {
            return f64::INFINITY;
        }
        let field = u64::try_from(biased).unwrap_or(0);
        // The exponent field and the mantissa's bits below the implicit one, which share no bit.
        f64::from_bits((field << MANTISSA_BITS).saturating_add(kept & (IMPLICIT - 1)))
    }
}
