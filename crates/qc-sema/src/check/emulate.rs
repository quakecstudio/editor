// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_StatementFlags

//! What an opcode becomes (FTEF-27): `QCC_PR_StatementFlags` folds its constant operands
//! (FTEF-26), emits it when the target has it (FTEF-25), and otherwise emulates it: converts
//! an operand and emits another opcode, calls a QuakeC helper the program defines, expands it
//! into other opcodes, or refuses it. Each step is an opcode of its own, folded, emitted or
//! emulated in turn.

use qc_diag::{Code, Span};
use qc_preproc::{FteFlag, Op, OpcodeTarget, TargetFormat};

use crate::check::constant::Constant;
use crate::check::etype::Etype;
use crate::check::expr::{Checker, Fatal};
use crate::check::fold::{Folded, fold};
use crate::check::opcodes::row;
use crate::check::{Constness, Value};

/// `QCTARGVER_FTE_DEF` and `QCTARGVER_FTE_MAX`: the FTE revisions `badopcode` suggests.
const FTE_DEFAULT: u32 = 5768;
/// See [`FTE_DEFAULT`].
const FTE_MAX: u32 = 6614;

/// How an opcode the target lacks is emulated (FTEF-27.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Emulation {
    /// Convert the right operand with the first opcode, then emit the second.
    ConvertRight(Op, Op),
    /// Convert the left operand with the first opcode, then emit the second.
    ConvertLeft(Op, Op),
    /// Call the helper the program defines with one of these names, else warn "NAME function
    /// not defined: cannot emulate WHAT" and refuse the opcode.
    Helper(&'static [&'static str], &'static str),
    /// Emit these opcodes on the operands in turn, the last giving the value; each is emitted
    /// or emulated in turn. For `%` when no `mod` function is defined (FTEF-27.4).
    Expand(&'static [Op]),
    /// `return QCC_PR_StatementFlags(OTHER, a, b, ...)`: this opcode instead, folded, emitted or
    /// emulated in turn.
    Instead(Op),
    /// `return QCC_PR_StatementFlags(OTHER, b, a, ...)`: this opcode on the operands swapped.
    Swapped(Op),
    /// `op = OTHER; break;`: emitted as this opcode, with no further check (FTEF-27.9).
    Replaced(Op),
    /// A bit clear, `b - (b & a)`: the store opcode when there is one and the target has it,
    /// else the `&` emitted in turn and the `-` replacing the opcode (FTEF-27.9).
    Clear {
        /// The `&` opcode.
        and: Op,
        /// The `-` opcode.
        sub: Op,
        /// The opcode that clears in place, used when the target has it.
        store: Option<Op>,
    },
    /// `QCC_SupplyConversion`'s conversions: fold a constant, else call the helper, else warn
    /// that denormals are unsafe and emit the opcode that stands for it.
    Conversion,
    /// `OP_ADD_SF`, `OP_ADD_SI` and the entity offsets: their own rules (FTEF-27.3).
    Special,
    /// Emitted as statements of another opcode with no check of the target.
    Emitted,
    /// No emulation: "Opcode ... not valid for target" (FTEF-27.5).
    Refuse,
}

