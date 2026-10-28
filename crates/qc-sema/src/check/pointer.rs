// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_StatementFlags, QCC_MakeIntConst, QCC_StoreToPointer, QCC_PR_Statement_SmallStore

//! Pointer arithmetic, loads and stores through a pointer, as FTEQCC emulates them when the
//! target lacks their opcodes (FTEF-27.10, FTEF-6.18): an offset scaled by the size of what the
//! pointer points at, and a load or store of fewer than 32 bits made on the word that holds it.

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
pub(super) enum Pointee {
    /// `void`, counted in bytes.
    Void,
    /// A type narrower than a word (`aux_type->bits`): `__int8`, `__int16` and their unsigned
    /// kinds.
    Bits(i32),
    /// A type of whole words (`aux_type->size`); `None` for a type whose size the semantic
    /// model does not follow.
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

    /// FTEF-29.5: a postfix's step of a pointer: for a narrow type (`aux_type->bits`) the step
    /// in bytes added by `OP_ADD_I`, first, for a value not `discarded` (`QCC_RefToDef`), under
    /// `undefwordsize` the fatal error "not allowed to make assumptions about pointer sizes.
    /// sorry."; else the step in words, `inc * aux_type->size` (a word for `void`), added by
    /// `OP_ADD_PIW`. The value keeps the pointer's type.
    pub(super) fn pointer_step(
        &mut self,
        pointer: &Value,
        step: i8,
        discarded: bool,
        span: Span,
    ) -> Result<Value, Fatal> {
        let typed = |value: Value| Value {
            ty: pointer.ty.clone(),
            ..value
        };
        let bytes = match self.pointee(&pointer.ty) {
            Pointee::Bits(bits) => bits.wrapping_div(8),
            // `type_size[ev_void]`, a word.
            Pointee::Void => {
                let step = int(i32::from(step));
                return self.emit(Op::AddPiw, pointer, &step, span).map(typed);
            }
            Pointee::Words(words) => {
                let step =
                    words.map_or_else(opaque_int, |w| int(i32::from(step).saturating_mul(w)));
                return self.emit(Op::AddPiw, pointer, &step, span).map(typed);
            }
        };
        if !discarded && self.env.flags_at(span.start).has(FteFlag::Undefwordsize) {
            let message = "not allowed to make assumptions about pointer sizes. sorry.";
            return self.fatal(Code::E0322, span, message.to_owned());
        }
        let step = int(i32::from(step).saturating_mul(bytes));
        self.emit(Op::AddI, pointer, &step, span).map(typed)
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
            self.refuse_emitted(op, span);
            return Ok(result);
        }
        self.emit(Op::ConvItof, rhs, &Value::unknown(), span)?;
        Ok(result)
    }

    /// What the pointer type `ty` points at.
    pub(super) fn pointee(&self, ty: &Type) -> Pointee {
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
            Some(Etype::Struct | Etype::Union) => self
                .env
                .layout(inner)
                .and_then(|layout| i32::try_from(layout.words).ok()),
            None => None,
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

    /// FTEF-6.18: a store of `source` to what a pointer points at, a `ty`, at `index`
    /// (`QCC_StoreToPointer`). A narrow type or a bitfield is stored by `OP_STOREP_I8` or
    /// `OP_STOREP_I16` ([`Self::small_store`]), any other width the fatal error `E0353`
    /// "ptr-to-bitfld has unsupported bit depth". With an index on a target whose stores take no
    /// offset, the index is added to the pointer by `OP_ADD_PIW`. A struct or union is stored
    /// word by word, a word type by its `OP_STOREP_*` (`OP_STOREP_I` or else `OP_STOREP_FLD` for
    /// an integer; `OP_STOREP_I64` or else two `OP_STOREP_FLD` for a 64-bit value; `OP_STOREP_P`,
    /// `OP_STOREP_I` or else `OP_STOREP_FLD` for a pointer), each a `QCC_PR_SimpleStatement`.
    pub(super) fn store_to_pointer(
        &mut self,
        lhs: &Value,
        index: Option<&Value>,
        source: &Value,
        span: Span,
    ) -> Result<(), Fatal> {
        // `if (type->type == ev_variant) type = source.cast;`
        let ty = if self.env.shape(&lhs.ty).map(|s| s.etype) == Some(Etype::Variant) {
            &source.ty
        } else {
            &lhs.ty
        };
        let narrow = lhs.bitfield.map(|bitfield| bitfield.bits).or_else(|| {
            match self.pointee(&Type::Pointer(ty.clone().into())) {
                Pointee::Bits(bits) => u32::try_from(bits).ok(),
                Pointee::Void | Pointee::Words(_) => None,
            }
        });
        if let Some(bits) = narrow {
            let op = match bits {
                8 => Op::StorepI8,
                16 => Op::StorepI16,
                _ => {
                    let message = "ptr-to-bitfld has unsupported bit depth".to_owned();
                    return self.fatal(Code::E0353, span, message).map(drop);
                }
            };
            return self.small_store(op, source, index, span);
        }
        let target = self.env.target_at(span.start);
        let pointer = Value::of(Type::Pointer(ty.clone().into()));
        let offset = target.stores_at_offset();
        let mut index = index.cloned();
        if let Some(at) = index.take_if(|_| !offset) {
            self.emit(Op::AddPiw, &pointer, &at, span)?;
        }
        let Some(etype) = self.env.shape(ty).map(|s| s.etype) else {
            return Ok(());
        };
        let op = match etype {
            Etype::Float => Op::StorepF,
            Etype::Vector => Op::StorepV,
            Etype::Entity => Op::StorepEnt,
            Etype::String => Op::StorepS,
            Etype::Function => Op::StorepFnc,
            Etype::Field => Op::StorepFld,
            Etype::Integer | Etype::Uint if target.valid(Op::StorepI) => Op::StorepI,
            Etype::Integer | Etype::Uint => Op::StorepFld,
            Etype::Double | Etype::Int64 | Etype::Uint64 if target.valid(Op::StorepI64) => {
                Op::StorepI64
            }
            Etype::Double | Etype::Int64 | Etype::Uint64 => {
                self.simple_statement(Op::StorepFld, span)?;
                if !offset {
                    self.emit(Op::AddPiw, &pointer, &int(1), span)?;
                } else if let Some(at) = &index {
                    self.emit(Op::AddI, at, &int(1), span)?;
                }
                Op::StorepFld
            }
            Etype::Pointer if target.valid(Op::StorepP) => Op::StorepP,
            Etype::Pointer if target.valid(Op::StorepI) => Op::StorepI,
            Etype::Pointer => Op::StorepFld,
            Etype::Void
            | Etype::Variant
            | Etype::Struct
            | Etype::Union
            | Etype::Enum
            | Etype::Boolean => {
                let words = self.env.layout(ty).map_or(1, |layout| layout.words);
                return self.store_words_to_pointer(&pointer, index.as_ref(), words, span);
            }
        };
        self.simple_statement(op, span)
    }

    /// `QCC_StoreToPointer`'s `case ev_struct: case ev_union:` and its `default`: `words` words
    /// stored. On a target whose stores take an offset, `OP_STOREP_V` for each three words when
    /// it has it, then `OP_STOREP_I` (or else `OP_STOREP_F`) for each word left, each word's
    /// offset added to the index by `OP_ADD_I`; on another, the pointer offset to each by
    /// `OP_ADD_PIW` and stored by `OP_STOREP_V` for each three, `OP_STOREP_I64` for each two
    /// when it has it, and `OP_STOREP_F`.
    fn store_words_to_pointer(
        &mut self,
        pointer: &Value,
        index: Option<&Value>,
        words: u32,
        span: Span,
    ) -> Result<(), Fatal> {
        let target = self.env.target_at(span.start);
        let word = |at: u32| int(i32::try_from(at).unwrap_or(i32::MAX));
        let mut at: u32 = 0;
        if target.stores_at_offset() {
            let store = |checker: &mut Self, at: u32, op: Op| -> Result<(), Fatal> {
                if let Some(index) = index {
                    checker.emit(Op::AddI, index, &word(at), span)?;
                }
                checker.simple_statement(op, span)
            };
            if target.valid(Op::StorepV) {
                while at.saturating_add(3) <= words {
                    store(self, at, Op::StorepV)?;
                    at = at.saturating_add(3);
                }
            }
            // `for (; i+(32*2) <= type->size; i+=32*2)`: bits against words, so only a struct
            // of 64 words or more stores by `OP_STOREP_I64`.
            if target.valid(Op::StorepI64) {
                while at.saturating_mul(32).saturating_add(64) <= words {
                    store(self, at, Op::StorepI64)?;
                    at = at.saturating_add(2);
                }
            }
            let op = if target.valid(Op::StorepI) {
                Op::StorepI
            } else {
                Op::StorepF
            };
            while at < words {
                store(self, at, op)?;
                at = at.saturating_add(1);
            }
            return Ok(());
        }
        let store = |checker: &mut Self, at: u32, op: Op| -> Result<(), Fatal> {
            checker.emit(Op::AddPiw, pointer, &word(at), span)?;
            checker.simple_statement(op, span)
        };
        while at.saturating_add(2) < words {
            store(self, at, Op::StorepV)?;
            at = at.saturating_add(3);
        }
        if target.valid(Op::StorepI64) {
            while at.saturating_add(1) < words {
                store(self, at, Op::StorepI64)?;
                at = at.saturating_add(2);
            }
        }
        while at < words {
            store(self, at, Op::StorepF)?;
            at = at.saturating_add(1);
        }
        Ok(())
    }

    /// FTEF-6.18: `QCC_PR_Statement_SmallStore`, a store of 8 or 16 bits through a pointer:
    /// `op` itself when the target has it; else, for `OP_STOREP_C` the value made a `uint`
    /// first, the index (0 when there is none) split into the word, `>>` 2 or 1, and the place
    /// in it, `& 3` or `& 1`; the word read by `OP_LOADP_I`, the value masked and shifted to its
    /// place, the word's bits there cleared and the two or-ed, and the word stored by
    /// `OP_STOREP_I`, or `OP_STOREP_F` when the target lacks it.
    pub(super) fn small_store(
        &mut self,
        op: Op,
        value: &Value,
        index: Option<&Value>,
        span: Span,
    ) -> Result<(), Fatal> {
        let target = self.env.target_at(span.start);
        if target.valid(op) {
            return Ok(());
        }
        let (bits, shift, place) = if op == Op::StorepI16 {
            (16, 1, 1)
        } else {
            (8, 2, 3)
        };
        let value = if op == Op::StorepC {
            self.converted(value, Etype::Uint, span)?
        } else {
            value.clone()
        };
        let zero = uint(0);
        let index = index.unwrap_or(&zero);
        let place = self.emit(Op::BitandI, index, &uint(place), span)?;
        let word_index = self.emit(Op::RshiftI, index, &uint(shift), span)?;
        let pointer = Value::of(Type::Pointer(Type::Named("__int32".into()).into()));
        let word = self.emit(Op::LoadpI, &pointer, &word_index, span)?;
        let bit = self.emit(Op::MulI, &place, &uint(bits), span)?;
        let mask = uint(if bits == 16 { 0xffff } else { 0xff });
        let value = self.emit(Op::BitandI, &value, &mask, span)?;
        let value = self.emit(Op::LshiftI, &value, &bit, span)?;
        let mask = self.emit(Op::LshiftI, &mask, &bit, span)?;
        let mask = self.emit(Op::BitnotI, &mask, &Value::unknown(), span)?;
        let word = self.emit(Op::BitandI, &word, &mask, span)?;
        self.emit(Op::BitorI, &word, &value, span)?;
        let store = if target.valid(Op::StorepI) {
            Op::StorepI
        } else {
            Op::StorepF
        };
        self.simple_statement(store, span)
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
