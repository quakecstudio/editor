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

use qc_lexer::LexFlags;

use crate::compiler_flags::{CompilerFlags, PragmaDefault, Priorities};
use crate::cond::fte_atoi;
use crate::fte_directive::com_parse;
use crate::fte_keywords::{Keyword, Keywords};
use crate::fte_ops::Op;
use crate::fte_tables::{
    DP_MAX_REVISION, FLAGS, FTE_MAX_REVISION, FteFlag, OPTIMISATIONS, QSS_MAX_REVISION, TARGETS,
    TargetFormat,
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
    /// What the preprocessor reads for every token, kept as the flags change so the table is
    /// not searched for it each time: the lexer's flags, `hashonly` and `nofileline`.
    read: Read,
    /// The operator priorities `QCC_PrioritiseOpcodes` set last (FTEF-21.4).
    priorities: Priorities,
    /// The defaults `#pragma noref`, `nosave` and `defaultstatic` set (FTEF-21.14), the other
    /// bits clear.
    pragma_defaults: CompilerFlags,
}

/// The flags of [`FlagSettings`] the preprocessor reads for every token (FTEF-12.12,
/// FTEF-12.13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Read {
    compiler: CompilerFlags,
    hash_only: bool,
    no_file_line: bool,
}

impl Default for FlagSettings {
    /// `*compiler_flag[p].enabled = !!(compiler_flag[p].flags & FLAG_ASDEFAULT);` for every row,
    /// the last row of a flag deciding it.
    fn default() -> Self {
        let mut settings = Self {
            on: [false; FLAGS.len()],
            read: Read::default(),
            priorities: Priorities::Qc,
            pragma_defaults: CompilerFlags::default().with(FteFlag::Qcfuncs, false),
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
        self.read.compiler.lex()
    }

    /// The flags among these that the lexer, the parser and the semantic model read, with the
    /// operator priorities (FTEF-21.1).
    #[must_use]
    pub fn compiler_flags(&self) -> CompilerFlags {
        [
            PragmaDefault::Static,
            PragmaDefault::Noref,
            PragmaDefault::Nosave,
        ]
        .into_iter()
        .fold(
            self.read.compiler.with_priorities(self.priorities),
            |flags, default| flags.with_default(default, self.pragma_defaults.default_on(default)),
        )
    }

    /// Sets the default `default` that `#pragma noref`, `nosave` or `defaultstatic` sets
    /// (FTEF-12.6, FTEF-21.14).
    pub(crate) const fn set_default(&mut self, default: PragmaDefault, on: bool) {
        self.pragma_defaults = self.pragma_defaults.with_default(default, on);
    }

    /// The operator priorities (FTEF-21.4).
    #[must_use]
    pub const fn priorities(&self) -> Priorities {
        self.priorities
    }

    /// `QCC_PrioritiseOpcodes`: the operator priorities by `cpriority` as it is now (FTEF-21.4),
    /// which a compile does once the command line is read and a `#pragma flag` that names
    /// `cpriority` does again.
    pub(crate) fn prioritise(&mut self) {
        self.priorities = if self.is_on(FteFlag::Cpriority) {
            Priorities::C
        } else {
            Priorities::Qc
        };
    }

    /// These settings with the operator priorities `priorities`, which no setting of a flag
    /// changes (FTEF-21.4).
    pub(crate) const fn with_priorities(mut self, priorities: Priorities) -> Self {
        self.priorities = priorities;
        self
    }

    /// The keywords on, with `kce` and `caseinsens`, under the standard target (FTEF-20.1).
    #[must_use]
    pub fn keywords(&self) -> Keywords {
        Keyword::ALL
            .into_iter()
            .fold(Keywords::NONE, |keywords, keyword| {
                keywords.with(keyword, self.is_on(keyword.flag()))
            })
            .with_coexist(self.is_on(FteFlag::KeywordsCoexist))
            .with_case_insensitive(self.is_on(FteFlag::Caseinsensitive))
    }

    /// Whether `hashonly` is on, under which only a name after a `#` is a macro's (FTEF-12.13).
    pub(crate) const fn hash_only(&self) -> bool {
        self.read.hash_only
    }

    /// Whether `nofileline` is on, under which `#pragma file` and `line` change nothing
    /// (FTEF-12.13).
    pub(crate) const fn no_file_line(&self) -> bool {
        self.read.no_file_line
    }

    /// [`Self::compiler_flags`] from the table, the priorities aside.
    fn read_compiler_flags(&self) -> CompilerFlags {
        FLAGS.iter().zip(self.on).fold(
            CompilerFlags::default().with(FteFlag::Qcfuncs, false),
            |flags, (row, on)| flags.with(row.flag, on),
        )
    }

    /// Sets the first row of `compiler_flag[]` whose abbreviation is `name` in any case, as a
    /// command-line option does, whether or not it may change mid-compile; nothing when no row
    /// is so named (FTEF-14.72). Returns whether a row is.
    pub(crate) fn set_named(&mut self, name: &[u8], on: bool) -> bool {
        let row = FLAGS
            .iter()
            .find(|row| row.name.as_bytes().eq_ignore_ascii_case(name));
        if let Some(row) = row {
            self.set(row.flag, on);
        }
        row.is_some()
    }

    /// Sets `flag`, in every row that names it.
    pub(crate) fn set(&mut self, flag: FteFlag, on: bool) {
        for (row, slot) in FLAGS.iter().zip(self.on.iter_mut()) {
            if row.flag == flag {
                *slot = on;
            }
        }
        self.read = Read {
            compiler: self.read_compiler_flags(),
            hash_only: self.is_on(FteFlag::Hashonly),
            no_file_line: self.is_on(FteFlag::Nopragmafileline),
        };
    }

    /// `#pragma flag STATE NAME ...` (or `keyword`) with the text `message`: STATE `enable` or
    /// `on` sets each flag, `disable` or `off` clears it, any other STATE is "compiler flag state
    /// not recognised"; each NAME is the first row of `compiler_flag[]` whose name it is in any
    /// case, "keyword/flag NAME not recognised" when none, and "Cannot enable/disable
    /// keyword/flag via a pragma" when its row lacks `FLAG_MIDCOMPILE`. Returns the warnings
    /// and the flags it set, in order.
    pub(crate) fn apply_pragma(&mut self, message: &[u8]) -> (Vec<String>, Vec<FteFlag>) {
        let mut warnings = Vec::new();
        let mut set = Vec::new();
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
            return (warnings, set);
        };
        while let Some((name, next)) = com_parse(message, at) {
            at = next;
            match FLAGS
                .iter()
                .find(|row| row.name.as_bytes().eq_ignore_ascii_case(&name))
            {
                Some(row) if row.mid_compile => {
                    self.set(row.flag, on);
                    // FTEF-21.4: `if (compiler_flag[f].enabled == &flag_cpriority)
                    // QCC_PrioritiseOpcodes();`
                    if row.flag == FteFlag::Cpriority {
                        self.prioritise();
                    }
                    set.push(row.flag);
                }
                Some(_) => {
                    warnings.push("Cannot enable/disable keyword/flag via a pragma".to_owned());
                }
                None => warnings.push(format!("keyword/flag {} not recognised", lossy(&name))),
            }
        }
        (warnings, set)
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
/// Returns each opcode it turns on or off, in order (FTEF-25.3), and the warnings.
pub(crate) fn opcode_pragma(message: &[u8]) -> (Vec<(Op, bool)>, Vec<String>) {
    let state =
        com_parse(message, 0).and_then(|(state, at)| match state.to_ascii_lowercase().as_slice() {
            b"enable" | b"on" => Some((true, at)),
            b"disable" | b"off" => Some((false, at)),
            _ => None,
        });
    let Some((on, mut at)) = state else {
        return (Vec::new(), vec!["opcode state not recognised".to_owned()]);
    };
    let mut edits = Vec::new();
    let mut warnings = Vec::new();
    while let Some((name, next)) = com_parse(message, at) {
        at = next;
        match Op::named_in_any_case(&name) {
            Some(op) if op.is_internal() => {
                warnings.push(format!("opcode {} is internal", lossy(&name)));
            }
            Some(op) => edits.push((op, on)),
            None => warnings.push(format!("opcode {} not recognised", lossy(&name))),
        }
    }
    (edits, warnings)
}

/// `#pragma TARGET NAME` with the text `message`: its first token, `QCC_COM_Parse`'s, a name of
/// `targets[]` in any case, with an optional `_` and revision after it (`fte_24`); an unknown name
/// is "Unknown target 'NAME'. Ignored." with FTEQCC's list, and a revision past the format's
/// highest "target revision N is unknown, assuming revision M". The switch to or from a Hexen II
/// format after the first statement, which FTEQCC refuses, is the compiler's to see. Returns the
/// format and revision it sets, if any, and the warnings, which have no name.
pub(crate) fn target_warnings(message: &[u8]) -> (Option<(TargetFormat, u32)>, Vec<String>) {
    let name = com_parse(message, 0)
        .map(|(name, _)| name)
        .unwrap_or_default();
    match target_named(&name) {
        Some((format, revision, warning)) => {
            (Some((format, revision)), warning.into_iter().collect())
        }
        None => (
            None,
            vec![format!(
                "Unknown target '{}'. Ignored.\nValid targets are: ID, HEXEN2, FTE, FTEH2, KK7, DP(patched)",
                lossy(&name)
            )],
        ),
    }
}

/// `QCC_OPCodeSetTargetName(name)`: the format of the row of `targets[]` that `name` names in any
/// case, with an optional `_` and revision after it (`fte_24`), and the warning "target revision N
/// is unknown, assuming revision M" for a revision past the format's highest; `None` when no row
/// is so named. The revision is the one asked for, which `OpcodeTarget::new` holds to the
/// highest.
pub(crate) fn target_named(name: &[u8]) -> Option<(TargetFormat, u32, Option<String>)> {
    let (base, revision) = match name.iter().position(|&b| b == b'_') {
        Some(at) => (
            name.get(..at).unwrap_or_default(),
            name.get(at.saturating_add(1)..),
        ),
        None => (name, None),
    };
    let &(_, format, default) = TARGETS
        .iter()
        .find(|(target, _, _)| target.as_bytes().eq_ignore_ascii_case(base))?;
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
    let warning = (revision > max && revision != u32::MAX)
        .then(|| format!("target revision {revision} is unknown, assuming revision {max}"));
    Some((format, revision, warning))
}

/// `bytes` as text, each byte that is no UTF-8 replaced.
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
