// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_RefExpression, QCC_StoreSRefToRef, QCC_StoreToSRef, QCC_StoreToPointer, QCC_PR_GenerateInitializerType, QCC_PR_StatementFlags, QCC_SupplyConversionForAssignment, opcodes_addstore

//! Assignments, `=` and the compound ones, and postfix `++` and `--`, as FTEQCC checks them
//! (FTEF-29).

use qc_diag::{Code, Span};
use qc_preproc::FteFlag;
use qc_progs::Op;
use qc_syntax::ast::{self, AstNode as _, BinaryOp};
use qc_syntax::c_scalar::CScalar;
use qc_syntax::{SyntaxKind, SyntaxNode};

use crate::cast::{Source, Supply};
use crate::check::call::call_opcode;
use crate::check::constant::Constant;
use crate::check::emulate::conversion_opcodes;
use crate::check::etype::Etype;
use crate::check::expr::{Checked, Checker, Fatal};
use crate::check::member::WORD_ALIGNMENT;
use crate::check::opcodes;
use crate::check::opcodes::row;
use crate::check::reference::word_type;
use crate::check::truth::{Test, is_truth};
use crate::check::{Constness, Fold, Reference, Sym, Value, ValueUse};
use crate::ir;
use crate::lower::{Qualifier, declares};
use crate::ty::{SimpleType, Type};
use crate::{VECTOR_WORDS, VECTOR_WORDS_FLOAT};

