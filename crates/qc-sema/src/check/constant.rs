// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_LexNumber
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_Eval_Truth

//! The value of a constant as FTEQCC's checker reads it (FTEF-26): its `QCC_eval_t`, a union of
//! words that each opcode reads as its own type, and the type the constant was made with.

use qc_lexer::{LexFlags, atof};

use crate::cast::{ConstType, Literal};

/// A constant: the words of its `QCC_eval_t` and the type it was made with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Constant {
    /// The type it was made with.
    pub(crate) ty: ConstType,
    /// Its words: one for a float, an integer or a string, two for a 64-bit value (low word
    /// first), three for a vector.
    words: [u32; 3],
}

impl Constant {
    /// `QCC_MakeFloatConst`.
    pub(crate) const fn float(value: f32) -> Self {
        Self {
            ty: ConstType::Float,
            words: [value.to_bits(), 0, 0],
        }
    }

    /// `QCC_MakeIntConst`.
    pub(crate) const fn int(value: i32) -> Self {
        Self {
            ty: ConstType::Integer,
            words: [u32::from_le_bytes(value.to_le_bytes()), 0, 0],
        }
    }

    /// `QCC_MakeUIntConst`.
    pub(crate) const fn uint(value: u32) -> Self {
        Self {
            ty: ConstType::Uint,
            words: [value, 0, 0],
        }
    }

    /// `QCC_MakeInt64Const`.
    pub(crate) fn int64(value: i64) -> Self {
        Self::wide(ConstType::Int64, value.to_le_bytes())
    }

    /// `QCC_MakeUInt64Const`.
    pub(crate) fn uint64(value: u64) -> Self {
        Self::wide(ConstType::Uint64, value.to_le_bytes())
    }

    /// `QCC_MakeDoubleConst`.
    pub(crate) fn double(value: f64) -> Self {
        Self::wide(ConstType::Double, value.to_bits().to_le_bytes())
    }

    /// `QCC_MakeVectorConst`.
    pub(crate) const fn vector(value: [f32; 3]) -> Self {
        Self {
            ty: ConstType::Vector,
            words: [value[0].to_bits(), value[1].to_bits(), value[2].to_bits()],
        }
    }

    /// A string constant, empty or not; the semantic model keeps no string table, and the only
    /// readings of a string's word are its truth (FTEF-26.3).
    pub(crate) const fn string(empty: bool) -> Self {
        Self {
            ty: ConstType::String,
            words: [if empty { 0 } else { 1 }, 0, 0],
        }
    }

    /// A 64-bit constant of `ty` from its eight bytes, low word first.
    fn wide(ty: ConstType, bytes: [u8; 8]) -> Self {
        let [a, b, c, d, e, f, g, h] = bytes;
        Self {
            ty,
            words: [
                u32::from_le_bytes([a, b, c, d]),
                u32::from_le_bytes([e, f, g, h]),
                0,
            ],
        }
    }

    /// `eval->_float`.
    pub(crate) const fn f(self) -> f32 {
        f32::from_bits(self.words[0])
    }

    /// `eval->_int`.
    pub(crate) const fn i(self) -> i32 {
        i32::from_le_bytes(self.words[0].to_le_bytes())
    }

    /// `eval->_uint`.
    pub(crate) const fn u(self) -> u32 {
        self.words[0]
    }

    /// `eval->i64`.
    pub(crate) fn i64(self) -> i64 {
        i64::from_le_bytes(self.wide_bytes())
    }

    /// `eval->u64`.
    pub(crate) fn u64(self) -> u64 {
        u64::from_le_bytes(self.wide_bytes())
    }

    /// `eval->_double`.
    pub(crate) fn d(self) -> f64 {
        f64::from_bits(self.u64())
    }

    /// `eval->vector`.
    pub(crate) const fn v(self) -> [f32; 3] {
        [
            f32::from_bits(self.words[0]),
            f32::from_bits(self.words[1]),
            f32::from_bits(self.words[2]),
        ]
    }

