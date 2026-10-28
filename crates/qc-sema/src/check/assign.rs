// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_RefExpression, QCC_StoreSRefToRef, QCC_SupplyConversionForAssignment, opcodes_addstore

//! Assignments, `=` and the compound ones, and postfix `++` and `--`, as FTEQCC checks them
//! (FTEF-29).

use qc_diag::{Code, Span};
use qc_preproc::Op;
use qc_syntax::ast::{self, AstNode as _, BinaryOp};
use qc_syntax::c_scalar::CScalar;
use qc_syntax::{SyntaxKind, SyntaxNode};

use crate::cast::{Source, Supply};
use crate::check::constant::Constant;
use crate::check::emulate::conversion_opcodes;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::opcodes::row;
use crate::check::reference::word_type;
use crate::check::truth::Test;
use crate::check::{Constness, Reference, Value};
use crate::ty::{SimpleType, Type};

/// What an assignment operator stores (FTEF-29.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Assign {
    /// `=`: the value cast to the target's type.
    Store,
    /// A compound assignment: its spelling, the opcodes it may be given in order
    /// (`opcodes_addstore` and the others), and those that update a field or pointer in place
    /// (`opcodes_addstorep`).
    Compound(&'static str, &'static [Op], &'static [Op]),
}

/// The assignment `op` is, if it is one.
pub(super) const fn assign_spelling(op: BinaryOp) -> Option<Assign> {
    use Assign::{Compound, Store};
    Some(match op {
        BinaryOp::Assign => Store,
        BinaryOp::AddAssign => Compound(
            "+=",
            ADD,
            &[
                Op::AddstorepF,
                Op::AddstorepV,
                Op::AddstorepI,
                Op::AddstorepIf,
                Op::AddstorepFi,
            ],
        ),
        BinaryOp::SubAssign => Compound(
            "-=",
            SUB,
            &[
                Op::SubstorepF,
                Op::SubstorepV,
                Op::SubstorepI,
                Op::SubstorepIf,
                Op::SubstorepFi,
            ],
        ),
        BinaryOp::MulAssign => Compound(
            "*=",
            MUL,
            &[
                Op::MulstorepF,
                Op::MulstorepVf,
                Op::MulstorepVi,
                Op::MulstorepI,
                Op::MulstorepIf,
                Op::MulstorepFi,
            ],
        ),
        BinaryOp::DivAssign => Compound("/=", DIV, &[Op::DivstorepF]),
        BinaryOp::BitOrAssign => Compound("|=", OR, &[Op::BitsetstorepF]),
        BinaryOp::BitAndAssign => Compound("&=", AND, &[]),
        BinaryOp::BitClearAssign => Compound("&~=", CLEAR, &[Op::BitclrstorepF]),
        BinaryOp::BitXorAssign => Compound("^=", XOR, &[]),
        BinaryOp::ShlAssign => Compound("<<=", SHL, &[]),
        BinaryOp::ShrAssign => Compound(">>=", SHR, &[]),
        _ => return None,
    })
}