impl Checker<'_> {
    /// `QCC_PR_StatementFlags` of `op` on `lhs` and `rhs` at `span`: folded (FTEF-26), else
    /// emitted when the target has it, else emulated (FTEF-27). An opaque constant operand
    /// decides nothing.
    pub(super) fn emit(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
    ) -> Result<Value, Fatal> {
        let result = replaced(op);
        if lhs.constness == Constness::Opaque || rhs.constness == Constness::Opaque {
            self.emitted = self.emitted.saturating_add(1);
            // Its value decides no folding or emulation; the comparison warnings read no value.
            if self.env.target_at(span.start).valid(op) {
                self.compare(op, lhs, rhs, span);
            }
            return Ok(Value {
                constness: Constness::Opaque,
                ..result
            });
        }
        let mut notices = Vec::new();
        let folded = fold(op, lhs.known(), rhs.known(), &mut notices);
        for notice in notices {
            self.env.report(notice.code, span, notice.message);
        }
        match folded {
            Some(Folded::Constant(constant)) => return Ok(Value::constant(constant)),
            Some(Folded::Left) => return Ok(lhs.clone()),
            Some(Folded::Right) => return Ok(rhs.clone()),
            None => {}
        }
        self.emitted = self.emitted.saturating_add(1);
        let target = self.env.target_at(span.start);
        if target.valid(op) {
            self.compare(op, lhs, rhs, span);
            return Ok(result);
        }
        self.emulate(op, lhs, rhs, span, result)
    }

    /// FTEF-27.2: the emulation of `op`, which the target lacks.
    fn emulate(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        match emulation(op) {
            Emulation::ConvertRight(conv, then) => {
                let rhs = self.emit(conv, rhs, &Value::unknown(), span)?;
                self.emit(then, lhs, &rhs, span)
            }
            Emulation::ConvertLeft(conv, then) => {
                let lhs = self.emit(conv, lhs, &Value::unknown(), span)?;
                self.emit(then, &lhs, rhs, span)
            }
            Emulation::Helper(names, what) => {
                if names.iter().any(|name| self.env.defines(name)) {
                    return Ok(result);
                }
                let name = names.first().copied().unwrap_or_default();
                let message = format!("{name} function not defined: cannot emulate {what}");
                self.env.report(Code::W0235, span, message);
                self.refuse(op, span);
                Ok(result)
            }
            Emulation::Expand(steps) => {
                let mut value = lhs.clone();
                for &step in steps {
                    value = self.emit(step, &value, rhs, span)?;
                }
                Ok(Value {
                    constness: Constness::Variable,
                    ..result
                })
            }
            Emulation::Instead(other) => Ok(Value {
                ty: result.ty,
                ..self.emit(other, lhs, rhs, span)?
            }),
            Emulation::Swapped(other) => Ok(Value {
                ty: result.ty,
                ..self.emit(other, rhs, lhs, span)?
            }),
            Emulation::Replaced(other) => Ok(replaced(other)),
            Emulation::Clear { and, sub, store } => {
                let target = self.env.target_at(span.start);
                if let Some(store) = store.filter(|&store| target.valid(store)) {
                    return Ok(replaced(store));
                }
                self.emit(and, lhs, rhs, span)?;
                Ok(replaced(sub))
            }
            Emulation::Conversion => self.conversion(op, lhs, span, result),
            Emulation::Special => self.special(op, lhs, rhs, span, result),
            Emulation::Emitted => Ok(result),
            Emulation::Refuse => {
                self.refuse(op, span);
                Ok(result)
            }
        }
    }

    /// FTEF-27.4: a conversion the target lacks: by the helper the program defines, else with
    /// the warning that denormals or big values are unsafe. With no unsigned helper,
    /// `OP_CONV_UF` is `OP_CONV_ITOF` first, then the warning.
    fn conversion(
        &mut self,
        op: Op,
        value: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        let (helpers, code, message): (&[&str], Code, String) = match op {
            Op::ConvItof | Op::StoreIf => (
                &["itof"],
                Code::W0237,
                format!("itof emulation: denormals are unsafe {}", value.label()),
            ),
            Op::ConvFtoi | Op::StoreFi => (
                &["ftoi"],
                Code::W0237,
                "ftoi emulation: denormals are unsafe".to_owned(),
            ),
            Op::ConvUf => (
                &["utof", "quake_utof", "itof", "quake_itof"],
                Code::W0238,
                format!("utof emulation: will break if {} is big", value.label()),
            ),
            _ => return Ok(result),
        };
        if helpers.iter().any(|name| self.env.defines(name)) {
            return Ok(result);
        }
        if op == Op::ConvUf {
            self.emit(Op::ConvItof, value, &Value::unknown(), span)?;
        }
        self.env.report(code, span, message);
        Ok(result)
    }

    /// FTEF-27.3: a string or entity offset.
    fn special(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        let qccx = self.env.flags_at(span.start).has(FteFlag::Qccx);
        match op {
            Op::AddSf => {
                if self.env.defines("AddStringFloat") {
                    return Ok(result);
                }
                if lhs.constness == Constness::Variable {
                    let message = "OP_ADD_SF: string+float may be unsafe".to_owned();
                    self.env.report(Code::W0236, span, message);
                }
                let rhs = self.emit(Op::ConvFtoi, rhs, &Value::unknown(), span)?;
                self.emit(Op::AddI, lhs, &rhs, span)?;
                Ok(result)
            }
            Op::AddSi | Op::AddIs => {
                let message = "OP_ADD_SI: string+int may be unsafe".to_owned();
                self.env.report(Code::W0236, span, message);
                self.emit(Op::AddI, lhs, rhs, span)?;
                Ok(result)
            }
            Op::AddEf | Op::SubEf => {
                if qccx {
                    let message =
                        "OP_ADD_EF: qccx entity offsets are unsafe, and denormals are unsafe";
                    self.env.report(Code::W0237, span, message.to_owned());
                    let then = if op == Op::AddEf { Op::AddIf } else { Op::SubF };
                    self.emit(then, lhs, rhs, span)?;
                    return Ok(result);
                }
                let rhs = self.emit(Op::ConvFtoi, rhs, &Value::unknown(), span)?;
                let then = if op == Op::AddEf {
                    Op::AddEi
                } else {
                    Op::SubEi
                };
                self.emit(then, lhs, &rhs, span)?;
                Ok(result)
            }
            Op::AddEi | Op::SubEi => {
                if qccx {
                    let message = "qccx entity offsets are unsafe".to_owned();
                    self.env.report(Code::W0235, span, message);
                } else if !self.env.defines("nextent") {
                    let message = "the nextent builtin is not defined".to_owned();
                    self.env.report(Code::W0235, span, message);
                    self.refuse(op, span);
                    return Ok(result);
                } else {
                    self.emit(Op::MulI, &Value::unknown(), rhs, span)?;
                }
                // `(op==&pr_opcodes[OP_ADD_EF])?OP_ADD_I:OP_SUB_I`: never `OP_ADD_EF` here.
                self.emit(Op::SubI, lhs, rhs, span)?;
                Ok(result)
            }
            Op::LshiftI | Op::RshiftI | Op::RshiftU => {
                let (helper, divide) = match op {
                    Op::LshiftI => ("LShiftInt", Op::MulI),
                    Op::RshiftI => {
                        let target = self.env.target_at(span.start);
                        (
                            "RShiftInt",
                            if target.valid(Op::DivU) {
                                Op::DivU
                            } else {
                                Op::DivI
                            },
                        )
                    }
                    _ => ("RShiftUInt", Op::DivU),
                };
                if self.env.defines(helper) {
                    return Ok(result);
                }
                // A shift by a constant is a product or quotient by `1 << n`; by a variable,
                // by `2 *^ n`.
                let factor = if rhs.known().is_some() {
                    Value::of(rhs.ty.clone())
                } else {
                    self.emit(Op::PowI, &Value::unknown(), rhs, span)?
                };
                if op == Op::RshiftI {
                    self.emit(Op::LtI, lhs, &Value::unknown(), span)?;
                    let flipped = self.emit(Op::BitnotI, lhs, &Value::unknown(), span)?;
                    let shifted = self.emit(divide, &flipped, &factor, span)?;
                    self.emit(Op::BitnotI, &shifted, &Value::unknown(), span)?;
                } else {
                    self.emit(divide, lhs, &factor, span)?;
                }
                Ok(result)
            }
            Op::LshiftF | Op::RshiftF => {
                let (int_op, what) = if op == Op::LshiftF {
                    (Op::LshiftI, "OP_LSHIFT_F*")
                } else {
                    (Op::RshiftI, "OP_RSHIFT_F*")
                };
                if self.env.target_at(span.start).valid(int_op) {
                    let lhs = self.emit(Op::ConvFtoi, lhs, &Value::unknown(), span)?;
                    let rhs = self.emit(Op::ConvFtoi, rhs, &Value::unknown(), span)?;
                    self.emit(int_op, &lhs, &rhs, span)?;
                    return Ok(result);
                }
                let pow = self.env.defines("powf") || self.env.defines("pow");
                if (pow && self.env.defines("floor")) || self.env.defines("bitshift") {
                    return Ok(result);
                }
                let message = format!("bitshift function not defined: cannot emulate {what}");
                self.env.report(Code::W0235, span, message);
                self.refuse(op, span);
                Ok(result)
            }
            Op::SpaceshipS => {
                if self.env.defines("strcmp") {
                    return Ok(result);
                }
                let message = "strcmp function not defined: cannot emulate string<=>string";
                self.env.report(Code::E0321, span, message.to_owned());
                Err(Fatal)
            }
            Op::AddPf
            | Op::AddFp
            | Op::AddPi
            | Op::AddIp
            | Op::AddPu
            | Op::AddUp
            | Op::SubPf
            | Op::SubPi
            | Op::SubPu => self.pointer_offset(op, lhs, rhs, span),
            Op::SubPp => self.pointer_difference(lhs, rhs, span),
            Op::AddPiw => self.word_offset(lhs, rhs, span),
            Op::LoadpU8 | Op::LoadpI8 | Op::LoadpU16 | Op::LoadpI16 => {
                self.narrow_load(op, lhs, rhs, span)
            }
            Op::LoadpF | Op::LoadpS | Op::LoadpEnt | Op::LoadpFld | Op::LoadpFnc | Op::LoadpI => {
                self.word_load(op, rhs, span, result)
            }
            Op::LtU | Op::LeU | Op::DivU | Op::BitextendI | Op::BitextendU => {
                self.unsigned(op, lhs, rhs, span, result)
            }
            Op::BitandD | Op::BitorD | Op::BitxorV | Op::BitxorD | Op::BitnotI => {
                self.bitwise(op, lhs, rhs, span, result)
            }
            Op::IfF | Op::IfnotF | Op::IfS | Op::IfnotS | Op::NotI => self.truth(op, lhs, span),
            _ => {
                self.refuse(op, span);
                Ok(result)
            }
        }
    }

    /// FTEF-27.7: the unsigned comparisons, division and sign extension.
    fn unsigned(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        match op {
            Op::LtU | Op::LeU => {
                // FTEF-27.7: as floats when the target has no integer comparison, else biased
                // by 2^31 to compare as signed integers.
                let target = self.env.target_at(span.start);
                if !target.valid(Op::LtI) {
                    let lhs = self.emit(Op::ConvUf, lhs, &Value::unknown(), span)?;
                    let rhs = self.emit(Op::ConvUf, rhs, &Value::unknown(), span)?;
                    let then = if op == Op::LtU { Op::LtF } else { Op::LeF };
                    return self.emit(then, &lhs, &rhs, span);
                }
                let bias = Value::constant(Constant::uint(0x8000_0000));
                let lhs = self.emit(Op::SubU, lhs, &bias, span)?;
                let rhs = self.emit(Op::SubU, rhs, &bias, span)?;
                let then = if op == Op::LtU { Op::LtI } else { Op::LeI };
                self.emit(then, &lhs, &rhs, span)
            }
            Op::DivU => {
                let message = "OP_DIV_U emulation: assuming the high bit is not set...";
                self.env.report(Code::W0235, span, message.to_owned());
                Ok(Value {
                    ty: result.ty,
                    ..self.emit(Op::DivI, lhs, rhs, span)?
                })
            }
            Op::BitextendI | Op::BitextendU => {
                // FTEF-27.7: by shifts, when the width and offset are a constant.
                let Some(field) = rhs.known() else {
                    self.refuse(op, span);
                    return Ok(result);
                };
                let bits = field.u() & 0xff;
                let offset = field.u() >> 8;
                let (up, down) = if op == Op::BitextendI {
                    (Op::LshiftI, Op::RshiftI)
                } else {
                    (Op::LshiftU, Op::RshiftU)
                };
                let int = |value: u32| {
                    let value = 32_u32.wrapping_sub(value);
                    Value::constant(Constant::int(i32::from_le_bytes(value.to_le_bytes())))
                };
                let high = self.emit(up, lhs, &int(bits.wrapping_add(offset)), span)?;
                self.emit(down, &high, &int(bits), span)
            }
            _ => Ok(result),
        }
    }

    /// FTEF-27.7: the bitwise opcodes of doubles, by 64-bit integers; `^` of vectors and
    /// doubles, as `(a & ~b) | (b & ~a)`; and `~` of an integer, `-1 - a` (FTEF-27.2).
    fn bitwise(
        &mut self,
        op: Op,
        lhs: &Value,
        rhs: &Value,
        span: Span,
        result: Value,
    ) -> Result<Value, Fatal> {
        match op {
            Op::BitnotI => {
                // `-1 - a`, `op` already `OP_SUB_I` when it is refused.
                if self.env.target_at(span.start).valid(Op::SubI) {
                    return Ok(replaced(Op::SubI));
                }
                if !self.env.defines("SubInt") {
                    let message = "SubInt function not defined: cannot emulate ~int";
                    self.env.report(Code::W0235, span, message.to_owned());
                    self.refuse(Op::SubI, span);
                }
                Ok(result)
            }
            Op::BitandD | Op::BitorD => {
                let lhs = self.emit(Op::ConvDi64, lhs, &Value::unknown(), span)?;
                let rhs = self.emit(Op::ConvDi64, rhs, &Value::unknown(), span)?;
                let then = if op == Op::BitandD {
                    Op::BitandI64
                } else {
                    Op::BitorI64
                };
                let value = self.emit(then, &lhs, &rhs, span)?;
                self.emit(Op::ConvI64d, &value, &Value::unknown(), span)
            }
            Op::BitxorV | Op::BitxorD => {
                // `(a & ~b) | (b & ~a)`.
                let (not, and, or) = if op == Op::BitxorV {
                    (Op::BitnotV, Op::BitandV, Op::BitorV)
                } else {
                    (Op::BitnotD, Op::BitandD, Op::BitorD)
                };
                let not_b = self.emit(not, rhs, &Value::unknown(), span)?;
                let left = self.emit(and, lhs, &not_b, span)?;
                let not_a = self.emit(not, lhs, &Value::unknown(), span)?;
                let right = self.emit(and, rhs, &not_a, span)?;
                self.emit(or, &left, &right, span)
            }
            _ => Ok(Value::unknown()),
        }
    }

    /// FTEF-27.9: a float or string tested by comparing it with 0, then the integer test; `!` of
    /// an integer as `== 0`.
    fn truth(&mut self, op: Op, lhs: &Value, span: Span) -> Result<Value, Fatal> {
        match op {
            Op::IfF | Op::IfnotF | Op::IfS | Op::IfnotS => {
                // A float or string tested by comparing it with 0, then the integer test.
                let (compare, zero) = if matches!(op, Op::IfF | Op::IfnotF) {
                    (Op::NeF, Constant::float(0.0))
                } else {
                    (Op::NeS, Constant::string(true))
                };
                self.emit(compare, lhs, &Value::constant(zero), span)?;
                Ok(replaced(if matches!(op, Op::IfF | Op::IfS) {
                    Op::IfI
                } else {
                    Op::IfnotI
                }))
            }
            Op::NotI => {
                let zero = Value::constant(Constant::int(0));
                self.emit(Op::EqFnc, lhs, &zero, span)
            }
            _ => Ok(Value::unknown()),
        }
    }

    /// FTEF-27.5: `badopcode`, "Opcode \"NAME|OPNAME\" not valid for target", suggesting the FTE
    /// target or its newest revision when either has the opcode, else "is not supported".
    pub(super) fn refuse(&mut self, op: Op, span: Span) {
        let name = row(op).map_or("", |r| r.name);
        let opname = op.name();
        let message = if OpcodeTarget::new(TargetFormat::Fte, FTE_DEFAULT).valid(op) {
            format!(
                "Opcode \"{name}|{opname}\" not valid for target. Consider the use of: #pragma \
                 target fte"
            )
        } else if OpcodeTarget::new(TargetFormat::Fte, FTE_MAX).valid(op) {
            format!(
                "Opcode \"{name}|{opname}\" not valid for target. Consider the use of: #pragma \
                 target fte_{FTE_MAX}"
            )
        } else {
            format!("Opcode \"{name}|{opname}\" is not supported.")
        };
        self.env.report(Code::E0320, span, message);
    }
}

