// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, pr_opcodes

//! FTEQCC's opcode table as its type checker reads it (FTEF-23): each operator's spellings
//! with the types of their operands and result, the priority class the parser reads them at,
//! and how each stores.
//!
//! An operator's type is the type of the opcode `QCC_PR_ChooseOpcode` picks for its operands
//! (`crate::check::convert`), so this table is the type rules of FTEQCC's operators. The rows
//! are `pr_opcodes[]` in its order, each keyed by its [`Op`], whose number is its row.

use qc_preproc::Op;

use crate::ty::{SimpleType, Type};

/// The type an opcode reads or writes, `pr_opcodes[]`'s `type_a`, `type_b` and `type_c`: a
/// basic type, or `None` where the row has none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpType {
    /// No operand (`NULL`).
    None,
    /// `type_void`.
    Void,
    /// `type_float`.
    Float,
    /// `type_vector`.
    Vector,
    /// `type_string`.
    String,
    /// `type_entity`.
    Entity,
    /// `type_field`: a field of any type.
    Field,
    /// `type_function`: a function of any type.
    Function,
    /// `type_pointer`: a pointer to any type.
    Pointer,
    /// `type_integer`.
    Integer,
    /// `type_uint`.
    Uint,
    /// `type_int64`.
    Int64,
    /// `type_uint64`.
    Uint64,
    /// `type_double`.
    Double,
    /// `type_variant`: any one-word value.
    Variant,
    /// `type_bfloat`: a float that is a truth value, the result of a comparison.
    BoolFloat,
    /// `type_bint`: an integer that is a truth value.
    BoolInt,
    /// `type_floatpointer`: a pointer to a float.
    FloatPointer,
    /// `type_intpointer`: a pointer to an integer.
    IntPointer,
}

impl OpType {
    /// The type an operand or result of this type has.
    pub(crate) fn value_type(self) -> Type {
        match self {
            Self::None | Self::Void => Type::Simple(SimpleType::Void),
            Self::Float => Type::Simple(SimpleType::Float),
            // `type_bfloat` and `type_bint`, a truth value's types, named as such (FTEF-23.7).
            Self::BoolFloat => Type::Named("__bfloat".into()),
            Self::BoolInt => Type::Named("__bint".into()),
            Self::Vector => Type::Simple(SimpleType::Vector),
            Self::String => Type::Simple(SimpleType::String),
            Self::Entity => Type::Simple(SimpleType::Entity),
            Self::Integer => Type::Named("__int32".into()),
            Self::Uint => Type::Named("__uint32".into()),
            Self::Int64 => Type::Named("__int64".into()),
            Self::Uint64 => Type::Named("__uint64".into()),
            Self::Double => Type::Named("__double".into()),
            Self::Variant => Type::Named("__variant".into()),
            Self::Field | Self::Function => Type::Unknown,
            Self::Pointer | Self::FloatPointer => {
                Type::Pointer(Type::Simple(SimpleType::Float).into())
            }
            Self::IntPointer => Type::Pointer(Type::Named("__int32".into()).into()),
        }
    }
}

/// The priority class an opcode's operator is read at (`PC_*`); `QCC_PrioritiseOpcodes` maps
/// each to a priority by the operator precedence in force (FTEF-13.24, FTEF-21.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    /// No operator: the opcode is only emitted.
    None,
    /// A unary operator.
    Unary,
    /// `.` and `->`.
    Member,
    /// `*`, `/`, `%` and the like.
    MulDiv,
    /// `+` and `-`.
    AddSub,
    /// `<<` and `>>`.
    Shift,
    /// `<`, `<=`, `>` and `>=`.
    Relation,
    /// `==`, `!=` and `<=>`.
    Equality,
    /// `&`.
    BitAnd,
    /// `^`.
    BitXor,
    /// `|`.
    BitOr,
    /// `&&`.
    LogicAnd,
    /// `||`.
    LogicOr,
    /// `=` and the compound assignments.
    Store,
}

/// How an opcode's operands associate (`ASSOC_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Assoc {
    /// `ASSOC_LEFT`: reads `a` and `b`, the ordinary binary operator.
    Left,
    /// `ASSOC_RIGHT`: a store into `b`.
    Right,
    /// `ASSOC_RIGHT_RESULT`: a store whose result is the value stored.
    RightResult,
}

/// An opcode's `OPF_*` flags, as the table gives them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Flags(u16);

impl Flags {
    /// No flag.
    pub(crate) const NONE: Self = Self(0);
    /// `OPF_STD`: reads `a` and `b`, writes `c`.
    pub(crate) const STD: Self = Self(0x002);
    /// `OPF_STORE`: `b = a`, or `b += a`.
    pub(crate) const STORE: Self = Self(0x010);
    /// `OPF_STOREPTR`: `c = (*b += a)`.
    pub(crate) const STORE_PTR: Self = Self(0x020);
    /// `OPF_STOREPTROFS`: `a[c] = b`.
    pub(crate) const STORE_PTR_OFS: Self = Self(0x040);
    /// `OPF_STOREFLD`: `a.b = c`.
    pub(crate) const STORE_FLD: Self = Self(0x080);
    /// `OPF_LOADPTR`.
    pub(crate) const LOAD_PTR: Self = Self(0x100);
    /// `OPF_STDUNARY`: reads `a`, writes `c`.
    pub(crate) const STD_UNARY: Self = Self(0x200);
}

/// One row of `pr_opcodes[]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Opcode {
    /// The opcode the row is, its `OP_*` constant.
    pub(crate) op: Op,
    /// `targetflags`: 6 for id's opcodes, 7 for the extensions.
    pub(crate) target: u8,
    /// `name`: the operator it implements (`"+"`), or `<NAME>` for an opcode no operator writes.
    pub(crate) name: &'static str,
    /// `priorityclass`.
    pub(crate) class: Class,
    /// `associative`.
    pub(crate) assoc: Assoc,
    /// `type_a`, `type_b` and `type_c`.
    pub(crate) types: [OpType; 3],
    /// `flags`.
    pub(crate) flags: Flags,
}

/// A row of the table.
const fn entry(
    op: Op,
    target: u8,
    name: &'static str,
    class: Class,
    assoc: Assoc,
    types: [OpType; 3],
    flags: Flags,
) -> Opcode {
    Opcode {
        op,
        target,
        name,
        class,
        assoc,
        types,
        flags,
    }
}

