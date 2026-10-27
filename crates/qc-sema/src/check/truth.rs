// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_GenerateLogicalTruth, QCC_PR_GenerateLogicalNot, QCC_PR_GenerateBitwiseNot, QCC_Generate_OP_IFNOT, QCC_PR_RefExpression

//! A value's truth as FTEQCC tests it (FTEF-30): `!`, `~`, a value made a boolean, the test of a
//! condition, and the conditional operator `?:` built on it.

use qc_diag::{Code, Span};
use qc_preproc::{FteFlag, Op};
use qc_syntax::ast::{self, AstNode as _};
use qc_syntax::c_scalar::CScalar;

use crate::check::Value;
use crate::check::constant::Constant;
use crate::check::emulate::conversion_opcodes;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::ty::Type;

/// Which of FTEQCC's tests of a value is made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Test {
    /// `QCC_PR_GenerateLogicalTruth`: the value compared with zero, `!= 0`.
    Truth,
    /// `QCC_PR_GenerateLogicalNot`: `!`.
    Not,
    /// `QCC_PR_GenerateBitwiseNot`: `~`.
    BitNot,
}

/// Which jump a condition is tested for: `QCC_Generate_OP_IFNOT`, taken when it is false, or
/// `QCC_Generate_OP_IF`, taken when it is true.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Jump {
    /// `QCC_Generate_OP_IFNOT`.
    IfNot,
    /// `QCC_Generate_OP_IF`, whose string warning ends with a full stop.
    If,
}

impl Test {
    /// The opcode the test is for a value of basic type `etype`, and the zero it is compared
    /// with; `None` for a type the test refuses.
    fn opcode(self, etype: Etype) -> Option<(Op, Option<Constant>)> {
        use Etype::{
            Double, Entity, Float, Function, Int64, Integer, Pointer, String, Uint, Uint64, Vector,
        };
        let int0 = Some(Constant::int(0));
        Some(match (self, etype) {
            (Self::Truth, Float) => (Op::NeF, Some(Constant::float(0.0))),
            (Self::Truth, String) => (Op::NeS, int0),
            (Self::Truth, Entity) => (Op::NeE, int0),
            (Self::Truth, Vector) => (Op::NeV, Some(Constant::vector([0.0; 3]))),
            (Self::Truth, Function) => (Op::NeFnc, int0),
            (Self::Truth, Integer | Uint | Pointer) => (Op::NeI, int0),
            (Self::Truth, Double) => (Op::NeD, Some(Constant::double(0.0))),
            (Self::Truth, Int64) => (Op::NeI64, Some(Constant::int64(0))),
            (Self::Truth, Uint64) => (Op::NeU64, Some(Constant::uint64(0))),
            (Self::Not, Float) => (Op::NotF, None),
            (Self::Not | Self::BitNot, String) => (Op::NotS, None),
            (Self::Not, Entity) => (Op::NotEnt, None),
            (Self::Not, Vector) => (Op::NotV, None),
            (Self::Not, Function) => (Op::NotFnc, None),
            (Self::Not, Integer | Uint | Pointer) => (Op::NotI, None),
            (Self::Not, Double) => (Op::EqD, Some(Constant::double(0.0))),
            (Self::Not, Int64) => (Op::EqI64, Some(Constant::int64(0))),
            (Self::Not, Uint64) => (Op::EqU64, Some(Constant::uint64(0))),
            (Self::BitNot, Float) => (Op::BitnotF, None),
            (Self::BitNot, Vector) => (Op::BitnotV, None),
            (Self::BitNot, Integer | Uint | Pointer) => (Op::BitnotI, None),
            (Self::BitNot, Double) => (Op::BitnotD, Some(Constant::double(0.0))),
            (Self::BitNot, Int64) => (Op::BitnotI64, Some(Constant::int64(0))),
            (Self::BitNot, Uint64) => (Op::BitnotU64, Some(Constant::uint64(0))),
            _ => return None,
        })
    }
}

