// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_Precompiler
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_OPCodeSetTargetName, QCC_OPCodeSetTarget

//! FTEQCC's build pragmas as `QCC_PR_Precompiler` reads them (`docs/spec/dialect-fteqcc.md`
//! FTEF-12.11): `flag` and `keyword`, which set the compiler flags this profile keeps
//! ([`FlagSettings`]), and `optimise`, `opcode` and `TARGET`, which change only the build, each
//! with the warnings FTEQCC gives for what it does not know.

use qc_lexer::{LexFlag, LexFlags};

use crate::cond::fte_atoi;
use crate::fte_directive::com_parse;
use crate::fte_tables::{
    DP_MAX_REVISION, FLAGS, FTE_MAX_REVISION, FteFlag, OPCODES, OPTIMISATIONS, QSS_MAX_REVISION,
    REAL_OPCODES, TARGETS, TargetFormat,
};

/// The pragmas `QCC_PR_Precompiler` knows; any other is "Unknown pragma" (FTEF-12.11).
const KNOWN: [&str; 24] = [
    "DONT_COMPILE_THIS_FILE",
    "COPYRIGHT",
    "compress",
    "forcecrc",
    "framerate",
    "once",
    "file",
    "line",
    "includedir",
    "noref",
    "nosave",
    "defaultstatic",
    "autoproto",
    "wrasm",
    "optimise",
    "optimize",
    "sourcefile",
    "TARGET",
    "PROGS_SRC",
    "PROGS_DAT",
    "opcode",
    "keyword",
    "flag",
    "warning",
];

/// Whether `QCC_PR_Precompiler` knows the pragma `name` (`QC_strcasecmp`).
#[must_use]
pub(crate) fn is_known(name: &[u8]) -> bool {
    KNOWN
        .iter()
        .any(|known| known.as_bytes().eq_ignore_ascii_case(name))
}

/// Which of FTEQCC's compiler flags and keywords are on (FTEF-12.11): `compiler_flag[]`'s
/// defaults, and what `#pragma flag` has set since.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlagSettings {
    on: [bool; FLAGS.len()],
}

impl Default for FlagSettings {
    /// `*compiler_flag[p].enabled = !!(compiler_flag[p].flags & FLAG_ASDEFAULT);` for every row,
    /// the last row of a flag deciding it.
    fn default() -> Self {
        let mut settings = Self {
            on: [false; FLAGS.len()],
        };
        for row in FLAGS {
            settings.set(row.flag, row.default);
        }
        settings
    }
}

impl FlagSettings {
    /// Whether `flag` is on.
    #[must_use]
    pub fn is_on(&self, flag: FteFlag) -> bool {
        FLAGS
            .iter()
            .position(|row| row.flag == flag)
            .and_then(|at| self.on.get(at))
            .copied()
            .unwrap_or(false)
    }

    /// The flags among these that the lexer reads (FTEF-12.12).
    #[must_use]
    pub fn lex_flags(&self) -> LexFlags {
        [
            (FteFlag::Qcfuncs, LexFlag::QcFuncs),
            (FteFlag::Qccx, LexFlag::Qccx),
            (FteFlag::Utf8strings, LexFlag::Utf8Strings),
            (FteFlag::Ilp32, LexFlag::Ilp32),
            (FteFlag::Dblstarexp, LexFlag::Ssp),
            (FteFlag::AssumeInteger, LexFlag::AssumeInt),
            (FteFlag::Macroinstrings, LexFlag::MacroInStrings),
        ]
        .into_iter()
        .fold(LexFlags::NONE, |flags, (fte, lex)| {
            flags.with(lex, self.is_on(fte))
        })
    }

    /// Sets `flag`, in every row that names it.
    fn set(&mut self, flag: FteFlag, on: bool) {
        for (row, slot) in FLAGS.iter().zip(self.on.iter_mut()) {
            if row.flag == flag {
                *slot = on;
            }
        }
    }

    /// `#pragma flag STATE NAME ...` (or `keyword`) with the text `message`: STATE `enable` or
    /// `on` sets each flag, `disable` or `off` clears it, any other STATE is "compiler flag state
    /// not recognised"; each NAME is the first row of `compiler_flag[]` whose name it is in any
    /// case, "keyword/flag NAME not recognised" when none, and "Cannot enable/disable
    /// keyword/flag via a pragma" when its row lacks `FLAG_MIDCOMPILE`. Returns the warnings.
    pub(crate) fn apply_pragma(&mut self, message: &[u8]) -> Vec<String> {
        let mut warnings = Vec::new();
        let state = com_parse(message, 0).and_then(|(state, at)| {
            let on = match state.to_ascii_lowercase().as_slice() {
                b"enable" | b"on" => true,
                b"disable" | b"off" => false,
                _ => return None,
            };
            Some((on, at))
        });
        let Some((on, mut at)) = state else {
            warnings.push("compiler flag state not recognised".to_owned());
            return warnings;
        };
        while let Some((name, next)) = com_parse(message, at) {
            at = next;
            match FLAGS
                .iter()
                .find(|row| row.name.as_bytes().eq_ignore_ascii_case(&name))
            {
                Some(row) if row.mid_compile => self.set(row.flag, on),
                Some(_) => {
                    warnings.push("Cannot enable/disable keyword/flag via a pragma".to_owned());
                }
                None => warnings.push(format!("keyword/flag {} not recognised", lossy(&name))),
            }
        }
        warnings
    }
}