/// `opcodes_addstore`.
const ADD: &[Op] = &[
    Op::AddF,
    Op::AddV,
    Op::AddI,
    Op::AddU,
    Op::AddI64,
    Op::AddU64,
    Op::AddD,
    Op::AddFi,
    Op::AddIf,
    Op::AddSf,
    Op::AddPi,
    Op::AddIp,
    Op::AddPu,
    Op::AddUp,
    Op::AddPf,
    Op::AddFp,
    Op::AddSi,
    Op::AddIs,
    Op::AddEi,
    Op::AddEf,
];
/// `opcodes_substore`.
const SUB: &[Op] = &[
    Op::SubF,
    Op::SubV,
    Op::SubI,
    Op::SubU,
    Op::SubFi,
    Op::SubIf,
    Op::SubS,
    Op::SubPp,
    Op::SubPi,
    Op::SubPu,
    Op::SubPf,
    Op::SubSi,
    Op::SubEi,
    Op::SubEf,
    Op::SubI64,
    Op::SubU64,
    Op::SubD,
];
/// `opcodes_mulstore`.
const MUL: &[Op] = &[
    Op::MulF,
    Op::MulV,
    Op::MulFv,
    Op::MulIv,
    Op::MulVf,
    Op::MulVi,
    Op::MulI,
    Op::MulU,
    Op::MulFi,
    Op::MulIf,
    Op::MulI64,
    Op::MulU64,
    Op::MulD,
];
/// `opcodes_divstore`.
const DIV: &[Op] = &[
    Op::DivF,
    Op::DivI,
    Op::DivU,
    Op::DivFi,
    Op::DivIf,
    Op::DivVf,
    Op::DivI64,
    Op::DivU64,
    Op::DivD,
];
/// `opcodes_orstore`.
const OR: &[Op] = &[
    Op::BitorF,
    Op::BitorI,
    Op::BitorU,
    Op::BitorIf,
    Op::BitorFi,
    Op::BitorV,
    Op::BitorI64,
    Op::BitorU64,
    Op::BitorD,
];
/// `opcodes_andstore`.
const AND: &[Op] = &[
    Op::BitandF,
    Op::BitandI,
    Op::BitandU,
    Op::BitandIf,
    Op::BitandFi,
    Op::BitandV,
    Op::BitandI64,
    Op::BitandU64,
    Op::BitandD,
];
/// `opcodes_clearstore`.
const CLEAR: &[Op] = &[
    Op::BitclrF,
    Op::BitclrI,
    Op::BitclrU,
    Op::BitclrV,
    Op::BitclrI64,
    Op::BitclrU64,
    Op::BitclrD,
];
/// `opcodes_xorstore`.
const XOR: &[Op] = &[
    Op::BitxorI,
    Op::BitxorU,
    Op::BitxorF,
    Op::BitxorV,
    Op::BitxorI64,
    Op::BitxorU64,
    Op::BitxorD,
];
/// `opcodes_shlstore`.
const SHL: &[Op] = &[
    Op::LshiftF,
    Op::LshiftI,
    Op::LshiftU,
    Op::LshiftIf,
    Op::LshiftFi,
    Op::LshiftI64i,
    Op::LshiftU64i,
    Op::LshiftDi,
];
/// `opcodes_shrstore`.
const SHR: &[Op] = &[
    Op::RshiftF,
    Op::RshiftI,
    Op::RshiftU,
    Op::RshiftIf,
    Op::RshiftFi,
    Op::RshiftI64i,
    Op::RshiftU64i,
    Op::RshiftDi,
];

