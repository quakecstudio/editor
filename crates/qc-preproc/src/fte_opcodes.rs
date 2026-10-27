// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_OPCodeValidForTarget, QCC_OPCodeSetTarget
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_Precompiler

//! Which opcodes the output target has (`docs/spec/dialect-fteqcc.md` FTEF-25): FTEQCC's
//! `OPF_VALID`, which `QCC_OPCodeSetTarget` sets for every opcode from the target's format and
//! revision and `#pragma opcode` turns on and off one by one. The check stage reads it where it
//! types an operator, to find the opcodes it must emulate or refuse.

use crate::fte_tables::{
    DP_MAX_REVISION, FTE_MAX_REVISION, OPCODES, QSS_MAX_REVISION, REAL_OPCODES, TargetFormat,
};

/// The words of the valid set, one bit per opcode of [`OPCODES`].
const WORDS: usize = OPCODES.len().div_ceil(64);

/// The output target where a token stands: its format, its revision and the opcodes it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpcodeTarget {
    format: TargetFormat,
    revision: u32,
    valid: [u64; WORDS],
}

impl Default for OpcodeTarget {
    /// The standard format, revision 0, before the command line's `-T` (FTEF-25.1).
    fn default() -> Self {
        Self::new(TargetFormat::Standard, 0)
    }
}

impl OpcodeTarget {
    /// `QCC_OPCodeSetTarget(format, revision)`: the revision held to the format's highest, and
    /// each opcode valid as `QCC_OPCodeValidForTarget` says (FTEF-25.1, FTEF-25.2).
    #[must_use]
    pub fn new(format: TargetFormat, revision: u32) -> Self {
        let revision = revision.min(max_revision(format));
        let mut target = Self {
            format,
            revision,
            valid: [0; WORDS],
        };
        for index in 0..OPCODES.len() {
            target.set(index, valid_for_target(format, revision, index));
        }
        target
    }

    /// The format.
    #[must_use]
    pub const fn format(&self) -> TargetFormat {
        self.format
    }

    /// The revision.
    #[must_use]
    pub const fn revision(&self) -> u32 {
        self.revision
    }

    /// Whether the opcode numbered `index` is valid, `QCC_OPCodeValid` (`OPF_VALID`).
    #[must_use]
    pub fn valid(&self, index: usize) -> bool {
        let (word, mask) = place(index);
        self.valid.get(word).is_some_and(|w| w & mask != 0)
    }

    /// Marks the opcode numbered `index` valid or not, as `#pragma opcode` does (FTEF-25.3).
    pub(crate) fn set(&mut self, index: usize, on: bool) {
        let (word, mask) = place(index);
        if let Some(w) = self.valid.get_mut(word) {
            if on {
                *w |= mask;
            } else {
                *w &= !mask;
            }
        }
    }
}

/// The word of the valid set the opcode numbered `index` is in, and its bit there.
fn place(index: usize) -> (usize, u64) {
    let bit = u32::try_from(index & 63).unwrap_or_default();
    (index >> 6, 1_u64.checked_shl(bit).unwrap_or_default())
}

/// The highest revision of `format` (`QCTARGVER_*_MAX`); a format with none has only 0.
const fn max_revision(format: TargetFormat) -> u32 {
    match format {
        TargetFormat::Fte | TargetFormat::Fteh2 | TargetFormat::FteDebug => FTE_MAX_REVISION,
        TargetFormat::Darkplaces => DP_MAX_REVISION,
        TargetFormat::Qss => QSS_MAX_REVISION,
        TargetFormat::Standard
        | TargetFormat::Hexen2
        | TargetFormat::Uhexen2
        | TargetFormat::Kk7
        | TargetFormat::Qtest => 0,
    }
}

/// The number of the opcode named `opname`, `OP_<opname>`.
fn op(opname: &str) -> usize {
    OPCODES
        .iter()
        .position(|&name| name == opname)
        .unwrap_or(OPCODES.len())
}

