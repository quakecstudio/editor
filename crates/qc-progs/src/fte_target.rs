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

/// The bits of a word of the valid set.
const WORD_BITS: usize = 64;

/// The words of the valid set, one bit per opcode.
const WORDS: usize = Op::ALL.len().div_ceil(WORD_BITS);

/// A revision of an output format (`qcc_targetversion`): the engine build whose opcodes a
/// program may use.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(u32);

impl Revision {
    /// `QCTARGVER_FTE_DEF`, FTE's default, from which it has the unsigned, 64-bit and double
    /// opcodes (`if (num >= OP_LT_U) return (qcc_targetversion>=5768);`).
    pub const FTE_DEFAULT: Self = Self(5768);
    /// `QCTARGVER_FTE_MAX`, `QCTARGVER_FTE_PRELOCS`: FTE's highest, from which it has `OP_PUSH`
    /// and the opcodes from `OP_STOREP_I16`.
    pub const FTE_MAX: Self = Self(6614);
    /// From which FTE has the 8-bit opcodes (`if (num >= OP_STOREP_I8) return
    /// (qcc_targetversion>=5744);`).
    pub const FTE_BYTES: Self = Self(5744);
    /// `QCTARGVER_FTE_STOREP_IDX`, from which FTE's stores through a pointer take an offset.
    pub const FTE_STOREP_INDEX: Self = Self(5712);
    /// From which FTE has the stores to a field (`if (num >= OP_STOREF_V) return
    /// (qcc_targetversion>=5698);`).
    pub const FTE_FIELD_STORES: Self = Self(5698);
    /// From which FTE has the opcodes from `OP_IF_F` (`return (qcc_targetversion>=3349);`).
    pub const FTE_IF_FLOAT: Self = Self(3349);
    /// `QCTARGVER_DP_DEF`, DarkPlaces' default.
    pub const DP_DEFAULT: Self = Self(12_901);
    /// `QCTARGVER_DP_STOREP_IDX`, from which DarkPlaces has the pointer opcodes and its stores
    /// through a pointer take an offset.
    pub const DP_STOREP_INDEX: Self = Self(20_241_108);
    /// `QCTARGVER_DP_MAX`, DarkPlaces' highest, from which it has the shifts.
    pub const DP_MAX: Self = Self(20_250_104);
    /// `QCTARGVER_QSS_MAX`, and the only revision of a format with none.
    pub const NONE: Self = Self(0);

    /// The revision numbered `number`.
    #[must_use]
    pub const fn new(number: u32) -> Self {
        Self(number)
    }

    /// Its number.
    #[must_use]
    pub const fn number(self) -> u32 {
        self.0
    }
}

/// The output target where a token stands: its format, its revision and the opcodes it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpcodeTarget {
    format: TargetFormat,
    revision: Revision,
    valid: [u64; WORDS],
}

impl Default for OpcodeTarget {
    /// The standard format, revision 0, before the command line's `-T` (FTEF-25.1).
    fn default() -> Self {
        Self::new(TargetFormat::Standard, Revision::NONE)
    }
}

