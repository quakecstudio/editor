// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_comp.h, OP_DONE, OP_MUL_F, OP_MUL_V, OP_MUL_FV, OP_MUL_VF, OP_DIV_F, OP_ADD_F, OP_ADD_V, OP_SUB_F, OP_SUB_V, OP_EQ_F, OP_EQ_V, OP_EQ_S, OP_EQ_E, OP_EQ_FNC, OP_NE_F, OP_NE_V, OP_NE_S, OP_NE_E, OP_NE_FNC, OP_LE, OP_GE, OP_LT, OP_GT, OP_LOAD_F, OP_LOAD_V, OP_LOAD_S, OP_LOAD_ENT, OP_LOAD_FLD, OP_LOAD_FNC, OP_ADDRESS, OP_STORE_F, OP_STORE_V, OP_STORE_S, OP_STORE_ENT, OP_STORE_FLD, OP_STORE_FNC, OP_STOREP_F, OP_STOREP_V, OP_STOREP_S, OP_STOREP_ENT, OP_STOREP_FLD, OP_STOREP_FNC, OP_RETURN, OP_NOT_F, OP_NOT_V, OP_NOT_S, OP_NOT_ENT, OP_NOT_FNC, OP_IF, OP_IFNOT, OP_CALL0, OP_CALL1, OP_CALL2, OP_CALL3, OP_CALL4, OP_CALL5, OP_CALL6, OP_CALL7, OP_CALL8, OP_STATE, OP_GOTO, OP_AND, OP_OR, OP_BITAND, OP_BITOR

//! The vanilla opcode numbers, id's anonymous `OP_` enum in `WinQuake/pr_comp.h`.
//!
//! Implements PROGS-8 of `docs/spec/progs.md`. What each opcode does is in `vm.md`; this
//! module only names the numbers a statement stores.

/// Number of vanilla opcodes, `OP_DONE` (0) to `OP_BITOR` (65).
///
/// `PROGS_OPCODE_COUNT` in PROGS-8.
pub const PROGS_OPCODE_COUNT: u16 = 66;

macro_rules! opcodes {
    ($($name:ident = $number:literal, $text:literal;)*) => {
        /// A vanilla opcode, numbered as in id's `OP_` enum (PROGS-8.1).
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Opcode {
            $(
                #[doc = concat!("`OP_", $text, "`, opcode ", stringify!($number), ".")]
                $name = $number,
            )*
        }

        impl Opcode {
            /// Every vanilla opcode, in number order.
            pub const ALL: [Self; PROGS_OPCODE_COUNT as usize] = [$(Self::$name,)*];

            /// The opcode with this number, or `None` above 65: such a statement fails only
            /// when executed (PROGS-7.4).
            #[must_use]
            pub const fn from_number(number: u32) -> Option<Self> {
                match number {
                    $($number => Some(Self::$name),)*
                    _ => None,
                }
            }

            /// The number a statement stores for this opcode (PROGS-8.1).
            #[must_use]
            pub const fn number(self) -> u16 {
                match self {
                    $(Self::$name => $number,)*
                }
            }

            /// id's name without the `OP_` prefix, such as `STORE_F` (PROGS-8.1).
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$name => $text,)*
                }
            }
        }
    };
}

opcodes! {
    Done = 0, "DONE";
    MulF = 1, "MUL_F";
    MulV = 2, "MUL_V";
    MulFv = 3, "MUL_FV";
    MulVf = 4, "MUL_VF";
    DivF = 5, "DIV_F";
    AddF = 6, "ADD_F";
    AddV = 7, "ADD_V";
    SubF = 8, "SUB_F";
    SubV = 9, "SUB_V";
    EqF = 10, "EQ_F";
    EqV = 11, "EQ_V";
    EqS = 12, "EQ_S";
    EqE = 13, "EQ_E";
    EqFnc = 14, "EQ_FNC";
    NeF = 15, "NE_F";
    NeV = 16, "NE_V";
    NeS = 17, "NE_S";
    NeE = 18, "NE_E";
    NeFnc = 19, "NE_FNC";
    Le = 20, "LE";
    Ge = 21, "GE";
    Lt = 22, "LT";
    Gt = 23, "GT";
    LoadF = 24, "LOAD_F";
    LoadV = 25, "LOAD_V";
    LoadS = 26, "LOAD_S";
    LoadEnt = 27, "LOAD_ENT";
    LoadFld = 28, "LOAD_FLD";
    LoadFnc = 29, "LOAD_FNC";
    Address = 30, "ADDRESS";
    StoreF = 31, "STORE_F";
    StoreV = 32, "STORE_V";
    StoreS = 33, "STORE_S";
    StoreEnt = 34, "STORE_ENT";
    StoreFld = 35, "STORE_FLD";
    StoreFnc = 36, "STORE_FNC";
    StorepF = 37, "STOREP_F";
    StorepV = 38, "STOREP_V";
    StorepS = 39, "STOREP_S";
    StorepEnt = 40, "STOREP_ENT";
    StorepFld = 41, "STOREP_FLD";
    StorepFnc = 42, "STOREP_FNC";
    Return = 43, "RETURN";
    NotF = 44, "NOT_F";
    NotV = 45, "NOT_V";
    NotS = 46, "NOT_S";
    NotEnt = 47, "NOT_ENT";
    NotFnc = 48, "NOT_FNC";
    If = 49, "IF";
    IfNot = 50, "IFNOT";
    Call0 = 51, "CALL0";
    Call1 = 52, "CALL1";
    Call2 = 53, "CALL2";
    Call3 = 54, "CALL3";
    Call4 = 55, "CALL4";
    Call5 = 56, "CALL5";
    Call6 = 57, "CALL6";
    Call7 = 58, "CALL7";
    Call8 = 59, "CALL8";
    State = 60, "STATE";
    Goto = 61, "GOTO";
    And = 62, "AND";
    Or = 63, "OR";
    BitAnd = 64, "BITAND";
    BitOr = 65, "BITOR";
}

#[cfg(test)]
mod tests {
    use super::{Opcode, PROGS_OPCODE_COUNT};

    // spec PROGS-8.1: 66 opcodes numbered from 0 in id's enum order, DONE to BITOR.
    #[test]
    fn opcodes_are_numbered_in_enum_order() {
        assert_eq!(PROGS_OPCODE_COUNT, 66);
        assert_eq!(Opcode::ALL.len(), usize::from(PROGS_OPCODE_COUNT));
        for (index, opcode) in Opcode::ALL.into_iter().enumerate() {
            let number = u16::try_from(index).unwrap_or(u16::MAX);
            assert_eq!(opcode.number(), number);
            assert_eq!(Opcode::from_number(u32::from(number)), Some(opcode));
        }
        let spot = [
            (0, "DONE"),
            (6, "ADD_F"),
            (24, "LOAD_F"),
            (30, "ADDRESS"),
            (31, "STORE_F"),
            (43, "RETURN"),
            (49, "IF"),
            (50, "IFNOT"),
            (51, "CALL0"),
            (59, "CALL8"),
            (60, "STATE"),
            (61, "GOTO"),
            (64, "BITAND"),
            (65, "BITOR"),
        ];
        for (number, name) in spot {
            assert_eq!(Opcode::from_number(number).map(Opcode::name), Some(name));
        }
    }

    // spec PROGS-7.4: an opcode above OP_BITOR (65) is not a vanilla opcode.
    #[test]
    fn numbers_above_65_are_not_opcodes() {
        assert_eq!(Opcode::from_number(66), None);
        assert_eq!(Opcode::from_number(u32::MAX), None);
    }
}