impl Checker<'_> {
    /// The assignment `binary` with the operator `op` (FTEF-29).
    pub(super) fn assign(&mut self, binary: &ast::BinaryExpr, op: BinaryOp) -> Checked {
        let Some(kind) = assign_spelling(op) else {
            return Ok(Value::unknown());
        };
        let span = binary
            .op_token()
            .map_or_else(|| binary.syntax().span(), |t| t.span());
        let in_condition = self.in_condition;
        let lhs = self.expr_of(binary.lhs())?;
        // FTEF-29.1: `lhsr->postinc` and `lhsr->readonly`, before the value is read.
        if lhs.postinc {
            return self.fatal(
                Code::E0322,
                span,
                "Assignment to post-inc result".to_owned(),
            );
        }
        if lhs.readonly && lhs.ty != Type::Unknown {
            self.env
                .report(Code::W0241, span, "Assignment to const".to_owned());
        }
        let rhs = self.value(binary.rhs())?;
        if in_condition {
            let message = "suggest parenthesis for assignment used as truth value".to_owned();
            self.env.report(Code::W0242, span, message);
        }
        if lhs.ty == Type::Unknown || rhs.ty == Type::Unknown {
            return Ok(Value::unknown());
        }
        // An array named bare is a pointer to its elements (`REF_ARRAYHEAD`).
        let target = if lhs.is_array_head() {
            Type::Pointer(lhs.ty.clone().into())
        } else {
            lhs.ty.clone()
        };
        let stored = match kind {
            Assign::Store => self.cast_to(&rhs, &target, span)?,
            Assign::Compound(spelling, ops, in_place) => {
                // `lhsd = QCC_RefToDef(lhsr, false)`: an array named bare is read as its address.
                let place = Value {
                    ty: target.clone(),
                    ..lhs.clone()
                };
                match self.compound(&place, &rhs, spelling, ops, in_place, span)? {
                    Some(value) => value,
                    None => return Ok(Value::of(lhs.ty)),
                }
            }
        };
        // FTEF-29.3: `QCC_StoreSRefToRef`.
        if lhs.readonly {
            let message = format!("Assignment to constant {}", lhs.label());
            self.env.report(Code::W0241, span, message);
        }
        self.store_mismatch(&stored, &lhs, &target, span)?;
        let packed = self
            .narrow_bitfield(&lhs, span)
            .unwrap_or_else(|| lhs.clone());
        let stored = self.bit_copy(&stored, &packed, span)?;
        self.store_reference(&lhs, &stored, span)?;
        self.not_an_lvalue(&lhs, span);
        Ok(Value {
            ty: lhs.ty,
            readonly: true,
            reference: Reference::Global,
            postinc: false,
            ..stored
        })
    }

    /// FTEF-6.15: a store to a bitfield, but through a pointer, reads the word that holds it and
    /// packs the value into it (`QCC_PR_Statement_BitCopy`): by `OP_BITCOPY_I` when the target
    /// has it, a plain statement, else the word's bits cleared by `OP_BITAND_I`, the value's
    /// masked by `OP_BITAND_I` and shifted by `OP_LSHIFT_I`, and the two joined by `OP_BITOR_I`.
    pub(super) fn bit_copy(&mut self, value: &Value, lhs: &Value, span: Span) -> Checked {
        let Some(bitfield) = lhs.bitfield else {
            return Ok(value.clone());
        };
        if matches!(lhs.reference, Reference::Pointer(_)) {
            return Ok(value.clone());
        }
        let word = Value {
            ty: word_type(&lhs.ty),
            bitfield: None,
            ..lhs.clone()
        };
        let old = self.read(word, span)?;
        if self.env.target_at(span.start).valid(Op::BitcopyI) {
            self.simple_statement(Op::BitcopyI, span)?;
            return Ok(old);
        }
        let mask = |bits: u32| Value::constant(Constant::uint(bits));
        // `(1u<<bits)-1u`, and `~(((1u<<bits)-1u)<<bitofs)` to clear the word's bits.
        let width = 1_u32
            .checked_shl(bitfield.bits)
            .map_or(u32::MAX, |b| b.wrapping_sub(1));
        let placed = width.checked_shl(bitfield.offset).unwrap_or(0);
        let cleared = self.emit(Op::BitandI, &old, &mask(!placed), span)?;
        let masked = self.emit(Op::BitandI, value, &mask(width), span)?;
        let shifted = self.emit(Op::LshiftI, &masked, &mask(bitfield.offset), span)?;
        self.emit(Op::BitorI, &cleared, &shifted, span)
    }

    /// FTEF-29.2: a compound assignment's opcode, the first of `ops` whose operands are the
    /// target's and the value's basic types, else after the value is cast to the target's type;
    /// "Type mismatch on assignment" when none. A field updated in place by an opcode of
    /// `in_place` the target has is no further checked. `None` when nothing more is checked.
    fn compound(
        &mut self,
        lhs: &Value,
        rhs: &Value,
        spelling: &str,
        ops: &[Op],
        in_place: &[Op],
        span: Span,
    ) -> Result<Option<Value>, Fatal> {
        let (Some(to), Some(from)) = (self.etype(&lhs.ty), self.etype(&rhs.ty)) else {
            return Ok(None);
        };
        let target = self.env.target_at(span.start);
        let updates_in_place = in_place.iter().any(|&op| {
            target.valid(op)
                && row(op).is_some_and(|r| {
                    r.types[1].etype() == from
                        && r.types[0].etype() == Etype::Pointer
                        && r.types[2].etype() == to
                })
        });
        if matches!(lhs.reference, Reference::Field(_)) && updates_in_place {
            return Ok(None);
        }
        // `lhsd = QCC_RefToDef(lhsr, false)`: the left side read.
        self.read(lhs.clone(), span)?;
        let find = |from: Etype| {
            ops.iter().copied().find(|&op| {
                row(op).is_some_and(|r| r.types[1].etype() == from && r.types[0].etype() == to)
            })
        };
        let (op, rhs) = match find(from) {
            Some(op) => (op, rhs.clone()),
            None => {
                let cast = self.cast_to(rhs, &lhs.ty, span)?;
                let Some(op) = self.etype(&cast.ty).and_then(find) else {
                    // FTEF-24.11: the types' own names. An array's address is `T*`
                    // (`QCC_PR_PointerType`), and a cast to the type the value has already
                    // leaves the value's.
                    let left = if lhs.is_array_head() {
                        self.env.type_name(&lhs.ty)
                    } else {
                        self.own_name(&lhs.ty)
                    };
                    let right = if rhs.ty == lhs.ty {
                        self.own_name(&rhs.ty)
                    } else {
                        left.clone()
                    };
                    let message = format!(
                        "Type mismatch on assignment. {left} {spelling} {right} is not supported"
                    );
                    return self.fatal(Code::E0323, span, message).map(Some);
                };
                (op, cast)
            }
        };
        let value = self.emit(op, lhs, &rhs, span)?;
        let flags = self.env.flags_at(span.start);
        let supply = Source::Typed(&value.ty).supply_for_assignment(&lhs.ty, flags);
        let message = || {
            format!(
                "Implicit type mismatch on assignment to {}. Needed {}, got {}.",
                lhs.ref_name(),
                self.env.type_name(&lhs.ty),
                self.env.type_name(&value.ty)
            )
        };
        match supply {
            Supply::Converted => {}
            Supply::Lax => {
                self.env.report(Code::W0243, span, message());
                return Ok(Some(value));
            }
            Supply::Explicit | Supply::Mismatch => {
                let message = message();
                return self.fatal(Code::E0324, span, message).map(Some);
            }
        }
        self.convert_to(&value, &lhs.ty, span).map(|converted| {
            Some(Value {
                ty: lhs.ty.clone(),
                ..converted
            })
        })
    }

    /// `QCC_EvaluateCast(value, to, true)`: the implicit cast's diagnostics, a refusal fatal,
    /// then the conversion opcodes it emits (FTEF-27.8); the value cast is a `to`, under `lax`
    /// too.
    pub(super) fn cast_to(&mut self, value: &Value, to: &Type, span: Span) -> Checked {
        if self.env.cast(value.source(), to, span) {
            return Err(Fatal);
        }
        let converted = self.convert_to(value, to, span)?;
        // FTEF-23.8: a value cast to a boolean is of the boolean's parent type.
        if matches!(to.fteqcc_scalar(), Some(CScalar::Bool | CScalar::BFloat)) {
            return Ok(converted);
        }
        Ok(Value {
            ty: to.clone(),
            ..converted
        })
    }

    /// `value` converted to a `to` by the conversion opcodes for their basic types, each folded,
    /// emitted or emulated (FTEF-27.8).
    pub(super) fn convert_to(&mut self, value: &Value, to: &Type, span: Span) -> Checked {
        // FTEF-23.8: a cast to a `__bint` or `__bfloat` is `QCC_PR_GenerateLogicalTruth`'s
        // comparison with zero, then that cast to the boolean's parent type.
        let parent = match to.fteqcc_scalar() {
            Some(CScalar::Bool) => Some(Type::Named(CScalar::Integer.name().into())),
            Some(CScalar::BFloat) => Some(Type::Simple(SimpleType::Float)),
            _ => None,
        };
        if let Some(parent) = parent
            && !matches!(
                value.ty.fteqcc_scalar(),
                Some(CScalar::Bool | CScalar::BFloat)
            )
            && let Some((op, zero)) = self.etype(&value.ty).and_then(|e| Test::Truth.opcode(e))
        {
            let zero = Value {
                ty: value.ty.clone(),
                ..zero.map_or_else(Value::unknown, Value::constant)
            };
            let truth = self.emit(op, value, &zero, span)?;
            return self.convert_to(&truth, &parent, span);
        }
        // FTEF-19.55: a float or an integer cast to a vector is multiplied by `'1 1 1'`
        // (`OP_MUL_FV`, `OP_MUL_IV`).
        if self.etype(to) == Some(Etype::Vector) {
            let op = match self.etype(&value.ty) {
                Some(Etype::Float) => Some(Op::MulFv),
                Some(Etype::Integer) => Some(Op::MulIv),
                _ => None,
            };
            if let Some(op) = op {
                let ones = Value::constant(Constant::vector([1.0; 3]));
                return self.emit(op, value, &ones, span);
            }
        }
        let mut converted = value.clone();
        if let (Some(from), Some(to)) = (self.etype(&value.ty), self.etype(to)) {
            for &conv in conversion_opcodes(from, to) {
                converted = self.emit(conv, &converted, &Value::unknown(), span)?;
            }
        }
        Ok(converted)
    }

    /// FTEF-29.7: `QCC_StoreSRefToRef`'s check of the value stored: unless it is null, or its type
    /// or a parent's is the target's by `typecmp_lax`, or one is a `variant` or the value a
    /// `void*` or `variant*` stored into a pointer, a number is converted to the target's type and
    /// anything else is the warning "type mismatch: A NAME to B TARGET".
    fn store_mismatch(
        &mut self,
        value: &Value,
        lhs: &Value,
        target: &Type,
        span: Span,
    ) -> Result<(), Fatal> {
        if value.known().is_some_and(Constant::null) || self.env.typecmp_lax(&value.ty, target) {
            return Ok(());
        }
        let (Some(from), Some(to)) = (self.env.shape(&value.ty), self.env.shape(target)) else {
            return Ok(());
        };
        let untyped_pointer = from.etype == Etype::Pointer
            && to.etype == Etype::Pointer
            && matches!(from.holds, Some(Etype::Void | Etype::Variant));
        if untyped_pointer || from.etype == Etype::Variant || to.etype == Etype::Variant {
            return Ok(());
        }
        let number = |etype: Etype| {
            matches!(
                etype,
                Etype::Float
                    | Etype::Integer
                    | Etype::Uint
                    | Etype::Int64
                    | Etype::Uint64
                    | Etype::Double
                    | Etype::Boolean
            )
        };
        if number(from.etype) && number(to.etype) {
            self.convert_to(value, target, span)?;
            return Ok(());
        }
        let message = format!(
            "type mismatch: {} {} to {} {}",
            self.env.type_name(&value.ty),
            value.label(),
            self.env.type_name(target),
            lhs.place()
        );
        self.env.report(Code::W0240, span, message);
        Ok(())
    }

    /// FTEF-29.5: a postfix `++` or `--`: an operand that is read-only is a fatal error, and the
    /// update is the operator's `+` or `-` of 1 of the operand's type.
    pub(super) fn postfix(&mut self, postfix: &ast::PostfixExpr) -> Checked {
        let operand = self.expr_of(postfix.operand())?;
        let increment = postfix
            .syntax()
            .child_tokens()
            .any(|t| t.kind() == qc_syntax::SyntaxKind::PlusPlus);
        let span = postfix.syntax().span();
        if operand.readonly && operand.ty != Type::Unknown {
            let message = if increment {
                "postincrement: lhs is readonly"
            } else {
                "postdecrement: lhs is readonly"
            };
            return self.fatal(Code::E0322, span, message.to_owned());
        }
        // An array named bare is its head, a pointer to its first element.
        let etype = if operand.is_array_head() {
            Some(Etype::Pointer)
        } else {
            self.etype(&operand.ty)
        };
        if let Some(etype) = etype {
            let step = if increment { 1 } else { -1 };
            self.step(&operand, etype, step, discarded(postfix.syntax()), span)?;
        }
        // The value before the update, a temporary, named by its type (`QCC_GetSRefName`); an
        // array's head is the pointer it reads as.
        let ty = if operand.is_array_head() {
            Type::Pointer(operand.ty.clone().into())
        } else {
            operand.ty.clone()
        };
        Ok(Value {
            ty,
            postinc: true,
            reference: Reference::Global,
            name: None,
            ..operand
        })
    }

    /// FTEF-29.5: the update of a postfix's operand of basic type `etype` by `step`, one of its
    /// type added by the opcode for it and stored back as an assignment is: `QCC_PR_DiscardRef`'s
    /// when its value is `discarded`, else `QCC_RefToDef`'s.
    fn step(
        &mut self,
        operand: &Value,
        etype: Etype,
        step: i8,
        discarded: bool,
        span: Span,
    ) -> Result<(), Fatal> {
        if operand
            .bitfield
            .is_some_and(|bitfield| bitfield.offset != 0)
        {
            let message = "post increment operator on bitfield".to_owned();
            return self.fatal(Code::E0322, span, message).map(drop);
        }
        let old = Value {
            constness: Constness::Variable,
            ..self.read(operand.clone(), span)?
        };
        // `switch(ref->cast->type)`: a bitfield, or an 8- or 16-bit integer, is `ev_bitfld`,
        // which only a discarded value steps, by the type it is read as.
        let narrow = matches!(
            operand.ty.fteqcc_scalar(),
            Some(CScalar::Int8 | CScalar::Uint8 | CScalar::Int16 | CScalar::Uint16)
        );
        let etype = if (operand.bitfield.is_some() || narrow) && !discarded {
            Etype::Void
        } else {
            etype
        };
        let by = |constant: Constant| Value::constant(constant);
        let updated = match etype {
            Etype::Float => self.emit(Op::AddF, &old, &by(Constant::float(step.into())), span)?,
            Etype::Double => self.emit(Op::AddD, &old, &by(Constant::double(step.into())), span)?,
            Etype::String | Etype::Integer => {
                self.emit(Op::AddI, &old, &by(Constant::int(step.into())), span)?
            }
            Etype::Uint => self.emit(Op::AddU, &old, &by(Constant::int(step.into())), span)?,
            Etype::Int64 => self.emit(Op::AddI64, &old, &by(Constant::int64(step.into())), span)?,
            Etype::Uint64 => {
                self.emit(Op::AddU64, &old, &by(Constant::int64(step.into())), span)?
            }
            Etype::Pointer => self.pointer_step(&old, step, discarded, span)?,
            Etype::Void
            | Etype::Vector
            | Etype::Entity
            | Etype::Field
            | Etype::Function
            | Etype::Variant
            | Etype::Struct
            | Etype::Union
            | Etype::Enum
            | Etype::Boolean => {
                let message = "post increment operator not supported with this type".to_owned();
                return self.fatal(Code::E0322, span, message).map(drop);
            }
        };
        // `QCC_StoreSRefToRef`: an integer stepping a string warns of the store; an array's
        // head is stored as the pointer it reads as.
        let target = if operand.is_array_head() {
            Type::Pointer(operand.ty.clone().into())
        } else {
            operand.ty.clone()
        };
        self.store_mismatch(&updated, operand, &target, span)?;
        let updated = self.bit_copy(&updated, operand, span)?;
        self.store_reference(operand, &updated, span)?;
        self.not_an_lvalue(operand, span);
        Ok(())
    }

    /// FTEF-29.3: a store to an array named bare, its head, is "left operand must be an
    /// l-value (did you mean NAME[0]?)" (`QCC_StoreSRefToRef`, `REF_ARRAYHEAD`).
    pub(super) fn not_an_lvalue(&mut self, lhs: &Value, span: Span) {
        if lhs.is_array_head() {
            let message = format!(
                "left operand must be an l-value (did you mean {}[0]?)",
                lhs.label()
            );
            self.env.report(Code::E0325, span, message);
        }
    }

    /// The basic type of `ty`, if the model follows it.
    fn etype(&self, ty: &Type) -> Option<Etype> {
        self.env.shape(ty).map(|s| s.etype)
    }
}