/// What a compound assignment updates in place through an address (FTEF-29.17, FTEF-29.22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Through {
    /// An entity's field, through `OP_ADDRESS` of the two.
    Field,
    /// What a pointer points at.
    Pointer,
    /// Neither: the left side is read, the operator applied and the result stored.
    Nothing,
}

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
        self.store_assigned(binary, (kind, span, in_condition), lhs, &rhs)
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
        rhs: &Value,
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
            Assign::Store => self.cast_to(rhs, &target, span)?,
            Assign::Compound(spelling, ops, in_place) => {
                // `lhsd = QCC_RefToDef(lhsr, false)`: an array named bare is read as its address.
                let place = Value {
                    ty: target.clone(),
                    ..lhs.clone()
                };
                match self.compound(&place, rhs, spelling, ops, in_place, span)? {
                    Compounded::Store(value) => value,
                    Compounded::InPlace(value) => return Ok(value),
                    Compounded::Unchecked => return Ok(Value::of(lhs.ty)),
                }
            }
        };
        // FTEF-29.21: `QCC_StoreSRefToRef` converts a number that is not of the target's type
        // before it stores it, a truth value's by its comparison with zero.
        let stored = match kind {
            Assign::Store => self.store_converted(stored, &target, span)?,
            Assign::Compound(..) => stored,
        };
        // FTEF-29.13: a store to a definition is its `OP_STORE_*`, unless it folds into the
        // statement that made the value's temporary (`opt_assignments`).
        // DIFF-14.11: not into a statement whose result is wider than the value, a vector whose
        // first word is stored.
        let wider = self.generated.last().is_some_and(|last| {
            let made = opcodes::row(last.op).map(|row| row.types[2].etype());
            let stored = self.etype(&target);
            made.map_or(1, Etype::words) > stored.map_or(1, Etype::words)
        });
        let folded = stored.temp.is_some() && stored.temp == self.last_temp && !wider;
        let structured = matches!(self.etype(&target), Some(Etype::Struct | Etype::Union));
        // `QCC_StoreToSRef`'s `default:`: a type it has no case for is copied by its size.
        let by_size = self.copied_by_size(&target);
        // FTEF-6.15: a bitfield, or an 8- or 16-bit element at a constant index, is stored by
        // packing the value into its word, which is the only statement that writes it.
        let packs = lhs.packs();
        // FTE-14.6, FTEF-6.31: a struct stored to a definition is copied piece by piece from a
        // definition's or a block's words, and so is any other type copied by its size whose
        // store folds into no statement.
        // A store that packs its value into a word is no store of the member's own type.
        let steps = self.store_steps(by_size, &target, span);
        let copied = matches!(kind, Assign::Store)
            && !packs
            && lhs.reference == Reference::Global
            && (structured || !folded)
            && (by_size.is_some() || steps.len() > 1)
            && self.copy_steps(&stored, lhs.name(), &steps);
        if matches!(kind, Assign::Store) && !copied && !packs {
            // The store chosen for the `=`, which the code generator reads: its first
            // statement's.
            let store = steps.first().map_or(Op::StoreF, |&(op, _)| op);
            self.opcodes.insert((span.start, span.end), store);
            if lhs.reference == Reference::Global && !folded {
                // More statements than one whose words the body could not name.
                if steps.len() > 1 {
                    self.followed = false;
                }
                self.emit(store, &stored, &Value::unknown(), span)?;
                let source = self.store_source(store, &stored);
                self.name_last(store, source, lhs.name());
                // A converting store a helper emulates ends with a plain store of its value.
                match store {
                    Op::StoreIf => self.name_last(Op::StoreF, None, lhs.name()),
                    Op::StoreFi => self.name_last(Op::StoreI, None, lhs.name()),
                    _ => {}
                }
                // An assignment whose value is read is still the call's, in the return global
                // (FTE-18.13).
                if stored.returned && !self.global && value_use(binary.syntax()) == ValueUse::Read {
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
        let element = if matches!(kind, Assign::Store | Assign::Compound(..)) && !packs {
            let fold = if folded { Fold::Folded } else { Fold::Stored };
            self.element_store(&lhs, &stored, (&target, span), fold)
        } else {
            ElementStore::NotOne
        };
        // FTEF-19.78: an element at an index that is no constant is stored by the array's
        // `ArraySet*` function: the index and the value stored to its parameters, then `CALL2`.
        if matches!(kind, Assign::Store | Assign::Compound(..)) {
            self.array_set(&lhs, &stored, &target, span)?;
        }
        self.field_store(&lhs, &stored, &target, span);
        if matches!(kind, Assign::Store | Assign::Compound(..)) {
            self.pointer_store(&lhs, &stored, &target, span)?;
        }
        // A struct stored through a pointer, or through an array's address at an index that is
        // no constant, is written word by word or refused where it is stored (FTEF-6.18,
        // FTEF-31.20). Any other store of a struct or a union is word groups the body does
        // not follow (FTE-14.5).
        // At such an index on a target with no address to store through, the struct is the
        // array's setter's argument (FTEF-32.27, FTEF-32.25).
        let by_address = match &lhs.reference {
            Reference::Pointer(_) => true,
            Reference::Element(at) => at.index.known().is_none(),
            _ => false,
        };
        if structured && !copied && element == ElementStore::NotOne && !by_address {
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
        if packs && !self.global {
            // A compound assignment of a packed value is not followed.
            if !matches!(kind, Assign::Store) {
                self.followed = false;
            }
            self.pack_into_word(&packed, &stored, span);
        }
        self.store_reference(&lhs, &stored, span)?;
        self.not_an_lvalue(&lhs, span);
        if matches!(kind, Assign::Compound(..)) {
            self.freed_again(&lhs, span);
        }
        // A store folded into the statement that made the value leaves the value in the
        // definition, which a store of the assignment's own value reads and no store folds
        // into again.
        // The definition is the target's own, or the word of it an element at a constant index
        // or a vector's component is: a fold into `v[1]` leaves the value in `v_y`, not in
        // the temporary the statement would have made.
        let place = match &lhs.reference {
            Reference::Global => Some(lhs.sym),
            Reference::Element(_) => lhs
                .definition_place()
                .and_then(|place| place.sym())
                .filter(|sym| matches!(sym, Sym::Def(..))),
            _ => None,
        };
        let stored = if let (true, Some(sym), false) = (folded, place, self.global) {
            Value {
                sym,
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
        // FTEF-6.15: the value of a packed store is the bitfield, read from its word again.
        if packs {
            return Ok(Value {
                readonly: true,
                postinc: false,
                ..lhs
            });
        }
        Ok(Value {
            ty: lhs.ty,
            readonly: true,
            reference: Reference::Global,
            postinc: false,
            ..stored
        })
    }

    /// FTEF-6.15: the word `joined`, [`Checker::bit_copy`]'s result with the value packed in,
    /// written to the word `packed` is in: the statement that made it, the last one, writes
    /// the word itself (`QCC_PR_Statement_BitCopy`'s last statement, whichever it folds to).
    /// A target with `OP_BITCOPY_I`, and a word that is no definition's, are not followed.
    fn pack_into_word(&mut self, packed: &Value, joined: &Value, span: Span) {
        let word = packed.definition_place().and_then(|place| place.word());
        let last = self.generated.len().checked_sub(1);
        let made_last = joined.made.and_then(|made| usize::try_from(made).ok()) == last;
        match word {
            Some(word)
                if made_last
                    && last.is_some()
                    && !self.env.target_at(span.start).valid(Op::BitcopyI) =>
            {
                self.fold_into(joined, Some(word));
            }
            _ => self.followed = false,
        }
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

    /// FTEF-29.17, FTEF-29.22: a compound assignment whose target has the in-place opcode
    /// `op` for its types, to an entity's field or to what a pointer points at (`if (ops_ptr
    /// && (lhsr->type == REF_FIELD || lhsr->type == REF_POINTER))`): the address, `OP_ADDRESS`
    /// of the entity and the field, the pointer itself, or `OP_ADD_PIW` of the pointer and an
    /// index (`QCC_PR_GenerateAddressOf`), then `op` of the value through it, whose result is
    /// the assignment's value. `None` for any other target, which is read, operated on and
    /// stored.
    fn update_in_place(&mut self, lhs: &Value, rhs: &Value, op: Op) -> Option<Compounded> {
        let through = match &lhs.reference {
            Reference::Field(_) if !self.global => Through::Field,
            Reference::Field(_) => return Some(Compounded::Unchecked),
            // What is narrower than a word is addressed in bytes, which is not followed here.
            Reference::Pointer(pointer)
                if !self.global
                    && pointer.named.is_some()
                    && self
                        .env
                        .layout(&lhs.ty)
                        .is_some_and(|l| l.bits == 0 || l.bits >= WORD_ALIGNMENT) =>
            {
                Through::Pointer
            }
            _ => Through::Nothing,
        };
        let address = match (&lhs.reference, through) {
            (Reference::Field(field), Through::Field) => {
                let Some(field_operand) = field.operand() else {
                    self.followed = false;
                    return Some(Compounded::Unchecked);
                };
                let address = self.next_made();
                self.generated.push(ir::BodyStatement {
                    a: self.copied(field.entity, field.entity_call),
                    b: Some(field_operand),
                    c: Some(ir::Name::Made(address)),
                    ..ir::BodyStatement::bare(Op::Address)
                });
                Some(ir::Name::Made(address))
            }
            // An index of 0 adds nothing: the address is the pointer.
            (Reference::Pointer(pointer), Through::Pointer) => match &pointer.index {
                Some(index) if !index.null() => self
                    .push_made(Op::AddPiw, pointer.named, self.name_of(index))
                    .map(ir::Name::Made),
                _ => pointer.named,
            },
            _ => return None,
        };
        // FTEF-29.18: an `ASSOC_RIGHT` opcode, `|=` and `&~=` of floats, has no result: the
        // assignment's value is the value it was given.
        if row(op).is_some_and(|r| r.assoc == opcodes::Assoc::Right) {
            self.generated.push(ir::BodyStatement {
                a: self.name_of(rhs),
                b: address,
                ..ir::BodyStatement::bare(op)
            });
            self.read_returned(rhs);
            self.env.temps().take(1);
            // `OpAssignsToC` is false of it, so no store folds into what came before.
            self.last_temp = None;
            return Some(Compounded::InPlace(rhs.clone()));
        }
        let result = self.next_made();
        self.generated.push(ir::BodyStatement {
            a: self.name_of(rhs),
            b: address,
            c: Some(ir::Name::Made(result)),
            ..ir::BodyStatement::bare(op)
        });
        self.read_returned(rhs);
        let temp = Some(self.env.temps().take(1));
        // `OpAssignsToC`: `if(op >= OP_MULSTORE_F && op <= OP_SUBSTOREP_V) return false;`, so
        // no store folds into one of those.
        let folds = !(Op::MulstoreF.number()..=Op::SubstorepV.number()).contains(&op.number());
        self.last_temp = temp.filter(|_| folds);
        Some(Compounded::InPlace(Value {
            made: Some(result),
            temp,
            ..Value::of(lhs.ty.clone())
        }))
    }

    /// FTEF-29.2: a compound assignment's opcode, the first of `ops` whose operands are the
    /// target's and the value's basic types, else after the value is cast to the target's type;
    /// "Type mismatch on assignment" when none. A field updated in place by an opcode of
    /// `in_place` the target has is written in place.
    fn compound(
        &mut self,
        lhs: &Value,
        rhs: &Value,
        spelling: &str,
        ops: &[Op],
        in_place: &[Op],
        span: Span,
    ) -> Result<Compounded, Fatal> {
        let (Some(to), Some(from)) = (self.etype(&lhs.ty), self.etype(&rhs.ty)) else {
            return Ok(Compounded::Unchecked);
        };
        let target = self.env.target_at(span.start);
        let updates_in_place = in_place.iter().copied().find(|&op| {
            target.valid(op)
                && row(op).is_some_and(|r| {
                    r.types[1].etype() == from
                        && r.types[0].etype() == Etype::Pointer
                        && r.types[2].etype() == to
                })
        });
        if let Some(op) = updates_in_place
            && let Some(updated) = self.update_in_place(lhs, rhs, op)
        {
            return Ok(updated);
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
                    return self
                        .fatal(Code::E0323, span, message)
                        .map(Compounded::Store);
                };
                (op, cast)
            }
        };
        // The opcode reads the value read, `lhsd`, and frees its temporary (FTEF-27.16).
        let read = Value {
            ty: read_ty.clone(),
            ..read
        };
        // The opcode on the value read and the right side, the one chosen as a binary operator's
        // is (FTEF-23.4).
        self.opcodes.insert((span.start, span.end), op);
        let value = self.emit(op, &read, &rhs, span)?;
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
                return Ok(Compounded::Store(value));
            }
            Supply::Explicit | Supply::Mismatch => {
                let message = message();
                return self
                    .fatal(Code::E0324, span, message)
                    .map(Compounded::Store);
            }
        }
        self.convert_to(&value, &lhs.ty, span).map(|converted| {
            Compounded::Store(Value {
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
            // `QCC_TryEvaluateCast(src, tmp->parentclass, implicit)`: the comparison's truth
            // value is read as its parent (`while (src.cast->type == ev_boolean) src.cast =
            // src.cast->parentclass;`), so the value is of the parent's type.
            let converted = self.convert_to(&truth, &parent, span)?;
            return Ok(Value {
                ty: parent,
                ..converted
            });
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
        // A number is converted before it is stored ([`Checker::store_converted`]).
        if from.etype.is_number() && to.etype.is_number() {
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

    /// FTEF-29.21: `QCC_StoreSRefToRef`'s conversion of the value it stores: a number whose
    /// type, or a parent's, is not the target's by `typecmp_lax` is converted to the target's
    /// type (`QCC_SupplyConversion(source, dest->cast->type, true)`), and to a truth value by
    /// its comparison with zero and then to the truth value's parent
    /// (`QCC_SupplyConversion(QCC_PR_GenerateLogicalTruth(source, "cannot convert to
    /// boolean"), dest->cast->parentclass->type, true)`). A value the cast already made a
    /// number of the truth value's parent is compared again here, as FTEQCC compares it.
    fn store_converted(&mut self, value: Value, target: &Type, span: Span) -> Checked {
        if value.known().is_some_and(Constant::null) {
            return Ok(value);
        }
        let (Some(from), Some(to)) = (self.env.shape(&value.ty), self.env.shape(target)) else {
            return Ok(value);
        };
        // `typecmp_lax`'s `if (a->type != b->type) ... return 1;`: a number the cast left of
        // a truth value's parent type is no truth value.
        let to_truth = is_truth(target) && !is_truth(&value.ty);
        if !to_truth && self.env.typecmp_lax(&value.ty, target) {
            return Ok(value);
        }
        if from.etype.is_number() && to.etype.is_number() {
            return self.convert_to(&value, target, span);
        }
        Ok(value)
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
            copy = self.step(&operand, etype, step, value_use(postfix.syntax()), span)?;
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
        value_use: ValueUse,
        span: Span,
    ) -> Result<Option<u32>, Fatal> {
        let discarded = value_use == ValueUse::Discarded;
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
        // FTEF-31.31: an element at a constant index is a word of its definition.
        let placed = match &operand.reference {
            Reference::Global => true,
            Reference::Element(element) => element.index.known().is_some(),
            _ => false,
        };
        if !discarded && (placed || old.returned) {
            let store = self.store_of(&operand.ty, span);
            self.emit(store, &old, &Value::unknown(), span)?;
            copy = self
                .generated
                .last()
                .filter(|statement| statement.op == store)
                .and_then(|_| self.last_made());
            self.name_last(store, self.name_of(&old), copy.map(ir::Name::Made));
        }
        // The step adds to the copy.
        let old = Value {
            made: copy.or(old.made),
            ..old
        };
        // FTEF-29.19: what a pointer points at, or an entity's field, is loaded to a temporary,
        // which is the old value.
        // FTEF-31.31: an element at an index that is no constant is loaded too, unless its
        // getter left it in the return global, which is copied above.
        let loaded = match &operand.reference {
            Reference::Pointer(_) | Reference::Field(_) => old.made,
            Reference::Element(element) if element.index.known().is_none() && !old.returned => {
                old.made
            }
            _ => None,
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
            Etype::Pointer => self.pointer_step(&old, step, value_use, span)?,
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
        // FTEF-6.15: a step of a bitfield or a packed element is not followed.
        if operand.packs() && !self.global {
            self.followed = false;
        }
        self.update_stores(operand, &updated, &target, span)?;
        let updated = self.bit_copy(&updated, operand, span)?;
        self.store_reference(operand, &updated, span)?;
        self.not_an_lvalue(operand, span);
        self.freed_again(operand, span);
        Ok(copy.or(loaded))
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
        let elements: Vec<ast::Expr> = aggregate.elements().collect();
        if self.global {
            return Ok(());
        }
        let local = self.env.reference(start);
        let array = local.is_some_and(|def| self.env.is_array(def));
        // A vector's own braces are its components'; an array of vectors' are its elements'.
        let vector = !array
            && self
                .env
                .local_type(start)
                .is_some_and(|ty| self.etype(&ty) == Some(Etype::Vector));
        let nested = aggregate.syntax().children().count() != elements.len();
        if array && !nested && !self.env.local_static(start) {
            return self.array_aggregate(declarator, local, &elements);
        }
        // FTEF-32.21: a `static` local's aggregate is its initial value, written with the
        // globals, and generates no statement.
        if self.env.local_static(start) {
            // One made static by `#pragma defaultstatic` alone is not written so yet.
            if !self.env.local_written_static(start) {
                self.followed = false;
            }
            return Ok(());
        }
        if !vector || elements.len() != qc_types::PROGS_VECTOR_SLOTS {
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
    /// type through it. FTEF-29.20: on a target with `OP_STOREF_*` it is that one statement, of
    /// the entity, the field and the value.
    fn field_store(&mut self, lhs: &Value, stored: &Value, target: &Type, span: Span) {
        let Reference::Field(field) = &lhs.reference else {
            return;
        };
        if self.global {
            return;
        }
        let valid = |op: Op| self.env.target_at(span.start).valid(op);
        let words = self.env.layout(target).map_or(1, |layout| layout.words);
        // `storef_opcode`: by the type's own kind, so an accessor's is none; `OP_STOREF_I` for
        // every kind held as integers, two words by `OP_STOREF_I64`.
        let storef = match self.etype(target) {
            _ if self.env.is_accessor(target) => None,
            Some(Etype::Float) => Some(Op::StorefF),
            Some(Etype::Vector) => Some(Op::StorefV),
            Some(Etype::String) => Some(Op::StorefS),
            Some(
                Etype::Entity
                | Etype::Field
                | Etype::Function
                | Etype::Pointer
                | Etype::Integer
                | Etype::Uint,
            ) => Some(Op::StorefI),
            Some(Etype::Enum) if words == 1 => Some(Op::StorefI),
            Some(Etype::Int64 | Etype::Uint64 | Etype::Double | Etype::Enum) if words == 2 => {
                Some(Op::StorefI64)
            }
            _ => None,
        };
        // `dest->index.cast->size == dest->cast->size`: not an array of fields named whole.
        let whole_array = field.selected.is_none()
            && field.element.is_none()
            && field.def.is_some_and(|def| self.env.is_array(def));
        // Two words with no `OP_STOREF_I64` are two `OP_STOREF_I`, which is not followed, as
        // is no store of two words through the field's address.
        if words == 2 && !valid(Op::StorefI64) {
            self.followed = false;
        }
        if let Some(storef) = storef.filter(|&op| valid(op) && !whole_array) {
            self.generated.push(ir::BodyStatement {
                a: self.copied(field.entity, field.entity_call),
                b: field.operand(),
                c: self.name_of(stored),
                ..ir::BodyStatement::bare(storef)
            });
            self.read_returned(stored);
            self.last_temp = None;
            return;
        }
        // FTEF-6.18: through the field's address, `QCC_StoreToPointer`'s opcode for the type.
        let storep = self.storep_through(target, span);
        let address = u32::try_from(self.generated.len()).ok().map(ir::Name::Made);
        self.generated.push(ir::BodyStatement {
            a: self.copied(field.entity, field.entity_call),
            b: field.operand(),
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
        self.field_store(operand, updated, target, span);
        self.pointer_store(operand, updated, target, span)?;
        let folded = updated.temp.is_some() && updated.temp == self.last_temp;
        self.definition_store(operand, updated, target, span)?;
        let fold = if folded { Fold::Folded } else { Fold::Stored };
        let _stored = self.element_store(operand, updated, (target, span), fold);
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
        let store = self.store_of(target, span);
        self.emit(store, stored, &Value::unknown(), span)?;
        self.name_last(store, self.name_of(stored), lhs.name());
        Ok(())
    }

    /// FTEF-31.10: the store of `stored` to the element `lhs` at a constant index, a word of
    /// the array's definition, stored as a definition is unless the value folds into the
    /// statement that made it (`folded`).
    fn element_store(
        &mut self,
        lhs: &Value,
        stored: &Value,
        (target, span): (&Type, Span),
        fold: Fold,
    ) -> ElementStore {
        let folded = fold == Fold::Folded;
        let Reference::Element(element) = &lhs.reference else {
            return ElementStore::NotOne;
        };
        if element.index.known().is_none() || self.global {
            return ElementStore::NotOne;
        }
        // The element's word, as a read of it at a constant index names it.
        let to = lhs.definition_place().and_then(|place| place.word());
        if folded {
            self.fold_into(stored, to);
            return ElementStore::Stored;
        }
        // The element is its definition's words, stored as a definition is
        // ([`Checker::store_steps`]): a struct, or a 64-bit value where the target has no
        // `OP_STORE_I64`, by more statements than one.
        let by_size = self.copied_by_size(target);
        match self.store_steps(by_size, target, span).as_slice() {
            [(store, _)] => {
                self.generated.push(ir::BodyStatement {
                    a: self.store_source(*store, stored),
                    b: to,
                    ..ir::BodyStatement::bare(*store)
                });
                self.read_returned(stored);
                self.last_temp = None;
            }
            steps => {
                if !self.copy_steps(stored, to, steps) {
                    self.followed = false;
                }
            }
        }
        ElementStore::Stored
    }

    /// FTEF-6.18: `QCC_StoreToPointer`'s opcode for a value of type `target`: an integer by
    /// `OP_STOREP_I` and a pointer by `OP_STOREP_P` or `OP_STOREP_I`, or else `OP_STOREP_FLD`;
    /// any other by its basic type's.
    pub(super) fn storep_through(&self, target: &Type, span: Span) -> Op {
        let valid = |op: Op| self.env.target_at(span.start).valid(op);
        match self.etype(target) {
            Some(Etype::Integer | Etype::Uint) if valid(Op::StorepI) => Op::StorepI,
            Some(Etype::Pointer) if valid(Op::StorepP) => Op::StorepP,
            Some(Etype::Pointer) if valid(Op::StorepI) => Op::StorepI,
            Some(Etype::Integer | Etype::Uint | Etype::Pointer) => Op::StorepFld,
            _ => self.storep_op(target),
        }
    }

    /// Whether one `STOREP_*` stores a value of type `target`: no struct, union, 64-bit value
    /// or narrow integer.
    pub(super) fn one_store(&self, target: &Type) -> bool {
        matches!(
            self.etype(target),
            Some(
                Etype::Float
                    | Etype::Vector
                    | Etype::String
                    | Etype::Entity
                    | Etype::Field
                    | Etype::Function
                    | Etype::Integer
                    | Etype::Uint
                    | Etype::Pointer
            )
        ) && !matches!(
            target.fteqcc_scalar(),
            Some(CScalar::Int8 | CScalar::Uint8 | CScalar::Int16 | CScalar::Uint16)
        )
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
    /// `STOREP_*` of the value through the pointer. FTEF-31.21: at an index, `p[i] = v` and
    /// `p->m = v`, the index made an integer and given to the store on a target whose stores
    /// take an offset, else added to the pointer first by `OP_ADD_PIW`
    /// (`QCC_StoreToPointer`).
    fn pointer_store(
        &mut self,
        lhs: &Value,
        stored: &Value,
        target: &Type,
        span: Span,
    ) -> Result<(), Fatal> {
        let Reference::Pointer(pointer) = &lhs.reference else {
            return Ok(());
        };
        if self.global {
            return Ok(());
        }
        let word = self.one_store(target) && lhs.bitfield.is_none();
        // FTEF-6.18: a struct or a union is stored word by word.
        let structured = matches!(self.etype(target), Some(Etype::Struct | Etype::Union))
            && lhs.bitfield.is_none();
        if pointer.named.is_none() || !(word || structured) {
            self.followed = false;
            return Ok(());
        }
        // An index converted by emulation, which [`Checker::store_reference`] reports, is not
        // followed: such a target has no store at an offset to write.
        let index = match &pointer.index {
            Some(index) if self.etype_of(&index.ty) != Some(Etype::Integer) => {
                if !self.env.target_at(span.start).valid(Op::ConvFtoi) {
                    self.followed = false;
                    return Ok(());
                }
                Some(self.supplied(index, &Type::Named("__int32".into()), span)?)
            }
            index => index.clone(),
        };
        if structured {
            let words = self.env.layout(target).map_or(0, |layout| layout.words);
            let at = index.as_ref().and_then(|index| self.name_of(index));
            let named = index.is_none() || at.is_some();
            if !(named && self.store_words_through(stored, pointer.named, at, words, span)) {
                self.followed = false;
            }
            return Ok(());
        }
        let mut through = pointer.named;
        let mut at = None;
        if let Some(index) = &index {
            if self.env.target_at(span.start).stores_at_offset() {
                at = self.name_of(index);
            } else {
                through = self
                    .push_made(Op::AddPiw, pointer.named, self.name_of(index))
                    .map(ir::Name::Made);
            }
        }
        let storep = self.storep_through(target, span);
        self.generated.push(ir::BodyStatement {
            a: self.name_of(stored),
            b: through,
            c: at,
            ..ir::BodyStatement::bare(storep)
        });
        self.read_returned(stored);
        self.last_temp = None;
        Ok(())
    }

    /// FTEF-6.18: `QCC_StoreToPointer`'s `case ev_struct: case ev_union:` on a target whose
    /// stores take an offset: the `words` words of `stored` stored through `pointer`,
    /// `OP_STOREP_V` for each three when the target has it, `OP_STOREP_I64` by FTEQCC's test
    /// of bits against words (`for (; i+(32*2) <= type->size; i+=32*2)`), then `OP_STOREP_I`,
    /// or else `OP_STOREP_F`, for each word left. Each is stored at its word's index: the
    /// word itself with no `index`, else `OP_ADD_I` of `index` and the word, which the first
    /// word folds away. Whether it was written: not on a target whose stores take no offset,
    /// nor from a value whose words the body cannot name.
    pub(super) fn store_words_through(
        &mut self,
        stored: &Value,
        pointer: Option<ir::Name>,
        index: Option<ir::Name>,
        words: u32,
        span: Span,
    ) -> bool {
        let target = self.env.target_at(span.start);
        let from = self.name_of(stored).filter(|name| {
            matches!(
                name,
                ir::Name::Def(..) | ir::Name::Made(_) | ir::Name::Immediate(_) | ir::Name::Return
            )
        });
        let (Some(from), Some(pointer), true) = (from, pointer, target.stores_at_offset()) else {
            return false;
        };
        let single = if target.valid(Op::StorepI) {
            Op::StorepI
        } else {
            Op::StorepF
        };
        let mut at: u32 = 0;
        while at < words {
            let left = words.saturating_sub(at);
            let (op, size) = if left >= VECTOR_WORDS && target.valid(Op::StorepV) {
                (Op::StorepV, VECTOR_WORDS)
            } else if target.valid(Op::StorepI64)
                && at.saturating_mul(WORD_ALIGNMENT).saturating_add(INT64_BITS) <= words
            {
                (Op::StorepI64, 2)
            } else {
                (single, 1)
            };
            let word = ir::Name::Immediate(ir::Immediate {
                kind: ir::ImmediateKind::Integer,
                words: [at, 0, 0],
            });
            let place = match index {
                None => Some(word),
                Some(index) if at == 0 => Some(index),
                Some(index) => self
                    .push_made(Op::AddI, Some(index), Some(word))
                    .map(ir::Name::Made),
            };
            self.generated.push(ir::BodyStatement {
                a: from.word(at),
                b: Some(pointer),
                c: place,
                ..ir::BodyStatement::bare(op)
            });
            at = at.saturating_add(size);
        }
        self.read_returned(stored);
        self.last_temp = None;
        true
    }

    /// A store of more statements than one, or of a type copied by its size, to the
    /// definition `to`: each of `steps` ([`Checker::store_steps`]) stores the word it starts
    /// at, from a definition's, a block's, an immediate's or a call's words. A constant 0 is
    /// stored from the zero of each step's size instead, as both of FTEQCC's functions store
    /// it: `QCC_StoreToSRef`'s `default:` (`if (QCC_SRef_IsNull(source))`:
    /// `QCC_MakeVectorConst(0,0,0)`, `QCC_MakeInt64Const(0)`, `QCC_MakeIntConst(0)`; FTE-14.6,
    /// FTEF-2.13, FTEF-6.31, FTEF-11.5) and `QCC_PR_StatementFlags` of `OP_STORE_I64` (`var_a =
    /// QCC_MakeInt64Const(0);`, FTEF-27.20). Whether it was: any other value is left to its
    /// one statement.
    pub(super) fn copy_steps(
        &mut self,
        stored: &Value,
        to: Option<ir::Name>,
        steps: &[(Op, u32)],
    ) -> bool {
        let Some(to) = to else {
            return false;
        };
        if self.global {
            return false;
        }
        let null = stored.null();
        let from = self.name_of(stored).filter(|name| {
            matches!(
                name,
                ir::Name::Def(..) | ir::Name::Made(_) | ir::Name::Immediate(_) | ir::Name::Return
            )
        });
        if !null && from.is_none() {
            return false;
        }
        for &(op, word) in steps {
            let a = if null {
                Zeroes::OfAStore
                    .source(op, word)
                    .or(Some(Zeroes::integer()))
            } else {
                from.and_then(|from| from.word(word))
            };
            self.generated.push(ir::BodyStatement {
                a,
                b: to.word(word),
                ..ir::BodyStatement::bare(op)
            });
        }
        if let Some(offset) = stored.temp {
            self.env.temps().free(offset);
        }
        self.last_temp = None;
        true
    }

    /// A local array's aggregate initialiser: a constant one is its value at compile time, as a
    /// global's is (FTEF-32.13), which the code generator writes; any other stores each element's
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
        // Its words are its value at compile time, which the code generator writes.
        if constant {
            match local {
                Some(local) => {
                    let at = self.next_made();
                    self.effects
                        .push((at, ir::Effect::Constant(ir::Name::Def(local, 0))));
                }
                None => self.followed = false,
            }
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
        let span = declarator.syntax().span();
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
                made.map_or(1, Etype::words) > self.etype(&ty).map_or(1, Etype::words)
            });
            if value.temp.is_some() && value.temp == self.last_temp && !wider {
                self.fold_into(&value, to);
            } else {
                // FTE-14.9: each element is stored as a local's initialiser is.
                self.store_initialiser((&value, self.name_of(&value)), to, &ty, span);
            }
            self.read_returned(&value);
            self.last_temp = None;
        }
        Ok(())
    }

    /// A store's read of `value`: a call's value is then out of the return global's keeping.
    pub(super) fn read_returned(&mut self, value: &Value) {
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
        // FTEF-31.20: a target whose stores take an offset stores through the array's address,
        // not by `ArraySet*`, as [`Checker::store_reference`] writes.
        let opcodes = self.env.target_at(span.start);
        if opcodes.valid(Op::Globaladdress) && opcodes.stores_at_offset() {
            return Ok(());
        }
        if element.vector {
            self.followed = false;
            return Ok(());
        }
        let parameter = |slot| Some(ir::Name::Parameter(slot));
        // FTEF-32.25: a struct of more than a vector's words is the setter's argument in
        // pieces, a slot each from the second, stored before the index, and the call counts
        // the slots.
        if let Some(wide) = self.wide_words(target) {
            self.setter_argument(self.name_of(stored), wide, span);
            self.generated.push(ir::BodyStatement {
                a: self.name_of(&element.index),
                b: parameter(0),
                ..ir::BodyStatement::bare(Op::StoreF)
            });
            self.read_returned(&element.index);
            self.clobber_return();
            let slots = usize::try_from(wide.div_ceil(VECTOR_WORDS)).unwrap_or(usize::MAX);
            match call_opcode(slots.saturating_add(1)) {
                Some(call) => self.generated.push(ir::BodyStatement {
                    a: lhs.sym.def().map(ir::Name::ArraySet),
                    ..ir::BodyStatement::bare(call)
                }),
                None => self.followed = false,
            }
            self.last_temp = None;
            return Ok(());
        }
        // A vector array's setter takes the element's index, its words' divided by 3.
        let index = if self.etype(target) == Some(Etype::Vector) {
            let three = Value::constant(Constant::float(VECTOR_WORDS_FLOAT));
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
        // The value is the setter's second argument, stored by its size (FTEF-32.27).
        let words = self.env.layout(target).map_or(1, |l| l.words);
        self.generated.push(ir::BodyStatement {
            a: self.name_of(stored),
            b: parameter(1),
            ..ir::BodyStatement::bare(self.argument_store(words, span))
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

    /// The words of a value of type `ty` when `QCC_StoreToSRef` has no case for its type and
    /// its `default:` copies it by its size: a struct, a union, a `__variant`, a double, an
    /// integer narrower than a word, which is `ev_bitfld` (its layout says so through a
    /// typedef), and an accessor (FTEF-2.13, FTEF-6.31, FTEF-10.15, FTEF-11.5). `None` for a
    /// type with a case of its own.
    pub(super) fn copied_by_size(&self, ty: &Type) -> Option<u32> {
        let layout = self.env.layout(ty)?;
        let narrow = layout.bits != 0 && layout.bits < WORD_ALIGNMENT;
        let no_case = self.env.is_accessor(ty)
            || match self.etype(ty) {
                Some(
                    Etype::Struct | Etype::Union | Etype::Enum | Etype::Variant | Etype::Double,
                ) => true,
                Some(Etype::Integer | Etype::Uint) => narrow,
                _ => false,
            };
        no_case.then_some(layout.words)
    }

    /// The statements that store a value to a definition on this target, each its opcode and
    /// the word it starts at. A type copied by its size, `by_size` its words, is its pieces
    /// ([`Checker::pieces`]); any other is its type's case of `QCC_StoreToSRef`, one
    /// statement, but `OP_STORE_I64` on a target without it, which `QCC_PR_StatementFlags`
    /// writes as `OP_STORE_FLD` of each of its two words (FTEF-27.20).
    pub(super) fn store_steps(
        &self,
        by_size: Option<u32>,
        ty: &Type,
        span: Span,
    ) -> Vec<(Op, u32)> {
        if let Some(words) = by_size {
            return self.pieces(words, span);
        }
        match Checker::store_op(self.etype(ty)) {
            Op::StoreI64 if !self.env.target_at(span.start).valid(Op::StoreI64) => {
                vec![(Op::StoreFld, 0), (Op::StoreFld, 1)]
            }
            op => vec![(op, 0)],
        }
    }

    /// The statements that store a local's initialiser, `QCC_PR_GenerateInitializerType`'s
    /// loop over the local's words, each its opcode and the word it starts at (FTE-14.9):
    /// `OP_STORE_V` while three words or more are left, `OP_STORE_I64` for two, which a
    /// target without it writes as `OP_STORE_FLD` of each (FTEF-27.20), and the last word by
    /// the local's basic type, `OP_STORE_F` for any but a function, a string, an entity, a
    /// field or an integer. The third of each is the half of an emulated `OP_STORE_I64` the
    /// statement is, for the zero it reads.
    fn initialiser_steps(&self, ty: &Type, span: Span) -> Vec<(Op, u32, Option<u32>)> {
        let words = self.env.layout(ty).map_or(1, |layout| layout.words);
        let wide = self.env.target_at(span.start).valid(Op::StoreI64);
        let mut steps = Vec::new();
        let mut at = 0;
        while at < words {
            let left = words.saturating_sub(at);
            let size = if left >= VECTOR_WORDS {
                steps.push((Op::StoreV, at, None));
                VECTOR_WORDS
            } else if left == 2 {
                if wide {
                    steps.push((Op::StoreI64, at, None));
                } else {
                    steps.push((Op::StoreFld, at, Some(0)));
                    steps.push((Op::StoreFld, at.saturating_add(1), Some(1)));
                }
                2
            } else {
                steps.push((
                    match self.etype(ty) {
                        Some(Etype::Function) => Op::StoreFnc,
                        Some(Etype::String) => Op::StoreS,
                        Some(Etype::Entity) => Op::StoreEnt,
                        Some(Etype::Field) => Op::StoreFld,
                        Some(Etype::Integer | Etype::Uint) => Op::StoreI,
                        _ => Op::StoreF,
                    },
                    at,
                    None,
                ));
                1
            };
            at = at.saturating_add(size);
        }
        steps
    }

    /// A local's initialiser `value`, named `from`, stored to `local` by
    /// [`Checker::initialiser_steps`], each statement a word of the two. A constant 0 is
    /// stored from zeroes, as `QCC_PR_GenerateInitializerType` stores it
    /// ([`Zeroes::OfAnInitialiser`]). A value of more statements than one whose words the body
    /// cannot name is not followed.
    fn store_initialiser(
        &mut self,
        (value, from): (&Value, Option<ir::Name>),
        local: Option<ir::Name>,
        ty: &Type,
        span: Span,
    ) {
        let steps = self.initialiser_steps(ty, span);
        let null = value.null();
        let named = from.is_some_and(|name| name.word(0).is_some());
        if steps.len() > 1 && !named && !null {
            self.followed = false;
        }
        for (op, word, half) in steps {
            let word_of = |name: ir::Name| {
                if word == 0 {
                    Some(name)
                } else {
                    name.word(word)
                }
            };
            let a = if null {
                // A word stored alone reads the integer 0, an emulated pair's halves the
                // words of the pair's zero.
                let zero = match (op, half) {
                    (Op::StoreFld, None) => None,
                    _ => Zeroes::OfAnInitialiser.source(op, half.unwrap_or(0)),
                };
                zero.or(Some(Zeroes::integer()))
            } else {
                from.and_then(word_of)
            };
            self.generated.push(ir::BodyStatement {
                a,
                b: local.and_then(word_of),
                ..ir::BodyStatement::bare(op)
            });
        }
    }

    /// What a store of `op` reads `stored` from: its name, or for a constant 0 stored by
    /// `OP_STORE_V` or `OP_STORE_I64` the zero of that size (`QCC_PR_StatementFlags`: `if (op
    /// - pr_opcodes == OP_STORE_V) { ... var_a = QCC_MakeVectorConst(0, 0, 0); }`, and
    /// `QCC_MakeInt64Const(0)` for `OP_STORE_I64`).
    pub(super) fn store_source(&self, op: Op, stored: &Value) -> Option<ir::Name> {
        stored
            .null()
            .then(|| Zeroes::OfAStore.source(op, 0))
            .flatten()
            .or_else(|| self.name_of(stored))
    }

    /// The one statement of [`Checker::store_steps`], or, where there are more, a body that
    /// is not followed: a site that writes one statement does not make the others.
    fn store_or_refuse(&mut self, by_size: Option<u32>, ty: &Type, span: Span) -> Op {
        match self.store_steps(by_size, ty, span).as_slice() {
            [(op, _)] => *op,
            _ => {
                self.followed = false;
                Op::StoreF
            }
        }
    }

    /// The one statement that stores a value of type `ty` to a definition
    /// ([`Checker::store_or_refuse`]).
    pub(super) fn store_of(&mut self, ty: &Type, span: Span) -> Op {
        let by_size = self.copied_by_size(ty);
        self.store_or_refuse(by_size, ty, span)
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

    /// FTEF-18.2: a local given a nested function's body: the function's number stored to it
    /// each time the function runs, which the code generator numbers, or a `static` local's
    /// value.
    fn nested_local(&mut self, declarator: &ast::Declarator) {
        let local = declarator
            .name()
            .and_then(|name| self.env.reference(name.span().start));
        let is_static = declarator
            .syntax()
            .parent()
            .is_some_and(|decl| declares(&decl, &Qualifier::Static));
        match local {
            Some(local) if is_static => {
                let at = self.next_made();
                self.effects
                    .push((at, ir::Effect::Nested(ir::Name::Def(local, 0))));
            }
            Some(local) => self.generated.push(ir::BodyStatement {
                a: Some(ir::Name::Nested(local)),
                b: Some(ir::Name::Def(local, 0)),
                ..ir::BodyStatement::bare(Op::StoreFnc)
            }),
            None => self.followed = false,
        }
    }

    /// FTEF-34.20: a constant local's constant value is its initial value too, copied when
    /// the program is written (`if (basedef && (!basedef->scope || basedef->constant ||
    /// basedef->isstatic))`, with a value that is `constant`); one whose value is no constant
    /// is stored as the function runs (`goto finalnotconst;`). Whether `value`, the
    /// initialiser `init` of `declarator`'s local, was such a value.
    fn constant_local_value(
        &mut self,
        declarator: &ast::Declarator,
        value: &Value,
        init: &ast::Expr,
    ) -> bool {
        let local = declarator
            .name()
            .and_then(|name| self.env.reference(name.span().start));
        let constant = !self.global
            && value.known().is_some()
            && declarator.array_suffixes().next().is_none()
            && local.is_some_and(|def| self.env.constant(def));
        if !constant {
            return false;
        }
        let at = self.next_made();
        let name = local.map(|def| ir::Name::Def(def, 0));
        let words = value.known().and_then(Constant::immediate);
        match (name, words, init) {
            // A number's words are the value it folded to.
            (Some(name), Some(words), _) => {
                self.effects.push((at, ir::Effect::Value(name, words)));
            }
            // A string is its literal's text, which the code generator copies.
            (Some(name), None, ast::Expr::Literal(_)) => {
                self.effects.push((at, ir::Effect::Constant(name)));
            }
            _ => self.followed = false,
        }
        self.last_temp = None;
        true
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
            // FTEF-18.2: a nested function is compiled after this one, its number stored to its
            // local each time the function runs, or a `static` local's value.
            Some(ast::Initializer::Body(_)) if self.env.fteqcc() && !self.global => {
                self.nested_local(declarator);
            }
            // A builtin local and a nested function elsewhere are compiled from the syntax tree.
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
        // FTE-14.9: a struct's or a union's initialiser of its own type is copied by
        // `QCC_PR_GenerateInitializerType`'s statements ([`Checker::initialiser_steps`]); any
        // other is not followed.
        if matches!(self.etype(&ty), Some(Etype::Struct | Etype::Union)) {
            let local = declarator
                .name()
                .filter(|name| !self.env.local_static(name.span().start))
                .and_then(|name| self.env.reference(name.span().start));
            // The value a definition holds, whose words are its own; a struct read word by word,
            // through an array's getter, is not followed yet.
            let from = self
                .name_of(&value)
                .filter(|name| matches!(name, ir::Name::Def(..) | ir::Name::Made(_)));
            if let Some(local) = local.filter(|_| self.env.typecmp(&value.ty, &ty) && !self.global)
                && from.is_some()
                && declarator.array_suffixes().next().is_none()
            {
                let local = Some(ir::Name::Def(local, 0));
                self.store_initialiser((&value, from), local, &ty, init.syntax().span());
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
        if !is_static && self.constant_local_value(declarator, &value, &init) {
            return Ok(());
        }
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
                let from = self.name_of(&value);
                self.store_initialiser((&value, from), local, &ty, span);
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
                let size = array_size(&suffix).unwrap_or_else(|| bytes.len().saturating_add(1));
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

/// Whether the value of the postfix `node` is read or discarded (`QCC_PR_DiscardExpression`,
/// `QCC_PR_DiscardRef`): discarded as a statement's expression, a `for` header's first or last
/// clause, or a comma's left side, through parentheses and a comma's right side.
fn value_use(node: &SyntaxNode) -> ValueUse {
    let mut node = node.clone();
    while let Some(parent) = node.parent() {
        match parent.kind() {
            SyntaxKind::ParenExpr => {}
            SyntaxKind::BinaryExpr => {
                let Some(binary) = ast::BinaryExpr::cast(parent.clone()) else {
                    return ValueUse::Read;
                };
                if binary.op() != Some(BinaryOp::Comma) {
                    return ValueUse::Read;
                }
                if binary.lhs().is_some_and(|lhs| *lhs.syntax() == node) {
                    return ValueUse::Discarded;
                }
            }
            SyntaxKind::ExprStmt => return ValueUse::Discarded,
            SyntaxKind::ForClause => {
                let Some(header) = parent.parent().and_then(ast::ForStmt::cast) else {
                    return ValueUse::Read;
                };
                let clause = header.init().as_ref() == Some(&parent)
                    || header.step().as_ref() == Some(&parent);
                return if clause {
                    ValueUse::Discarded
                } else {
                    ValueUse::Read
                };
            }
            _ => return ValueUse::Read,
        }
        node = parent;
    }
    ValueUse::Read
}

/// The bytes of the string literal `expr` as FTEQCC's lexer reads them, `pr_immediate_string`.
pub(super) fn string_bytes(expr: &ast::Expr) -> Option<Vec<u8>> {
    let ast::Expr::Literal(literal) = expr else {
        return None;
    };
    if literal.kind() != Some(ast::LiteralKind::String) {
        return None;
    }
    joined_string(literal.syntax())
}

/// FTEF-16.8: the bytes of the string literal whose node is `literal`, its adjacent parts
/// joined into the one string `QCC_PR_LexString` reads them as; `None` when a part is no string.
pub(crate) fn joined_string(literal: &SyntaxNode) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for part in literal.child_tokens() {
        if !part.kind().is_trivia() {
            bytes.extend(lexed_string(part.text())?);
        }
    }
    Some(bytes)
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

/// What a compound assignment leaves to the assignment around it (FTEF-29.2).
enum Compounded {
    /// The value to store to the target.
    Store(Value),
    /// FTEF-29.17: the update written in place, whose value is the assignment's.
    InPlace(Value),
    /// Nothing more is checked.
    Unchecked,
}

/// The bits of a 64-bit value, which `QCC_StoreToPointer` tests a struct's words against
/// (`for (; i+(32*2) <= type->size; i+=32*2)`, FTEF-6.18).
const INT64_BITS: u32 = 64;

/// Whether an assignment's target was an element at a constant index, which
/// [`Checker::element_store`] stores.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ElementStore {
    /// It was, and its store is written.
    Stored,
    /// The target is no such element.
    NotOne,
}

/// Which of FTEQCC's functions stores a constant 0, each from zeroes of its own choosing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zeroes {
    /// `QCC_StoreToSRef`'s `default:` and `QCC_PR_StatementFlags`: the zero vector for
    /// `OP_STORE_V` and the 64-bit zero for `OP_STORE_I64`, whose words an emulated one's two
    /// `OP_STORE_FLD` read.
    OfAStore,
    /// `QCC_PR_GenerateInitializerType`: the zero vector for `OP_STORE_V` and for
    /// `OP_STORE_I64` alike (`nullsource?QCC_MakeVectorConst(0,0,0):rhs`).
    OfAnInitialiser,
}

impl Zeroes {
    /// The zero a store of `op` reads, its word `word` for an emulated pair's `OP_STORE_FLD`;
    /// `None` for a store of one word, which reads [`Zeroes::integer`] or the constant itself.
    fn source(self, op: Op, word: u32) -> Option<ir::Name> {
        let zero = |kind| {
            ir::Name::Immediate(ir::Immediate {
                kind,
                words: [0, 0, 0],
            })
        };
        let pair = match self {
            Self::OfAStore => ir::ImmediateKind::Int64,
            Self::OfAnInitialiser => ir::ImmediateKind::Vector,
        };
        match op {
            Op::StoreV => Some(zero(ir::ImmediateKind::Vector)),
            Op::StoreI64 => Some(zero(pair)),
            Op::StoreFld => zero(pair).word(word),
            _ => None,
        }
    }

    /// The integer 0, `QCC_MakeIntConst(0)`, a word's zero.
    const fn integer() -> ir::Name {
        ir::Name::Immediate(ir::Immediate {
            kind: ir::ImmediateKind::Integer,
            words: [0, 0, 0],
        })
    }
}
