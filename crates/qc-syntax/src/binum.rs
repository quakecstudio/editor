// SPDX-License-Identifier: GPL-3.0-or-later

//! The builtin number a compiler binds for a number literal (QC-13.10, FTE-17.6), shared by the
//! parser, which reports a literal no compiler binds, and the semantic layers, which key and
//! compare builtins by the number.

use qc_lexer::{Dialect, TokenKind};

/// The `int` the dialect's compiler binds for the builtin number literal `text`, or `None` for
/// its "Bad builtin immediate".
///
/// Implements QC-13.10 and FTE-17.6: `(int)` of a `float` literal's value once it is whole and
/// from -2^31 to 2^31 - 1, read with `atof` under the vanilla dialect (QC-4.1) and in FTEQCC's
/// number forms under an FTEQCC dialect (FTEF-3.1); under an FTEQCC dialect also an `i`
/// literal's whole number kept to 32 bits, and no number for a `u` or `l` literal. `text` that
/// is not one number token of the dialect binds nothing.
#[must_use]
pub fn builtin_binum(text: &str, dialect: Dialect) -> Option<i32> {
    let lexed = qc_lexer::lex_with(text.as_bytes(), dialect);
    let [token] = lexed.tokens.as_slice() else {
        return None;
    };
    let TokenKind::Float(value) = token.kind else {
        return None;
    };
    // FTE-17.6: the suffix letters `i`, `u` and `l` are no digit of any base, and a vanilla
    // number token has none.
    let digits = text.trim_end_matches(['i', 'I', 'u', 'U', 'l', 'L']);
    match text.get(digits.len()..).unwrap_or_default() {
        "" => float_binum(value),
        "i" | "I" => Some(fteqcc_int(digits)),
        _ => None,
    }
}

/// `(int)pr_immediate._float`, once `pr_immediate._float == (int)pr_immediate._float` holds
/// (QC-13.10): outside `int` C's conversion is undefined, and the x86 build gives -2147483648
/// (VM-9.2), which fails the comparison.
fn float_binum(value: f32) -> Option<i32> {
    #[expect(
        clippy::float_cmp,
        reason = "spec QC-13.10: the compilers bind only when `_float == (int)_float`"
    )]
    let whole = value.trunc() == value;
    let int_range = f64::from(i32::MIN)..-f64::from(i32::MIN);
    if !whole || !int_range.contains(&f64::from(value)) {
        return None;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec QC-13.10: a whole value in `int`'s range converts exactly"
    )]
    let number = value as i32;
    Some(number)
}

/// `pr_immediate._int = num` for an FTEQCC integer literal without its suffix: the digits in
/// their base (FTEF-3.1), times the sign, kept to 32 bits as C's conversion to `int` keeps them
/// on every build FTEQCC supports (FTE-17.6).
fn fteqcc_int(literal: &str) -> i32 {
    let (negative, rest) = literal
        .strip_prefix('-')
        .map_or((false, literal), |rest| (true, rest));
    let (base, digits) = if let Some(hex) = rest.strip_prefix("0x").or(rest.strip_prefix("0X")) {
        (16, hex)
    } else if let Some(octal) = rest.strip_prefix('0') {
        (8, octal)
    } else {
        (10, rest)
    };
    let mut num: u64 = 0;
    for digit in digits.chars().map_while(|c| c.to_digit(base)) {
        num = num
            .wrapping_mul(u64::from(base))
            .wrapping_add(u64::from(digit));
    }
    if negative {
        num = num.wrapping_neg();
    }
    let [a, b, c, d, ..] = num.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}