impl Checker<'_> {
    /// FTEF-32.4: a local's initialiser, `QCC_PR_ParseDefs`'s store to the new local: the value
    /// read and converted to the local's type, its cast reported here for an expression the
    /// resolver's initialiser check does not read (a literal, `_()`, a name, a cast of a
    /// literal or an element it reports itself, FTEF-19.31).
    pub(super) fn local(&mut self, local: &ast::LocalDecl) -> Result<(), Fatal> {
        for declarator in local.declarators() {
            self.local_size(&declarator)?;
            let Some(ast::Initializer::Value(init)) = declarator.initializer() else {
                continue;
            };
            let value = self.value(Some(init.clone()))?;
            let Some(ty) = declarator
                .name()
                .and_then(|name| self.env.local_type(name.span().start))
            else {
                continue;
            };
            if value.ty == Type::Unknown {
                continue;
            }
            let span = init.syntax().span();
            let resolver_casts = match &init {
                ast::Expr::Literal(_) | ast::Expr::Name(_) | ast::Expr::Index(_) => true,
                ast::Expr::Cast(cast) => matches!(cast.operand(), Some(ast::Expr::Literal(_))),
                ast::Expr::Call(call) => matches!(
                    call.callee(),
                    Some(ast::Expr::Name(name)) if name.name().is_some_and(|t| t.text_lossless() == "_")
                ),
                _ => false,
            };
            let value = if resolver_casts {
                self.convert_to(&value, &ty, span)?
            } else {
                self.cast_to(&value, &ty, span)?
            };
            // FTEF-2.12: an 8- or 16-bit local is initialised as a bitfield at bit 0
            // (`QCC_PR_GenerateInitializerType`), packed into its word; an array of 8-bit ones
            // by a string, a character, then zeros, to each element.
            let mut suffixes = declarator.array_suffixes();
            match (suffixes.next(), string_bytes(&init)) {
                (None, _) => {
                    if let Some(local) = self.narrow_local(&ty, 0) {
                        self.bit_copy(&value, &local, span)?;
                    }
                }
                (Some(suffix), Some(bytes)) if self.narrow_local(&ty, 0).is_some() => {
                    let size = array_size(&suffix).unwrap_or(bytes.len().saturating_add(1));
                    let mut offset: u32 = 0;
                    for at in 0..size {
                        let byte = bytes.get(at).copied().unwrap_or(0);
                        let character = Value::constant(Constant::int(i32::from(byte)));
                        if let Some(element) = self.narrow_local(&ty, offset) {
                            self.bit_copy(&character, &element, span)?;
                        }
                        offset = offset.wrapping_add(8);
                    }
                }
                (Some(_), _) => {}
            }
        }
        Ok(())
    }
}