/// `#pragma optimise` (or `optimize`, the pragma's name as written `name`) with the text
/// `message`, outside a function: a level `0` to `3`, `addon` or `mutator` sets the
/// optimisations; any other text, after an optional `no-`, names one, matched in any case by its
/// `abbrev` or `fullname`, and is "pragma NAME: TEXT unsupported" when it names none. Returns the
/// warnings.
pub(crate) fn optimise_warnings(name: &[u8], message: &[u8]) -> Vec<String> {
    if message.first().is_some_and(|b| (b'0'..=b'3').contains(b)) {
        return Vec::new();
    }
    let starts = |prefix: &str| {
        message
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
    };
    if starts("addon") || starts("mutator") {
        return Vec::new();
    }
    let opt = if starts("no-") {
        message.get(3..).unwrap_or_default()
    } else {
        message
    };
    let known = OPTIMISATIONS.iter().any(|(abbrev, full)| {
        (!abbrev.is_empty() && abbrev.as_bytes().eq_ignore_ascii_case(opt))
            || full.as_bytes().eq_ignore_ascii_case(opt)
    });
    if known {
        Vec::new()
    } else {
        vec![format!(
            "pragma {}: {} unsupported",
            lossy(name),
            lossy(opt)
        )]
    }
}

/// `#pragma opcode STATE NAME ...` with the text `message`: STATE `enable`, `on`, `disable` or
/// `off`, else "opcode state not recognised"; each NAME an `opname` of `pr_opcodes[]` in any case,
/// "opcode NAME is internal" past `OP_NUMREALOPS` and "opcode NAME not recognised" when none.
/// Returns the warnings.
pub(crate) fn opcode_warnings(message: &[u8]) -> Vec<String> {
    let state = com_parse(message, 0).filter(|(state, _)| {
        matches!(
            state.to_ascii_lowercase().as_slice(),
            b"enable" | b"on" | b"disable" | b"off"
        )
    });
    let Some((_, mut at)) = state else {
        return vec!["opcode state not recognised".to_owned()];
    };
    let mut warnings = Vec::new();
    while let Some((name, next)) = com_parse(message, at) {
        at = next;
        match OPCODES
            .iter()
            .position(|op| op.as_bytes().eq_ignore_ascii_case(&name))
        {
            Some(index) if index >= REAL_OPCODES => {
                warnings.push(format!("opcode {} is internal", lossy(&name)));
            }
            Some(_) => {}
            None => warnings.push(format!("opcode {} not recognised", lossy(&name))),
        }
    }
    warnings
}

/// `#pragma TARGET NAME` with the text `message`: its first token, `QCC_COM_Parse`'s, a name of
/// `targets[]` in any case, with an optional `_` and revision after it (`fte_24`); an unknown name
/// is "Unknown target 'NAME'. Ignored." with FTEQCC's list, and a revision past the format's
/// highest "target revision N is unknown, assuming revision M". The switch to or from a Hexen II
/// format after the first statement, which FTEQCC refuses, is the compiler's to see. Returns the
/// warnings, which have no name.
pub(crate) fn target_warnings(message: &[u8]) -> Vec<String> {
    let name = com_parse(message, 0)
        .map(|(name, _)| name)
        .unwrap_or_default();
    let (base, revision) = match name.iter().position(|&b| b == b'_') {
        Some(at) => (
            name.get(..at).unwrap_or_default(),
            name.get(at.saturating_add(1)..),
        ),
        None => (name.as_slice(), None),
    };
    let Some(&(_, format, default)) = TARGETS
        .iter()
        .find(|(target, _, _)| target.as_bytes().eq_ignore_ascii_case(base))
    else {
        return vec![format!(
            "Unknown target '{}'. Ignored.\nValid targets are: ID, HEXEN2, FTE, FTEH2, KK7, DP(patched)",
            lossy(&name)
        )];
    };
    // `ver?atoi(ver):targets[i].defaultrev`, kept as an `unsigned int`.
    let revision = revision.map_or(default, |text| {
        u32::from_le_bytes(fte_atoi(text).to_le_bytes())
    });
    let max = match format {
        TargetFormat::Fte | TargetFormat::Fteh2 | TargetFormat::FteDebug => FTE_MAX_REVISION,
        TargetFormat::Darkplaces => DP_MAX_REVISION,
        TargetFormat::Qss => QSS_MAX_REVISION,
        TargetFormat::Standard
        | TargetFormat::Hexen2
        | TargetFormat::Uhexen2
        | TargetFormat::Kk7
        | TargetFormat::Qtest => 0,
    };
    if revision > max && revision != u32::MAX {
        vec![format!(
            "target revision {revision} is unknown, assuming revision {max}"
        )]
    } else {
        Vec::new()
    }
}

/// `bytes` as text, each byte that is no UTF-8 replaced.
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
