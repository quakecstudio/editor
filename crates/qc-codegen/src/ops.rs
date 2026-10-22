// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_comp.c, pr_opcodes, PR_Expression

//! The operator table: which opcode an operator emits for its operands' types (QC-16).
//!
//! id's `pr_opcodes` lists, for each opcode, its spelling and the types of its operands and its
//! result. `PR_Expression` takes the first row of an operator's spelling and walks on through
//! the rows of the same spelling until one has the operands' types exactly, comparing only the
//! basic type of each (`type_a != op->type_a->type->type`), so two function types or two field
//! types always agree (QC-16.7, QC-17.6). The rows below are id's, in id's order, for the
//! spellings an expression can reach.

use qc_progs::opcode::Opcode;
use qc_types::Etype;

/// A binary operator's spelling, as `pr_opcodes` names it (QC-16.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Operator {
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `==`.
    Eq,
    /// `!=`.
    Ne,
    /// `<=`.
    Le,
    /// `>=`.
    Ge,
    /// `<`.
    Lt,
    /// `>`.
    Gt,
    /// `.`, a field access.
    Dot,
    /// `=`.
    Assign,
    /// `&&`.
    And,
    /// `||`.
    Or,
    /// `&`.
    BitAnd,
    /// `|`.
    BitOr,
}

impl Operator {
    /// The spelling id's messages use, as in "type mismatch for OP" (QCD-4.2).
    pub(crate) const fn spelling(self) -> &'static str {
        match self {
            Self::Mul => "*",
            Self::Div => "/",
            Self::Add => "+",
            Self::Sub => "-",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Dot => ".",
            Self::Assign => "=",
            Self::And => "&&",
            Self::Or => "||",
            Self::BitAnd => "&",
            Self::BitOr => "|",
        }
    }
}

/// One row of `pr_opcodes`: an opcode and the basic types of its operands and result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// The spelling.
    pub(crate) operator: Operator,
    /// The opcode the row emits.
    pub(crate) opcode: Opcode,
    /// Operand a's type.
    pub(crate) a: Etype,
    /// Operand b's type.
    pub(crate) b: Etype,
    /// The result's type: the temporary `PR_Statement` allocates holds one, unless the row is
    /// a store, which allocates none (QC-21.5).
    pub(crate) c: Etype,
}

const fn row(operator: Operator, opcode: Opcode, a: Etype, b: Etype, c: Etype) -> Row {
    Row {
        operator,
        opcode,
        a,
        b,
        c,
    }
}

use Etype::Vector as V;
use Etype::{Entity as E, Field as Fld, Float as F, Function as Fnc, Pointer as P, String as S};

