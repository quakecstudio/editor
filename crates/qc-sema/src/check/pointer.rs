// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_StatementFlags, QCC_MakeIntConst

//! Pointer arithmetic and loads through a pointer, as FTEQCC emulates them when the target
//! lacks their opcodes (FTEF-27.10): an offset scaled by the size of what the pointer points
//! at, and a load of fewer than 32 bits read from the word that holds it.

use qc_diag::{Code, Span};
use qc_preproc::{FteFlag, Op};
use qc_syntax::c_scalar::CScalar;

use crate::check::constant::Constant;
use crate::check::emulate::conversion_opcodes;
use crate::check::etype::Etype;
use crate::check::expr::{Checker, Fatal};
use crate::check::{Constness, Value};
use crate::ty::{SimpleType, Type};

/// `VMWORDSIZE`: the bytes of a word, by which `OP_ADD_PIW` scales its offset.
const WORD_BYTES: i32 = 4;

/// What a pointer points at, as FTEQCC's pointer arithmetic sizes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pointee {
    /// `void`, counted in bytes.
    Void,
    /// A type narrower than a word (`aux_type->bits`): `__int8`, `__int16` and their unsigned
    /// kinds.
    Bits(i32),
    /// A type of whole words (`aux_type->size`); `None` for a struct or union, whose layout the
    /// semantic model does not follow.
    Words(Option<i32>),
}

/// A narrow load: how many of a word's bytes it reads, and whether it extends the sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Narrow {
    /// 8 or 16.
    bits: i32,
    /// `OP_LOADP_I8` and `OP_LOADP_I16`.
    signed: bool,
}

