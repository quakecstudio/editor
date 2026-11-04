// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_RefExpression, QCC_StoreSRefToRef, QCC_SupplyConversionForAssignment, opcodes_addstore

//! Assignments, `=` and the compound ones, and postfix `++` and `--`, as FTEQCC checks them
//! (FTEF-29).

use qc_diag::{Code, Span};
use qc_preproc::{FteFlag, Op};
use qc_syntax::ast::{self, AstNode as _, BinaryOp};
use qc_syntax::c_scalar::CScalar;
use qc_syntax::{SyntaxKind, SyntaxNode};

use crate::cast::{Source, Supply};
use crate::check::constant::Constant;
use crate::check::emulate::conversion_opcodes;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::opcodes;
use crate::check::opcodes::row;
use crate::check::reference::{eval_int, word_type};
use crate::check::truth::Test;
use crate::check::truth::words;
use crate::check::{Constness, Reference, Value};
use crate::ir;
use crate::lower::{Qualifier, declares};
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
        self.assignable(&lhs, span)?;
        #[expect(
            clippy::question_mark,
            reason = "QC-27.16: `?` adds a copy of the value to the frame each nested `=` adds"
        )]
        let rhs = match self.value(binary.rhs()) {
            Ok(rhs) => rhs,
            Err(fatal) => return Err(fatal),
        };
        self.store_assigned(binary, (kind, span, in_condition), lhs, rhs)
    }

    /// FTEF-29.1: `lhsr->postinc` and `lhsr->readonly`, before the value is read.
    fn assignable(&mut self, lhs: &Value, span: Span) -> Result<(), Fatal> {
        if lhs.postinc {
            let message = "Assignment to post-inc result".to_owned();
            return self.fatal(Code::E0322, span, message).map(drop);
        }
        if lhs.readonly && lhs.ty != Type::Unknown {
            self.env
                .report(Code::W0241, span, "Assignment to const".to_owned());
        }
        Ok(())
    }

    /// The store of an assignment's value `rhs` to `lhs`, of `kind` at `span`; a function of
    /// its own, so that what it holds is not in the frame each nested `=` adds (QC-27.16).
    #[inline(never)]
    fn store_assigned(
        &mut self,
        binary: &ast::BinaryExpr,
        (kind, span, in_condition): (Assign, Span, bool),
        lhs: Value,
        rhs: Value,
    ) -> Checked {
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
        // FTEF-29.13: a store to a definition is its `OP_STORE_*`, unless it folds into the
        // statement that made the value's temporary (`opt_assignments`).
        // DIFF-14.11: not into a statement whose result is wider than the value, a vector whose
        // first word is stored.
        let wider = self.generated.last().is_some_and(|last| {
            let made = opcodes::row(last.op).map(|row| row.types[2].etype());
            let stored = self.etype(&target);
            made.map_or(1, words) > stored.map_or(1, words)
        });
        let folded = stored.temp.is_some() && stored.temp == self.last_temp && !wider;
        if matches!(kind, Assign::Store) {
            let store = Checker::store_op(self.etype(&target));
            // The store chosen for the `=`, which the code generator reads.
            self.opcodes.insert((span.start, span.end), store);
            if lhs.reference == Reference::Global && !folded {
                self.emit(store, &stored, &Value::unknown(), span)?;
                self.name_last(store, self.name_of(&stored), lhs.name());
                // A converting store a helper emulates ends with a plain store of its value.
                match store {
                    Op::StoreIf => self.name_last(Op::StoreF, None, lhs.name()),
                    Op::StoreFi => self.name_last(Op::StoreI, None, lhs.name()),
                    _ => {}
                }
                // An assignment whose value is read is still the call's, in the return global
                // (FTE-18.13).
                if stored.returned && !self.global && !discarded(binary.syntax()) {
                    self.live_return = Some(self.env.layout(&stored.ty).map_or(1, |l| l.words));
                    self.live_call = stored.call;
                }
            }
            if lhs.reference == Reference::Global && folded {
                self.fold_into(&stored, lhs.name());
            }
        }
        if matches!(kind, Assign::Compound(..)) {
            self.definition_store(&lhs, &stored, &target, span)?;
        }
        if matches!(kind, Assign::Store | Assign::Compound(..)) {
            self.element_store(&lhs, &stored, &target, folded);
        }
        // FTEF-19.78: an element at an index that is no constant is stored by the array's
        // `ArraySet*` function: the index and the value stored to its parameters, then `CALL2`.
        if matches!(kind, Assign::Store | Assign::Compound(..)) {
            self.array_set(&lhs, &stored, &target, span)?;
        }
        self.field_store(&lhs, &stored, &target);
        if matches!(kind, Assign::Store) {
            self.pointer_store(&lhs, &stored, &target);
        }
        // A struct's or a union's store is word groups the body does not follow (FTE-14.5).
        if matches!(self.etype(&target), Some(Etype::Struct | Etype::Union)) {
            self.followed = false;
        }
        // An accessor's store, and a compound one through a pointer, are not followed.
        if matches!(lhs.reference, Reference::Accessor(_))
            || matches!(
                (&lhs.reference, kind),
                (Reference::Pointer(_), Assign::Compound(..))
            )
        {
            self.followed = false;
        }
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
        if matches!(kind, Assign::Compound(..)) {
            self.freed_again(&lhs, span);
        }
        // A store folded into the statement that made the value leaves the value in the
        // definition, which a store of the assignment's own value reads and no store folds
        // into again.
        let stored = if folded && lhs.reference == Reference::Global && !self.global {
            Value {
                sym: lhs.sym,
                temp: None,
                made: None,
                call: None,
                returned: false,
                string: None,
                ..stored
            }
        } else {
            stored
        };
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
        // `lhsd = QCC_RefToDef(lhsr, false)`: the left side read, which for a string's character
        // is the load's own type (FTEF-31.18).
        // The update stores to the element again, so its index is held.
        self.index_held = true;
        let read = self.read(lhs.clone(), span);
        self.index_held = false;
        let read = read?;
        let (to, read_ty) = match lhs.reference {
            Reference::Character(_) => (self.etype(&read.ty).unwrap_or(to), read.ty.clone()),
            _ => (to, lhs.ty.clone()),
        };
        let find = |from: Etype| {
            ops.iter().copied().find(|&op| {
                row(op).is_some_and(|r| r.types[1].etype() == from && r.types[0].etype() == to)
            })
        };
        let (op, rhs) = match find(from) {
            Some(op) => (op, rhs.clone()),
            None => {
                let cast = self.cast_to(rhs, &read_ty, span)?;
                let Some(op) = self.etype(&cast.ty).and_then(find) else {
                    // FTEF-24.11: the types' own names. An array's address is `T*`
                    // (`QCC_PR_PointerType`), and a cast to the type the value has already
                    // leaves the value's.
                    let left = if lhs.is_array_head() {
                        self.env.type_name(&read_ty)
                    } else {
                        self.own_name(&read_ty)
                    };
                    // A cast that keeps a type of its own, `OP_CONV_FU`'s, names that one.
                    let right = if rhs.ty == read_ty {
                        self.own_name(&rhs.ty)
                    } else if cast.ty == read_ty {
                        left.clone()
                    } else {
                        self.own_name(&cast.ty)
                    };
                    let message = format!(
                        "Type mismatch on assignment. {left} {spelling} {right} is not supported"
                    );
                    return self.fatal(Code::E0323, span, message).map(Some);
                };
                (op, cast)
            }
        };
        // The opcode reads the value read, `lhsd`, and frees its temporary (FTEF-27.16).
        let read = Value {
            ty: read_ty.clone(),
            ..read
        };
        // The opcode on the value read and the right side is a fragment of the intermediate
        // layer, by the operator's span, as a binary operator's is, and the opcode the one chosen
        // there (FTEF-23.4).
        self.opcodes.insert((span.start, span.end), op);
        let value = self
            .lowering(span, &read, &rhs, |checker, read, rhs| {
                checker.emit(op, read, rhs, span).map(Some)
            })?
            .unwrap_or_else(Value::unknown);
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
        if self.env.cast(self.cast_source(value), to, span) {
            return Err(Fatal);
        }
        let converted = self.convert_to(value, to, span)?;
        // FTEF-23.8: a value cast to a boolean is of the boolean's parent type.
        if matches!(to.fteqcc_scalar(), Some(CScalar::Bool | CScalar::BFloat)) {
            return Ok(converted);
        }
        // FTEF-31.18: a float cast to an unsigned integer is what `OP_CONV_FU` gives, which
        // `QCC_TryEvaluateCast` does not recast.
        if self.etype(&value.ty) == Some(Etype::Float) && self.etype(to) == Some(Etype::Uint) {
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
        if let Some(parent) = parent.clone()
            && let Some((op, zero)) = self.etype(&value.ty).and_then(|e| Test::Truth.opcode(e))
        {
            let zero = Value {
                ty: value.ty.clone(),
                ..zero.map_or_else(Value::unknown, Value::constant)
            };
            let truth = self.emit(op, value, &zero, span)?;
            return self.convert_to(&truth, &parent, span);
        }
        // `t == ev_void && flag_laxcasts`: "cast to boolean", then `OP_NE_F` (FTEF-23.11).
        if let Some(parent) = parent
            && self.etype(&value.ty) == Some(Etype::Void)
            && self.env.flags_at(span.start).has(FteFlag::Laxcasts)
        {
            let truth = self.test(Test::Truth, value, span, "cast to boolean")?;
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
        let mut copy = None;
        if let Some(etype) = etype {
            let step = if increment { 1 } else { -1 };
            copy = self.step(&operand, etype, step, discarded(postfix.syntax()), span)?;
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
            made: copy.or(operand.made),
            ..operand
        })
    }

    /// FTEF-29.5: the update of a postfix's operand of basic type `etype` by `step`, one of its
    /// type added by the opcode for it and stored back as an assignment is: `QCC_PR_DiscardRef`'s
    /// when its value is `discarded`, else `QCC_RefToDef`'s; the generated copy of the old
    /// value, if one was made.
    fn step(
        &mut self,
        operand: &Value,
        etype: Etype,
        step: i8,
        discarded: bool,
        span: Span,
    ) -> Result<Option<u32>, Fatal> {
        if operand
            .bitfield
            .is_some_and(|bitfield| bitfield.offset != 0)
        {
            let message = "post increment operator on bitfield".to_owned();
            return self.fatal(Code::E0322, span, message).map(|_| None);
        }
        self.index_held = true;
        let old = self.read(operand.clone(), span);
        self.index_held = false;
        let old = Value {
            constness: Constness::Variable,
            ..old?
        };
        // FTEF-29.13: a value the postfix gives is the old one, copied to a temporary first.
        let mut copy = None;
        // The old value is copied first: a definition's, and an element's the getter left in
        // the return global.
        if !discarded && (operand.reference == Reference::Global || old.returned) {
            let store = Checker::store_op(self.etype(&operand.ty));
            self.emit(store, &old, &Value::unknown(), span)?;
            copy = self
                .generated
                .last()
                .filter(|statement| statement.op == store)
                .and(self.last_made());
            self.name_last(store, self.name_of(&old), copy.map(ir::Name::Made));
        }
        // The step adds to the copy.
        let old = Value {
            made: copy.or(old.made),
            ..old
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
            Etype::Float => {
                // The step's opcode, which the code generator reads.
                self.opcodes.insert((span.start, span.end), Op::AddF);
                self.emit(Op::AddF, &old, &by(Constant::float(step.into())), span)?
            }
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
                return self.fatal(Code::E0322, span, message).map(|_| None);
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
        self.update_stores(operand, &updated, &target, span)?;
        let updated = self.bit_copy(&updated, operand, span)?;
        self.store_reference(operand, &updated, span)?;
        self.not_an_lvalue(operand, span);
        self.freed_again(operand, span);
        Ok(copy)
    }

    /// FTE-13.1: `vector v = {x, y, z};` of a local not `static`, each element read and
    /// stored to its component in turn by `STORE_F`, as `QCC_PR_ParseInitializerType` stores an
    /// aggregate's elements; any other aggregate is not followed.
    fn vector_aggregate(
        &mut self,
        declarator: &ast::Declarator,
        aggregate: &ast::AggregateInit,
    ) -> Result<(), Fatal> {
        let Some(name) = declarator.name() else {
            return Ok(());
        };
        let start = name.span().start;
        let vector = self
            .env
            .local_type(start)
            .is_some_and(|ty| self.etype(&ty) == Some(Etype::Vector));
        let elements: Vec<ast::Expr> = aggregate.elements().collect();
        if self.global {
            return Ok(());
        }
        let local = self.env.reference(start);
        let array = local.is_some_and(|def| self.env.is_array(def));
        let nested = aggregate.syntax().children().count() != elements.len();
        if array && !vector && !nested && !self.env.local_static(start) {
            return self.array_aggregate(declarator, local, &elements);
        }
        if !vector || self.env.local_static(start) || elements.len() != 3 {
            self.followed = false;
            return Ok(());
        }
        let local = self.env.reference(start);
        for (word, element) in (0_u32..).zip(elements) {
            let value = self.value(Some(element))?;
            self.generated.push(ir::BodyStatement {
                a: self.name_of(&value),
                b: local.map(|def| ir::Name::Def(def, word)),
                ..ir::BodyStatement::bare(Op::StoreF)
            });
            self.read_returned(&value);
            self.last_temp = None;
        }
        Ok(())
    }

    /// FTE-6.4: the store of `stored` to the field `lhs` of type `target`, a compound one's
    /// and a postfix's too: its `ADDRESS`, made after the value, and `STOREP_*` of the field's
    /// type through it.
    fn field_store(&mut self, lhs: &Value, stored: &Value, target: &Type) {
        let Reference::Field(field) = &lhs.reference else {
            return;
        };
        if self.global {
            return;
        }
        let storep = self.storep_op(target);
        let address = u32::try_from(self.generated.len()).ok().map(ir::Name::Made);
        self.generated.push(ir::BodyStatement {
            a: self.copied(field.entity, field.entity_call),
            b: field.def.map(|def| ir::Name::Def(def, 0)),
            c: address,
            ..ir::BodyStatement::bare(Op::Address)
        });
        self.generated.push(ir::BodyStatement {
            a: self.name_of(stored),
            b: address,
            ..ir::BodyStatement::bare(storep)
        });
        self.read_returned(stored);
        self.last_temp = None;
    }

    /// The store an update, a postfix's or a prefix's, makes of `updated` to `operand` of type
    /// `target`: by the array's setter, through the field's address, to the definition, or to
    /// a constant-index element, folding into the statement that made the value where it can.
    pub(super) fn update_stores(
        &mut self,
        operand: &Value,
        updated: &Value,
        target: &Type,
        span: Span,
    ) -> Result<(), Fatal> {
        self.array_set(operand, updated, target, span)?;
        self.field_store(operand, updated, target);
        let folded = updated.temp.is_some() && updated.temp == self.last_temp;
        self.definition_store(operand, updated, target, span)?;
        self.element_store(operand, updated, target, folded);
        Ok(())
    }

    /// FTEF-29.13: the store of `stored` to the definition `lhs` of type `target` by a compound
    /// assignment or a postfix: folded into the statement that made the value
    /// (`opt_assignments`), else its `OP_STORE_*`.
    fn definition_store(
        &mut self,
        lhs: &Value,
        stored: &Value,
        target: &Type,
        span: Span,
    ) -> Result<(), Fatal> {
        if self.global || lhs.reference != Reference::Global {
            return Ok(());
        }
        if stored.temp.is_some() && stored.temp == self.last_temp {
            self.fold_into(stored, lhs.name());
            return Ok(());
        }
        let store = Checker::store_op(self.etype(target));
        self.emit(store, stored, &Value::unknown(), span)?;
        self.name_last(store, self.name_of(stored), lhs.name());
        Ok(())
    }

    /// FTEF-31.10: the store of `stored` to the element `lhs` at a constant index, a word of
    /// the array's definition, stored as a definition is unless the value folds into the
    /// statement that made it (`folded`).
    fn element_store(&mut self, lhs: &Value, stored: &Value, target: &Type, folded: bool) {
        let Reference::Element(element) = &lhs.reference else {
            return;
        };
        let Some(index) = element.index.known() else {
            return;
        };
        if self.global {
            return;
        }
        // The element's word, as a read of it at a constant index names it.
        let word = eval_int(index).map(|word| u32::from_le_bytes(word.to_le_bytes()));
        let to = word.and_then(|word| {
            Value {
                sym: lhs.sym.at(word),
                ..Value::of(Type::Unknown)
            }
            .name()
        });
        if folded {
            self.fold_into(stored, to);
        } else {
            self.generated.push(ir::BodyStatement {
                a: self.name_of(stored),
                b: to,
                ..ir::BodyStatement::bare(Checker::store_op(self.etype(target)))
            });
            self.read_returned(stored);
            self.last_temp = None;
        }
    }

    /// `STOREP_*` of a value of type `target`, by its basic type.
    fn storep_op(&self, target: &Type) -> Op {
        match self.etype(target) {
            Some(Etype::Vector) => Op::StorepV,
            Some(Etype::String) => Op::StorepS,
            Some(Etype::Entity) => Op::StorepEnt,
            Some(Etype::Field) => Op::StorepFld,
            Some(Etype::Function) => Op::StorepFnc,
            _ => Op::StorepF,
        }
    }

    /// FTEF-31.3: the store of `stored` through the pointer `lhs`, `*p = v`, of type `target`:
    /// `STOREP_*` of the value through the pointer.
    fn pointer_store(&mut self, lhs: &Value, stored: &Value, target: &Type) {
        let Reference::Pointer(pointer) = &lhs.reference else {
            return;
        };
        if self.global {
            return;
        }
        if pointer.index.is_some() || pointer.named.is_none() {
            self.followed = false;
            return;
        }
        let storep = self.storep_op(target);
        self.generated.push(ir::BodyStatement {
            a: self.name_of(stored),
            b: pointer.named,
            ..ir::BodyStatement::bare(storep)
        });
        self.read_returned(stored);
        self.last_temp = None;
    }

    /// A local array's aggregate initialiser: a constant one is its value at compile time, as a
    /// global's is (FTEF-32.13), which the body does not follow; any other stores each element's
    /// value to its element in turn by the `STORE_*` of its type, or folds it into the statement
    /// that made it (FTEF-32.15, FTEF-29.13).
    fn array_aggregate(
        &mut self,
        declarator: &ast::Declarator,
        local: Option<crate::DefId>,
        elements: &[ast::Expr],
    ) -> Result<(), Fatal> {
        let constant = declarator
            .syntax()
            .parent()
            .is_some_and(|decl| declares(&decl, &Qualifier::Const));
        // Its words are set where the code generator declares the local, which the writer does
        // not do yet.
        if constant {
            self.followed = false;
            return Ok(());
        }
        let Some(ty) = declarator
            .name()
            .and_then(|name| self.env.local_type(name.span().start))
        else {
            self.followed = false;
            return Ok(());
        };
        let width = self.env.layout(&ty).map_or(1, |l| l.words);
        let store = Checker::store_op(self.etype(&ty));
        for (at, element) in (0_u32..).zip(elements) {
            let value = self.value(Some(element.clone()))?;
            // An element of another type is converted as it is stored, which the body does not
            // follow; the check reports one no cast takes.
            if self.etype(&value.ty) != self.etype(&ty) {
                self.followed = false;
            }
            let to = local.map(|def| ir::Name::Def(def, at.saturating_mul(width)));
            let wider = self.generated.last().is_some_and(|last| {
                let made = opcodes::row(last.op).map(|row| row.types[2].etype());
                made.map_or(1, words) > self.etype(&ty).map_or(1, words)
            });
            if value.temp.is_some() && value.temp == self.last_temp && !wider {
                self.fold_into(&value, to);
            } else {
                self.generated.push(ir::BodyStatement {
                    a: self.name_of(&value),
                    b: to,
                    ..ir::BodyStatement::bare(store)
                });
            }
            self.read_returned(&value);
            self.last_temp = None;
        }
        Ok(())
    }

    /// A store's read of `value`: a call's value is then out of the return global's keeping.
    fn read_returned(&mut self, value: &Value) {
        if value.returned {
            self.live_return = None;
            self.live_call = None;
        }
    }

    /// FTEF-19.78: the store of `stored` to the element `lhs` at an index that is no constant,
    /// by the array's `ArraySet*` function: the index stored to its first parameter unless it
    /// folds, the value to its second, then `CALL2`.
    fn array_set(
        &mut self,
        lhs: &Value,
        stored: &Value,
        target: &Type,
        span: Span,
    ) -> Result<(), Fatal> {
        let Reference::Element(element) = &lhs.reference else {
            return Ok(());
        };
        if element.index.known().is_some() || self.global {
            return Ok(());
        }
        if element.vector {
            self.followed = false;
            return Ok(());
        }
        let parameter = |slot| Some(ir::Name::Parameter(slot));
        // A vector array's setter takes the element's index, its words' divided by 3.
        let index = if self.etype(target) == Some(Etype::Vector) {
            let three = Value::constant(Constant::float(3.0));
            self.emit(Op::DivF, &element.index, &three, span)?
        } else {
            element.index.clone()
        };
        if index.temp.is_some() && index.temp == self.last_temp {
            self.fold_into(&index, parameter(0));
        } else {
            self.generated.push(ir::BodyStatement {
                a: self.name_of(&index),
                b: parameter(0),
                ..ir::BodyStatement::bare(Op::StoreF)
            });
            self.read_returned(&index);
        }
        let words = self.env.layout(target).map_or(1, |l| l.words);
        self.generated.push(ir::BodyStatement {
            a: self.name_of(stored),
            b: parameter(1),
            ..ir::BodyStatement::bare(if words == 3 { Op::StoreV } else { Op::StoreF })
        });
        self.clobber_return();
        self.generated.push(ir::BodyStatement {
            a: lhs.sym.def().map(ir::Name::ArraySet),
            ..ir::BodyStatement::bare(Op::Call2)
        });
        self.last_temp = None;
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

    /// FTEF-29.12: a compound assignment or an increment of an array named bare, after
    /// [`Checker::not_an_lvalue`]: `QCC_PR_DiscardRef` has freed the head it read, which is then
    /// freed again, "INTERNAL: over-freed refcount to NAME" (`QCC_FreeTemp`).
    /// A struct's member array frees its offset constants too, which the model does not follow.
    pub(super) fn freed_again(&mut self, lhs: &Value, span: Span) {
        if lhs.reference == Reference::ArrayHead(None) {
            let message = format!("INTERNAL: over-freed refcount to {}", lhs.label());
            self.env.report(Code::W0319, span, message);
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
            let checked = self.local_declarator(&declarator);
            let stray = self.stray.take();
            checked?;
            // FTEF-18.38: the definition ends at the `[`, which starts a statement of its own.
            if let Some((span, _)) = stray {
                let message = "Missing semicolon at end of definition".to_owned();
                self.env.report(Code::W0203, span, message);
                let message = "Statement does not do anything".to_owned();
                self.env.report(Code::W0313, span, message);
                return Err(Fatal);
            }
        }
        Ok(())
    }

    /// [`Checker::local`] of one declarator: its size, then its value cast to its type.
    fn local_declarator(&mut self, declarator: &ast::Declarator) -> Result<(), Fatal> {
        // The local is declared before its value is read, where the next statement goes.
        if let Some(def) = declarator
            .name()
            .and_then(|name| self.env.reference(name.span().start))
        {
            let at = self.next_made();
            self.locals.push((at, def));
            self.effects
                .push((at, ir::Effect::Make(ir::Name::Def(def, 0))));
        }
        self.local_size(declarator)?;
        match declarator.initializer() {
            Some(ast::Initializer::Aggregate(aggregate)) => {
                return self.vector_aggregate(declarator, &aggregate);
            }
            // A nested function and a builtin local are compiled from the syntax tree.
            Some(ast::Initializer::Body(_) | ast::Initializer::Builtin(_)) => {
                self.followed = false;
            }
            Some(ast::Initializer::Value(_)) | None => {}
        }
        let Some(ast::Initializer::Value(init)) = declarator.initializer() else {
            return Ok(());
        };
        let value = self.value(Some(init.clone()))?;
        let Some(ty) = declarator
            .name()
            .and_then(|name| self.env.local_type(name.span().start))
        else {
            return Ok(());
        };
        if value.ty == Type::Unknown {
            return Ok(());
        }
        // A struct's or a union's initialiser of its own type is copied a word at a time by
        // `STORE_FLD`, the words being fields of the struct to `QCC_PR_ParseInitializerType`;
        // any other is not followed.
        if matches!(self.etype(&ty), Some(Etype::Struct | Etype::Union)) {
            let local = declarator
                .name()
                .filter(|name| !self.env.local_static(name.span().start))
                .and_then(|name| self.env.reference(name.span().start));
            // The value a definition holds, whose words are its own; a struct read word by word,
            // through an array's getter, is not followed yet.
            let from = self
                .name_of(&value)
                .filter(|name| matches!(name, ir::Name::Def(..)));
            if let Some(local) = local.filter(|_| self.env.typecmp(&value.ty, &ty) && !self.global)
                && from.is_some()
                && declarator.array_suffixes().next().is_none()
            {
                let words = self.env.layout(&ty).map_or(0, |l| l.words);
                for word in 0..words {
                    self.generated.push(ir::BodyStatement {
                        a: from.and_then(|name| name.word(word)),
                        b: Some(ir::Name::Def(local, word)),
                        ..ir::BodyStatement::bare(Op::StoreFld)
                    });
                }
                if value.returned {
                    self.live_return = None;
                }
                self.last_temp = None;
                return Ok(());
            }
            self.followed = false;
        }
        let span = init.syntax().span();
        let resolver_casts = match &init {
            ast::Expr::Literal(_) | ast::Expr::Name(_) => true,
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
        // FTEF-19.70: a `static` local's value is a constant, which the resolver checks of a
        // name and an element of one.
        let element = matches!(
            &init,
            ast::Expr::Index(index)
                if matches!(index.base(), Some(ast::Expr::Name(base)) if base.scope().is_none())
        );
        if let Some(name) = declarator.name()
            && !resolver_casts
            && !element
            && self.env.local_static(name.span().start)
        {
            self.constant_initialiser(&value, &name.text_lossless(), span);
        }
        // FTEF-29.13: a local's value is stored to it unless it folds into the statement that
        // made it (`opt_assignments`); a `static` one is its initial value instead.
        let is_static = declarator
            .name()
            .is_some_and(|name| self.env.local_static(name.span().start));
        if !self.global && !is_static && declarator.array_suffixes().next().is_none() {
            if value.returned {
                self.live_return = None;
            }
            let folded = value.temp.is_some() && value.temp == self.last_temp;
            if folded {
                let local = declarator
                    .name()
                    .and_then(|name| self.env.reference(name.span().start))
                    .map(|def| ir::Name::Def(def, 0));
                self.fold_into(&value, local);
            }
            if !folded {
                let local = declarator
                    .name()
                    .and_then(|name| self.env.reference(name.span().start))
                    .map(|def| ir::Name::Def(def, 0));
                self.generated.push(ir::BodyStatement {
                    a: self.name_of(&value),
                    b: local,
                    ..ir::BodyStatement::bare(Checker::store_op(self.etype(&ty)))
                });
                self.last_temp = None;
            }
        }
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
    lexed_string(literal.token()?.text())
}

/// The bytes of the one quoted string `text`, with its escapes read as FTEQCC's lexer reads them.
pub(super) fn lexed_string(text: &[u8]) -> Option<Vec<u8>> {
    let lexed = qc_lexer::lex_with(text, qc_syntax::Dialect::Fteqcc);
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