    /// The first two words as the bytes of a 64-bit value.
    fn wide_bytes(self) -> [u8; 8] {
        let [a, b, c, d] = self.words[0].to_le_bytes();
        let [e, f, g, h] = self.words[1].to_le_bytes();
        [a, b, c, d, e, f, g, h]
    }

    /// `QCC_SRef_IsNull`: an integer, unsigned or float constant whose word is 0.
    pub(crate) const fn null(self) -> bool {
        matches!(
            self.ty,
            ConstType::Integer | ConstType::Uint | ConstType::Float
        ) && self.words[0] == 0
    }

    /// `QCC_Eval_Truth` of the constant as its own type (FTEF-26.3): a float or double not 0, an
    /// integer or 64-bit value not 0, a string not empty, a vector by its first component.
    pub(crate) fn truth(self) -> bool {
        match self.ty {
            ConstType::Float | ConstType::Vector => self.f() != 0.0,
            ConstType::Double => self.d() != 0.0,
            ConstType::Integer | ConstType::Uint | ConstType::String => self.words[0] != 0,
            ConstType::Int64 | ConstType::Uint64 => self.u64() != 0,
        }
    }

    /// The value `QCC_PR_LexNumber` gives the number token `text` (FTEF-26.1): its type as
    /// [`Literal::of_fteqcc_number`] reads it, the digits accumulated in a 64-bit integer that
    /// wraps, with the sign, then held to the type; a fraction read with `atof`.
    pub(crate) fn of_number(text: &str, flags: LexFlags) -> Self {
        let ty = Literal::of_fteqcc_number(text, flags).ty;
        let negative = text.starts_with('-');
        let unsigned = text.strip_prefix('-').unwrap_or(text);
        let (base, digits) = match unsigned
            .strip_prefix("0x")
            .or_else(|| unsigned.strip_prefix("0X"))
        {
            Some(hex) => (16_u64, hex),
            None if unsigned.starts_with('0') => (8, unsigned),
            None => (10, unsigned),
        };
        let mut num: u64 = 0;
        let mut rest = digits;
        for (at, c) in digits.char_indices() {
            let Some(digit) = c.to_digit(36).map(u64::from).filter(|&d| d < base) else {
                rest = digits.get(at..).unwrap_or_default();
                break;
            };
            num = num.wrapping_mul(base).wrapping_add(digit);
            rest = digits
                .get(at.saturating_add(c.len_utf8())..)
                .unwrap_or_default();
        }
        let signed = i64::from_le_bytes(num.to_le_bytes());
        let signed = if negative {
            signed.wrapping_neg()
        } else {
            signed
        };
        let fraction = rest.starts_with('.');
        match ty {
            ConstType::Float if fraction => Self::float(atof(text.as_bytes())),
            ConstType::Double if fraction => {
                let end = text
                    .find(['d', 'D', 'f', 'F'])
                    .unwrap_or(text.len());
                Self::double(text.get(..end).and_then(|t| t.parse().ok()).unwrap_or(0.0))
            }
            ConstType::Float => Self::float(i64_to_f32(signed)),
            ConstType::Double => Self::double(i64_to_f64(signed)),
            ConstType::Integer => Self::int(low_word(signed)),
            ConstType::Uint => Self::uint(u32::from_le_bytes(low_word(signed).to_le_bytes())),
            ConstType::Int64 => Self::int64(signed),
            ConstType::Uint64 => Self::uint64(u64::from_le_bytes(signed.to_le_bytes())),
            ConstType::Vector | ConstType::String => Self::float(0.0),
        }
    }
}

/// The low 32 bits of `value`, C's `(int)` of a `long long`.
fn low_word(value: i64) -> i32 {
    let [a, b, c, d, ..] = value.to_le_bytes();
    i32::from_le_bytes([a, b, c, d])
}

/// C's `(float)` of a `long long`, rounding to nearest.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec FTEF-26.1: C's integer to float conversion rounds"
)]
const fn i64_to_f32(value: i64) -> f32 {
    value as f32
}

/// C's `(double)` of a `long long`, rounding to nearest.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec FTEF-26.1: C's integer to double conversion rounds"
)]
const fn i64_to_f64(value: i64) -> f64 {
    value as f64
}