/// `QCC_OPCodeValidForTarget(format, revision, &pr_opcodes[index])` (FTEF-25.2).
fn valid_for_target(format: TargetFormat, revision: u32, index: usize) -> bool {
    if index >= REAL_OPCODES {
        return false;
    }
    match format {
        TargetFormat::Standard | TargetFormat::Kk7 | TargetFormat::Qtest => {
            index < op("MULSTORE_F")
        }
        TargetFormat::Hexen2 | TargetFormat::Uhexen2 => {
            if (op("SWITCH_V")..=op("SWITCH_FNC")).contains(&index) {
                return false;
            }
            index <= op("CALL8H")
        }
        TargetFormat::Fte | TargetFormat::Fteh2 | TargetFormat::FteDebug => {
            if index == op("PUSH") || index >= op("STOREP_I16") {
                revision >= FTE_MAX_REVISION
            } else if index >= op("LT_U") {
                revision >= 5768
            } else if index >= op("STOREP_I8") {
                revision >= 5744
            } else if index >= op("STOREF_V") {
                revision >= 5698
            } else if index >= op("IF_F") {
                revision >= 3349
            } else {
                true
            }
        }
        TargetFormat::Qss => index < op("MULSTORE_F") || QSS_OPCODES.contains(&name_of(index)),
        TargetFormat::Darkplaces => {
            if index < op("MULSTORE_F") {
                return true;
            }
            let name = name_of(index);
            if DP_SHIFTS.contains(&name) {
                revision >= 20_250_104
            } else if DP_POINTERS.contains(&name) {
                revision >= 20_241_108
            } else if DP_FIXED.contains(&name) {
                revision >= 12_901
            } else {
                DP_OPCODES.contains(&name)
            }
        }
    }
}

/// The `opname` of the opcode numbered `index`.
fn name_of(index: usize) -> &'static str {
    OPCODES.get(index).copied().unwrap_or_default()
}

/// The extended opcodes `QCF_QSS` has.
const QSS_OPCODES: &[&str] = &[
    "ADD_I",
    "ADD_IF",
    "ADD_FI",
    "SUB_I",
    "SUB_IF",
    "SUB_FI",
    "MUL_I",
    "MUL_IF",
    "MUL_FI",
    "MUL_VI",
    "DIV_VF",
    "DIV_I",
    "DIV_IF",
    "DIV_FI",
    "BITAND_I",
    "BITOR_I",
    "BITAND_IF",
    "BITOR_IF",
    "BITAND_FI",
    "BITOR_FI",
    "GE_I",
    "LE_I",
    "GT_I",
    "LT_I",
    "AND_I",
    "OR_I",
    "GE_IF",
    "LE_IF",
    "GT_IF",
    "LT_IF",
    "AND_IF",
    "OR_IF",
    "GE_FI",
    "LE_FI",
    "GT_FI",
    "LT_FI",
    "AND_FI",
    "OR_FI",
    "NOT_I",
    "EQ_I",
    "EQ_IF",
    "EQ_FI",
    "NE_I",
    "NE_IF",
    "NE_FI",
    "CONV_ITOF",
    "CONV_FTOI",
    "BITXOR_I",
    "RSHIFT_I",
    "LSHIFT_I",
    "STORE_I",
    "STORE_IF",
    "STORE_FI",
    "IF_F",
    "IFNOT_F",
    "IF_S",
    "IFNOT_S",
    "STOREF_V",
    "STOREF_F",
    "STOREF_S",
    "STOREF_I",
    "GLOAD_I",
    "GLOAD_F",
    "GLOAD_FLD",
    "GLOAD_ENT",
    "GLOAD_S",
    "GLOAD_FNC",
    "GLOAD_V",
    "GSTOREP_I",
    "GSTOREP_F",
    "GSTOREP_ENT",
    "GSTOREP_FLD",
    "GSTOREP_S",
    "GSTOREP_FNC",
    "GSTOREP_V",
    "BOUNDCHECK",
    "LOADA_F",
    "LOADA_V",
    "LOADA_S",
    "LOADA_ENT",
    "LOADA_FLD",
    "LOADA_FNC",
    "LOADA_I",
    "GLOBALADDRESS",
    "FETCH_GBL_F",
    "FETCH_GBL_S",
    "FETCH_GBL_E",
    "FETCH_GBL_FNC",
    "FETCH_GBL_V",
    "CALL1H",
    "CALL2H",
    "CALL3H",
    "CALL4H",
    "CALL5H",
    "CALL6H",
    "CALL7H",
    "CALL8H",
];

/// `QCF_DARKPLACES`'s shift and unsigned opcodes, from revision 20250104.
const DP_SHIFTS: &[&str] = &["RSHIFT_I", "LSHIFT_I", "LE_U", "LT_U", "DIV_U", "RSHIFT_U"];

/// `QCF_DARKPLACES`'s pointer opcodes, from revision 20241108 (`QCTARGVER_DP_STOREP_IDX`).
const DP_POINTERS: &[&str] = &[
    "ADD_PIW",
    "GLOBALADDRESS",
    "LOADA_F",
    "LOADA_V",
    "LOADA_S",
    "LOADA_ENT",
    "LOADA_FLD",
    "LOADA_FNC",
    "LOADA_I",
    "LOAD_P",
    "LOADP_F",
    "LOADP_V",
    "LOADP_S",
    "LOADP_ENT",
    "LOADP_FLD",
    "LOADP_FNC",
    "LOADP_I",
];