use OpType as T;

/// `pr_opcodes[]`, in its order.
#[rustfmt::skip]
pub(crate) static OPCODES: [Opcode; 413] = [
    entry(Op::Done, 6, "<DONE>", Class::None, Assoc::Left, [T::Void, T::Void, T::Void], Flags::NONE),
    entry(Op::MulF, 6, "*", Class::MulDiv, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::MulV, 6, "*", Class::MulDiv, Assoc::Left, [T::Vector, T::Vector, T::Float], Flags::STD),
    entry(Op::MulFv, 6, "*", Class::MulDiv, Assoc::Left, [T::Float, T::Vector, T::Vector], Flags::STD),
    entry(Op::MulVf, 6, "*", Class::MulDiv, Assoc::Left, [T::Vector, T::Float, T::Vector], Flags::STD),
    entry(Op::DivF, 6, "/", Class::MulDiv, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::AddF, 6, "+", Class::AddSub, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::AddV, 6, "+", Class::AddSub, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    entry(Op::SubF, 6, "-", Class::AddSub, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::SubV, 6, "-", Class::AddSub, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    entry(Op::EqF, 6, "==", Class::Equality, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    entry(Op::EqV, 6, "==", Class::Equality, Assoc::Left, [T::Vector, T::Vector, T::BoolFloat], Flags::STD),
    entry(Op::EqS, 6, "==", Class::Equality, Assoc::Left, [T::String, T::String, T::BoolFloat], Flags::STD),
    entry(Op::EqE, 6, "==", Class::Equality, Assoc::Left, [T::Entity, T::Entity, T::BoolFloat], Flags::STD),
    entry(Op::EqFnc, 6, "==", Class::Equality, Assoc::Left, [T::Function, T::Function, T::BoolFloat], Flags::STD),
    entry(Op::NeF, 6, "!=", Class::Equality, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    entry(Op::NeV, 6, "!=", Class::Equality, Assoc::Left, [T::Vector, T::Vector, T::BoolFloat], Flags::STD),
    entry(Op::NeS, 6, "!=", Class::Equality, Assoc::Left, [T::String, T::String, T::BoolFloat], Flags::STD),
    entry(Op::NeE, 6, "!=", Class::Equality, Assoc::Left, [T::Entity, T::Entity, T::BoolFloat], Flags::STD),
    entry(Op::NeFnc, 6, "!=", Class::Equality, Assoc::Left, [T::Function, T::Function, T::BoolFloat], Flags::STD),
    entry(Op::LeF, 6, "<=", Class::Relation, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    entry(Op::GeF, 6, ">=", Class::Relation, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    entry(Op::LtF, 6, "<", Class::Relation, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    entry(Op::GtF, 6, ">", Class::Relation, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    entry(Op::LoadF, 6, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Float], Flags::NONE),
    entry(Op::LoadV, 6, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Vector], Flags::NONE),
    entry(Op::LoadS, 6, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::String], Flags::NONE),
    entry(Op::LoadEnt, 6, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Entity], Flags::NONE),
    entry(Op::LoadFld, 6, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Field], Flags::NONE),
    entry(Op::LoadFnc, 6, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Function], Flags::NONE),
    entry(Op::Address, 6, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Pointer], Flags::NONE),
    entry(Op::StoreF, 6, "=", Class::Store, Assoc::Right, [T::Float, T::Float, T::Float], Flags::STORE),
    entry(Op::StoreV, 6, "=", Class::Store, Assoc::Right, [T::Vector, T::Vector, T::Vector], Flags::STORE),
    entry(Op::StoreS, 6, "=", Class::Store, Assoc::Right, [T::String, T::String, T::String], Flags::STORE),
    entry(Op::StoreEnt, 6, "=", Class::Store, Assoc::Right, [T::Entity, T::Entity, T::Entity], Flags::STORE),
    entry(Op::StoreFld, 6, "=", Class::Store, Assoc::Right, [T::Field, T::Field, T::Field], Flags::STORE),
    entry(Op::StoreFnc, 6, "=", Class::Store, Assoc::Right, [T::Function, T::Function, T::Function], Flags::STORE),
    entry(Op::StorepF, 6, "=", Class::Store, Assoc::Right, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR_OFS),
    entry(Op::StorepV, 6, "=", Class::Store, Assoc::Right, [T::Pointer, T::Vector, T::Vector], Flags::STORE_PTR_OFS),
    entry(Op::StorepS, 6, "=", Class::Store, Assoc::Right, [T::Pointer, T::String, T::String], Flags::STORE_PTR_OFS),
    entry(Op::StorepEnt, 6, "=", Class::Store, Assoc::Right, [T::Pointer, T::Entity, T::Entity], Flags::STORE_PTR_OFS),
    entry(Op::StorepFld, 6, "=", Class::Store, Assoc::Right, [T::Pointer, T::Field, T::Field], Flags::STORE_PTR_OFS),
    entry(Op::StorepFnc, 6, "=", Class::Store, Assoc::Right, [T::Pointer, T::Function, T::Function], Flags::STORE_PTR_OFS),
    entry(Op::Return, 6, "<RETURN>", Class::None, Assoc::Left, [T::Vector, T::Void, T::Void], Flags::NONE),
    entry(Op::NotF, 6, "!", Class::Unary, Assoc::Left, [T::Float, T::Void, T::BoolFloat], Flags::STD_UNARY),
    entry(Op::NotV, 6, "!", Class::Unary, Assoc::Left, [T::Vector, T::Void, T::BoolFloat], Flags::STD_UNARY),
    entry(Op::NotS, 6, "!", Class::Unary, Assoc::Left, [T::Vector, T::Void, T::BoolFloat], Flags::STD_UNARY),
    entry(Op::NotEnt, 6, "!", Class::Unary, Assoc::Left, [T::Entity, T::Void, T::BoolFloat], Flags::STD_UNARY),
    entry(Op::NotFnc, 6, "!", Class::Unary, Assoc::Left, [T::Function, T::Void, T::BoolFloat], Flags::STD_UNARY),
    entry(Op::IfI, 6, "<IF>", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    entry(Op::IfnotI, 6, "<IFNOT>", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    entry(Op::Call0, 6, "<CALL0>", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    entry(Op::Call1, 6, "<CALL1>", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    entry(Op::Call2, 6, "<CALL2>", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    entry(Op::Call3, 6, "<CALL3>", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    entry(Op::Call4, 6, "<CALL4>", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    entry(Op::Call5, 6, "<CALL5>", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    entry(Op::Call6, 6, "<CALL6>", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    entry(Op::Call7, 6, "<CALL7>", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    entry(Op::Call8, 6, "<CALL8>", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    entry(Op::State, 6, "<STATE>", Class::None, Assoc::Left, [T::Float, T::Float, T::Void], Flags::NONE),
    entry(Op::Goto, 6, "<GOTO>", Class::None, Assoc::Right, [T::None, T::Void, T::Void], Flags::NONE),
    entry(Op::AndF, 6, "&&", Class::LogicAnd, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    entry(Op::OrF, 6, "||", Class::LogicOr, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    entry(Op::BitandF, 6, "&", Class::BitAnd, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::BitorF, 6, "|", Class::BitOr, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::MulstoreF, 7, "*=", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    entry(Op::MulstoreVf, 7, "*=", Class::Store, Assoc::RightResult, [T::Vector, T::Float, T::Vector], Flags::STORE),
    entry(Op::MulstorepF, 7, "*=", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::MulstorepVf, 7, "*=", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Vector], Flags::STORE_PTR),
    entry(Op::DivstoreF, 7, "/=", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    entry(Op::DivstorepF, 7, "/=", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::AddstoreF, 7, "+=", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    entry(Op::AddstoreV, 7, "+=", Class::Store, Assoc::RightResult, [T::Vector, T::Vector, T::Vector], Flags::STORE),
    entry(Op::AddstorepF, 7, "+=", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::AddstorepV, 7, "+=", Class::Store, Assoc::RightResult, [T::Pointer, T::Vector, T::Vector], Flags::STORE_PTR),
    entry(Op::SubstoreF, 7, "-=", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    entry(Op::SubstoreV, 7, "-=", Class::Store, Assoc::RightResult, [T::Vector, T::Vector, T::Vector], Flags::STORE),
    entry(Op::SubstorepF, 7, "-=", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::SubstorepV, 7, "-=", Class::Store, Assoc::RightResult, [T::Pointer, T::Vector, T::Vector], Flags::STORE_PTR),
    entry(Op::FetchGblF, 7, "<FETCH_GBL_F>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::FetchGblV, 7, "<FETCH_GBL_V>", Class::None, Assoc::Left, [T::Vector, T::Float, T::Vector], Flags::NONE),
    entry(Op::FetchGblS, 7, "<FETCH_GBL_S>", Class::None, Assoc::Left, [T::String, T::Float, T::String], Flags::NONE),
    entry(Op::FetchGblE, 7, "<FETCH_GBL_E>", Class::None, Assoc::Left, [T::Entity, T::Float, T::Entity], Flags::NONE),
    entry(Op::FetchGblFnc, 7, "<FETCH_GBL_FNC>", Class::None, Assoc::Left, [T::Function, T::Float, T::Function], Flags::NONE),
    entry(Op::Cstate, 7, "<CSTATE>", Class::None, Assoc::Left, [T::Float, T::Float, T::Void], Flags::NONE),
    entry(Op::Cwstate, 7, "<CWSTATE>", Class::None, Assoc::Left, [T::Float, T::Float, T::Void], Flags::NONE),
    entry(Op::Thinktime, 7, "<THINKTIME>", Class::None, Assoc::Left, [T::Entity, T::Float, T::Void], Flags::NONE),
    entry(Op::BitsetstoreF, 7, "|=", Class::Store, Assoc::Right, [T::Float, T::Float, T::Float], Flags::STORE),
    entry(Op::BitsetstorepF, 7, "|=", Class::Store, Assoc::Right, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::BitclrstoreF, 7, "&~=", Class::Store, Assoc::Right, [T::Float, T::Float, T::Float], Flags::STORE),
    entry(Op::BitclrstorepF, 7, "&~=", Class::Store, Assoc::Right, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::Rand0, 7, "<RAND0>", Class::None, Assoc::Left, [T::Void, T::Void, T::Float], Flags::NONE),
    entry(Op::Rand1, 7, "<RAND1>", Class::None, Assoc::Left, [T::Float, T::Void, T::Float], Flags::NONE),
    entry(Op::Rand2, 7, "<RAND2>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::Randv0, 7, "<RANDV0>", Class::None, Assoc::Left, [T::Void, T::Void, T::Vector], Flags::NONE),
    entry(Op::Randv1, 7, "<RANDV1>", Class::None, Assoc::Left, [T::Vector, T::Void, T::Vector], Flags::NONE),
    entry(Op::Randv2, 7, "<RANDV2>", Class::None, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::NONE),
    entry(Op::SwitchF, 7, "<SWITCH_F>", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    entry(Op::SwitchV, 7, "<SWITCH_V>", Class::None, Assoc::Right, [T::Vector, T::None, T::Void], Flags::NONE),
    entry(Op::SwitchS, 7, "<SWITCH_S>", Class::None, Assoc::Right, [T::String, T::None, T::Void], Flags::NONE),
    entry(Op::SwitchE, 7, "<SWITCH_E>", Class::None, Assoc::Right, [T::Entity, T::None, T::Void], Flags::NONE),
    entry(Op::SwitchFnc, 7, "<SWITCH_FNC>", Class::None, Assoc::Right, [T::Function, T::None, T::Void], Flags::NONE),
    entry(Op::Case, 7, "<CASE>", Class::None, Assoc::Right, [T::Variant, T::None, T::Void], Flags::NONE),
    entry(Op::Caserange, 7, "<CASERANGE>", Class::None, Assoc::Right, [T::Float, T::Float, T::None], Flags::NONE),
    entry(Op::Call1h, 7, "<CALL1H>", Class::None, Assoc::Right, [T::Function, T::Variant, T::Void], Flags::NONE),
    entry(Op::Call2h, 7, "<CALL2H>", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    entry(Op::Call3h, 7, "<CALL3H>", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    entry(Op::Call4h, 7, "<CALL4H>", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    entry(Op::Call5h, 7, "<CALL5H>", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    entry(Op::Call6h, 7, "<CALL6H>", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    entry(Op::Call7h, 7, "<CALL7H>", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    entry(Op::Call8h, 7, "<CALL8H>", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    entry(Op::StoreI, 7, "=", Class::Store, Assoc::Right, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    entry(Op::StoreIf, 7, "=", Class::Store, Assoc::Right, [T::Float, T::Integer, T::Integer], Flags::STORE),
    entry(Op::StoreFi, 7, "=", Class::Store, Assoc::Right, [T::Integer, T::Float, T::Float], Flags::STORE),
    entry(Op::AddI, 7, "+", Class::AddSub, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::AddFi, 7, "+", Class::AddSub, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    entry(Op::AddIf, 7, "+", Class::AddSub, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    entry(Op::SubI, 7, "-", Class::AddSub, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::SubFi, 7, "-", Class::AddSub, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    entry(Op::SubIf, 7, "-", Class::AddSub, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    entry(Op::ConvItof, 7, "<CIF>", Class::Store, Assoc::Left, [T::Integer, T::Void, T::Float], Flags::NONE),
    entry(Op::ConvFtoi, 7, "<CFI>", Class::Store, Assoc::Left, [T::Float, T::Void, T::Integer], Flags::NONE),
    entry(Op::LoadpItof, 7, "<CPIF>", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Float], Flags::NONE),
    entry(Op::LoadpFtoi, 7, "<CPFI>", Class::Store, Assoc::Left, [T::Pointer, T::Float, T::Integer], Flags::NONE),
    entry(Op::LoadI, 7, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Integer], Flags::NONE),
    entry(Op::StorepI, 7, "=", Class::Store, Assoc::Right, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR_OFS),
    entry(Op::StorepIf, 7, "=", Class::Store, Assoc::Right, [T::Pointer, T::Float, T::Integer], Flags::STORE_PTR_OFS),
    entry(Op::StorepFi, 7, "=", Class::Store, Assoc::Right, [T::Pointer, T::Integer, T::Float], Flags::STORE_PTR_OFS),
    entry(Op::BitandI, 7, "&", Class::BitAnd, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::BitorI, 7, "|", Class::BitOr, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::MulI, 7, "*", Class::MulDiv, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::DivI, 7, "/", Class::MulDiv, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::EqI, 7, "==", Class::Equality, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::NeI, 7, "!=", Class::Equality, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::IfnotS, 7, "<IFNOTS>", Class::None, Assoc::Right, [T::String, T::None, T::Void], Flags::NONE),
    entry(Op::IfS, 7, "<IFS>", Class::None, Assoc::Right, [T::String, T::None, T::Void], Flags::NONE),
    entry(Op::NotI, 7, "!", Class::Unary, Assoc::Left, [T::Integer, T::Void, T::BoolInt], Flags::STD_UNARY),
    entry(Op::DivVf, 7, "/", Class::MulDiv, Assoc::Left, [T::Vector, T::Float, T::Vector], Flags::STD),
    entry(Op::BitxorI, 7, "^", Class::BitXor, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::RshiftI, 7, ">>", Class::Shift, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::LshiftI, 7, "<<", Class::Shift, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::Globaladdress, 7, "<ARRAY>", Class::None, Assoc::Left, [T::Float, T::Integer, T::Pointer], Flags::NONE),
    entry(Op::AddPiw, 7, "<ARRAY>", Class::None, Assoc::Left, [T::Pointer, T::Integer, T::Pointer], Flags::NONE),
    entry(Op::LoadaF, 7, "=", Class::Store, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::NONE),
    entry(Op::LoadaV, 7, "=", Class::Store, Assoc::Left, [T::Vector, T::Integer, T::Vector], Flags::NONE),
    entry(Op::LoadaS, 7, "=", Class::Store, Assoc::Left, [T::String, T::Integer, T::String], Flags::NONE),
    entry(Op::LoadaEnt, 7, "=", Class::Store, Assoc::Left, [T::Entity, T::Integer, T::Entity], Flags::NONE),
    entry(Op::LoadaFld, 7, "=", Class::Store, Assoc::Left, [T::Field, T::Integer, T::Field], Flags::NONE),
    entry(Op::LoadaFnc, 7, "=", Class::Store, Assoc::Left, [T::Function, T::Integer, T::Function], Flags::NONE),
    entry(Op::LoadaI, 7, "=", Class::Store, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::NONE),
    entry(Op::StoreP, 7, "=", Class::Store, Assoc::Right, [T::Pointer, T::Pointer, T::Void], Flags::STORE),
    entry(Op::LoadP, 7, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Pointer], Flags::NONE),
    entry(Op::LoadpF, 7, "=", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Float], Flags::LOAD_PTR),
    entry(Op::LoadpV, 7, "=", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Vector], Flags::LOAD_PTR),
    entry(Op::LoadpS, 7, "=", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::String], Flags::LOAD_PTR),
    entry(Op::LoadpEnt, 7, "=", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Entity], Flags::LOAD_PTR),
    entry(Op::LoadpFld, 7, "=", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Field], Flags::LOAD_PTR),
    entry(Op::LoadpFnc, 7, "=", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Function], Flags::LOAD_PTR),
    entry(Op::LoadpI, 7, "=", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Integer], Flags::LOAD_PTR),
    entry(Op::LeI, 7, "<=", Class::Relation, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::GeI, 7, ">=", Class::Relation, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::LtI, 7, "<", Class::Relation, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::GtI, 7, ">", Class::Relation, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::LeIf, 7, "<=", Class::Relation, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    entry(Op::GeIf, 7, ">=", Class::Relation, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    entry(Op::LtIf, 7, "<", Class::Relation, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    entry(Op::GtIf, 7, ">", Class::Relation, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    entry(Op::LeFi, 7, "<=", Class::Relation, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::GeFi, 7, ">=", Class::Relation, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::LtFi, 7, "<", Class::Relation, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::GtFi, 7, ">", Class::Relation, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::EqIf, 7, "==", Class::Equality, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    entry(Op::EqFi, 7, "==", Class::Equality, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::AddSf, 7, "+", Class::AddSub, Assoc::Left, [T::String, T::Float, T::String], Flags::STD),
    entry(Op::SubS, 7, "-", Class::AddSub, Assoc::Left, [T::String, T::String, T::Float], Flags::STD),
    entry(Op::StorepC, 7, "<STOREP_C>", Class::Store, Assoc::Right, [T::String, T::Float, T::Float], Flags::STORE_PTR_OFS),
    entry(Op::LoadpC, 7, "<LOADP_C>", Class::Store, Assoc::Left, [T::String, T::Float, T::Float], Flags::LOAD_PTR),
    entry(Op::MulIf, 7, "*", Class::MulDiv, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    entry(Op::MulFi, 7, "*", Class::MulDiv, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    entry(Op::MulVi, 7, "*", Class::MulDiv, Assoc::Left, [T::Vector, T::Integer, T::Vector], Flags::STD),
    entry(Op::MulIv, 7, "*", Class::MulDiv, Assoc::Left, [T::Integer, T::Vector, T::Vector], Flags::STD),
    entry(Op::DivIf, 7, "/", Class::MulDiv, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    entry(Op::DivFi, 7, "/", Class::MulDiv, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    entry(Op::BitandIf, 7, "&", Class::BitAnd, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    entry(Op::BitorIf, 7, "|", Class::BitOr, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    entry(Op::BitandFi, 7, "&", Class::BitAnd, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    entry(Op::BitorFi, 7, "|", Class::BitOr, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    entry(Op::AndI, 7, "&&", Class::LogicAnd, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::OrI, 7, "||", Class::LogicOr, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::AndIf, 7, "&&", Class::LogicAnd, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    entry(Op::OrIf, 7, "||", Class::LogicOr, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    entry(Op::AndFi, 7, "&&", Class::LogicAnd, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::OrFi, 7, "||", Class::LogicOr, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::NeIf, 7, "!=", Class::Equality, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    entry(Op::NeFi, 7, "!=", Class::Equality, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    entry(Op::GstorepI, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GstorepF, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GstorepEnt, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GstorepFld, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GstorepS, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GstorepFnc, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GstorepV, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::Gaddress, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GloadI, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GloadF, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GloadFld, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GloadEnt, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GloadS, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::GloadFnc, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::Boundcheck, 7, "<>", Class::None, Assoc::Left, [T::Integer, T::None, T::None], Flags::NONE),
    entry(Op::Unused, 7, "<UNUSED>", Class::None, Assoc::Right, [T::Void, T::Void, T::Void], Flags::NONE),
    entry(Op::Push, 7, "<PUSH>", Class::None, Assoc::Right, [T::Float, T::Void, T::Pointer], Flags::NONE),
    entry(Op::Pop, 7, "<POP>", Class::None, Assoc::Right, [T::Float, T::Void, T::Void], Flags::NONE),
    entry(Op::SwitchI, 7, "<SWITCH_I>", Class::None, Assoc::Left, [T::Void, T::None, T::Void], Flags::NONE),
    entry(Op::GloadV, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::IfF, 7, "<IF_F>", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    entry(Op::IfnotF, 7, "<IFNOT_F>", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    entry(Op::StorefV, 7, "=", Class::None, Assoc::Right, [T::Entity, T::Field, T::Vector], Flags::STORE_FLD),
    entry(Op::StorefF, 7, "=", Class::None, Assoc::Right, [T::Entity, T::Field, T::Float], Flags::STORE_FLD),
    entry(Op::StorefS, 7, "=", Class::None, Assoc::Right, [T::Entity, T::Field, T::String], Flags::STORE_FLD),
    entry(Op::StorefI, 7, "=", Class::None, Assoc::Right, [T::Entity, T::Field, T::Integer], Flags::STORE_FLD),
    entry(Op::StorepI8, 7, "<STOREP_B>", Class::Store, Assoc::Right, [T::String, T::Integer, T::Integer], Flags::STORE_PTR_OFS),
    entry(Op::LoadpU8, 7, "<LOADP_B>", Class::Store, Assoc::Left, [T::String, T::Integer, T::Integer], Flags::LOAD_PTR),
    entry(Op::LeU, 7, "<=", Class::Relation, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::LOAD_PTR),
    entry(Op::LtU, 7, "<", Class::Relation, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::LOAD_PTR),
    entry(Op::DivU, 7, "/", Class::MulDiv, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::LOAD_PTR),
    entry(Op::RshiftU, 7, ">>", Class::Shift, Assoc::Left, [T::Uint, T::Integer, T::Uint], Flags::LOAD_PTR),
    entry(Op::AddI64, 7, "+", Class::AddSub, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    entry(Op::SubI64, 7, "-", Class::AddSub, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    entry(Op::MulI64, 7, "*", Class::MulDiv, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    entry(Op::DivI64, 7, "/", Class::MulDiv, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    entry(Op::BitandI64, 7, "&", Class::BitAnd, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    entry(Op::BitorI64, 7, "|", Class::BitOr, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    entry(Op::BitxorI64, 7, "^", Class::BitXor, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    entry(Op::LshiftI64i, 7, "<<", Class::Shift, Assoc::Left, [T::Int64, T::Integer, T::Int64], Flags::STD),
    entry(Op::RshiftI64i, 7, ">>", Class::Shift, Assoc::Left, [T::Int64, T::Integer, T::Int64], Flags::STD),
    entry(Op::LeI64, 7, "<=", Class::Relation, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    entry(Op::LtI64, 7, "<", Class::Relation, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    entry(Op::EqI64, 7, "==", Class::Equality, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    entry(Op::NeI64, 7, "!=", Class::Equality, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    entry(Op::LeU64, 7, "<", Class::Relation, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    entry(Op::LtU64, 7, "<", Class::Relation, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    entry(Op::DivU64, 7, "%", Class::MulDiv, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    entry(Op::RshiftU64i, 7, ">>", Class::Shift, Assoc::Left, [T::Uint64, T::Uint, T::Uint64], Flags::STD),
    entry(Op::StoreI64, 7, "=", Class::Store, Assoc::Right, [T::Int64, T::Int64, T::Int64], Flags::STORE),
    entry(Op::StorepI64, 7, "=", Class::Store, Assoc::Right, [T::Pointer, T::Int64, T::Int64], Flags::STORE_PTR_OFS),
    entry(Op::StorefI64, 7, "<=>", Class::None, Assoc::Right, [T::Entity, T::Field, T::Int64], Flags::STORE_FLD),
    entry(Op::LoadI64, 7, ".", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Pointer], Flags::NONE),
    entry(Op::LoadaI64, 7, "=", Class::Store, Assoc::Left, [T::Int64, T::Integer, T::Int64], Flags::NONE),
    entry(Op::LoadpI64, 7, "=", Class::Store, Assoc::Left, [T::Pointer, T::Int64, T::Int64], Flags::LOAD_PTR),
    entry(Op::ConvUi64, 7, "=", Class::Store, Assoc::Left, [T::Uint, T::Void, T::Int64], Flags::NONE),
    entry(Op::ConvIi64, 7, "=", Class::Store, Assoc::Left, [T::Integer, T::Void, T::Int64], Flags::NONE),
    entry(Op::ConvI64i, 7, "=", Class::Store, Assoc::Left, [T::Int64, T::Void, T::Integer], Flags::NONE),
    entry(Op::ConvFd, 7, "=", Class::Store, Assoc::Left, [T::Float, T::Void, T::Double], Flags::NONE),
    entry(Op::ConvDf, 7, "=", Class::Store, Assoc::Left, [T::Double, T::Void, T::Float], Flags::NONE),
    entry(Op::ConvI64f, 7, "=", Class::Store, Assoc::Left, [T::Int64, T::Void, T::Float], Flags::NONE),
    entry(Op::ConvFi64, 7, "=", Class::Store, Assoc::Left, [T::Float, T::Void, T::Int64], Flags::NONE),
    entry(Op::ConvI64d, 7, "=", Class::Store, Assoc::Left, [T::Int64, T::Void, T::Double], Flags::NONE),
    entry(Op::ConvDi64, 7, "=", Class::Store, Assoc::Left, [T::Double, T::Void, T::Int64], Flags::NONE),
    entry(Op::AddD, 7, "+", Class::AddSub, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    entry(Op::SubD, 7, "-", Class::AddSub, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    entry(Op::MulD, 7, "*", Class::MulDiv, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    entry(Op::DivD, 7, "/", Class::MulDiv, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    entry(Op::LeD, 7, "<=", Class::Relation, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    entry(Op::LtD, 7, "<", Class::Relation, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    entry(Op::EqD, 7, "==", Class::Equality, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    entry(Op::NeD, 7, "!=", Class::Equality, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    entry(Op::StorepI16, 7, "<STOREP_I16>", Class::Store, Assoc::Right, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR_OFS),
    entry(Op::LoadpI16, 7, "<LOADP_I16>", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Integer], Flags::LOAD_PTR),
    entry(Op::LoadpU16, 7, "<LOADP_U16>", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Integer], Flags::LOAD_PTR),
    entry(Op::LoadpI8, 7, "<LOADP_I8>", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Integer], Flags::LOAD_PTR),
    entry(Op::BitextendI, 7, "<BITEXTEND_I>", Class::None, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::BitextendU, 7, "<BITEXTEND_U>", Class::None, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::BitcopyI, 7, "<BITCOPY_I>", Class::None, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::ConvUf, 7, "=", Class::Store, Assoc::Left, [T::Uint, T::Void, T::Float], Flags::NONE),
    entry(Op::ConvFu, 7, "=", Class::Store, Assoc::Left, [T::Float, T::Void, T::Uint], Flags::NONE),
    entry(Op::ConvU64d, 7, "=", Class::Store, Assoc::Left, [T::Uint64, T::Void, T::Double], Flags::NONE),
    entry(Op::ConvDu64, 7, "=", Class::Store, Assoc::Left, [T::Double, T::Void, T::Uint64], Flags::NONE),
    entry(Op::ConvU64f, 7, "=", Class::Store, Assoc::Left, [T::Uint64, T::Void, T::Float], Flags::NONE),
    entry(Op::ConvFu64, 7, "=", Class::Store, Assoc::Left, [T::Float, T::Void, T::Uint64], Flags::NONE),
    entry(Op::Numrealops, 7, "<>", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    entry(Op::BitsetstoreI, 7, "|=", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    entry(Op::BitsetstorepI, 7, "|=", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    entry(Op::BitclrstoreI, 7, "&~=", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    entry(Op::MulstoreI, 7, "*=", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    entry(Op::DivstoreI, 7, "/=", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    entry(Op::AddstoreI, 7, "+=", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    entry(Op::SubstoreI, 7, "-=", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    entry(Op::MulstorepI, 7, "*=", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    entry(Op::DivstorepI, 7, "/=", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    entry(Op::AddstorepI, 7, "+=", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    entry(Op::SubstorepI, 7, "-=", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    entry(Op::MulstoreIf, 7, "*=", Class::Store, Assoc::RightResult, [T::Integer, T::Float, T::Float], Flags::STORE),
    entry(Op::MulstorepIf, 7, "*=", Class::Store, Assoc::RightResult, [T::IntPointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::DivstoreIf, 7, "/=", Class::Store, Assoc::RightResult, [T::Integer, T::Float, T::Float], Flags::STORE),
    entry(Op::DivstorepIf, 7, "/=", Class::Store, Assoc::RightResult, [T::IntPointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::AddstoreIf, 7, "+=", Class::Store, Assoc::RightResult, [T::Integer, T::Float, T::Float], Flags::STORE),
    entry(Op::AddstorepIf, 7, "+=", Class::Store, Assoc::RightResult, [T::IntPointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::SubstoreIf, 7, "-=", Class::Store, Assoc::RightResult, [T::Integer, T::Float, T::Float], Flags::STORE),
    entry(Op::SubstorepIf, 7, "-=", Class::Store, Assoc::RightResult, [T::IntPointer, T::Float, T::Float], Flags::STORE_PTR),
    entry(Op::MulstoreFi, 7, "*=", Class::Store, Assoc::RightResult, [T::Float, T::Integer, T::Float], Flags::STORE),
    entry(Op::MulstorepFi, 7, "*=", Class::Store, Assoc::RightResult, [T::FloatPointer, T::Integer, T::Float], Flags::STORE_PTR),
    entry(Op::DivstoreFi, 7, "/=", Class::Store, Assoc::RightResult, [T::Float, T::Integer, T::Float], Flags::STORE),
    entry(Op::DivstorepFi, 7, "/=", Class::Store, Assoc::RightResult, [T::FloatPointer, T::Integer, T::Float], Flags::STORE_PTR),
    entry(Op::AddstoreFi, 7, "+=", Class::Store, Assoc::RightResult, [T::Float, T::Integer, T::Float], Flags::STORE),
    entry(Op::AddstorepFi, 7, "+=", Class::Store, Assoc::RightResult, [T::FloatPointer, T::Integer, T::Float], Flags::STORE_PTR),
    entry(Op::SubstoreFi, 7, "-=", Class::Store, Assoc::RightResult, [T::Float, T::Integer, T::Float], Flags::STORE),
    entry(Op::SubstorepFi, 7, "-=", Class::Store, Assoc::RightResult, [T::FloatPointer, T::Integer, T::Float], Flags::STORE_PTR),
    entry(Op::MulstoreVi, 7, "*=", Class::Store, Assoc::RightResult, [T::Vector, T::Integer, T::Vector], Flags::STORE),
    entry(Op::MulstorepVi, 7, "*=", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Vector], Flags::STORE_PTR),
    entry(Op::LoadaStruct, 7, "=", Class::Store, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::NONE),
    entry(Op::LoadpP, 7, "=", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Pointer], Flags::LOAD_PTR),
    entry(Op::StorepP, 7, "=", Class::Store, Assoc::Right, [T::Pointer, T::Pointer, T::Pointer], Flags::STORE_PTR_OFS),
    entry(Op::BitnotF, 7, "~", Class::Unary, Assoc::Left, [T::Float, T::Void, T::Float], Flags::STD_UNARY),
    entry(Op::BitnotI, 7, "~", Class::Unary, Assoc::Left, [T::Integer, T::Void, T::Integer], Flags::STD_UNARY),
    entry(Op::EqP, 7, "==", Class::Equality, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    entry(Op::NeP, 7, "!=", Class::Equality, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    entry(Op::LeP, 7, "<=", Class::Relation, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    entry(Op::GeP, 7, ">=", Class::Relation, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    entry(Op::LtP, 7, "<", Class::Relation, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    entry(Op::GtP, 7, ">", Class::Relation, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    entry(Op::AndstoreF, 7, "&=", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    entry(Op::BitclrF, 7, "&~=", Class::Store, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STORE),
    entry(Op::BitclrI, 7, "&~=", Class::Store, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    entry(Op::BitclrV, 7, "&~=", Class::Store, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STORE),
    entry(Op::AddSi, 7, "+", Class::AddSub, Assoc::Left, [T::String, T::Integer, T::String], Flags::STD),
    entry(Op::AddIs, 7, "+", Class::AddSub, Assoc::Left, [T::Integer, T::String, T::String], Flags::STD),
    entry(Op::AddPf, 7, "+", Class::AddSub, Assoc::Left, [T::Pointer, T::Float, T::Pointer], Flags::STD),
    entry(Op::AddFp, 7, "+", Class::AddSub, Assoc::Left, [T::Float, T::Pointer, T::Pointer], Flags::STD),
    entry(Op::AddPi, 7, "+", Class::AddSub, Assoc::Left, [T::Pointer, T::Integer, T::Pointer], Flags::STD),
    entry(Op::AddIp, 7, "+", Class::AddSub, Assoc::Left, [T::Integer, T::Pointer, T::Pointer], Flags::STD),
    entry(Op::AddPu, 7, "+", Class::AddSub, Assoc::Left, [T::Pointer, T::Uint, T::Pointer], Flags::STD),
    entry(Op::AddUp, 7, "+", Class::AddSub, Assoc::Left, [T::Uint, T::Pointer, T::Pointer], Flags::STD),
    entry(Op::SubSi, 7, "-", Class::AddSub, Assoc::Left, [T::String, T::Integer, T::String], Flags::STD),
    entry(Op::SubPf, 7, "-", Class::AddSub, Assoc::Left, [T::Pointer, T::Float, T::Pointer], Flags::STD),
    entry(Op::SubPi, 7, "-", Class::AddSub, Assoc::Left, [T::Pointer, T::Integer, T::Pointer], Flags::STD),
    entry(Op::SubPu, 7, "-", Class::AddSub, Assoc::Left, [T::Pointer, T::Uint, T::Pointer], Flags::STD),
    entry(Op::SubPp, 7, "-", Class::AddSub, Assoc::Left, [T::Pointer, T::Pointer, T::Integer], Flags::STD),
    entry(Op::ModF, 7, "%", Class::MulDiv, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::ModI, 7, "%", Class::MulDiv, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::ModFi, 7, "%", Class::MulDiv, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    entry(Op::ModIf, 7, "%", Class::MulDiv, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    entry(Op::ModV, 7, "%", Class::MulDiv, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    entry(Op::BitxorF, 7, "^", Class::BitXor, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::RshiftF, 7, ">>", Class::Shift, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::LshiftF, 7, "<<", Class::Shift, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::RshiftIf, 7, ">>", Class::Shift, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    entry(Op::LshiftIf, 7, "<<", Class::Shift, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    entry(Op::RshiftFi, 7, ">>", Class::Shift, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    entry(Op::LshiftFi, 7, "<<", Class::Shift, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    entry(Op::AndAny, 7, "&&", Class::LogicAnd, Assoc::Left, [T::Variant, T::Variant, T::BoolFloat], Flags::STD),
    entry(Op::OrAny, 7, "||", Class::LogicOr, Assoc::Left, [T::Variant, T::Variant, T::BoolFloat], Flags::STD),
    entry(Op::AddEi, 7, "+", Class::AddSub, Assoc::Left, [T::Entity, T::Integer, T::Entity], Flags::STD),
    entry(Op::AddEf, 7, "+", Class::AddSub, Assoc::Left, [T::Entity, T::Float, T::Entity], Flags::STD),
    entry(Op::SubEi, 7, "-", Class::AddSub, Assoc::Left, [T::Entity, T::Integer, T::Entity], Flags::STD),
    entry(Op::SubEf, 7, "-", Class::AddSub, Assoc::Left, [T::Entity, T::Float, T::Entity], Flags::STD),
    entry(Op::BitandV, 7, "&", Class::BitAnd, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    entry(Op::BitorV, 7, "|", Class::BitOr, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    entry(Op::BitnotV, 7, "~", Class::Unary, Assoc::Left, [T::Vector, T::Void, T::Vector], Flags::STD),
    entry(Op::BitxorV, 7, "^", Class::BitXor, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    entry(Op::PowF, 7, "*^", Class::MulDiv, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::PowI, 7, "*^", Class::MulDiv, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    entry(Op::PowFi, 7, "*^", Class::MulDiv, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    entry(Op::PowIf, 7, "*^", Class::MulDiv, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    entry(Op::CrossV, 7, "><", Class::MulDiv, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    entry(Op::EqFld, 7, "==", Class::Equality, Assoc::Left, [T::Field, T::Field, T::BoolFloat], Flags::STD),
    entry(Op::NeFld, 7, "!=", Class::Equality, Assoc::Left, [T::Field, T::Field, T::BoolFloat], Flags::STD),
    entry(Op::SpaceshipF, 7, "<=>", Class::Equality, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    entry(Op::SpaceshipS, 7, "<=>", Class::Equality, Assoc::Left, [T::String, T::String, T::Float], Flags::STD),
    entry(Op::AddU, 7, "+", Class::AddSub, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    entry(Op::SubU, 7, "-", Class::AddSub, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    entry(Op::MulU, 7, "*", Class::MulDiv, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    entry(Op::ModU, 7, "%", Class::MulDiv, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    entry(Op::BitandU, 7, "&", Class::BitAnd, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    entry(Op::BitorU, 7, "|", Class::BitOr, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    entry(Op::BitxorU, 7, "^", Class::BitXor, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    entry(Op::BitnotU, 7, "~", Class::Unary, Assoc::Left, [T::Uint, T::Void, T::Uint], Flags::STD_UNARY),
    entry(Op::BitclrU, 7, "&~=", Class::Store, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STORE),
    entry(Op::LshiftU, 7, "<<", Class::Shift, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    entry(Op::GeU, 7, ">=", Class::Relation, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::STD),
    entry(Op::GtU, 7, ">", Class::Relation, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::STD),
    entry(Op::EqU, 7, "==", Class::Equality, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::STD),
    entry(Op::NeU, 7, "!=", Class::Equality, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::STD),
    entry(Op::BitnotI64, 7, "~", Class::Unary, Assoc::Left, [T::Int64, T::Void, T::Int64], Flags::STD_UNARY),
    entry(Op::BitclrI64, 7, "&~=", Class::Store, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STORE),
    entry(Op::GeI64, 7, ">=", Class::Relation, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    entry(Op::GtI64, 7, ">", Class::Relation, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    entry(Op::AddU64, 7, "+", Class::AddSub, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    entry(Op::SubU64, 7, "-", Class::AddSub, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    entry(Op::MulU64, 7, "*", Class::MulDiv, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    entry(Op::ModU64, 7, "%", Class::MulDiv, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    entry(Op::BitandU64, 7, "&", Class::BitAnd, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    entry(Op::BitorU64, 7, "|", Class::BitOr, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    entry(Op::BitxorU64, 7, "^", Class::BitXor, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    entry(Op::BitnotU64, 7, "~", Class::Unary, Assoc::Left, [T::Uint64, T::Void, T::Uint64], Flags::STD_UNARY),
    entry(Op::BitclrU64, 7, "&~=", Class::Store, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STORE),
    entry(Op::LshiftU64i, 7, "<<", Class::Shift, Assoc::Left, [T::Uint64, T::Integer, T::Uint64], Flags::STD),
    entry(Op::GeU64, 7, ">=", Class::Relation, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    entry(Op::GtU64, 7, ">", Class::Relation, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    entry(Op::EqU64, 7, "==", Class::Equality, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    entry(Op::NeU64, 7, "!=", Class::Equality, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    entry(Op::BitandD, 7, "&", Class::BitAnd, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    entry(Op::BitorD, 7, "|", Class::BitOr, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    entry(Op::BitxorD, 7, "^", Class::BitXor, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    entry(Op::BitnotD, 7, "~", Class::Unary, Assoc::Left, [T::Double, T::Void, T::BoolInt], Flags::STD_UNARY),
    entry(Op::BitclrD, 7, "&~=", Class::Store, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STORE),
    entry(Op::LshiftDi, 7, "<<", Class::Shift, Assoc::Left, [T::Double, T::Integer, T::Double], Flags::STD),
    entry(Op::RshiftDi, 7, ">>", Class::Shift, Assoc::Left, [T::Double, T::Integer, T::Double], Flags::STD),
    entry(Op::GeD, 7, ">=", Class::Relation, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    entry(Op::GtD, 7, ">", Class::Relation, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    entry(Op::Wstate, 7, "<WSTATE>", Class::None, Assoc::Left, [T::Float, T::Float, T::Void], Flags::NONE),
];

/// The row of `pr_opcodes[]` that is `op`.
pub(crate) fn row(op: Op) -> Option<&'static Opcode> {
    OPCODES.get(op.number())
}

impl Class {
    /// Whether `QCC_PrioritiseOpcodes` lists the class's rows for the expression parser: every
    /// class of a binary operator. The unary, member and assignment rows are found otherwise.
    const fn is_binary(self) -> bool {
        match self {
            Self::MulDiv
            | Self::AddSub
            | Self::Shift
            | Self::Relation
            | Self::Equality
            | Self::BitAnd
            | Self::BitXor
            | Self::BitOr
            | Self::LogicAnd
            | Self::LogicOr => true,
            Self::None | Self::Unary | Self::Member | Self::Store => false,
        }
    }
}

/// The opcodes a binary operator spelled `spelling` may be given, in table order
/// (FTEF-23.1).
pub(crate) fn candidates(spelling: &str) -> Vec<Op> {
    let mut out = Vec::new();
    for row in &OPCODES {
        if row.name == spelling && row.class.is_binary() && row.assoc == Assoc::Left {
            out.push(row.op);
        }
    }
    out
}