/// FTEF-27.2: how `QCC_PR_StatementFlags`'s switch emulates `op`.
const fn emulation(op: Op) -> Emulation {
    use Emulation::{
        Conversion, ConvertLeft, ConvertRight, Emitted, Expand, Helper, Instead, Refuse, Special,
    };
    if let Some(substitution) = substitution(op) {
        return substitution;
    }
    match op {
        Op::AddSf
        | Op::AddSi
        | Op::AddIs
        | Op::AddEf
        | Op::SubEf
        | Op::AddEi
        | Op::SubEi
        | Op::SpaceshipS => Special,
        Op::BitandI => Helper(&["BitandInt"], "int&int"),
        Op::BitorI => Helper(&["BitorInt"], "int|int"),
        Op::AddI => Helper(&["AddInt"], "int+int"),
        Op::SubI => Helper(&["SubInt"], "int-int"),
        Op::MulI => Helper(&["MulInt"], "int*int"),
        Op::DivI => Helper(&["DivInt"], "int/int"),
        Op::ModV => Helper(&["ModVec"], "vector%vector"),
        Op::PowF => Helper(&["powf", "pow"], "float*^float"),
        Op::PowI => Helper(&["powf", "pow"], "int*^int"),
        Op::PowFi => Helper(&["powf", "pow"], "float*^int"),
        Op::PowIf => Helper(&["powf", "pow"], "int*^float"),
        Op::ModF => Expand(&[Op::DivF, Op::BitorF, Op::MulF, Op::SubF]),
        Op::ModI => Expand(&[Op::DivI, Op::MulI, Op::SubI]),
        Op::ModU => Expand(&[Op::DivU, Op::MulU, Op::SubU]),
        Op::ModFi => Expand(&[Op::DivFi, Op::BitorF, Op::MulIf, Op::SubF]),
        Op::ModIf => Expand(&[Op::DivIf, Op::BitorF, Op::MulF, Op::SubIf]),
        Op::BitxorF => Expand(&[Op::BitnotF, Op::BitandF, Op::BitorF]),
        Op::BitxorI => Expand(&[Op::BitnotI, Op::BitandI, Op::BitorI]),
        Op::DivVf => Expand(&[Op::DivF, Op::MulVf]),
        Op::CrossV | Op::SpaceshipF => Expand(&[Op::MulF, Op::SubF]),
        Op::ConvItof | Op::StoreIf | Op::ConvFtoi | Op::StoreFi | Op::ConvUf => Conversion,
        Op::ConvFu => Instead(Op::ConvFtoi),
        Op::LshiftDi => ConvertLeft(Op::ConvDi64, Op::LshiftI64i),
        Op::RshiftDi => ConvertLeft(Op::ConvDi64, Op::RshiftI64i),
        Op::AddPf
        | Op::AddFp
        | Op::AddPi
        | Op::AddIp
        | Op::AddPu
        | Op::AddUp
        | Op::SubPf
        | Op::SubPi
        | Op::SubPu
        | Op::SubPp
        | Op::AddPiw
        | Op::LoadpF
        | Op::LoadpS
        | Op::LoadpEnt
        | Op::LoadpFld
        | Op::LoadpFnc
        | Op::LoadpI
        | Op::LoadpU8
        | Op::LoadpI8
        | Op::LoadpU16
        | Op::LoadpI16
        | Op::LtU
        | Op::LeU
        | Op::DivU
        | Op::BitextendI
        | Op::BitextendU
        | Op::BitandD
        | Op::BitorD
        | Op::BitxorV
        | Op::BitxorD
        | Op::IfF
        | Op::IfnotF
        | Op::IfS
        | Op::IfnotS
        | Op::NotI
        | Op::BitnotI => Special,
        Op::LshiftIf => ConvertRight(Op::ConvFtoi, Op::LshiftI),
        Op::RshiftIf => ConvertRight(Op::ConvFtoi, Op::RshiftI),
        Op::LshiftFi => ConvertLeft(Op::ConvFtoi, Op::LshiftI),
        Op::RshiftFi => ConvertLeft(Op::ConvFtoi, Op::RshiftI),
        Op::LshiftF | Op::RshiftF | Op::LshiftI | Op::RshiftI | Op::RshiftU => Special,
        Op::AddFi => ConvertRight(Op::ConvItof, Op::AddF),
        Op::SubFi => ConvertRight(Op::ConvItof, Op::SubF),
        Op::BitandFi => ConvertRight(Op::ConvItof, Op::BitandF),
        Op::BitorFi => ConvertRight(Op::ConvItof, Op::BitorF),
        Op::LtFi => ConvertRight(Op::ConvItof, Op::LtF),
        Op::LeFi => ConvertRight(Op::ConvItof, Op::LeF),
        Op::GtFi => ConvertRight(Op::ConvItof, Op::GtF),
        Op::GeFi => ConvertRight(Op::ConvItof, Op::GeF),
        Op::EqFi => ConvertRight(Op::ConvItof, Op::EqF),
        Op::NeFi => ConvertRight(Op::ConvItof, Op::NeF),
        Op::DivFi => ConvertRight(Op::ConvItof, Op::DivF),
        Op::MulFi => ConvertRight(Op::ConvItof, Op::MulF),
        Op::MulVi => ConvertRight(Op::ConvItof, Op::MulVf),
        Op::AddIf => ConvertLeft(Op::ConvItof, Op::AddF),
        Op::SubIf => ConvertLeft(Op::ConvItof, Op::SubF),
        Op::BitandIf => ConvertLeft(Op::ConvItof, Op::BitandF),
        Op::BitorIf => ConvertLeft(Op::ConvItof, Op::BitorF),
        Op::LtIf => ConvertLeft(Op::ConvItof, Op::LtF),
        Op::LeIf => ConvertLeft(Op::ConvItof, Op::LeF),
        Op::GtIf => ConvertLeft(Op::ConvItof, Op::GtF),
        Op::GeIf => ConvertLeft(Op::ConvItof, Op::GeF),
        Op::EqIf => ConvertLeft(Op::ConvItof, Op::EqF),
        Op::NeIf => ConvertLeft(Op::ConvItof, Op::NeF),
        Op::DivIf => ConvertLeft(Op::ConvItof, Op::DivF),
        Op::MulIf => ConvertLeft(Op::ConvItof, Op::MulF),
        Op::MulIv => ConvertLeft(Op::ConvItof, Op::MulFv),
        Op::LeI => ConvertLeft(Op::ConvItof, Op::LeFi),
        Op::GeI => ConvertLeft(Op::ConvItof, Op::GeFi),
        Op::LtI => ConvertLeft(Op::ConvItof, Op::LtFi),
        Op::GtI => ConvertLeft(Op::ConvItof, Op::GtFi),
        Op::EqFld | Op::EqI => Instead(Op::EqFnc),
        Op::NeFld | Op::NeI => Instead(Op::NeFnc),
        // FTEF-27.6: the logic opcodes are `OP_AND_F` and `OP_OR_F` when the target has no
        // other, their operands' truth taken as FTEQCC takes it.
        Op::AndI | Op::AndFi | Op::AndIf | Op::AndAny => Instead(Op::AndF),
        Op::OrI | Op::OrFi | Op::OrIf | Op::OrAny => Instead(Op::OrF),
        // FTEF-27.7: the unsigned opcodes are the signed ones, the 64-bit pairs of words.
        Op::AddU => Instead(Op::AddI),
        Op::SubU => Instead(Op::SubI),
        Op::MulU => Instead(Op::MulI),
        Op::BitandU => Instead(Op::BitandI),
        Op::BitorU => Instead(Op::BitorI),
        Op::BitxorU => Instead(Op::BitxorI),
        Op::BitnotU => Instead(Op::BitnotI),
        Op::BitclrU => Instead(Op::BitclrI),
        Op::LshiftU => Instead(Op::LshiftI),
        Op::EqU => Instead(Op::EqI),
        Op::NeU => Instead(Op::NeI),
        Op::AddU64 => Instead(Op::AddI64),
        Op::SubU64 => Instead(Op::SubI64),
        Op::MulU64 => Instead(Op::MulI64),
        Op::BitandU64 => Instead(Op::BitandI64),
        Op::BitorU64 => Instead(Op::BitorI64),
        Op::BitxorU64 => Instead(Op::BitxorI64),
        Op::BitnotU64 => Instead(Op::BitnotI64),
        Op::BitclrU64 => Instead(Op::BitclrI64),
        Op::LshiftU64i => Instead(Op::LshiftI64i),
        Op::EqU64 => Instead(Op::EqI64),
        Op::NeU64 => Instead(Op::NeI64),
        Op::EqI64 => Expand(&[Op::EqI, Op::AndI]),
        Op::NeI64 => Expand(&[Op::NeI, Op::OrI]),
        // `QCC_PR_SimpleStatement`, which emits with no check of the target.
        Op::LoadI64
        | Op::BitandI64
        | Op::BitorI64
        | Op::BitxorI64
        | Op::BitandV
        | Op::BitorV
        | Op::StoreI64
        | Op::LoadaStruct => Emitted,
        _ => Refuse,
    }
}