/// Whether the value of the postfix `node` is discarded (`QCC_PR_DiscardExpression`,
/// `QCC_PR_DiscardRef`): it is a statement's expression, a `for` header's first or last clause,
/// or a comma's left side, through parentheses and a comma's right side.
fn discarded(node: &SyntaxNode) -> bool {
    let mut node = node.clone();
    while let Some(parent) = node.parent() {
        match parent.kind() {
            SyntaxKind::ParenExpr => {}
            SyntaxKind::BinaryExpr => {
                let Some(binary) = ast::BinaryExpr::cast(parent.clone()) else {
                    return false;
                };
                if binary.op() != Some(BinaryOp::Comma) {
                    return false;
                }
                if binary.lhs().is_some_and(|lhs| *lhs.syntax() == node) {
                    return true;
                }
            }
            SyntaxKind::ExprStmt => return true,
            SyntaxKind::ForClause => {
                let Some(header) = parent.parent().and_then(ast::ForStmt::cast) else {
                    return false;
                };
                return header.init().as_ref() == Some(&parent)
                    || header.step().as_ref() == Some(&parent);
            }
            _ => return false,
        }
        node = parent;
    }
    false
}

/// The bytes of the string literal `expr` as FTEQCC's lexer reads them, `pr_immediate_string`.
pub(super) fn string_bytes(expr: &ast::Expr) -> Option<Vec<u8>> {
    let ast::Expr::Literal(literal) = expr else {
        return None;
    };
    if literal.kind() != Some(ast::LiteralKind::String) {
        return None;
    }
    lexed_string(&literal.token()?.text_lossless())
}

/// The bytes of the one quoted string `text`, with its escapes read as FTEQCC's lexer reads them.
pub(super) fn lexed_string(text: &str) -> Option<Vec<u8>> {
    let lexed = qc_lexer::lex_with(text.as_bytes(), qc_syntax::Dialect::Fteqcc);
    match lexed.tokens.as_slice() {
        [token] => match &token.kind {
            qc_lexer::TokenKind::String(bytes) => Some(bytes.clone()),
            _ => None,
        },
        _ => None,
    }
}

/// The size an array suffix `[N]` writes, when N is an integer literal.
fn array_size(suffix: &SyntaxNode) -> Option<usize> {
    let Some(ast::Expr::Literal(literal)) = suffix.children().find_map(ast::Expr::cast) else {
        return None;
    };
    literal.token()?.text_lossless().parse().ok()
}