impl Checker<'_> {
    /// `OP_ADD_PF`, `OP_ADD_FP`, `OP_ADD_PI`, `OP_ADD_IP`, `OP_ADD_PU`, `OP_ADD_UP`, `OP_SUB_PF`,
    /// `OP_SUB_PI` and `OP_SUB_PU`: the offset made an integer, scaled by what the pointer
    /// points at and added to it, as bytes for `void` and the narrow types, else by
    /// `OP_ADD_PIW`; a subtraction scales by the size negated, which `QCC_MakeIntConst` warns is
    /// truncated (FTEF-27.10).
    pub(super) fn pointer_offset(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
    ) -> Result<Value, Fatal> {
        let pointer_first = matches!(
            op,
            Op::AddPf | Op::AddPi | Op::AddPu | Op::SubPf | Op::SubPi | Op::SubPu
        );
        let (pointer, offset) = if pointer_first {
            (lhs, rhs)
        } else {
            (rhs, lhs)
        };
        let mut offset = offset.clone();
        if matches!(op, Op::AddPf | Op::AddFp | Op::SubPf) {
            for &conv in conversion_opcodes(Etype::Float, Etype::Integer) {
                offset = self.emit(conv, &offset, &Value::unknown(), span)?;
            }
        }
        let subtract = matches!(op, Op::SubPf | Op::SubPi | Op::SubPu);
        let bytewise = if subtract { Op::SubI } else { Op::AddI };
        let value = match self.pointee(&pointer.ty) {
            Pointee::Void => {
                self.undefined_word_size(span, "void* maths was disabled.");
                self.emit(bytewise, pointer, &offset, span)?
            }
            Pointee::Bits(bits) => {
                self.undefined_word_size(span, "pointer* maths was disabled on this type.");
                if bits != 8 {
                    offset = self.emit(Op::MulI, &offset, &bytes(bits), span)?;
                }
                self.emit(bytewise, pointer, &offset, span)?
            }
            Pointee::Words(words) => {
                let size = if subtract {
                    self.negated_size(words, span)
                } else {
                    words.map_or_else(opaque_int, int)
                };
                offset = self.emit(Op::MulI, &offset, &size, span)?;
                self.emit(Op::AddPiw, pointer, &offset, span)?
            }
        };
        Ok(Value {
            ty: pointer.ty.clone(),
            ..value
        })
    }

    /// `OP_SUB_PP`: two pointers of different types are "incompatible pointer types", a fatal
    /// error; else their difference in bytes, divided by the size of what they point at
    /// (FTEF-27.10).
    pub(super) fn pointer_difference(
        &mut self,
        lhs: &Value,
        rhs: &Value,
        span: Span,
    ) -> Result<Value, Fatal> {
        if !lhs.ty.fteqcc_same(&rhs.ty) {
            let message = "incompatible pointer types".to_owned();
            return self.fatal(Code::E0327, span, message);
        }
        let difference = self.emit(Op::SubI, lhs, rhs, span)?;
        let divisor = match self.pointee(&lhs.ty) {
            Pointee::Void => {
                self.undefined_word_size(span, "void* maths was disabled.");
                return Ok(difference);
            }
            Pointee::Bits(bits) => {
                self.undefined_word_size(span, "pointer sizes were marked as undefined.");
                bytes(bits)
            }
            Pointee::Words(words) => {
                let size = words.map_or_else(opaque_int, int);
                self.emit(Op::AddPiw, &int(0), &size, span)?
            }
        };
        self.emit(Op::DivI, &difference, &divisor, span)
    }

    /// `OP_ADD_PIW`: the offset in words made bytes, `VMWORDSIZE *` it, and added
    /// (FTEF-27.10).
    pub(super) fn word_offset(
        &mut self,
        lhs: &Value,
        rhs: &Value,
        span: Span,
    ) -> Result<Value, Fatal> {
        let message = "Making assumptions about pointer sizes was blocked.";
        self.undefined_word_size(span, message);
        let offset = self.emit(Op::MulI, &int(WORD_BYTES), rhs, span)?;
        let value = self.emit(Op::AddI, lhs, &offset, span)?;
        Ok(Value {
            ty: lhs.ty.clone(),
            ..value
        })
    }

    /// `OP_LOADP_U8`, `OP_LOADP_I8`, `OP_LOADP_U16` and `OP_LOADP_I16`: the word that holds the
    /// value read with `OP_LOADP_I`, shifted up to drop the bytes above it and down to drop
    /// those below, extending the sign for a signed load; a signed byte is an unsigned one
    /// sign-extended when the target has `OP_LOADP_U8` (FTEF-27.10).
    pub(super) fn narrow_load(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
    ) -> Result<Value, Fatal> {
        let narrow = match op {
            Op::LoadpU8 => Narrow {
                bits: 8,
                signed: false,
            },
            Op::LoadpI8 => Narrow {
                bits: 8,
                signed: true,
            },
            Op::LoadpU16 => Narrow {
                bits: 16,
                signed: false,
            },
            _ => Narrow {
                bits: 16,
                signed: true,
            },
        };
        let target = self.env.target_at(span.start);
        if op == Op::LoadpI8 && target.valid(Op::LoadpU8) {
            let value = self.emit(Op::LoadpU8, lhs, rhs, span)?;
            return self.emit(Op::BitextendI, &value, &uint(8), span);
        }
        let (mask, words) = if narrow.bits == 8 { (3, 2) } else { (1, 1) };
        let (up, down) = if narrow.signed {
            (Op::LshiftI, Op::RshiftI)
        } else {
            (Op::LshiftU, Op::RshiftU)
        };
        let within = self.emit(Op::BitandI, rhs, &uint(mask), span)?;
        let index = self.emit(Op::RshiftI, rhs, &uint(words), span)?;
        let word = self.emit(Op::LoadpI, lhs, &index, span)?;
        let bit = self.emit(Op::MulI, &uint(narrow.bits), &within, span)?;
        let unused = 32_i32.saturating_sub(narrow.bits);
        let shift = self.emit(Op::SubI, &uint(unused), &bit, span)?;
        let high = self.emit(up, &word, &shift, span)?;
        self.emit(down, &high, &int(unused), span)
    }

    /// `OP_LOADP_F` and the other word loads through a pointer: a call of the program's
    /// `memgetval` with the offset made a float, else "memgetval function not defined: cannot
    /// emulate OP_LOADP_*" and the opcode refused (FTEF-27.10).
    pub(super) fn word_load(
        &mut self,
        op: Op,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        if !self.env.defines("memgetval") {
            let message = "memgetval function not defined: cannot emulate OP_LOADP_*";
            self.env.report(Code::W0235, span, message.to_owned());
            self.refuse(op, span);
            return Ok(result);
        }
        self.emit(Op::ConvItof, rhs, &Value::unknown(), span)?;
        Ok(result)
    }

    /// What the pointer type `ty` points at.
    fn pointee(&self, ty: &Type) -> Pointee {
        let Type::Pointer(inner) = ty else {
            return Pointee::Words(Some(1));
        };
        if **inner == Type::Simple(SimpleType::Void) {
            return Pointee::Void;
        }
        match inner.fteqcc_scalar() {
            Some(CScalar::Int8 | CScalar::Uint8) => return Pointee::Bits(8),
            Some(CScalar::Int16 | CScalar::Uint16) => return Pointee::Bits(16),
            _ => {}
        }
        Pointee::Words(match self.env.shape(inner).map(|shape| shape.etype) {
            Some(Etype::Vector) => Some(3),
            Some(Etype::Int64 | Etype::Uint64 | Etype::Double) => Some(2),
            Some(Etype::Struct | Etype::Union) | None => None,
            Some(_) => Some(1),
        })
    }

    /// `QCC_MakeIntConst(-aux_type->size)`: the size is unsigned, so its negation is
    /// `2^32 - size`, which "will be truncated" to `-size`.
    fn negated_size(&mut self, words: Option<i32>, span: Span) -> Value {
        let Some(words) = words else {
            return opaque_int();
        };
        let negated = words.wrapping_neg();
        let unsigned = 1_u64 << 32_u32;
        let promoted = unsigned.saturating_sub(u64::from(words.unsigned_abs()));
        let message = format!("Constant int operand {promoted} will be truncated to {negated}");
        self.env.report(Code::W0234, span, message);
        int(negated)
    }

    /// Under `undefwordsize`, the error that pointer arithmetic assumes a size it may not.
    fn undefined_word_size(&mut self, span: Span, message: &str) {
        if self.env.flags_at(span.start).has(FteFlag::Undefwordsize) {
            self.env.report(Code::E0326, span, message.to_owned());
        }
    }
}

/// `QCC_MakeIntConst(value)`.
fn int(value: i32) -> Value {
    Value::constant(Constant::int(value))
}

/// `QCC_MakeUIntConst(value)`.
fn uint(value: i32) -> Value {
    Value::constant(Constant::uint(value.unsigned_abs()))
}

/// The bytes of a type `bits` wide, `QCC_MakeIntConst(bits / 8)`.
fn bytes(bits: i32) -> Value {
    int(bits.checked_div(8).unwrap_or(0))
}

/// An integer constant whose value the model does not know.
fn opaque_int() -> Value {
    Value {
        constness: Constness::Opaque,
        ..int(0)
    }
}
