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
//! are `pr_opcodes[]` in its order, which is the opcode numbering: `qc_preproc`'s opcode names
//! are the same list.

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
    /// `targetflags`: 6 for id's opcodes, 7 for the extensions.
    pub(crate) target: u8,
    /// `name`: the operator it implements (`"+"`), or `<NAME>` for an opcode no operator writes.
    pub(crate) name: &'static str,
    /// `opname`: the opcode's own name (`"ADD_F"`).
    pub(crate) opname: &'static str,
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
const fn op(
    target: u8,
    name: &'static str,
    opname: &'static str,
    class: Class,
    assoc: Assoc,
    types: [OpType; 3],
    flags: Flags,
) -> Opcode {
    Opcode {
        target,
        name,
        opname,
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
    op(6, "<DONE>", "DONE", Class::None, Assoc::Left, [T::Void, T::Void, T::Void], Flags::NONE),
    op(6, "*", "MUL_F", Class::MulDiv, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(6, "*", "MUL_V", Class::MulDiv, Assoc::Left, [T::Vector, T::Vector, T::Float], Flags::STD),
    op(6, "*", "MUL_FV", Class::MulDiv, Assoc::Left, [T::Float, T::Vector, T::Vector], Flags::STD),
    op(6, "*", "MUL_VF", Class::MulDiv, Assoc::Left, [T::Vector, T::Float, T::Vector], Flags::STD),
    op(6, "/", "DIV_F", Class::MulDiv, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(6, "+", "ADD_F", Class::AddSub, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(6, "+", "ADD_V", Class::AddSub, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    op(6, "-", "SUB_F", Class::AddSub, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(6, "-", "SUB_V", Class::AddSub, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    op(6, "==", "EQ_F", Class::Equality, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    op(6, "==", "EQ_V", Class::Equality, Assoc::Left, [T::Vector, T::Vector, T::BoolFloat], Flags::STD),
    op(6, "==", "EQ_S", Class::Equality, Assoc::Left, [T::String, T::String, T::BoolFloat], Flags::STD),
    op(6, "==", "EQ_E", Class::Equality, Assoc::Left, [T::Entity, T::Entity, T::BoolFloat], Flags::STD),
    op(6, "==", "EQ_FNC", Class::Equality, Assoc::Left, [T::Function, T::Function, T::BoolFloat], Flags::STD),
    op(6, "!=", "NE_F", Class::Equality, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    op(6, "!=", "NE_V", Class::Equality, Assoc::Left, [T::Vector, T::Vector, T::BoolFloat], Flags::STD),
    op(6, "!=", "NE_S", Class::Equality, Assoc::Left, [T::String, T::String, T::BoolFloat], Flags::STD),
    op(6, "!=", "NE_E", Class::Equality, Assoc::Left, [T::Entity, T::Entity, T::BoolFloat], Flags::STD),
    op(6, "!=", "NE_FNC", Class::Equality, Assoc::Left, [T::Function, T::Function, T::BoolFloat], Flags::STD),
    op(6, "<=", "LE_F", Class::Relation, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    op(6, ">=", "GE_F", Class::Relation, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    op(6, "<", "LT_F", Class::Relation, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    op(6, ">", "GT_F", Class::Relation, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    op(6, ".", "LOADF_F", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Float], Flags::NONE),
    op(6, ".", "LOADF_V", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Vector], Flags::NONE),
    op(6, ".", "LOADF_S", Class::Member, Assoc::Left, [T::Entity, T::Field, T::String], Flags::NONE),
    op(6, ".", "LOADF_ENT", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Entity], Flags::NONE),
    op(6, ".", "LOADF_FLD", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Field], Flags::NONE),
    op(6, ".", "LOADF_FNC", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Function], Flags::NONE),
    op(6, ".", "FLDADDRESS", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Pointer], Flags::NONE),
    op(6, "=", "STORE_F", Class::Store, Assoc::Right, [T::Float, T::Float, T::Float], Flags::STORE),
    op(6, "=", "STORE_V", Class::Store, Assoc::Right, [T::Vector, T::Vector, T::Vector], Flags::STORE),
    op(6, "=", "STORE_S", Class::Store, Assoc::Right, [T::String, T::String, T::String], Flags::STORE),
    op(6, "=", "STORE_ENT", Class::Store, Assoc::Right, [T::Entity, T::Entity, T::Entity], Flags::STORE),
    op(6, "=", "STORE_FLD", Class::Store, Assoc::Right, [T::Field, T::Field, T::Field], Flags::STORE),
    op(6, "=", "STORE_FNC", Class::Store, Assoc::Right, [T::Function, T::Function, T::Function], Flags::STORE),
    op(6, "=", "STOREP_F", Class::Store, Assoc::Right, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR_OFS),
    op(6, "=", "STOREP_V", Class::Store, Assoc::Right, [T::Pointer, T::Vector, T::Vector], Flags::STORE_PTR_OFS),
    op(6, "=", "STOREP_S", Class::Store, Assoc::Right, [T::Pointer, T::String, T::String], Flags::STORE_PTR_OFS),
    op(6, "=", "STOREP_ENT", Class::Store, Assoc::Right, [T::Pointer, T::Entity, T::Entity], Flags::STORE_PTR_OFS),
    op(6, "=", "STOREP_FLD", Class::Store, Assoc::Right, [T::Pointer, T::Field, T::Field], Flags::STORE_PTR_OFS),
    op(6, "=", "STOREP_FNC", Class::Store, Assoc::Right, [T::Pointer, T::Function, T::Function], Flags::STORE_PTR_OFS),
    op(6, "<RETURN>", "RETURN", Class::None, Assoc::Left, [T::Vector, T::Void, T::Void], Flags::NONE),
    op(6, "!", "NOT_F", Class::Unary, Assoc::Left, [T::Float, T::Void, T::BoolFloat], Flags::STD_UNARY),
    op(6, "!", "NOT_V", Class::Unary, Assoc::Left, [T::Vector, T::Void, T::BoolFloat], Flags::STD_UNARY),
    op(6, "!", "NOT_S", Class::Unary, Assoc::Left, [T::Vector, T::Void, T::BoolFloat], Flags::STD_UNARY),
    op(6, "!", "NOT_ENT", Class::Unary, Assoc::Left, [T::Entity, T::Void, T::BoolFloat], Flags::STD_UNARY),
    op(6, "!", "NOT_FNC", Class::Unary, Assoc::Left, [T::Function, T::Void, T::BoolFloat], Flags::STD_UNARY),
    op(6, "<IF>", "IF", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    op(6, "<IFNOT>", "IFNOT", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    op(6, "<CALL0>", "CALL0", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    op(6, "<CALL1>", "CALL1", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    op(6, "<CALL2>", "CALL2", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    op(6, "<CALL3>", "CALL3", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    op(6, "<CALL4>", "CALL4", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    op(6, "<CALL5>", "CALL5", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    op(6, "<CALL6>", "CALL6", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    op(6, "<CALL7>", "CALL7", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    op(6, "<CALL8>", "CALL8", Class::None, Assoc::Left, [T::Function, T::Void, T::Void], Flags::NONE),
    op(6, "<STATE>", "STATE", Class::None, Assoc::Left, [T::Float, T::Float, T::Void], Flags::NONE),
    op(6, "<GOTO>", "GOTO", Class::None, Assoc::Right, [T::None, T::Void, T::Void], Flags::NONE),
    op(6, "&&", "AND_F", Class::LogicAnd, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    op(6, "||", "OR_F", Class::LogicOr, Assoc::Left, [T::Float, T::Float, T::BoolFloat], Flags::STD),
    op(6, "&", "BITAND", Class::BitAnd, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(6, "|", "BITOR", Class::BitOr, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(7, "*=", "MULSTORE_F", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    op(7, "*=", "MULSTORE_VF", Class::Store, Assoc::RightResult, [T::Vector, T::Float, T::Vector], Flags::STORE),
    op(7, "*=", "MULSTOREP_F", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "*=", "MULSTOREP_VF", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Vector], Flags::STORE_PTR),
    op(7, "/=", "DIVSTORE_F", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    op(7, "/=", "DIVSTOREP_F", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "+=", "ADDSTORE_F", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    op(7, "+=", "ADDSTORE_V", Class::Store, Assoc::RightResult, [T::Vector, T::Vector, T::Vector], Flags::STORE),
    op(7, "+=", "ADDSTOREP_F", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "+=", "ADDSTOREP_V", Class::Store, Assoc::RightResult, [T::Pointer, T::Vector, T::Vector], Flags::STORE_PTR),
    op(7, "-=", "SUBSTORE_F", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    op(7, "-=", "SUBSTORE_V", Class::Store, Assoc::RightResult, [T::Vector, T::Vector, T::Vector], Flags::STORE),
    op(7, "-=", "SUBSTOREP_F", Class::Store, Assoc::RightResult, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "-=", "SUBSTOREP_V", Class::Store, Assoc::RightResult, [T::Pointer, T::Vector, T::Vector], Flags::STORE_PTR),
    op(7, "<FETCH_GBL_F>", "FETCH_GBL_F", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<FETCH_GBL_V>", "FETCH_GBL_V", Class::None, Assoc::Left, [T::Vector, T::Float, T::Vector], Flags::NONE),
    op(7, "<FETCH_GBL_S>", "FETCH_GBL_S", Class::None, Assoc::Left, [T::String, T::Float, T::String], Flags::NONE),
    op(7, "<FETCH_GBL_E>", "FETCH_GBL_E", Class::None, Assoc::Left, [T::Entity, T::Float, T::Entity], Flags::NONE),
    op(7, "<FETCH_GBL_FNC>", "FETCH_GBL_FNC", Class::None, Assoc::Left, [T::Function, T::Float, T::Function], Flags::NONE),
    op(7, "<CSTATE>", "CSTATE", Class::None, Assoc::Left, [T::Float, T::Float, T::Void], Flags::NONE),
    op(7, "<CWSTATE>", "CWSTATE", Class::None, Assoc::Left, [T::Float, T::Float, T::Void], Flags::NONE),
    op(7, "<THINKTIME>", "THINKTIME", Class::None, Assoc::Left, [T::Entity, T::Float, T::Void], Flags::NONE),
    op(7, "|=", "BITSETSTORE_F", Class::Store, Assoc::Right, [T::Float, T::Float, T::Float], Flags::STORE),
    op(7, "|=", "BITSETSTOREP_F", Class::Store, Assoc::Right, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "&~=", "BITCLRSTORE_F", Class::Store, Assoc::Right, [T::Float, T::Float, T::Float], Flags::STORE),
    op(7, "&~=", "BITCLRSTOREP_F", Class::Store, Assoc::Right, [T::Pointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "<RAND0>", "RAND0", Class::None, Assoc::Left, [T::Void, T::Void, T::Float], Flags::NONE),
    op(7, "<RAND1>", "RAND1", Class::None, Assoc::Left, [T::Float, T::Void, T::Float], Flags::NONE),
    op(7, "<RAND2>", "RAND2", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<RANDV0>", "RANDV0", Class::None, Assoc::Left, [T::Void, T::Void, T::Vector], Flags::NONE),
    op(7, "<RANDV1>", "RANDV1", Class::None, Assoc::Left, [T::Vector, T::Void, T::Vector], Flags::NONE),
    op(7, "<RANDV2>", "RANDV2", Class::None, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::NONE),
    op(7, "<SWITCH_F>", "SWITCH_F", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    op(7, "<SWITCH_V>", "SWITCH_V", Class::None, Assoc::Right, [T::Vector, T::None, T::Void], Flags::NONE),
    op(7, "<SWITCH_S>", "SWITCH_S", Class::None, Assoc::Right, [T::String, T::None, T::Void], Flags::NONE),
    op(7, "<SWITCH_E>", "SWITCH_E", Class::None, Assoc::Right, [T::Entity, T::None, T::Void], Flags::NONE),
    op(7, "<SWITCH_FNC>", "SWITCH_FNC", Class::None, Assoc::Right, [T::Function, T::None, T::Void], Flags::NONE),
    op(7, "<CASE>", "CASE", Class::None, Assoc::Right, [T::Variant, T::None, T::Void], Flags::NONE),
    op(7, "<CASERANGE>", "CASERANGE", Class::None, Assoc::Right, [T::Float, T::Float, T::None], Flags::NONE),
    op(7, "<CALL1H>", "CALL1H", Class::None, Assoc::Right, [T::Function, T::Variant, T::Void], Flags::NONE),
    op(7, "<CALL2H>", "CALL2H", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    op(7, "<CALL3H>", "CALL3H", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    op(7, "<CALL4H>", "CALL4H", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    op(7, "<CALL5H>", "CALL5H", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    op(7, "<CALL6H>", "CALL6H", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    op(7, "<CALL7H>", "CALL7H", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    op(7, "<CALL8H>", "CALL8H", Class::None, Assoc::Right, [T::Function, T::Variant, T::Variant], Flags::NONE),
    op(7, "=", "STORE_I", Class::Store, Assoc::Right, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    op(7, "=", "STORE_IF", Class::Store, Assoc::Right, [T::Float, T::Integer, T::Integer], Flags::STORE),
    op(7, "=", "STORE_FI", Class::Store, Assoc::Right, [T::Integer, T::Float, T::Float], Flags::STORE),
    op(7, "+", "ADD_I", Class::AddSub, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "+", "ADD_FI", Class::AddSub, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    op(7, "+", "ADD_IF", Class::AddSub, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    op(7, "-", "SUB_I", Class::AddSub, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "-", "SUB_FI", Class::AddSub, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    op(7, "-", "SUB_IF", Class::AddSub, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    op(7, "<CIF>", "CONV_IF", Class::Store, Assoc::Left, [T::Integer, T::Void, T::Float], Flags::NONE),
    op(7, "<CFI>", "CONV_FI", Class::Store, Assoc::Left, [T::Float, T::Void, T::Integer], Flags::NONE),
    op(7, "<CPIF>", "CONVP_IF", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Float], Flags::NONE),
    op(7, "<CPFI>", "CONVP_FI", Class::Store, Assoc::Left, [T::Pointer, T::Float, T::Integer], Flags::NONE),
    op(7, ".", "LOADF_I", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Integer], Flags::NONE),
    op(7, "=", "STOREP_I", Class::Store, Assoc::Right, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR_OFS),
    op(7, "=", "STOREP_IF", Class::Store, Assoc::Right, [T::Pointer, T::Float, T::Integer], Flags::STORE_PTR_OFS),
    op(7, "=", "STOREP_FI", Class::Store, Assoc::Right, [T::Pointer, T::Integer, T::Float], Flags::STORE_PTR_OFS),
    op(7, "&", "BITAND_I", Class::BitAnd, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "|", "BITOR_I", Class::BitOr, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "*", "MUL_I", Class::MulDiv, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "/", "DIV_I", Class::MulDiv, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "==", "EQ_I", Class::Equality, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    op(7, "!=", "NE_I", Class::Equality, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    op(7, "<IFNOTS>", "IFNOTS", Class::None, Assoc::Right, [T::String, T::None, T::Void], Flags::NONE),
    op(7, "<IFS>", "IFS", Class::None, Assoc::Right, [T::String, T::None, T::Void], Flags::NONE),
    op(7, "!", "NOT_I", Class::Unary, Assoc::Left, [T::Integer, T::Void, T::BoolInt], Flags::STD_UNARY),
    op(7, "/", "DIV_VF", Class::MulDiv, Assoc::Left, [T::Vector, T::Float, T::Vector], Flags::STD),
    op(7, "^", "BITXOR_I", Class::BitXor, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, ">>", "RSHIFT_I", Class::Shift, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "<<", "LSHIFT_I", Class::Shift, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "<ARRAY>", "GLOBALADDRESS", Class::None, Assoc::Left, [T::Float, T::Integer, T::Pointer], Flags::NONE),
    op(7, "<ARRAY>", "ADD_PIW", Class::None, Assoc::Left, [T::Pointer, T::Integer, T::Pointer], Flags::NONE),
    op(7, "=", "LOADA_F", Class::Store, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::NONE),
    op(7, "=", "LOADA_V", Class::Store, Assoc::Left, [T::Vector, T::Integer, T::Vector], Flags::NONE),
    op(7, "=", "LOADA_S", Class::Store, Assoc::Left, [T::String, T::Integer, T::String], Flags::NONE),
    op(7, "=", "LOADA_ENT", Class::Store, Assoc::Left, [T::Entity, T::Integer, T::Entity], Flags::NONE),
    op(7, "=", "LOADA_FLD", Class::Store, Assoc::Left, [T::Field, T::Integer, T::Field], Flags::NONE),
    op(7, "=", "LOADA_FNC", Class::Store, Assoc::Left, [T::Function, T::Integer, T::Function], Flags::NONE),
    op(7, "=", "LOADA_I", Class::Store, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::NONE),
    op(7, "=", "STORE_P", Class::Store, Assoc::Right, [T::Pointer, T::Pointer, T::Void], Flags::STORE),
    op(7, ".", "LOADF_P", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Pointer], Flags::NONE),
    op(7, "=", "LOADP_F", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Float], Flags::LOAD_PTR),
    op(7, "=", "LOADP_V", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Vector], Flags::LOAD_PTR),
    op(7, "=", "LOADP_S", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::String], Flags::LOAD_PTR),
    op(7, "=", "LOADP_ENT", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Entity], Flags::LOAD_PTR),
    op(7, "=", "LOADP_FLD", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Field], Flags::LOAD_PTR),
    op(7, "=", "LOADP_FNC", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Function], Flags::LOAD_PTR),
    op(7, "=", "LOADP_I", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Integer], Flags::LOAD_PTR),
    op(7, "<=", "LE_I", Class::Relation, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    op(7, ">=", "GE_I", Class::Relation, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    op(7, "<", "LT_I", Class::Relation, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    op(7, ">", "GT_I", Class::Relation, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    op(7, "<=", "LE_IF", Class::Relation, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    op(7, ">=", "GE_IF", Class::Relation, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    op(7, "<", "LT_IF", Class::Relation, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    op(7, ">", "GT_IF", Class::Relation, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    op(7, "<=", "LE_FI", Class::Relation, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    op(7, ">=", "GE_FI", Class::Relation, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    op(7, "<", "LT_FI", Class::Relation, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    op(7, ">", "GT_FI", Class::Relation, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    op(7, "==", "EQ_IF", Class::Equality, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    op(7, "==", "EQ_FI", Class::Equality, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    op(7, "+", "ADD_SF", Class::AddSub, Assoc::Left, [T::String, T::Float, T::String], Flags::STD),
    op(7, "-", "SUB_S", Class::AddSub, Assoc::Left, [T::String, T::String, T::Float], Flags::STD),
    op(7, "<STOREP_C>", "STOREP_C", Class::Store, Assoc::Right, [T::String, T::Float, T::Float], Flags::STORE_PTR_OFS),
    op(7, "<LOADP_C>", "LOADP_C", Class::Store, Assoc::Left, [T::String, T::Float, T::Float], Flags::LOAD_PTR),
    op(7, "*", "MUL_IF", Class::MulDiv, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    op(7, "*", "MUL_FI", Class::MulDiv, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    op(7, "*", "MUL_VI", Class::MulDiv, Assoc::Left, [T::Vector, T::Integer, T::Vector], Flags::STD),
    op(7, "*", "MUL_IV", Class::MulDiv, Assoc::Left, [T::Integer, T::Vector, T::Vector], Flags::STD),
    op(7, "/", "DIV_IF", Class::MulDiv, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    op(7, "/", "DIV_FI", Class::MulDiv, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    op(7, "&", "BITAND_IF", Class::BitAnd, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    op(7, "|", "BITOR_IF", Class::BitOr, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    op(7, "&", "BITAND_FI", Class::BitAnd, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    op(7, "|", "BITOR_FI", Class::BitOr, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    op(7, "&&", "AND_I", Class::LogicAnd, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    op(7, "||", "OR_I", Class::LogicOr, Assoc::Left, [T::Integer, T::Integer, T::BoolInt], Flags::STD),
    op(7, "&&", "AND_IF", Class::LogicAnd, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    op(7, "||", "OR_IF", Class::LogicOr, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    op(7, "&&", "AND_FI", Class::LogicAnd, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    op(7, "||", "OR_FI", Class::LogicOr, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    op(7, "!=", "NE_IF", Class::Equality, Assoc::Left, [T::Integer, T::Float, T::BoolInt], Flags::STD),
    op(7, "!=", "NE_FI", Class::Equality, Assoc::Left, [T::Float, T::Integer, T::BoolInt], Flags::STD),
    op(7, "<>", "GSTOREP_I", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GSTOREP_F", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GSTOREP_ENT", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GSTOREP_FLD", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GSTOREP_S", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GSTOREP_FNC", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GSTOREP_V", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GADDRESS", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GLOAD_I", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GLOAD_F", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GLOAD_FLD", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GLOAD_ENT", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GLOAD_S", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "GLOAD_FNC", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<>", "BOUNDCHECK", Class::None, Assoc::Left, [T::Integer, T::None, T::None], Flags::NONE),
    op(7, "<UNUSED>", "UNUSED", Class::None, Assoc::Right, [T::Void, T::Void, T::Void], Flags::NONE),
    op(7, "<PUSH>", "PUSH", Class::None, Assoc::Right, [T::Float, T::Void, T::Pointer], Flags::NONE),
    op(7, "<POP>", "POP", Class::None, Assoc::Right, [T::Float, T::Void, T::Void], Flags::NONE),
    op(7, "<SWITCH_I>", "SWITCH_I", Class::None, Assoc::Left, [T::Void, T::None, T::Void], Flags::NONE),
    op(7, "<>", "GLOAD_V", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "<IF_F>", "IF_F", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    op(7, "<IFNOT_F>", "IFNOT_F", Class::None, Assoc::Right, [T::Float, T::None, T::Void], Flags::NONE),
    op(7, "=", "STOREF_V", Class::None, Assoc::Right, [T::Entity, T::Field, T::Vector], Flags::STORE_FLD),
    op(7, "=", "STOREF_F", Class::None, Assoc::Right, [T::Entity, T::Field, T::Float], Flags::STORE_FLD),
    op(7, "=", "STOREF_S", Class::None, Assoc::Right, [T::Entity, T::Field, T::String], Flags::STORE_FLD),
    op(7, "=", "STOREF_I", Class::None, Assoc::Right, [T::Entity, T::Field, T::Integer], Flags::STORE_FLD),
    op(7, "<STOREP_B>", "STOREP_I8", Class::Store, Assoc::Right, [T::String, T::Integer, T::Integer], Flags::STORE_PTR_OFS),
    op(7, "<LOADP_B>", "LOADP_U8", Class::Store, Assoc::Left, [T::String, T::Integer, T::Integer], Flags::LOAD_PTR),
    op(7, "<=", "LE_U", Class::Relation, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::LOAD_PTR),
    op(7, "<", "LT_U", Class::Relation, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::LOAD_PTR),
    op(7, "/", "DIV_U", Class::MulDiv, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::LOAD_PTR),
    op(7, ">>", "RSHIFT_U", Class::Shift, Assoc::Left, [T::Uint, T::Integer, T::Uint], Flags::LOAD_PTR),
    op(7, "+", "ADD_I64", Class::AddSub, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    op(7, "-", "SUB_I64", Class::AddSub, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    op(7, "*", "MUL_I64", Class::MulDiv, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    op(7, "/", "DIV_I64", Class::MulDiv, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    op(7, "&", "BITAND_I64", Class::BitAnd, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    op(7, "|", "BITOR_I64", Class::BitOr, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    op(7, "^", "BITXOR_I64", Class::BitXor, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STD),
    op(7, "<<", "LSHIFT_I64I", Class::Shift, Assoc::Left, [T::Int64, T::Integer, T::Int64], Flags::STD),
    op(7, ">>", "RSHIFT_I64I", Class::Shift, Assoc::Left, [T::Int64, T::Integer, T::Int64], Flags::STD),
    op(7, "<=", "LE_I64", Class::Relation, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    op(7, "<", "LT_I64", Class::Relation, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    op(7, "==", "EQ_I64", Class::Equality, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    op(7, "!=", "NE_I64", Class::Equality, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    op(7, "<", "LE_U64", Class::Relation, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    op(7, "<", "LT_U64", Class::Relation, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    op(7, "%", "DIV_U64", Class::MulDiv, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    op(7, ">>", "RSHIFT_U64I", Class::Shift, Assoc::Left, [T::Uint64, T::Uint, T::Uint64], Flags::STD),
    op(7, "=", "STORE_I64", Class::Store, Assoc::Right, [T::Int64, T::Int64, T::Int64], Flags::STORE),
    op(7, "=", "STOREP_I64", Class::Store, Assoc::Right, [T::Pointer, T::Int64, T::Int64], Flags::STORE_PTR_OFS),
    op(7, "<=>", "STOREF_I64", Class::None, Assoc::Right, [T::Entity, T::Field, T::Int64], Flags::STORE_FLD),
    op(7, ".", "LOADF_I64", Class::Member, Assoc::Left, [T::Entity, T::Field, T::Pointer], Flags::NONE),
    op(7, "=", "LOADA_I64", Class::Store, Assoc::Left, [T::Int64, T::Integer, T::Int64], Flags::NONE),
    op(7, "=", "LOADP_I64", Class::Store, Assoc::Left, [T::Pointer, T::Int64, T::Int64], Flags::LOAD_PTR),
    op(7, "=", "CONV_UI64", Class::Store, Assoc::Left, [T::Uint, T::Void, T::Int64], Flags::NONE),
    op(7, "=", "CONV_II64", Class::Store, Assoc::Left, [T::Integer, T::Void, T::Int64], Flags::NONE),
    op(7, "=", "CONV_I64I", Class::Store, Assoc::Left, [T::Int64, T::Void, T::Integer], Flags::NONE),
    op(7, "=", "CONV_FD", Class::Store, Assoc::Left, [T::Float, T::Void, T::Double], Flags::NONE),
    op(7, "=", "CONV_DF", Class::Store, Assoc::Left, [T::Double, T::Void, T::Float], Flags::NONE),
    op(7, "=", "CONV_I64F", Class::Store, Assoc::Left, [T::Int64, T::Void, T::Float], Flags::NONE),
    op(7, "=", "CONV_FI64", Class::Store, Assoc::Left, [T::Float, T::Void, T::Int64], Flags::NONE),
    op(7, "=", "CONV_I64D", Class::Store, Assoc::Left, [T::Int64, T::Void, T::Double], Flags::NONE),
    op(7, "=", "CONV_DI64", Class::Store, Assoc::Left, [T::Double, T::Void, T::Int64], Flags::NONE),
    op(7, "+", "ADD_D", Class::AddSub, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    op(7, "-", "SUB_D", Class::AddSub, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    op(7, "*", "MUL_D", Class::MulDiv, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    op(7, "/", "DIV_D", Class::MulDiv, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    op(7, "<=", "LE_D", Class::Relation, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    op(7, "<", "LT_D", Class::Relation, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    op(7, "==", "EQ_D", Class::Equality, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    op(7, "!=", "NE_D", Class::Equality, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    op(7, "<STOREP_I16>", "STOREP_I16", Class::Store, Assoc::Right, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR_OFS),
    op(7, "<LOADP_I16>", "LOADP_I16", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Integer], Flags::LOAD_PTR),
    op(7, "<LOADP_U16>", "LOADP_U16", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Integer], Flags::LOAD_PTR),
    op(7, "<LOADP_I8>", "LOADP_I8", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Integer], Flags::LOAD_PTR),
    op(7, "<BITEXTEND_I>", "BITEXTEND_I", Class::None, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "<BITEXTEND_U>", "BITEXTEND_U", Class::None, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "<BITCOPY_I>", "BITCOPY_I", Class::None, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "=", "CONV_UF", Class::Store, Assoc::Left, [T::Uint, T::Void, T::Float], Flags::NONE),
    op(7, "=", "CONV_FU", Class::Store, Assoc::Left, [T::Float, T::Void, T::Uint], Flags::NONE),
    op(7, "=", "CONV_U64D", Class::Store, Assoc::Left, [T::Uint64, T::Void, T::Double], Flags::NONE),
    op(7, "=", "CONV_DU64", Class::Store, Assoc::Left, [T::Double, T::Void, T::Uint64], Flags::NONE),
    op(7, "=", "CONV_U64F", Class::Store, Assoc::Left, [T::Uint64, T::Void, T::Float], Flags::NONE),
    op(7, "=", "CONV_FU64", Class::Store, Assoc::Left, [T::Float, T::Void, T::Uint64], Flags::NONE),
    op(7, "<>", "OP_EMULATED", Class::None, Assoc::Left, [T::Float, T::Float, T::Float], Flags::NONE),
    op(7, "|=", "BITSET_I", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    op(7, "|=", "BITSETP_I", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    op(7, "&~=", "BITCLR_I", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    op(7, "*=", "MULSTORE_I", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    op(7, "/=", "DIVSTORE_I", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    op(7, "+=", "ADDSTORE_I", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    op(7, "-=", "SUBSTORE_I", Class::Store, Assoc::RightResult, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    op(7, "*=", "MULSTOREP_I", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    op(7, "/=", "DIVSTOREP_I", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    op(7, "+=", "ADDSTOREP_I", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    op(7, "-=", "SUBSTOREP_I", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Integer], Flags::STORE_PTR),
    op(7, "*=", "MULSTORE_IF", Class::Store, Assoc::RightResult, [T::Integer, T::Float, T::Float], Flags::STORE),
    op(7, "*=", "MULSTOREP_IF", Class::Store, Assoc::RightResult, [T::IntPointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "/=", "DIVSTORE_IF", Class::Store, Assoc::RightResult, [T::Integer, T::Float, T::Float], Flags::STORE),
    op(7, "/=", "DIVSTOREP_IF", Class::Store, Assoc::RightResult, [T::IntPointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "+=", "ADDSTORE_IF", Class::Store, Assoc::RightResult, [T::Integer, T::Float, T::Float], Flags::STORE),
    op(7, "+=", "ADDSTOREP_IF", Class::Store, Assoc::RightResult, [T::IntPointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "-=", "SUBSTORE_IF", Class::Store, Assoc::RightResult, [T::Integer, T::Float, T::Float], Flags::STORE),
    op(7, "-=", "SUBSTOREP_IF", Class::Store, Assoc::RightResult, [T::IntPointer, T::Float, T::Float], Flags::STORE_PTR),
    op(7, "*=", "MULSTORE_FI", Class::Store, Assoc::RightResult, [T::Float, T::Integer, T::Float], Flags::STORE),
    op(7, "*=", "MULSTOREP_FI", Class::Store, Assoc::RightResult, [T::FloatPointer, T::Integer, T::Float], Flags::STORE_PTR),
    op(7, "/=", "DIVSTORE_FI", Class::Store, Assoc::RightResult, [T::Float, T::Integer, T::Float], Flags::STORE),
    op(7, "/=", "DIVSTOREP_FI", Class::Store, Assoc::RightResult, [T::FloatPointer, T::Integer, T::Float], Flags::STORE_PTR),
    op(7, "+=", "ADDSTORE_FI", Class::Store, Assoc::RightResult, [T::Float, T::Integer, T::Float], Flags::STORE),
    op(7, "+=", "ADDSTOREP_FI", Class::Store, Assoc::RightResult, [T::FloatPointer, T::Integer, T::Float], Flags::STORE_PTR),
    op(7, "-=", "SUBSTORE_FI", Class::Store, Assoc::RightResult, [T::Float, T::Integer, T::Float], Flags::STORE),
    op(7, "-=", "SUBSTOREP_FI", Class::Store, Assoc::RightResult, [T::FloatPointer, T::Integer, T::Float], Flags::STORE_PTR),
    op(7, "*=", "MULSTORE_VI", Class::Store, Assoc::RightResult, [T::Vector, T::Integer, T::Vector], Flags::STORE),
    op(7, "*=", "MULSTOREP_VI", Class::Store, Assoc::RightResult, [T::Pointer, T::Integer, T::Vector], Flags::STORE_PTR),
    op(7, "=", "LOADA_STRUCT", Class::Store, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::NONE),
    op(7, "=", "LOADP_P", Class::Store, Assoc::Left, [T::Pointer, T::Integer, T::Pointer], Flags::LOAD_PTR),
    op(7, "=", "STOREP_P", Class::Store, Assoc::Right, [T::Pointer, T::Pointer, T::Pointer], Flags::STORE_PTR_OFS),
    op(7, "~", "BITNOT_F", Class::Unary, Assoc::Left, [T::Float, T::Void, T::Float], Flags::STD_UNARY),
    op(7, "~", "BITNOT_I", Class::Unary, Assoc::Left, [T::Integer, T::Void, T::Integer], Flags::STD_UNARY),
    op(7, "==", "EQ_P", Class::Equality, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    op(7, "!=", "NE_P", Class::Equality, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    op(7, "<=", "LE_P", Class::Relation, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    op(7, ">=", "GE_P", Class::Relation, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    op(7, "<", "LT_P", Class::Relation, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    op(7, ">", "GT_P", Class::Relation, Assoc::Left, [T::Pointer, T::Pointer, T::BoolFloat], Flags::STD),
    op(7, "&=", "ANDSTORE_F", Class::Store, Assoc::RightResult, [T::Float, T::Float, T::Float], Flags::STORE),
    op(7, "&~=", "BITCLR_F", Class::Store, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STORE),
    op(7, "&~=", "BITCLR_I", Class::Store, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STORE),
    op(7, "&~=", "BITCLR_V", Class::Store, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STORE),
    op(7, "+", "ADD_SI", Class::AddSub, Assoc::Left, [T::String, T::Integer, T::String], Flags::STD),
    op(7, "+", "ADD_IS", Class::AddSub, Assoc::Left, [T::Integer, T::String, T::String], Flags::STD),
    op(7, "+", "ADD_PF", Class::AddSub, Assoc::Left, [T::Pointer, T::Float, T::Pointer], Flags::STD),
    op(7, "+", "ADD_FP", Class::AddSub, Assoc::Left, [T::Float, T::Pointer, T::Pointer], Flags::STD),
    op(7, "+", "ADD_PI", Class::AddSub, Assoc::Left, [T::Pointer, T::Integer, T::Pointer], Flags::STD),
    op(7, "+", "ADD_IP", Class::AddSub, Assoc::Left, [T::Integer, T::Pointer, T::Pointer], Flags::STD),
    op(7, "+", "ADD_PU", Class::AddSub, Assoc::Left, [T::Pointer, T::Uint, T::Pointer], Flags::STD),
    op(7, "+", "ADD_UP", Class::AddSub, Assoc::Left, [T::Uint, T::Pointer, T::Pointer], Flags::STD),
    op(7, "-", "SUB_SI", Class::AddSub, Assoc::Left, [T::String, T::Integer, T::String], Flags::STD),
    op(7, "-", "SUB_PF", Class::AddSub, Assoc::Left, [T::Pointer, T::Float, T::Pointer], Flags::STD),
    op(7, "-", "SUB_PI", Class::AddSub, Assoc::Left, [T::Pointer, T::Integer, T::Pointer], Flags::STD),
    op(7, "-", "SUB_PU", Class::AddSub, Assoc::Left, [T::Pointer, T::Uint, T::Pointer], Flags::STD),
    op(7, "-", "SUB_PP", Class::AddSub, Assoc::Left, [T::Pointer, T::Pointer, T::Integer], Flags::STD),
    op(7, "%", "MOD_F", Class::MulDiv, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(7, "%", "MOD_I", Class::MulDiv, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "%", "MOD_FI", Class::MulDiv, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    op(7, "%", "MOD_IF", Class::MulDiv, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    op(7, "%", "MOD_V", Class::MulDiv, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    op(7, "^", "BITXOR_F", Class::BitXor, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(7, ">>", "RSHIFT_F", Class::Shift, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(7, "<<", "LSHIFT_F", Class::Shift, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(7, ">>", "RSHIFT_IF", Class::Shift, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    op(7, "<<", "LSHIFT_IF", Class::Shift, Assoc::Left, [T::Integer, T::Float, T::Integer], Flags::STD),
    op(7, ">>", "RSHIFT_FI", Class::Shift, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    op(7, "<<", "LSHIFT_FI", Class::Shift, Assoc::Left, [T::Float, T::Integer, T::Integer], Flags::STD),
    op(7, "&&", "AND_ANY", Class::LogicAnd, Assoc::Left, [T::Variant, T::Variant, T::BoolFloat], Flags::STD),
    op(7, "||", "OR_ANY", Class::LogicOr, Assoc::Left, [T::Variant, T::Variant, T::BoolFloat], Flags::STD),
    op(7, "+", "ADD_EI", Class::AddSub, Assoc::Left, [T::Entity, T::Integer, T::Entity], Flags::STD),
    op(7, "+", "ADD_EF", Class::AddSub, Assoc::Left, [T::Entity, T::Float, T::Entity], Flags::STD),
    op(7, "-", "SUB_EI", Class::AddSub, Assoc::Left, [T::Entity, T::Integer, T::Entity], Flags::STD),
    op(7, "-", "SUB_EF", Class::AddSub, Assoc::Left, [T::Entity, T::Float, T::Entity], Flags::STD),
    op(7, "&", "BITAND_V", Class::BitAnd, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    op(7, "|", "BITOR_V", Class::BitOr, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    op(7, "~", "BITNOT_V", Class::Unary, Assoc::Left, [T::Vector, T::Void, T::Vector], Flags::STD),
    op(7, "^", "BITXOR_V", Class::BitXor, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    op(7, "*^", "POW_F", Class::MulDiv, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(7, "*^", "POW_I", Class::MulDiv, Assoc::Left, [T::Integer, T::Integer, T::Integer], Flags::STD),
    op(7, "*^", "POW_FI", Class::MulDiv, Assoc::Left, [T::Float, T::Integer, T::Float], Flags::STD),
    op(7, "*^", "POW_IF", Class::MulDiv, Assoc::Left, [T::Integer, T::Float, T::Float], Flags::STD),
    op(7, "><", "CROSS_V", Class::MulDiv, Assoc::Left, [T::Vector, T::Vector, T::Vector], Flags::STD),
    op(7, "==", "EQ_FLD", Class::Equality, Assoc::Left, [T::Field, T::Field, T::BoolFloat], Flags::STD),
    op(7, "!=", "NE_FLD", Class::Equality, Assoc::Left, [T::Field, T::Field, T::BoolFloat], Flags::STD),
    op(7, "<=>", "SPACESHIP_F", Class::Equality, Assoc::Left, [T::Float, T::Float, T::Float], Flags::STD),
    op(7, "<=>", "SPACESHIP_S", Class::Equality, Assoc::Left, [T::String, T::String, T::Float], Flags::STD),
    op(7, "+", "ADD_U", Class::AddSub, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    op(7, "-", "SUB_U", Class::AddSub, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    op(7, "*", "MUL_U", Class::MulDiv, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    op(7, "%", "MOD_U", Class::MulDiv, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    op(7, "&", "BITAND_U", Class::BitAnd, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    op(7, "|", "BITOR_U", Class::BitOr, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    op(7, "^", "BITXOR_U", Class::BitXor, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    op(7, "~", "BITNOT_U", Class::Unary, Assoc::Left, [T::Uint, T::Void, T::Uint], Flags::STD_UNARY),
    op(7, "&~=", "BITCLR_U", Class::Store, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STORE),
    op(7, "<<", "LSHIFT_U", Class::Shift, Assoc::Left, [T::Uint, T::Uint, T::Uint], Flags::STD),
    op(7, ">=", "GE_U", Class::Relation, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::STD),
    op(7, ">", "GT_U", Class::Relation, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::STD),
    op(7, "==", "EQ_U", Class::Equality, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::STD),
    op(7, "!=", "NE_U", Class::Equality, Assoc::Left, [T::Uint, T::Uint, T::BoolInt], Flags::STD),
    op(7, "~", "BITNOT_I64", Class::Unary, Assoc::Left, [T::Int64, T::Void, T::Int64], Flags::STD_UNARY),
    op(7, "&~=", "BITCLR_I64", Class::Store, Assoc::Left, [T::Int64, T::Int64, T::Int64], Flags::STORE),
    op(7, ">=", "GE_I64", Class::Relation, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    op(7, ">", "GT_I64", Class::Relation, Assoc::Left, [T::Int64, T::Int64, T::BoolInt], Flags::STD),
    op(7, "+", "ADD_U64", Class::AddSub, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    op(7, "-", "SUB_U64", Class::AddSub, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    op(7, "*", "MUL_U64", Class::MulDiv, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    op(7, "%", "MOD_U64", Class::MulDiv, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    op(7, "&", "BITAND_U64", Class::BitAnd, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    op(7, "|", "BITOR_U64", Class::BitOr, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    op(7, "^", "BITXOR_U64", Class::BitXor, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STD),
    op(7, "~", "BITNOT_U64", Class::Unary, Assoc::Left, [T::Uint64, T::Void, T::Uint64], Flags::STD_UNARY),
    op(7, "&~=", "BITCLR_U64", Class::Store, Assoc::Left, [T::Uint64, T::Uint64, T::Uint64], Flags::STORE),
    op(7, "<<", "LSHIFT_U64I", Class::Shift, Assoc::Left, [T::Uint64, T::Integer, T::Uint64], Flags::STD),
    op(7, ">=", "GE_U64", Class::Relation, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    op(7, ">", "GT_U64", Class::Relation, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    op(7, "==", "EQ_U64", Class::Equality, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    op(7, "!=", "NE_U64", Class::Equality, Assoc::Left, [T::Uint64, T::Uint64, T::BoolInt], Flags::STD),
    op(7, "&", "BITAND_D", Class::BitAnd, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    op(7, "|", "BITOR_D", Class::BitOr, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    op(7, "^", "BITXOR_D", Class::BitXor, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STD),
    op(7, "~", "BITNOT_D", Class::Unary, Assoc::Left, [T::Double, T::Void, T::BoolInt], Flags::STD_UNARY),
    op(7, "&~=", "BITCLR_D", Class::Store, Assoc::Left, [T::Double, T::Double, T::Double], Flags::STORE),
    op(7, "<<", "LSHIFT_DI", Class::Shift, Assoc::Left, [T::Double, T::Integer, T::Double], Flags::STD),
    op(7, ">>", "RSHIFT_DI", Class::Shift, Assoc::Left, [T::Double, T::Integer, T::Double], Flags::STD),
    op(7, ">=", "GE_D", Class::Relation, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    op(7, ">", "GT_D", Class::Relation, Assoc::Left, [T::Double, T::Double, T::BoolInt], Flags::STD),
    op(7, "<WSTATE>", "WSTATE", Class::None, Assoc::Left, [T::Float, T::Float, T::Void], Flags::NONE),
];

/// The index of the opcode named `opname`, its `OP_*` number.
#[cfg(test)]
pub(crate) fn index_of(opname: &str) -> Option<usize> {
    OPCODES.iter().position(|op| op.opname == opname)
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

/// The opcodes a binary operator spelled `spelling` may be given, by their `OP_*` numbers, in
/// table order (FTEF-23.1).
pub(crate) fn candidates(spelling: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (index, op) in OPCODES.iter().enumerate() {
        if op.name == spelling && op.class.is_binary() && op.assoc == Assoc::Left {
            out.push(index);
        }
    }
    out
}
