// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_NumericTypeRanking, QCC_canConv, QCC_PR_ChooseOpcode

//! Which opcode an operator is given for its operands: the conversions each candidate would
//! need, and the one needing fewest (FTEF-23.2 to FTEF-23.4).

use crate::check::etype::Etype;
use qc_preproc::Op;

use crate::check::opcodes::{Assoc, OpType, Opcode, row};

/// What the choice of an opcode reads of an operand: its basic type after accessors and
/// truth values are taken for their underlying type, the basic type a field holds, and
/// whether it is a constant zero, `QCC_SRef_IsNull`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Operand {
    /// The operand's `etype_t`, an accessor or truth value already taken for its
    /// `parentclass`.
    pub(crate) etype: Etype,
    /// For a field, the `etype_t` of what it holds (`aux_type`).
    pub(crate) holds: Option<Etype>,
    /// Whether it is an integer, float or pointer constant 0 (FTEF-23.3).
    pub(crate) null: bool,
}

impl OpType {
    /// The `etype_t` of the `QCC_type_t` the row names.
    pub(crate) const fn etype(self) -> Etype {
        match self {
            Self::None | Self::Void => Etype::Void,
            Self::Float => Etype::Float,
            Self::Vector => Etype::Vector,
            Self::String => Etype::String,
            Self::Entity => Etype::Entity,
            Self::Field => Etype::Field,
            Self::Function => Etype::Function,
            Self::Pointer | Self::FloatPointer | Self::IntPointer => Etype::Pointer,
            Self::Integer => Etype::Integer,
            Self::Uint => Etype::Uint,
            Self::Int64 => Etype::Int64,
            Self::Uint64 => Etype::Uint64,
            Self::Double => Etype::Double,
            Self::Variant => Etype::Variant,
            Self::BoolFloat | Self::BoolInt => Etype::Boolean,
        }
    }

    /// The `name` of the `QCC_type_t` the row names, as FTEQCC's messages print it.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::None | Self::Void => "void",
            Self::Float => "float",
            Self::Vector => "vector",
            Self::String => "string",
            Self::Entity => "entity",
            Self::Field => "__field",
            Self::Function => "__function",
            Self::Pointer => "__pointer",
            Self::Integer => "__int32",
            Self::Uint => "__uint32",
            Self::Int64 => "__int64",
            Self::Uint64 => "__uint64",
            Self::Double => "__double",
            Self::Variant => "__variant",
            Self::BoolFloat => "__bfloat",
            Self::BoolInt => "__bint",
            Self::FloatPointer => "__floatpointer",
            Self::IntPointer => "__intpointer",
        }
    }

    /// The `etype_t` of what the row's pointer type points at, `aux_type`: `type_pointer`'s
    /// is `__pointeraux`, a float, as `QCC_main` makes it (FTEF-23.4).
    pub(crate) const fn pointee(self) -> Option<Etype> {
        match self {
            Self::Pointer | Self::FloatPointer => Some(Etype::Float),
            Self::IntPointer => Some(Etype::Integer),
            Self::None
            | Self::Void
            | Self::Float
            | Self::Vector
            | Self::String
            | Self::Entity
            | Self::Field
            | Self::Function
            | Self::Integer
            | Self::Uint
            | Self::Int64
            | Self::Uint64
            | Self::Double
            | Self::Variant
            | Self::BoolFloat
            | Self::BoolInt => None,
        }
    }
}

impl Operand {
    /// `QCC_canConv`: the cost of converting the operand to `to`, `None` when it cannot be
    /// (FTEF-23.3). `in_class` is `pr_classtype`, set inside a member function's body.
    pub(crate) fn cost_to(self, to: Etype, in_class: bool) -> Option<i32> {
        if self.etype == to {
            return Some(0);
        }
        if in_class && self.etype == Etype::Field && self.holds == Some(to) {
            return Some(1);
        }
        if self.null {
            return Some(2);
        }
        if let (Some(from_rank), Some(to_rank)) =
            (self.etype.numeric_ranking(), to.numeric_ranking())
        {
            return Some(if to_rank >= from_rank {
                3_i32.saturating_add(to_rank.saturating_sub(from_rank))
            } else {
                150_i32.saturating_add(from_rank.saturating_sub(to_rank))
            });
        }
        match (self.etype, to) {
            (_, Etype::Variant) => Some(9),
            (Etype::Variant, _) => Some(10),
            (Etype::Float, Etype::Vector) => Some(200),
            (Etype::Vector, Etype::Float) => Some(201),
            _ => None,
        }
    }
}

/// The opcode an operator is given for its operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    /// The candidate needing fewest conversions.
    Opcode(Op),
    /// None can take the operands: "type mismatch for OP (A and B)" (FTEF-23.4). FTEQCC then
    /// emits the first candidate anyway.
    Mismatch(Op),
}

impl Choice {
    /// The opcode emitted either way.
    pub(crate) const fn op(self) -> Op {
        match self {
            Self::Opcode(op) | Self::Mismatch(op) => op,
        }
    }
}

/// `QCC_PR_ChooseOpcode` over `candidates`, the opcodes of the operator's spelling in its
/// priority class in table order (FTEF-23.1), for `lhs` and `rhs` (FTEF-23.4). The first
/// candidate needing no conversion is taken; otherwise the first needing fewest.
pub(crate) fn choose_opcode(
    candidates: &[Op],
    lhs: Operand,
    rhs: Operand,
    in_class: bool,
) -> Option<Choice> {
    let first = *candidates.first()?;
    let field_result = row(first)
        .filter(|row| row.name.starts_with('.'))
        .map(|_| rhs.holds);
    let mut best: Option<(Op, i32)> = None;
    for &op in candidates {
        let Some(candidate) = row(op) else {
            continue;
        };
        if let Some(holds) = field_result
            && holds != Some(candidate.types[2].etype())
        {
            continue;
        }
        let Some(cost) = candidate.cost(lhs, rhs, in_class) else {
            continue;
        };
        if best.is_none_or(|(_, least)| cost < least) {
            best = Some((op, cost));
            if cost == 0 {
                break;
            }
        }
    }
    Some(best.map_or(Choice::Mismatch(first), |(op, _)| Choice::Opcode(op)))
}

impl Opcode {
    /// The conversions the row needs for `lhs` and `rhs`, `None` when it cannot take them
    /// (FTEF-23.4).
    fn cost(&self, lhs: Operand, rhs: Operand, in_class: bool) -> Option<i32> {
        let [a, b, _] = self.types;
        if self.assoc == Assoc::Left {
            let left = lhs.cost_to(a.etype(), in_class)?;
            let right = rhs.cost_to(b.etype(), in_class)?;
            return Some(left.max(right));
        }
        // An assignment: the value is converted, the target never changes type.
        let cost = rhs.cost_to(b.etype(), in_class)?;
        if lhs.etype != a.etype() {
            return None;
        }
        if a.etype() == Etype::Pointer && lhs.holds != a.pointee() {
            return None;
        }
        Some(cost)
    }
}
