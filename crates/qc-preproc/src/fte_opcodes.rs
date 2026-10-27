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

use crate::fte_ops::Op;
use crate::fte_tables::{DP_MAX_REVISION, FTE_MAX_REVISION, QSS_MAX_REVISION, TargetFormat};

/// The words of the valid set, one bit per opcode.
const WORDS: usize = Op::ALL.len().div_ceil(64);

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
        for op in Op::ALL {
            target.set(op, valid_for_target(format, revision, op));
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

    /// Whether `op` is valid, `QCC_OPCodeValid` (`OPF_VALID`).
    #[must_use]
    pub fn valid(&self, op: Op) -> bool {
        let (word, mask) = place(op);
        self.valid.get(word).is_some_and(|w| w & mask != 0)
    }

    /// Marks `op` valid or not, as `#pragma opcode` does (FTEF-25.3).
    pub(crate) fn set(&mut self, op: Op, on: bool) {
        let (word, mask) = place(op);
        if let Some(w) = self.valid.get_mut(word) {
            if on {
                *w |= mask;
            } else {
                *w &= !mask;
            }
        }
    }
}

/// The word of the valid set `op` is in, and its bit there.
fn place(op: Op) -> (usize, u64) {
    let index = op.number();
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

/// `QCC_OPCodeValidForTarget(format, revision, op)` (FTEF-25.2).
fn valid_for_target(format: TargetFormat, revision: u32, op: Op) -> bool {
    if op.is_internal() {
        return false;
    }
    match format {
        TargetFormat::Standard | TargetFormat::Kk7 | TargetFormat::Qtest => op < Op::MulstoreF,
        TargetFormat::Hexen2 | TargetFormat::Uhexen2 => {
            if (Op::SwitchV..=Op::SwitchFnc).contains(&op) {
                return false;
            }
            op <= Op::Call8h
        }
        TargetFormat::Fte | TargetFormat::Fteh2 | TargetFormat::FteDebug => {
            if op == Op::Push || op >= Op::StorepI16 {
                revision >= FTE_MAX_REVISION
            } else if op >= Op::LtU {
                revision >= 5768
            } else if op >= Op::StorepI8 {
                revision >= 5744
            } else if op >= Op::StorefV {
                revision >= 5698
            } else if op >= Op::IfF {
                revision >= 3349
            } else {
                true
            }
        }
        TargetFormat::Qss => op < Op::MulstoreF || QSS_OPCODES.contains(&op),
        TargetFormat::Darkplaces => {
            if op < Op::MulstoreF {
                true
            } else if DP_SHIFTS.contains(&op) {
                revision >= 20_250_104
            } else if DP_POINTERS.contains(&op) {
                revision >= 20_241_108
            } else if DP_FIXED.contains(&op) {
                revision >= 12_901
            } else {
                DP_OPCODES.contains(&op)
            }
        }
    }
}

/// The extended opcodes `QCF_QSS` has.
const QSS_OPCODES: &[Op] = &[
    Op::AddI,
    Op::AddIf,
    Op::AddFi,
    Op::SubI,
    Op::SubIf,
    Op::SubFi,
    Op::MulI,
    Op::MulIf,
    Op::MulFi,
    Op::MulVi,
    Op::DivVf,
    Op::DivI,
    Op::DivIf,
    Op::DivFi,
    Op::BitandI,
    Op::BitorI,
    Op::BitandIf,
    Op::BitorIf,
    Op::BitandFi,
    Op::BitorFi,
    Op::GeI,
    Op::LeI,
    Op::GtI,
    Op::LtI,
    Op::AndI,
    Op::OrI,
    Op::GeIf,
    Op::LeIf,
    Op::GtIf,
    Op::LtIf,
    Op::AndIf,
    Op::OrIf,
    Op::GeFi,
    Op::LeFi,
    Op::GtFi,
    Op::LtFi,
    Op::AndFi,
    Op::OrFi,
    Op::NotI,
    Op::EqI,
    Op::EqIf,
    Op::EqFi,
    Op::NeI,
    Op::NeIf,
    Op::NeFi,
    Op::ConvItof,
    Op::ConvFtoi,
    Op::BitxorI,
    Op::RshiftI,
    Op::LshiftI,
    Op::StoreI,
    Op::StoreIf,
    Op::StoreFi,
    Op::IfF,
    Op::IfnotF,
    Op::IfS,
    Op::IfnotS,
    Op::StorefV,
    Op::StorefF,
    Op::StorefS,
    Op::StorefI,
    Op::GloadI,
    Op::GloadF,
    Op::GloadFld,
    Op::GloadEnt,
    Op::GloadS,
    Op::GloadFnc,
    Op::GloadV,
    Op::GstorepI,
    Op::GstorepF,
    Op::GstorepEnt,
    Op::GstorepFld,
    Op::GstorepS,
    Op::GstorepFnc,
    Op::GstorepV,
    Op::Boundcheck,
    Op::LoadaF,
    Op::LoadaV,
    Op::LoadaS,
    Op::LoadaEnt,
    Op::LoadaFld,
    Op::LoadaFnc,
    Op::LoadaI,
    Op::Globaladdress,
    Op::FetchGblF,
    Op::FetchGblS,
    Op::FetchGblE,
    Op::FetchGblFnc,
    Op::FetchGblV,
    Op::Call1h,
    Op::Call2h,
    Op::Call3h,
    Op::Call4h,
    Op::Call5h,
    Op::Call6h,
    Op::Call7h,
    Op::Call8h,
];

/// `QCF_DARKPLACES`'s shift and unsigned opcodes, from revision 20250104.
const DP_SHIFTS: &[Op] = &[
    Op::RshiftI,
    Op::LshiftI,
    Op::LeU,
    Op::LtU,
    Op::DivU,
    Op::RshiftU,
];

/// `QCF_DARKPLACES`'s pointer opcodes, from revision 20241108 (`QCTARGVER_DP_STOREP_IDX`).
const DP_POINTERS: &[Op] = &[
    Op::AddPiw,
    Op::Globaladdress,
    Op::LoadaF,
    Op::LoadaV,
    Op::LoadaS,
    Op::LoadaEnt,
    Op::LoadaFld,
    Op::LoadaFnc,
    Op::LoadaI,
    Op::LoadP,
    Op::LoadpF,
    Op::LoadpV,
    Op::LoadpS,
    Op::LoadpEnt,
    Op::LoadpFld,
    Op::LoadpFnc,
    Op::LoadpI,
];

/// `QCF_DARKPLACES`'s opcodes that were buggy before revision 12901.
const DP_FIXED: &[Op] = &[
    Op::AddIf,
    Op::SubIf,
    Op::MulIf,
    Op::DivIf,
    Op::BitandFi,
    Op::BitorFi,
    Op::StoreI,
    Op::StoreP,
];

/// `QCF_DARKPLACES`'s other extended opcodes, at every revision.
const DP_OPCODES: &[Op] = &[
    Op::AddI,
    Op::AddFi,
    Op::SubI,
    Op::SubFi,
    Op::ConvItof,
    Op::ConvFtoi,
    Op::LoadI,
    Op::StorepI,
    Op::BitandI,
    Op::BitorI,
    Op::MulI,
    Op::DivI,
    Op::EqI,
    Op::NeI,
    Op::NotI,
    Op::DivVf,
    Op::LeI,
    Op::GeI,
    Op::LtI,
    Op::GtI,
    Op::LeIf,
    Op::GeIf,
    Op::LtIf,
    Op::GtIf,
    Op::LeFi,
    Op::GeFi,
    Op::LtFi,
    Op::GtFi,
    Op::EqIf,
    Op::EqFi,
    Op::MulFi,
    Op::MulVi,
    Op::DivFi,
    Op::BitandIf,
    Op::BitorIf,
    Op::AndI,
    Op::OrI,
    Op::AndIf,
    Op::OrIf,
    Op::AndFi,
    Op::OrFi,
    Op::NeIf,
    Op::NeFi,
    Op::GstorepI,
    Op::GstorepF,
    Op::GstorepEnt,
    Op::GstorepFld,
    Op::GstorepS,
    Op::GstorepFnc,
    Op::GstorepV,
    Op::GloadI,
    Op::GloadF,
    Op::GloadFld,
    Op::GloadEnt,
    Op::GloadS,
    Op::GloadFnc,
    Op::Boundcheck,
    Op::GloadV,
];

#[cfg(test)]
mod tests {
    use super::OpcodeTarget;
    use crate::fte_ops::Op;
    use crate::fte_tables::TargetFormat;

    // spec FTEF-25.2: the standard format has id's opcodes and no extended one
    #[test]
    fn the_standard_format_has_ids_opcodes() {
        let target = OpcodeTarget::default();
        assert!(target.valid(Op::AddF));
        assert!(target.valid(Op::BitorF));
        assert!(!target.valid(Op::MulstoreF));
        assert!(!target.valid(Op::AddI));
        assert!(!target.valid(Op::AddSf));
    }

    // spec FTEF-25.1: the revision is held to the format's highest, and each FTE opcode is valid
    // from its revision
    // spec FTEF-25.2: the FTE formats by revision, and nothing from OP_NUMREALOPS
    #[test]
    fn fte_opcodes_by_revision() {
        let fte = OpcodeTarget::new(TargetFormat::Fte, 5768);
        assert!(fte.valid(Op::AddI));
        assert!(fte.valid(Op::LtU));
        assert!(!fte.valid(Op::Push));
        let old = OpcodeTarget::new(TargetFormat::Fte, 3000);
        assert!(!old.valid(Op::IfF));
        assert!(old.valid(Op::AddI));
        let past = OpcodeTarget::new(TargetFormat::Fte, u32::MAX);
        assert_eq!(past.revision(), 6614);
        assert!(past.valid(Op::Push));
        assert!(!past.valid(Op::Numrealops));
    }

    // spec FTEF-25.2: Hexen II, QSS and DarkPlaces have their own extended opcodes
    #[test]
    fn other_formats() {
        let h2 = OpcodeTarget::new(TargetFormat::Hexen2, 0);
        assert!(h2.valid(Op::Call8h));
        assert!(!h2.valid(Op::SwitchV));
        let qss = OpcodeTarget::new(TargetFormat::Qss, 0);
        assert!(qss.valid(Op::AddI));
        assert!(!qss.valid(Op::AddSf));
        let dp = OpcodeTarget::new(TargetFormat::Darkplaces, 12_901);
        assert!(dp.valid(Op::AddIf));
        assert!(!dp.valid(Op::LshiftI));
        assert!(!OpcodeTarget::new(TargetFormat::Darkplaces, 0).valid(Op::AddIf));
        assert!(OpcodeTarget::new(TargetFormat::Darkplaces, 20_250_104).valid(Op::LshiftI));
    }

    // spec FTEF-25.3: an opcode is turned on and off one by one
    #[test]
    fn opcodes_turn_on_and_off() {
        let mut target = OpcodeTarget::default();
        target.set(Op::AddI, true);
        assert!(target.valid(Op::AddI));
        target.set(Op::AddF, false);
        assert!(!target.valid(Op::AddF));
    }
}