/// `QCF_DARKPLACES`'s opcodes that were buggy before revision 12901.
const DP_FIXED: &[&str] = &[
    "ADD_IF",
    "SUB_IF",
    "MUL_IF",
    "DIV_IF",
    "BITAND_FI",
    "BITOR_FI",
    "STORE_I",
    "STORE_P",
];

/// `QCF_DARKPLACES`'s other extended opcodes, at every revision.
const DP_OPCODES: &[&str] = &[
    "ADD_I",
    "ADD_FI",
    "SUB_I",
    "SUB_FI",
    "CONV_ITOF",
    "CONV_FTOI",
    "LOAD_I",
    "STOREP_I",
    "BITAND_I",
    "BITOR_I",
    "MUL_I",
    "DIV_I",
    "EQ_I",
    "NE_I",
    "NOT_I",
    "DIV_VF",
    "LE_I",
    "GE_I",
    "LT_I",
    "GT_I",
    "LE_IF",
    "GE_IF",
    "LT_IF",
    "GT_IF",
    "LE_FI",
    "GE_FI",
    "LT_FI",
    "GT_FI",
    "EQ_IF",
    "EQ_FI",
    "MUL_FI",
    "MUL_VI",
    "DIV_FI",
    "BITAND_IF",
    "BITOR_IF",
    "AND_I",
    "OR_I",
    "AND_IF",
    "OR_IF",
    "AND_FI",
    "OR_FI",
    "NE_IF",
    "NE_FI",
    "GSTOREP_I",
    "GSTOREP_F",
    "GSTOREP_ENT",
    "GSTOREP_FLD",
    "GSTOREP_S",
    "GSTOREP_FNC",
    "GSTOREP_V",
    "GLOAD_I",
    "GLOAD_F",
    "GLOAD_FLD",
    "GLOAD_ENT",
    "GLOAD_S",
    "GLOAD_FNC",
    "BOUNDCHECK",
    "GLOAD_V",
];

#[cfg(test)]
mod tests {
    use super::{OpcodeTarget, op};
    use crate::fte_tables::TargetFormat;

    // spec FTEF-25.2: the standard format has id's opcodes and no extended one
    #[test]
    fn the_standard_format_has_ids_opcodes() {
        let target = OpcodeTarget::default();
        assert!(target.valid(op("ADD_F")));
        assert!(target.valid(op("MULSTORE_F") - 1));
        assert!(!target.valid(op("MULSTORE_F")));
        assert!(!target.valid(op("ADD_I")));
        assert!(!target.valid(op("ADD_SF")));
    }

    // spec FTEF-25.1: the revision is held to the format's highest, and each FTE opcode is valid
    // from its revision
    // spec FTEF-25.2: the FTE formats by revision, and nothing from OP_NUMREALOPS
    #[test]
    fn fte_opcodes_by_revision() {
        let fte = OpcodeTarget::new(TargetFormat::Fte, 5768);
        assert!(fte.valid(op("ADD_I")));
        assert!(fte.valid(op("LT_U")));
        assert!(!fte.valid(op("PUSH")));
        let old = OpcodeTarget::new(TargetFormat::Fte, 3000);
        assert!(!old.valid(op("IF_F")));
        assert!(old.valid(op("ADD_I")));
        let past = OpcodeTarget::new(TargetFormat::Fte, u32::MAX);
        assert_eq!(past.revision(), 6614);
        assert!(past.valid(op("PUSH")));
        assert!(!past.valid(282));
    }

    // spec FTEF-25.2: Hexen II, QSS and DarkPlaces have their own extended opcodes
    #[test]
    fn other_formats() {
        let h2 = OpcodeTarget::new(TargetFormat::Hexen2, 0);
        assert!(h2.valid(op("CALL8H")));
        assert!(!h2.valid(op("SWITCH_V")));
        let qss = OpcodeTarget::new(TargetFormat::Qss, 0);
        assert!(qss.valid(op("ADD_I")));
        assert!(!qss.valid(op("ADD_SF")));
        let dp = OpcodeTarget::new(TargetFormat::Darkplaces, 12_901);
        assert!(dp.valid(op("ADD_IF")));
        assert!(!dp.valid(op("LSHIFT_I")));
        assert!(!OpcodeTarget::new(TargetFormat::Darkplaces, 0).valid(op("ADD_IF")));
        assert!(OpcodeTarget::new(TargetFormat::Darkplaces, 20_250_104).valid(op("LSHIFT_I")));
    }

    // spec FTEF-25.3: an opcode is turned on and off one by one
    #[test]
    fn opcodes_turn_on_and_off() {
        let mut target = OpcodeTarget::default();
        target.set(op("ADD_I"), true);
        assert!(target.valid(op("ADD_I")));
        target.set(op("ADD_F"), false);
        assert!(!target.valid(op("ADD_F")));
    }
}