impl Checker<'_> {
    /// FTEF-30.1: the test `test` of `value`: its opcode emitted (FTEF-27); a `void` value under
    /// `lax` the warning `message` and `OP_NE_F` or `OP_NOT_F`; any other type the fatal error
    /// `message`, `%s` its type's name.
    pub(super) fn test(&mut self, test: Test, value: &Value, span: Span, message: &str) -> Checked {
        let Some(etype) = self.env.shape(&value.ty).map(|shape| shape.etype) else {
            return Ok(Value::unknown());
        };
        if let Some((op, zero)) = test.opcode(etype) {
            let rhs = zero.map_or_else(Value::unknown, Value::constant);
            return self.emit(op, value, &rhs, span);
        }
        if etype == Etype::Void && self.env.flags_at(span.start).has(FteFlag::Laxcasts) {
            self.env
                .report(Code::W0248, span, message.replace("%s", "void"));
            let (op, rhs) = if test == Test::Truth {
                (Op::NeF, Value::constant(Constant::float(0.0)))
            } else {
                (Op::NotF, Value::unknown())
            };
            return self.emit(op, value, &rhs, span);
        }
        let message = message.replace("%s", &self.env.type_name(&value.ty));
        self.fatal(Code::E0328, span, message)
    }

    /// FTEF-30.2: `QCC_Generate_OP_IFNOT` and `QCC_Generate_OP_IF`, the test of a condition:
    /// integers, entities, fields, functions, pointers and truth values as integers; a string
    /// warns "if (string) tests for null, not empty" (with a full stop for `jump` `If`); a vector
    /// is `!v` under `ifvector`, else warns that only its first element is tested; a double or
    /// 64-bit value is compared with 0; anything else warns "conditional type mismatch".
    pub(super) fn branch(&mut self, value: &Value, jump: Jump, span: Span) -> Result<(), Fatal> {
        let Some(etype) = self.env.shape(&value.ty).map(|shape| shape.etype) else {
            return Ok(());
        };
        match etype {
            Etype::String => {
                let stop = if jump == Jump::If { "." } else { "" };
                let message = format!("if (string) tests for null, not empty{stop}");
                self.env.report(Code::W0245, span, message);
            }
            Etype::Vector if self.env.flags_at(span.start).has(FteFlag::Ifvector) => {
                self.emit(Op::NotV, value, &Value::unknown(), span)?;
            }
            Etype::Vector => {
                let message =
                    "if (vector) tests only the first element with the current compiler flags";
                self.env.report(Code::W0246, span, message.to_owned());
            }
            Etype::Double => {
                self.emit(
                    Op::NeD,
                    value,
                    &Value::constant(Constant::double(0.0)),
                    span,
                )?;
            }
            Etype::Int64 | Etype::Uint64 => {
                self.emit(Op::NeI64, value, &Value::constant(Constant::int64(0)), span)?;
            }
            Etype::Variant | Etype::Struct | Etype::Union | Etype::Void | Etype::Enum => {
                let message = format!("conditional type mismatch: {}", etype.basic_name());
                self.env.report(Code::W0247, span, message);
            }
            Etype::Entity
            | Etype::Field
            | Etype::Function
            | Etype::Pointer
            | Etype::Integer
            | Etype::Uint
            | Etype::Boolean
            | Etype::Float => {}
        }
        Ok(())
    }

    /// FTEF-30.3: `c ? a : b` and `c ?: b`. The condition is tested (FTEF-30.2); an integer arm
    /// is made a float when the target has no `OP_STORE_IF`; the arms' types are reconciled: an
    /// integer and a float are a float, an integer and an unsigned unsigned, and a null takes
    /// the other's type when they are of one size; else two types `typecmp_lax` tells apart
    /// warn "Type mismatch on ternary operator: A vs B", and two it does not are the fatal error
    /// "Ternary operator with mismatching types".
    pub(super) fn ternary(&mut self, ternary: &ast::TernaryExpr) -> Checked {
        let span = ternary.syntax().span();
        let condition = self.value(ternary.condition())?;
        self.branch(&condition, Jump::IfNot, span)?;
        self.generate(Op::IfnotI, span);
        let (middle, last) = ternary.arms();
        let (kept, kept_null) = match middle {
            None => {
                let null = condition.known().is_some_and(Constant::null);
                (condition, null)
            }
            Some(middle) => {
                let value = self.value(Some(middle))?;
                let value = self.stored_arm(value, span)?;
                let null = value.known().is_some_and(Constant::null);
                (value, null)
            }
        };
        self.generate(Self::store_op(self.etype_of(&kept.ty)), span);
        self.generate(Op::Goto, span);
        let other = self.value(last)?;
        let other = self.stored_arm(other, span)?;
        self.generate(Self::store_op(self.etype_of(&other.ty)), span);
        self.arms(kept, kept_null, other, span)
    }

    /// FTEF-30.3: the type of `?:` from its arms, `kept` the condition or middle arm and
    /// `other` the last.
    fn arms(&mut self, mut kept: Value, kept_null: bool, mut other: Value, span: Span) -> Checked {
        if kept.ty == Type::Unknown || other.ty == Type::Unknown || !self.env.fteqcc() {
            return Ok(Value::unknown());
        }
        let (Some(a), Some(b)) = (self.etype_of(&kept.ty), self.etype_of(&other.ty)) else {
            return Ok(Value::unknown());
        };
        match (a, b) {
            (Etype::Integer, Etype::Float) => kept.ty = other.ty.clone(),
            (Etype::Float, Etype::Integer | Etype::Uint) => {
                other = self.converted(&other, Etype::Float, span)?;
            }
            (Etype::Uint, Etype::Integer) | (Etype::Uint64, Etype::Int64) => {
                other.ty = kept.ty.clone();
            }
            (Etype::Integer, Etype::Uint) | (Etype::Int64, Etype::Uint64) => {
                kept.ty = other.ty.clone();
            }
            _ => {}
        }
        // `typecmp`, a truth value taken for its parent's type.
        let same = self.env.typecmp(&kept.ty, &other.ty)
            || ((is_truth(&kept.ty) || is_truth(&other.ty))
                && self.etype_of(&kept.ty) == self.etype_of(&other.ty));
        if !same {
            let other_null = other.known().is_some_and(Constant::null);
            let same_size = words(a) == words(b);
            if other_null && same_size {
                other.ty = kept.ty.clone();
            } else if kept_null && same_size {
                kept.ty = other.ty.clone();
            } else if !self.env.typecmp_lax(&other.ty, &kept.ty) {
                let message = format!(
                    "Type mismatch on ternary operator: {} vs {}",
                    self.env.type_name(&kept.ty),
                    self.env.type_name(&other.ty)
                );
                self.env.report(Code::W0244, span, message);
            } else {
                let message = "Ternary operator with mismatching types".to_owned();
                return self.fatal(Code::E0332, span, message);
            }
        }
        Ok(Value::of(kept.ty))
    }

    /// An arm of `?:` as it is stored: an integer made a float when the target has no
    /// `OP_STORE_IF` (`QCC_SupplyConversion(val, ev_float, true)`).
    fn stored_arm(&mut self, value: Value, span: Span) -> Checked {
        let target = self.env.target_at(span.start);
        if self.etype_of(&value.ty) == Some(Etype::Integer) && !target.valid(Op::StoreIf) {
            return self.converted(&value, Etype::Float, span);
        }
        Ok(value)
    }

    /// `value` converted to the basic type `to` by its conversion opcodes (FTEF-27.8), a
    /// variable of the type the last gives.
    pub(super) fn converted(&mut self, value: &Value, to: Etype, span: Span) -> Checked {
        let Some(from) = self.etype_of(&value.ty) else {
            return Ok(value.clone());
        };
        let mut converted = value.clone();
        for &conv in conversion_opcodes(from, to) {
            converted = self.emit(conv, &converted, &Value::unknown(), span)?;
        }
        Ok(converted)
    }

    /// The basic type of `ty`, if the model follows it.
    pub(super) fn etype_of(&self, ty: &Type) -> Option<Etype> {
        self.env.shape(ty).map(|shape| shape.etype)
    }
}

/// The words a value of basic type `etype` takes, `type->size`.
const fn words(etype: Etype) -> u32 {
    match etype {
        Etype::Vector => 3,
        Etype::Int64 | Etype::Uint64 | Etype::Double => 2,
        _ => 1,
    }
}

/// Whether `ty` is a truth value, `__bfloat` or `__bint`.
fn is_truth(ty: &Type) -> bool {
    matches!(ty.fteqcc_scalar(), Some(CScalar::Bool | CScalar::BFloat))
}