/// FTEF-27.9: the opcodes `QCC_PR_StatementFlags` replaces by others: `op = OTHER; break;`,
/// emitted with no further check, a bit clear, and a comparison of the operands swapped.
const fn substitution(op: Op) -> Option<Emulation> {
    use Emulation::{Clear, Replaced, Swapped};
    Some(match op {
        Op::BitnotF => Replaced(Op::SubF),
        Op::BitnotV => Replaced(Op::SubV),
        Op::BitnotI64 => Replaced(Op::SubI64),
        Op::StoreP | Op::StoreI => Replaced(Op::StoreF),
        Op::LoadP | Op::LoadI => Replaced(Op::LoadF),
        Op::StorepP => Replaced(Op::StorepI),
        Op::AddstoreF => Replaced(Op::AddF),
        Op::AddstoreI => Replaced(Op::AddI),
        Op::AddstoreFi => Replaced(Op::AddFi),
        Op::SubstoreF => Replaced(Op::SubF),
        Op::SubstoreFi => Replaced(Op::SubFi),
        Op::SubstoreI => Replaced(Op::SubI),
        Op::MulstoreF => Replaced(Op::MulF),
        Op::MulstoreFi => Replaced(Op::MulFi),
        Op::DivstoreF => Replaced(Op::DivF),
        Op::DivstoreFi => Replaced(Op::DivFi),
        Op::DivstoreI => Replaced(Op::DivI),
        Op::AddstoreV => Replaced(Op::AddV),
        Op::SubstoreV => Replaced(Op::SubV),
        Op::MulstoreVf => Replaced(Op::MulVf),
        Op::MulstoreVi => Replaced(Op::MulVi),
        Op::BitsetstoreF => Replaced(Op::BitorF),
        Op::BitsetstoreI => Replaced(Op::BitorI),
        Op::EqP => Replaced(Op::EqE),
        Op::NeP => Replaced(Op::NeE),
        Op::GtP => Replaced(Op::GtI),
        Op::GeP => Replaced(Op::GeI),
        Op::LeP => Replaced(Op::LeI),
        Op::LtP => Replaced(Op::LtI),
        Op::BitclrF => Clear {
            and: Op::BitandF,
            sub: Op::SubF,
            store: Some(Op::BitclrstoreF),
        },
        Op::BitclrstoreF => Clear {
            and: Op::BitandF,
            sub: Op::SubF,
            store: None,
        },
        Op::BitclrI => Clear {
            and: Op::BitandI,
            sub: Op::SubI,
            store: Some(Op::BitclrstoreI),
        },
        Op::BitclrstoreI => Clear {
            and: Op::BitandI,
            sub: Op::SubI,
            store: None,
        },
        Op::BitclrV => Clear {
            and: Op::BitandV,
            sub: Op::SubV,
            store: None,
        },
        Op::BitclrD => Clear {
            and: Op::BitandD,
            sub: Op::SubD,
            store: None,
        },
        Op::BitclrI64 => Clear {
            and: Op::BitandI64,
            sub: Op::SubI64,
            store: None,
        },
        Op::GeU => Swapped(Op::LeU),
        Op::GtU => Swapped(Op::LtU),
        Op::GeU64 => Swapped(Op::LeU64),
        Op::GtU64 => Swapped(Op::LtU64),
        Op::GeI64 => Swapped(Op::LeI64),
        Op::GtI64 => Swapped(Op::LtI64),
        Op::GeD => Swapped(Op::LeD),
        Op::GtD => Swapped(Op::LtD),
        _ => return None,
    })
}