/// id's `pr_opcodes`, the rows an operator can select, in id's order (QC-16.1).
pub(crate) const ROWS: [Row; 46] = [
    row(Operator::Mul, Opcode::MulF, F, F, F),
    row(Operator::Mul, Opcode::MulV, V, V, F),
    row(Operator::Mul, Opcode::MulFv, F, V, V),
    row(Operator::Mul, Opcode::MulVf, V, F, V),
    row(Operator::Div, Opcode::DivF, F, F, F),
    row(Operator::Add, Opcode::AddF, F, F, F),
    row(Operator::Add, Opcode::AddV, V, V, V),
    row(Operator::Sub, Opcode::SubF, F, F, F),
    row(Operator::Sub, Opcode::SubV, V, V, V),
    row(Operator::Eq, Opcode::EqF, F, F, F),
    row(Operator::Eq, Opcode::EqV, V, V, F),
    row(Operator::Eq, Opcode::EqS, S, S, F),
    row(Operator::Eq, Opcode::EqE, E, E, F),
    row(Operator::Eq, Opcode::EqFnc, Fnc, Fnc, F),
    row(Operator::Ne, Opcode::NeF, F, F, F),
    row(Operator::Ne, Opcode::NeV, V, V, F),
    row(Operator::Ne, Opcode::NeS, S, S, F),
    row(Operator::Ne, Opcode::NeE, E, E, F),
    row(Operator::Ne, Opcode::NeFnc, Fnc, Fnc, F),
    row(Operator::Le, Opcode::Le, F, F, F),
    row(Operator::Ge, Opcode::Ge, F, F, F),
    row(Operator::Lt, Opcode::Lt, F, F, F),
    row(Operator::Gt, Opcode::Gt, F, F, F),
    row(Operator::Dot, Opcode::LoadF, E, Fld, F),
    row(Operator::Dot, Opcode::LoadV, E, Fld, V),
    row(Operator::Dot, Opcode::LoadS, E, Fld, S),
    row(Operator::Dot, Opcode::LoadEnt, E, Fld, E),
    row(Operator::Dot, Opcode::LoadFld, E, Fld, Fld),
    row(Operator::Dot, Opcode::LoadFnc, E, Fld, Fnc),
    row(Operator::Dot, Opcode::Address, E, Fld, P),
    row(Operator::Assign, Opcode::StoreF, F, F, F),
    row(Operator::Assign, Opcode::StoreV, V, V, V),
    row(Operator::Assign, Opcode::StoreS, S, S, S),
    row(Operator::Assign, Opcode::StoreEnt, E, E, E),
    row(Operator::Assign, Opcode::StoreFld, Fld, Fld, Fld),
    row(Operator::Assign, Opcode::StoreFnc, Fnc, Fnc, Fnc),
    row(Operator::Assign, Opcode::StorepF, P, F, F),
    row(Operator::Assign, Opcode::StorepV, P, V, V),
    row(Operator::Assign, Opcode::StorepS, P, S, S),
    row(Operator::Assign, Opcode::StorepEnt, P, E, E),
    row(Operator::Assign, Opcode::StorepFld, P, Fld, Fld),
    row(Operator::Assign, Opcode::StorepFnc, P, Fnc, Fnc),
    row(Operator::And, Opcode::And, F, F, F),
    row(Operator::Or, Opcode::Or, F, F, F),
    row(Operator::BitAnd, Opcode::BitAnd, F, F, F),
    row(Operator::BitOr, Opcode::BitOr, F, F, F),
];

/// The row `operator` selects for operands of basic types `a` and `b`, and for a field access
/// the field's own type `c` (QC-16.1): `PR_Expression`'s walk from the first row of the
/// spelling. `None` is "type mismatch for OP" (QC-16.2, QCD-4.2).
pub(crate) fn select(operator: Operator, a: Etype, b: Etype, c: Option<Etype>) -> Option<Row> {
    ROWS.iter()
        .filter(|r| r.operator == operator)
        .find(|r| r.a == a && r.b == b && c.is_none_or(|c| r.c == c))
        .copied()
}

/// The `NOT_*` opcode `PR_Term` emits for an operand of basic type `t` (QC-16.4), or `None`
/// for "type mismatch for !".
pub(crate) const fn not_opcode(t: Etype) -> Option<Opcode> {
    match t {
        Etype::Float => Some(Opcode::NotF),
        Etype::String => Some(Opcode::NotS),
        Etype::Entity => Some(Opcode::NotEnt),
        Etype::Vector => Some(Opcode::NotV),
        Etype::Function => Some(Opcode::NotFnc),
        Etype::Void | Etype::Field | Etype::Pointer => None,
    }
}

/// The `CALLn` opcode for `n` arguments, `n` at most 8 (QC-18.3).
pub(crate) const fn call_opcode(n: usize) -> Option<Opcode> {
    match n {
        0 => Some(Opcode::Call0),
        1 => Some(Opcode::Call1),
        2 => Some(Opcode::Call2),
        3 => Some(Opcode::Call3),
        4 => Some(Opcode::Call4),
        5 => Some(Opcode::Call5),
        6 => Some(Opcode::Call6),
        7 => Some(Opcode::Call7),
        8 => Some(Opcode::Call8),
        _ => None,
    }
}