impl OpcodeTarget {
    /// `QCC_OPCodeSetTarget(format, revision)`: the revision held to the format's highest, and
    /// each opcode valid as `QCC_OPCodeValidForTarget` says (FTEF-25.1, FTEF-25.2).
    #[must_use]
    pub fn new(format: TargetFormat, revision: Revision) -> Self {
        let revision = revision.min(format.max_revision());
        let mut target = Self {
            format,
            revision,
            valid: [0; WORDS],
        };
        for op in Op::ALL {
            if target.has(op) {
                target.enable(op);
            }
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
    pub const fn revision(&self) -> Revision {
        self.revision
    }

    /// Whether `op` is valid, `QCC_OPCodeValid` (`OPF_VALID`).
    #[must_use]
    pub fn valid(&self, op: Op) -> bool {
        let (word, mask) = Self::bit(op);
        self.valid.get(word).is_some_and(|w| w & mask != 0)
    }

    /// `QCC_OPCode_StorePOffset`: whether a store through a pointer takes an offset, on FTE's
    /// formats from revision 5712 (`QCTARGVER_FTE_STOREP_IDX`), on DarkPlaces' from 20241108
    /// (`QCTARGVER_DP_STOREP_IDX`) and on every other format (FTEF-6.18).
    #[must_use]
    pub fn stores_at_offset(&self) -> bool {
        match self.format {
            TargetFormat::Fte | TargetFormat::Fteh2 | TargetFormat::FteDebug => {
                self.revision >= Revision::FTE_STOREP_INDEX
            }
            TargetFormat::Darkplaces => self.revision >= Revision::DP_STOREP_INDEX,
            TargetFormat::Standard
            | TargetFormat::Qss
            | TargetFormat::Hexen2
            | TargetFormat::Uhexen2
            | TargetFormat::Kk7
            | TargetFormat::Qtest => true,
        }
    }

    /// Marks `op` valid, as `#pragma opcode enable` does (FTEF-25.3).
    pub fn enable(&mut self, op: Op) {
        let (word, mask) = Self::bit(op);
        if let Some(w) = self.valid.get_mut(word) {
            *w |= mask;
        }
    }

    /// Marks `op` not valid, as `#pragma opcode disable` does (FTEF-25.3).
    pub fn disable(&mut self, op: Op) {
        let (word, mask) = Self::bit(op);
        if let Some(w) = self.valid.get_mut(word) {
            *w &= !mask;
        }
    }

    /// The word of the valid set `op` is in, and its bit there.
    fn bit(op: Op) -> (usize, u64) {
        let index = op.number();
        let bit =
            u32::try_from(index.checked_rem(WORD_BITS).unwrap_or_default()).unwrap_or_default();
        (
            index.checked_div(WORD_BITS).unwrap_or_default(),
            1_u64.checked_shl(bit).unwrap_or_default(),
        )
    }

    /// `QCC_OPCodeValidForTarget(format, revision, op)` (FTEF-25.2).
    fn has(&self, op: Op) -> bool {
        let revision = self.revision;
        if op.is_internal() {
            return false;
        }
        match self.format {
            TargetFormat::Standard | TargetFormat::Kk7 | TargetFormat::Qtest => op < Op::MulstoreF,
            TargetFormat::Hexen2 | TargetFormat::Uhexen2 => {
                if (Op::SwitchV..=Op::SwitchFnc).contains(&op) {
                    return false;
                }
                op <= Op::Call8h
            }
            TargetFormat::Fte | TargetFormat::Fteh2 | TargetFormat::FteDebug => {
                if op == Op::Push || op >= Op::StorepI16 {
                    revision >= Revision::FTE_MAX
                } else if op >= Op::LtU {
                    revision >= Revision::FTE_DEFAULT
                } else if op >= Op::StorepI8 {
                    revision >= Revision::FTE_BYTES
                } else if op >= Op::StorefV {
                    revision >= Revision::FTE_FIELD_STORES
                } else if op >= Op::IfF {
                    revision >= Revision::FTE_IF_FLOAT
                } else {
                    true
                }
            }
            TargetFormat::Qss => op < Op::MulstoreF || QSS_OPCODES.contains(&op),
            TargetFormat::Darkplaces => {
                if op < Op::MulstoreF {
                    true
                } else if DP_SHIFTS.contains(&op) {
                    revision >= Revision::DP_MAX
                } else if DP_POINTERS.contains(&op) {
                    revision >= Revision::DP_STOREP_INDEX
                } else if DP_FIXED.contains(&op) {
                    revision >= Revision::DP_DEFAULT
                } else {
                    DP_OPCODES.contains(&op)
                }
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

/// An output format of `targets[]` (`qcc_targetformat_t`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TargetFormat {
    /// `QCF_STANDARD`, the default.
    #[default]
    Standard,
    /// `QCF_QSS`.
    Qss,
    /// `QCF_HEXEN2`.
    Hexen2,
    /// `QCF_UHEXEN2`.
    Uhexen2,
    /// `QCF_KK7`.
    Kk7,
    /// `QCF_FTE`.
    Fte,
    /// `QCF_FTEH2`.
    Fteh2,
    /// `QCF_FTEDEBUG`.
    FteDebug,
    /// `QCF_DARKPLACES`.
    Darkplaces,
    /// `QCF_QTEST`.
    Qtest,
}

impl TargetFormat {
    /// Whether `QCC_PR_DummyDef` takes it for Hexen II's, under which a definition named `float`
    /// leaves the keyword on: `QCF_HEXEN2` or `QCF_UHEXEN2` (FTEF-20.4).
    #[must_use]
    pub const fn keeps_float(self) -> bool {
        matches!(self, Self::Hexen2 | Self::Uhexen2)
    }

    /// Whether `QCC_SetDefaultProperties` turns on `thinktime`, `until` and `loop` for it:
    /// `QCF_HEXEN2`, `QCF_UHEXEN2` or `QCF_FTEH2` (FTEF-20.3).
    #[must_use]
    pub const fn is_hexen2(self) -> bool {
        matches!(self, Self::Hexen2 | Self::Uhexen2 | Self::Fteh2)
    }

    /// The `flag_undefwordsize` `QCC_OPCodeSetTarget` sets for it, `true` only for
    /// `QCF_DARKPLACES`: `flag_undefwordsize = false; ... case QCF_DARKPLACES: flag_undefwordsize
    /// = true;` (FTEF-22.4).
    #[must_use]
    pub const fn undefined_word_size(self) -> bool {
        matches!(self, Self::Darkplaces)
    }

    /// Whether `QCC_PR_FinishCompilation` lets a function be `extern` under it: `QCF_FTE`,
    /// `QCF_FTEDEBUG` or `QCF_FTEH2` (`if (qcc_targetformat == QCF_FTE || qcc_targetformat ==
    /// QCF_FTEDEBUG || qcc_targetformat == QCF_FTEH2) externokay = true;`, FTEF-22.5).
    #[must_use]
    pub const fn supports_extern(self) -> bool {
        matches!(self, Self::Fte | Self::FteDebug | Self::Fteh2)
    }

    /// The highest revision of the format (`QCTARGVER_*_MAX`); a format with none has only
    /// [`Revision::NONE`].
    #[must_use]
    pub const fn max_revision(self) -> Revision {
        match self {
            Self::Fte | Self::Fteh2 | Self::FteDebug => Revision::FTE_MAX,
            Self::Darkplaces => Revision::DP_MAX,
            Self::Qss | Self::Standard | Self::Hexen2 | Self::Uhexen2 | Self::Kk7 | Self::Qtest => {
                Revision::NONE
            }
        }
    }

    /// Whether `QCC_WriteData` writes version 7 for it in the 16-bit layout:
    /// `progs.version = PROG_EXTENDEDVERSION;` under `QCF_FTE`, `QCF_FTEH2`, `QCF_FTEDEBUG` and
    /// `QCF_QSS` (FTE-24.3); DarkPlaces' stays version 6.
    #[must_use]
    pub const fn writes_version_7(self) -> bool {
        matches!(self, Self::Fte | Self::Fteh2 | Self::FteDebug | Self::Qss)
    }
}

/// `targets[]`: each name `#pragma TARGET` matches in any case, its format and its default
/// revision.
pub const TARGETS: [(&str, TargetFormat, Revision); 21] = [
    ("standard", TargetFormat::Standard, Revision::NONE),
    ("vanilla", TargetFormat::Standard, Revision::NONE),
    ("q1", TargetFormat::Standard, Revision::NONE),
    ("id", TargetFormat::Standard, Revision::NONE),
    ("quakec", TargetFormat::Standard, Revision::NONE),
    ("qs", TargetFormat::Standard, Revision::NONE),
    ("qss", TargetFormat::Qss, Revision::NONE),
    ("hexen2", TargetFormat::Hexen2, Revision::NONE),
    ("h2", TargetFormat::Hexen2, Revision::NONE),
    ("uhexen2", TargetFormat::Uhexen2, Revision::NONE),
    ("kkqwsv", TargetFormat::Kk7, Revision::NONE),
    ("kk7", TargetFormat::Kk7, Revision::NONE),
    ("version7", TargetFormat::Kk7, Revision::NONE),
    ("fte", TargetFormat::Fte, Revision::FTE_DEFAULT),
    ("fteh2", TargetFormat::Fteh2, Revision::FTE_DEFAULT),
    ("ftedebug", TargetFormat::FteDebug, Revision::FTE_DEFAULT),
    ("debug", TargetFormat::FteDebug, Revision::FTE_DEFAULT),
    ("quake2c", TargetFormat::Fte, Revision::FTE_BYTES),
    ("darkplaces", TargetFormat::Darkplaces, Revision::DP_DEFAULT),
    ("dp", TargetFormat::Darkplaces, Revision::DP_DEFAULT),
    ("qtest", TargetFormat::Qtest, Revision::NONE),
];

#[cfg(test)]
mod tests {
    use super::{OpcodeTarget, Revision, TargetFormat};
    use crate::fte_ops::Op;

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
        let fte = OpcodeTarget::new(TargetFormat::Fte, Revision::new(5768));
        assert!(fte.valid(Op::AddI));
        assert!(fte.valid(Op::LtU));
        assert!(!fte.valid(Op::Push));
        let old = OpcodeTarget::new(TargetFormat::Fte, Revision::new(3000));
        assert!(!old.valid(Op::IfF));
        assert!(old.valid(Op::AddI));
        let past = OpcodeTarget::new(TargetFormat::Fte, Revision::new(u32::MAX));
        assert_eq!(past.revision(), Revision::FTE_MAX);
        assert!(past.valid(Op::Push));
        assert!(!past.valid(Op::Numrealops));
    }

    // spec FTEF-6.18: stores through a pointer take an offset from FTE's revision 5712 and
    // DarkPlaces' 20241108, and on every other format
    #[test]
    fn stores_take_an_offset_from_their_revision() {
        for format in [
            TargetFormat::Fte,
            TargetFormat::Fteh2,
            TargetFormat::FteDebug,
        ] {
            assert!(OpcodeTarget::new(format, Revision::new(5712)).stores_at_offset());
            assert!(!OpcodeTarget::new(format, Revision::new(5711)).stores_at_offset());
        }
        assert!(
            OpcodeTarget::new(TargetFormat::Darkplaces, Revision::new(20_241_108))
                .stores_at_offset()
        );
        assert!(
            !OpcodeTarget::new(TargetFormat::Darkplaces, Revision::new(20_241_107))
                .stores_at_offset()
        );
        for format in [
            TargetFormat::Standard,
            TargetFormat::Qss,
            TargetFormat::Hexen2,
            TargetFormat::Uhexen2,
            TargetFormat::Kk7,
            TargetFormat::Qtest,
        ] {
            assert!(OpcodeTarget::new(format, Revision::new(0)).stores_at_offset());
        }
    }

    // spec FTEF-25.2: Hexen II, QSS and DarkPlaces have their own extended opcodes
    #[test]
    fn other_formats() {
        let h2 = OpcodeTarget::new(TargetFormat::Hexen2, Revision::new(0));
        assert!(h2.valid(Op::Call8h));
        assert!(!h2.valid(Op::SwitchV));
        let qss = OpcodeTarget::new(TargetFormat::Qss, Revision::new(0));
        assert!(qss.valid(Op::AddI));
        assert!(!qss.valid(Op::AddSf));
        let dp = OpcodeTarget::new(TargetFormat::Darkplaces, Revision::new(12_901));
        assert!(dp.valid(Op::AddIf));
        assert!(!dp.valid(Op::LshiftI));
        assert!(!OpcodeTarget::new(TargetFormat::Darkplaces, Revision::new(0)).valid(Op::AddIf));
        assert!(
            OpcodeTarget::new(TargetFormat::Darkplaces, Revision::new(20_250_104))
                .valid(Op::LshiftI)
        );
    }

    // spec FTEF-25.3: an opcode is turned on and off one by one
    #[test]
    fn opcodes_turn_on_and_off() {
        let mut target = OpcodeTarget::default();
        target.enable(Op::AddI);
        assert!(target.valid(Op::AddI));
        target.disable(Op::AddF);
        assert!(!target.valid(Op::AddF));
    }
}