/// The conversion opcodes `QCC_TryEvaluateCast` emits to make a `from` into a `to`, in order
/// (FTEF-27.8); none when it reinterprets the value or the cast is no numeric conversion.
pub(super) fn conversion_opcodes(from: Etype, to: Etype) -> &'static [Op] {
    use Etype::{Double, Float, Int64, Integer, Uint, Uint64};
    match (from, to) {
        (Uint, Float) => &[Op::ConvUf],
        (Float, Uint) => &[Op::ConvFu],
        (Integer, Float) => &[Op::ConvItof],
        (Float, Integer) => &[Op::ConvFtoi],
        (Double, Float) => &[Op::ConvDf],
        (Float, Double) => &[Op::ConvFd],
        (Int64, Float) => &[Op::ConvI64f],
        (Float, Int64) => &[Op::ConvFi64],
        (Uint64, Float) => &[Op::ConvU64f],
        (Float, Uint64) => &[Op::ConvFu64],
        (Double, Uint64) => &[Op::ConvDu64],
        (Uint64, Double) => &[Op::ConvU64d],
        (Double, Int64) => &[Op::ConvDi64],
        (Int64, Double) => &[Op::ConvI64d],
        (Double, Uint) => &[Op::ConvDu64, Op::ConvI64i],
        (Double, Integer) => &[Op::ConvDi64, Op::ConvI64i],
        (Uint, Double) => &[Op::ConvUi64, Op::ConvI64d],
        (Integer, Double) => &[Op::ConvIi64, Op::ConvI64d],
        (Uint, Int64 | Uint64) => &[Op::ConvUi64],
        (Integer, Int64 | Uint64) => &[Op::ConvIi64],
        (Int64 | Uint64, Integer | Uint) => &[Op::ConvI64i],
        _ => &[],
    }
}

/// The value of `op` emitted: a temporary of the opcode's result type.
fn replaced(op: Op) -> Value {
    row(op).map_or_else(Value::unknown, |r| Value::of(r.types[2].value_type()))
}
