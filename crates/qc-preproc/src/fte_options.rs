// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, QCC_PR_CommandLinePrecompilerOptions, QCC_SetDefaultProperties

//! FTEQCC's command line, as far as it sets the warnings, the compiler flags and keywords and
//! the target (`docs/spec/dialect-fteqcc.md` FTEF-14.72): the `-W`, `-F`, `-K`, `-std=` and `-T`
//! options, read as `QCC_PR_CommandLinePrecompilerOptions` reads them, again each time
//! `QCC_SetDefaultProperties` resets the settings. Ported from FTEQCC's behaviour under
//! ADR-0017's clean-port policy.

use crate::fte_pragma::{FlagSettings, target_named};
use crate::fte_tables::{FteFlag, TargetFormat};
use crate::fte_warning::{FteWarning, WarningAction, WarningSettings};

/// The options FTEQCC was run with, kept to be read again with each reset of the settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CommandLine {
    args: Vec<Vec<u8>>,
}

/// What one `-W` option does.
enum WarningOption<'a> {
    /// `-Wall`.
    All,
    /// `-Wextra`.
    Extra,
    /// `-Wnone`.
    None,
    /// `-Werror`.
    Error,
    /// `-Wno-mundane`.
    NoMundane,
    /// `-WNAME`, `-Wno-NAME` or `-Werror-NAME`: the setting and the name.
    Set(WarningAction, &'a [u8]),
}

/// `strnicmp(arg, prefix, n)`: whether `arg` starts with `prefix`, in any ASCII case.
fn starts_with_any_case(arg: &[u8], prefix: &[u8]) -> bool {
    arg.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// `arg` past `prefix`, matched in any ASCII case.
fn after<'a>(arg: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    starts_with_any_case(arg, prefix)
        .then(|| arg.get(prefix.len()..))
        .flatten()
}

impl WarningOption<'_> {
    /// What the `-W` option `arg` does; `None` when it is no `-W` option. Its text after `-W`
    /// is compared in any case, `-W` itself too (`strnicmp(myargv[i], "-W", 2)`).
    fn of(arg: &[u8]) -> Option<WarningOption<'_>> {
        let a = after(arg, b"-W")?;
        let named = |word: &[u8]| a.eq_ignore_ascii_case(word);
        Some(if named(b"all") {
            WarningOption::All
        } else if named(b"extra") {
            WarningOption::Extra
        } else if named(b"none") {
            WarningOption::None
        } else if named(b"error") {
            WarningOption::Error
        } else if named(b"no-mundane") {
            WarningOption::NoMundane
        } else if let Some(name) = after(a, b"error-") {
            WarningOption::Set(WarningAction::Error, name)
        } else if let Some(name) = after(a, b"no-") {
            WarningOption::Set(WarningAction::Ignore, name)
        } else {
            WarningOption::Set(WarningAction::Warn, a)
        })
    }
}

/// The warnings `-Wall` leaves ignored however `WARN_GMQCC_SPECIFIC` is set: "these warnings
/// require -Wextra to enable".
const EXTRA_ONLY: [FteWarning; 5] = [
    FteWarning::NotReferencedConst,
    FteWarning::ExtraPrecache,
    FteWarning::FteSpecific,
    FteWarning::MutedDeprecatedVariable,
    FteWarning::DivisionBy0,
];

/// The warnings `-Wall` turns on only while `WARN_GMQCC_SPECIFIC` is not ignored: "these
/// warnings do not get switched on with -Wall when using -std=gmqcc".
const GMQCC_GUARDED: [FteWarning; 9] = [
    FteWarning::ConstantComparison,
    FteWarning::PointlessStatement,
    FteWarning::Overflow,
    FteWarning::StrictTypeMismatch,
    FteWarning::ParamWithNoName,
    FteWarning::IfstringUsed,
    FteWarning::GmqccSpecific,
    FteWarning::SystemCrc,
    FteWarning::SystemCrc2,
];

/// The warnings `-Wno-mundane` ignores, those this profile has of its list.
const MUNDANE: [FteWarning; 14] = [
    FteWarning::SameNameAsGlobal,
    FteWarning::DuplicateDefinition,
    FteWarning::ConstantComparison,
    FteWarning::AssignmentInConditional,
    FteWarning::DeadCode,
    FteWarning::NotReferencedConst,
    FteWarning::NotReferenced,
    FteWarning::PointlessStatement,
    FteWarning::BadPragma,
    FteWarning::IdenticalPrecompiler,
    FteWarning::UndefNotDefined,
    FteWarning::ExtraPrecache,
    FteWarning::NotUtf8,
    FteWarning::SelfNotThis,
];

use FteFlag as F;

/// `keyword_vector = keyword_entity = keyword_float = keyword_string = false;`, "not to be
/// confused with actual types, but rather the absence of the keyword local", of every dialect
/// but `fteqcc`'s that `-std=` sets keywords for.
const CORE_TYPE_WORDS: &[FteFlag] = &[
    F::VectorKeyword,
    F::EntityKeyword,
    F::FloatKeyword,
    F::StringKeyword,
];

/// Groups of flags, each set on or off, in order.
type Groups = &'static [(&'static [FteFlag], bool)];

/// `-std=c` and the other C dialects: "set up for greatest C compatibility".
const STD_C: &[(&[FteFlag], bool)] = &[
    (&[F::AsmKeyword], false),
    (
        &[
            F::BreakKeyword,
            F::ContinueKeyword,
            F::ForKeyword,
            F::GotoKeyword,
            F::ConstKeyword,
            F::ExternKeyword,
            F::StaticKeyword,
            F::AutoKeyword,
            F::SwitchKeyword,
            F::CaseKeyword,
            F::DefaultKeyword,
        ],
        true,
    ),
    (
        &[
            F::AccessorKeyword,
            F::ClassKeyword,
            F::VarKeyword,
            F::InoutKeyword,
            F::OptionalKeyword,
            F::StateKeyword,
            F::InlineKeyword,
            F::NosaveKeyword,
            F::SharedKeyword,
            F::NorefKeyword,
            F::UnusedKeyword,
            F::UsedKeyword,
            F::LocalKeyword,
            F::NonstaticKeyword,
            F::IgnoreKeyword,
            F::StripKeyword,
        ],
        false,
    ),
    (CORE_TYPE_WORDS, false),
    (&[F::IntegerKeyword, F::EnumflagsKeyword], false),
    (
        &[
            F::FloatKeyword,
            F::IntKeyword,
            F::TypedefKeyword,
            F::StructKeyword,
            F::UnionKeyword,
            F::EnumKeyword,
            F::DoubleKeyword,
            F::LongKeyword,
            F::ShortKeyword,
            F::CharKeyword,
            F::SignedKeyword,
            F::UnsignedKeyword,
            F::RegisterKeyword,
            F::VolatileKeyword,
        ],
        true,
    ),
    (
        &[F::ThinktimeKeyword, F::UntilKeyword, F::LoopKeyword],
        false,
    ),
    (
        &[
            F::Ilp32,
            F::LogicopsOpt,
            F::Assumevar,
            F::PrSubscopedlocals,
            F::Cpriority,
            F::AssumeInteger,
            F::AssumeDouble,
        ],
        true,
    ),
    (&[F::Qcfuncs, F::Macroinstrings, F::Boundchecks], false),
    (&[F::Iffloat], true),
];

/// What `-std=c++` sets after the C dialects': `keyword_class = keyword_inline = true;`.
const CPP_AFTER_C: &[(&[FteFlag], bool)] = &[(&[F::ClassKeyword, F::InlineKeyword], true)];

/// `keyword_int = keyword_integer = keyword_typedef = keyword_struct = keyword_union =
/// keyword_enum = keyword_enumflags = false;`, of `qcc` and `hcc`.
const QCC_TYPE_WORDS: &[FteFlag] = &[
    F::IntKeyword,
    F::IntegerKeyword,
    F::TypedefKeyword,
    F::StructKeyword,
    F::UnionKeyword,
    F::EnumKeyword,
    F::EnumflagsKeyword,
];

/// The qualifiers `qcc` and `hcc` turn off: `keyword_const = keyword_var = keyword_inout = ...
/// = keyword_strip = false;`.
const QCC_QUALIFIERS: &[FteFlag] = &[
    F::ConstKeyword,
    F::VarKeyword,
    F::InoutKeyword,
    F::OptionalKeyword,
    F::StateKeyword,
    F::InlineKeyword,
    F::NosaveKeyword,
    F::ExternKeyword,
    F::SharedKeyword,
    F::NorefKeyword,
    F::UnusedKeyword,
    F::UsedKeyword,
    F::StaticKeyword,
    F::NonstaticKeyword,
    F::IgnoreKeyword,
    F::StripKeyword,
];

/// `-std=qcc` and `-std=id`.
const STD_QCC: &[(&[FteFlag], bool)] = &[
    (&[F::Ifvector, F::Vectorlogic], false),
    (
        &[
            F::AsmKeyword,
            F::BreakKeyword,
            F::ContinueKeyword,
            F::ForKeyword,
            F::GotoKeyword,
        ],
        false,
    ),
    (QCC_QUALIFIERS, false),
    (
        &[
            F::SwitchKeyword,
            F::CaseKeyword,
            F::DefaultKeyword,
            F::AccessorKeyword,
            F::ClassKeyword,
            F::ConstKeyword,
        ],
        false,
    ),
    (CORE_TYPE_WORDS, false),
    (QCC_TYPE_WORDS, false),
    (
        &[F::ThinktimeKeyword, F::UntilKeyword, F::LoopKeyword],
        false,
    ),
    (&[F::WrapKeyword, F::WeakKeyword], false),
];

/// `-std=hcc` and `-std=hexenc`.
const STD_HCC: &[(&[FteFlag], bool)] = &[
    (&[F::Ifvector, F::Vectorlogic, F::Macroinstrings], false),
    (
        &[
            F::AsmKeyword,
            F::ContinueKeyword,
            F::ForKeyword,
            F::GotoKeyword,
        ],
        false,
    ),
    (QCC_QUALIFIERS, false),
    (
        &[F::AccessorKeyword, F::ClassKeyword, F::ConstKeyword],
        false,
    ),
    (CORE_TYPE_WORDS, false),
    (QCC_TYPE_WORDS, false),
    (&[F::WrapKeyword, F::WeakKeyword], false),
    (
        &[F::ThinktimeKeyword, F::UntilKeyword, F::LoopKeyword],
        true,
    ),
    (
        &[
            F::SwitchKeyword,
            F::CaseKeyword,
            F::DefaultKeyword,
            F::BreakKeyword,
        ],
        true,
    ),
];

/// `-std=gmqcc`.
const STD_GMQCC: &[(&[FteFlag], bool)] = &[
    (&[F::Ifvector, F::Vectorlogic], true),
    (
        &[
            F::Dblstarexp,
            F::Attributes,
            F::Assumevar,
            F::PrSubscopedlocals,
            F::Cpriority,
            F::Allowuninit,
        ],
        true,
    ),
    (&[F::Boundchecks, F::Macroinstrings], false),
    (
        &[F::Reciprocalmaths, F::Undefwordsize, F::LogicopsOpt],
        true,
    ),
    (&[F::AsmKeyword], false),
    (
        &[
            F::InoutKeyword,
            F::OptionalKeyword,
            F::StateKeyword,
            F::InlineKeyword,
            F::NosaveKeyword,
            F::ExternKeyword,
            F::SharedKeyword,
            F::UnusedKeyword,
            F::UsedKeyword,
            F::NonstaticKeyword,
            F::IgnoreKeyword,
            F::StripKeyword,
        ],
        false,
    ),
    (&[F::AccessorKeyword, F::ClassKeyword], false),
    (CORE_TYPE_WORDS, false),
    (
        &[
            F::IntKeyword,
            F::IntegerKeyword,
            F::StructKeyword,
            F::UnionKeyword,
            F::EnumKeyword,
            F::EnumflagsKeyword,
        ],
        false,
    ),
    (
        &[F::ThinktimeKeyword, F::UntilKeyword, F::LoopKeyword],
        false,
    ),
    (&[F::WrapKeyword, F::WeakKeyword], false),
    (&[F::EnumKeyword], true),
    (
        &[
            F::BreakKeyword,
            F::ContinueKeyword,
            F::ForKeyword,
            F::GotoKeyword,
        ],
        true,
    ),
    (&[F::TypedefKeyword], true),
    (&[F::SwitchKeyword, F::CaseKeyword, F::DefaultKeyword], true),
    (
        &[
            F::ConstKeyword,
            F::VarKeyword,
            F::StaticKeyword,
            F::NorefKeyword,
        ],
        true,
    ),
];

/// A dialect `-std=NAME` names (FTEF-14.72).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Std {
    /// `C`, `c89`, `c90`, `c99`, `c11` or `c17`, in any case.
    C,
    /// `c++`, in any case.
    Cpp,
    /// `qccx`.
    Qccx,
    /// `preqcc`.
    Preqcc,
    /// `reacc`.
    Reacc,
    /// `frikqcc`.
    Frikqcc,
    /// `fteqcc`, the default.
    Fteqcc,
    /// `qcc` or `id`.
    Qcc,
    /// `hcc` or `hexenc`.
    Hcc,
    /// `gmqcc`.
    Gmqcc,
    /// Any other, "Unrecognised std parameter", after the flags were set to their defaults.
    Unknown,
}

impl Std {
    /// The dialect `-std=NAME` names: the C ones in any case (`stricmp`), the rest exactly
    /// (`strcmp`). `c23` is no C one: the test that would set `__STDC_VERSION__` for it stands
    /// inside the branch it does not enter.
    fn of(name: &[u8]) -> Self {
        let any_case = |word: &[u8]| name.eq_ignore_ascii_case(word);
        if any_case(b"c++") {
            return Self::Cpp;
        }
        if [&b"C"[..], b"c89", b"c90", b"c99", b"c11", b"c17"]
            .iter()
            .any(|word| any_case(word))
        {
            return Self::C;
        }
        match name {
            b"qccx" => Self::Qccx,
            b"preqcc" => Self::Preqcc,
            b"reacc" => Self::Reacc,
            b"frikqcc" => Self::Frikqcc,
            b"fteqcc" => Self::Fteqcc,
            b"qcc" | b"id" => Self::Qcc,
            b"hcc" | b"hexenc" => Self::Hcc,
            b"gmqcc" => Self::Gmqcc,
            _ => Self::Unknown,
        }
    }

    /// The flags and keywords it sets, in FTEQCC's order, after every flag was set to its
    /// default (`*compiler_flag[p].enabled = !!(compiler_flag[p].flags & FLAG_ASDEFAULT)`).
    fn flags(self) -> impl Iterator<Item = (FteFlag, bool)> {
        let (groups, after): (Groups, Groups) = match self {
            Self::C => (STD_C, &[]),
            Self::Cpp => (STD_C, CPP_AFTER_C),
            Self::Qccx => (&[(&[FteFlag::Qccx], true)], &[]),
            Self::Preqcc => (
                &[(&[FteFlag::Hashonly, FteFlag::Macroinstrings], true)],
                &[],
            ),
            Self::Reacc => (
                &[(&[FteFlag::Acc], true), (&[FteFlag::Macroinstrings], false)],
                &[],
            ),
            Self::Frikqcc => (
                &[
                    (&[FteFlag::StateKeyword], true),
                    (&[FteFlag::Macroinstrings], false),
                ],
                &[],
            ),
            Self::Qcc => (STD_QCC, &[]),
            Self::Hcc => (STD_HCC, &[]),
            Self::Gmqcc => (STD_GMQCC, &[]),
            Self::Fteqcc | Self::Unknown => (&[], &[]),
        };
        groups
            .iter()
            .chain(after)
            .flat_map(|&(flags, on)| flags.iter().map(move |&flag| (flag, on)))
    }

    /// The warnings it sets, in FTEQCC's order.
    fn warnings(self) -> Vec<(FteWarning, WarningAction)> {
        use FteWarning as W;
        use WarningAction::{Error, Ignore, Warn};
        match self {
            Self::C | Self::Cpp => vec![
                (W::Uninitialized, Warn),
                (W::TooManyParams, Error),
                (W::TooFewParams, Error),
                (W::AssignmentToConstant, Error),
                (W::SameNameAsGlobal, Ignore),
                (W::Octal, Ignore),
                (W::PointlessStatement, Ignore),
            ],
            Self::Qccx => vec![(W::Denormal, Ignore), (W::LaxCast, Ignore)],
            Self::Qcc => vec![(W::ParamWithNoName, Error)],
            Self::Gmqcc => vec![
                (W::NotReferencedConst, Ignore),
                (W::ConstantComparison, Ignore),
                (W::PointlessStatement, Ignore),
                (W::Overflow, Ignore),
                (W::StrictTypeMismatch, Ignore),
                (W::ParamWithNoName, Ignore),
                (W::IfstringUsed, Ignore),
                (W::Uninitialized, Ignore),
                (W::GmqccSpecific, Ignore),
                (W::Octal, Ignore),
                (W::SystemCrc, Ignore),
                (W::SystemCrc2, Ignore),
                (W::ArgumentCheck, Ignore),
                (W::AssignmentToConstant, Error),
            ],
            Self::Preqcc
            | Self::Reacc
            | Self::Frikqcc
            | Self::Fteqcc
            | Self::Hcc
            | Self::Unknown => Vec::new(),
        }
    }
}

/// The flags a `-FNAME` that names no row of `compiler_flag[]` sets, FTEQCC's "compat flags":
/// `short-logic` the `lo` optimisation, `correct-logic` `ifvector` and `vectorlogic`, and
/// `false-empty-strings` `ifstring`; the others set no flag this profile keeps.
fn compat_flags(name: &[u8]) -> &'static [FteFlag] {
    if name.eq_ignore_ascii_case(b"short-logic") {
        &[FteFlag::LogicopsOpt]
    } else if name.eq_ignore_ascii_case(b"correct-logic") {
        &[FteFlag::Ifvector, FteFlag::Vectorlogic]
    } else if name.eq_ignore_ascii_case(b"false-empty-strings") {
        &[FteFlag::Ifstring]
    } else {
        &[]
    }
}

/// For `-Farithmetic-exceptions`, `true`, and for `-Fno-arithmetic-exceptions`, `false`.
fn arithmetic_exceptions(arg: &[u8]) -> Option<bool> {
    let rest = after(arg, b"-F")?;
    let (name, on) = match after(rest, b"no-") {
        Some(name) => (name, false),
        None => (rest, true),
    };
    name.eq_ignore_ascii_case(b"arithmetic-exceptions")
        .then_some(on)
}

/// The options that take the next argument as their value (`if (++i == myargc)`, `i++;`): it
/// is a file or a number, not an option.
const TAKES_VALUE: [&[u8]; 12] = [
    b"-srcfile",
    b"-src",
    b"-o",
    b"-max_regs",
    b"-max_strings",
    b"-max_globals",
    b"-max_fields",
    b"-max_statements",
    b"-max_functions",
    b"-max_types",
    b"-max_temps",
    b"-max_macros",
];

impl CommandLine {
    /// The arguments read as options, in order: each but the value an option before it takes,
    /// and a bare `-I`'s directory (`if (!*name && i+1<myargc) name = myargv[++i];`).
    fn options(&self) -> impl Iterator<Item = &[u8]> {
        let mut value_next = false;
        self.args.iter().filter_map(move |arg| {
            if value_next {
                value_next = false;
                return None;
            }
            value_next = TAKES_VALUE.contains(&arg.as_slice()) || arg.as_slice() == b"-I";
            Some(arg.as_slice())
        })
    }

    /// The command line of the arguments `args`, in order. Arguments that are no `-W`, `-F`
    /// or `-K` option are kept and change nothing here.
    #[must_use]
    pub fn new<A: AsRef<[u8]>>(args: &[A]) -> Self {
        Self {
            args: args.iter().map(|a| a.as_ref().to_vec()).collect(),
        }
    }

    /// `QCC_SetDefaultProperties`' settings of the warnings, `defaults`, after its
    /// `QCC_CheckParm ("-Fqccx")` (F313 and F314 ignored) and the command line's `-W` and `-std=`
    /// options, in order; then, after `-Werror`, every warning not ignored is an error
    /// (FTEF-14.72).
    #[must_use]
    pub fn warnings(&self, defaults: WarningSettings) -> WarningSettings {
        let mut settings = defaults;
        // `QCC_CheckParm` compares with `QC_strcasecmp`.
        if self.options().any(|a| a.eq_ignore_ascii_case(b"-Fqccx")) {
            settings.set(FteWarning::Denormal, WarningAction::Ignore);
            settings.set(FteWarning::LaxCast, WarningAction::Ignore);
        }
        let mut werror = false;
        for arg in self.options() {
            if let Some(name) = after(arg, b"-std=") {
                for (warning, action) in Std::of(name).warnings() {
                    settings.set(warning, action);
                }
                continue;
            }
            // `-Farithmetic-exceptions`, a compatibility flag no row of `compiler_flag[]` names:
            // `qccwarningaction[WARN_DIVISIONBY0] = state?WA_ERROR:WA_IGNORE;`.
            if let Some(on) = arithmetic_exceptions(arg) {
                let action = if on {
                    WarningAction::Error
                } else {
                    WarningAction::Ignore
                };
                settings.set(FteWarning::DivisionBy0, action);
                continue;
            }
            let Some(option) = WarningOption::of(arg) else {
                continue;
            };
            match option {
                WarningOption::All => {
                    let gmqcc = settings.action(FteWarning::GmqccSpecific) != WarningAction::Ignore;
                    settings.turn_on_ignored(|w| {
                        if EXTRA_ONLY.contains(&w) {
                            false
                        } else if GMQCC_GUARDED.contains(&w) {
                            gmqcc
                        } else {
                            true
                        }
                    });
                }
                WarningOption::Extra => settings.turn_on_ignored(|_| true),
                WarningOption::None => settings.set_all(|_| WarningAction::Ignore),
                WarningOption::Error => werror = true,
                WarningOption::NoMundane => {
                    for warning in MUNDANE {
                        settings.set(warning, WarningAction::Ignore);
                    }
                }
                WarningOption::Set(action, name) => {
                    if let Some(warning) = FteWarning::for_name(name) {
                        settings.set(warning, action);
                    }
                }
            }
        }
        if werror {
            settings.set_all(|action| match action {
                WarningAction::Ignore => WarningAction::Ignore,
                WarningAction::Warn | WarningAction::Error => WarningAction::Error,
            });
        }
        settings
    }

    /// `flags` after the command line's `-K`, `-std=` and `-F` options, in order (FTEF-14.72):
    /// `-FNAME` or `-KNAME` turns on, and `-Fno-NAME` or `-Kno-NAME` off, the first
    /// row of `compiler_flag[]` whose abbreviation is NAME in any case, whether or not it may
    /// change mid-compile, and a `-F` NAME no row has may be a compatibility flag; `-std=NAME`
    /// sets every flag to its default and then the dialect's. Then, as `QCC_SetDefaultProperties`
    /// does after the options, a Hexen II target turns on `thinktime`, `until` and `loop`
    /// (FTEF-20.3).
    #[must_use]
    pub fn flags(&self, mut flags: FlagSettings) -> FlagSettings {
        for arg in self.options() {
            if let Some(name) = after(arg, b"-std=") {
                flags = FlagSettings::default();
                for (flag, on) in Std::of(name).flags() {
                    flags.set(flag, on);
                }
                continue;
            }
            let keyword = after(arg, b"-K");
            let Some(rest) = keyword.or_else(|| after(arg, b"-F")) else {
                continue;
            };
            let (name, on) = match after(rest, b"no-") {
                Some(name) => (name, false),
                None => (rest, true),
            };
            if !flags.set_named(name, on) && keyword.is_none() {
                for &flag in compat_flags(name) {
                    flags.set(flag, on);
                }
            }
        }
        if self.target().is_hexen2() {
            for flag in [
                FteFlag::ThinktimeKeyword,
                FteFlag::UntilKeyword,
                FteFlag::LoopKeyword,
            ] {
                flags.set(flag, true);
            }
        }
        flags
    }

    /// The target the options set (FTEF-14.72): the standard one, as the module is compiled as
    /// `fteqcc -Tid` with these options after it (PRJ-3.4), so the `-h2`, `-fte`, `-fteh2` and
    /// `-dp` that `QCC_SetDefaultProperties` reads first are set again by the `-Tid`; then each
    /// `-TNAME` that names one, in order, `-Tparse` none.
    pub(crate) fn target(&self) -> TargetFormat {
        let mut target = TargetFormat::Standard;
        for arg in self.options() {
            // `!strncmp(myargv[i], "-T", 2)`, in this case only.
            let Some(name) = arg.strip_prefix(b"-T") else {
                continue;
            };
            if name != b"parse"
                && let Some((format, _)) = target_named(name)
            {
                target = format;
            }
        }
        target
    }
}

#[cfg(test)]
mod tests {
    use super::{CommandLine, Std};
    use crate::fte_pragma::FlagSettings;
    use crate::fte_tables::{FteFlag, TargetFormat};
    use crate::fte_warning::{FteWarning as W, WarningAction as A, WarningSettings};

    fn action(args: &[&str], warning: W) -> A {
        CommandLine::new(args)
            .warnings(WarningSettings::default())
            .action(warning)
    }

    // spec FTEF-14.72: `-W` options set the warnings in order, in any case, and `-Werror` makes
    // every warning not ignored an error after them all
    #[test]
    fn warning_options() {
        assert_eq!(action(&[], W::SameNameAsGlobal), A::Warn);
        assert_eq!(action(&["-Wno-F306"], W::SameNameAsGlobal), A::Ignore);
        assert_eq!(action(&["-wNO-f306"], W::SameNameAsGlobal), A::Ignore);
        assert_eq!(action(&["-Werror-F306"], W::SameNameAsGlobal), A::Error);
        assert_eq!(
            action(&["-Werror-F306", "-WF306"], W::SameNameAsGlobal),
            A::Warn
        );
        assert_eq!(action(&["-Werror"], W::SameNameAsGlobal), A::Error);
        assert_eq!(action(&["-Werror"], W::FteSpecific), A::Ignore);
        assert_eq!(
            action(&["-Werror", "-Wno-F306"], W::SameNameAsGlobal),
            A::Ignore
        );
        assert_eq!(action(&["-Wnone"], W::EvilPreprocessor), A::Ignore);
        assert_eq!(action(&["-Wextra"], W::FteSpecific), A::Warn);
        assert_eq!(action(&["-Wall"], W::FteSpecific), A::Ignore);
        assert_eq!(action(&["-Wall"], W::NotUtf8), A::Warn);
        assert_eq!(action(&["-Wall"], W::IfstringUsed), A::Warn);
        assert_eq!(action(&["-Wno-F321", "-Wall"], W::IfstringUsed), A::Ignore);
        assert_eq!(action(&["-Wno-mundane"], W::DuplicateDefinition), A::Ignore);
        assert_eq!(action(&["-Wno-mundane"], W::SameNameAsGlobal), A::Ignore);
        assert_eq!(action(&["-Wno-mundane"], W::LaxCast), A::Warn);
        assert_eq!(action(&["-Wbogus"], W::SameNameAsGlobal), A::Warn);
        assert_eq!(action(&["-Fqccx"], W::LaxCast), A::Ignore);
        assert_eq!(action(&["-Fqccx"], W::Denormal), A::Ignore);
        assert_eq!(action(&["-fQCCX"], W::LaxCast), A::Ignore);
        // The value an option takes is no option.
        assert_eq!(action(&["-o", "-Wno-F306"], W::SameNameAsGlobal), A::Warn);
        assert_eq!(action(&["-I", "-Wno-F306"], W::SameNameAsGlobal), A::Warn);
        assert_eq!(
            action(&["-max_temps", "-Wno-F306"], W::SameNameAsGlobal),
            A::Warn
        );
        assert_eq!(
            action(&["-Isrc", "-Wno-F306"], W::SameNameAsGlobal),
            A::Ignore
        );
        assert_eq!(
            action(&["-Farithmetic-exceptions"], W::DivisionBy0),
            A::Error
        );
        assert_eq!(
            action(
                &["-Farithmetic-exceptions", "-Fno-arithmetic-exceptions"],
                W::DivisionBy0
            ),
            A::Ignore
        );
        assert_eq!(action(&["-Werror"], W::UndesirableConvention), A::Error);
        assert_eq!(action(&["-Wnone"], W::KeywordDisabled), A::Ignore);
    }

    // spec FTEF-14.72: `-std=` sets every flag to its default and then its dialect's, in order
    // with `-K` and `-F`; the C names in any case, the others exactly; an unknown one only resets
    #[test]
    fn std_options() {
        let on = |args: &[&str], flag| {
            CommandLine::new(args)
                .flags(FlagSettings::default())
                .is_on(flag)
        };
        assert_eq!(Std::of(b"fteqcc"), Std::Fteqcc);
        assert_eq!(Std::of(b"FTEQCC"), Std::Unknown);
        assert!(on(&["-std=c"], FteFlag::LongKeyword));
        assert!(on(&["-std=C"], FteFlag::Ilp32));
        assert!(on(&["-STD=c99"], FteFlag::AssumeInteger));
        assert!(!on(&["-std=c11"], FteFlag::Qcfuncs));
        assert!(!on(&["-std=c17"], FteFlag::LocalKeyword));
        assert!(on(&["-std=c89"], FteFlag::FloatKeyword));
        assert!(!on(&["-std=c90"], FteFlag::IntegerKeyword));
        assert!(!on(&["-std=c"], FteFlag::ClassKeyword));
        assert!(on(&["-std=c++"], FteFlag::ClassKeyword));
        assert!(on(&["-std=C++"], FteFlag::InlineKeyword));
        assert!(!on(&["-std=c23"], FteFlag::LongKeyword));
        assert!(!on(&["-std=qcc"], FteFlag::ForKeyword));
        assert!(!on(&["-std=id"], FteFlag::IntKeyword));
        assert!(on(&["-std=qcc"], FteFlag::LocalKeyword));
        assert!(on(&["-std=Qcc"], FteFlag::ForKeyword));
        assert!(!on(&["-std=qcc"], FteFlag::Ifvector));
        assert!(on(&["-std=hcc"], FteFlag::LoopKeyword));
        assert!(on(&["-std=hexenc"], FteFlag::BreakKeyword));
        assert!(!on(&["-std=hcc"], FteFlag::ForKeyword));
        assert!(!on(&["-std=hcc"], FteFlag::Macroinstrings));
        assert!(on(&["-std=gmqcc"], FteFlag::EnumKeyword));
        assert!(!on(&["-std=gmqcc"], FteFlag::StructKeyword));
        assert!(on(&["-std=gmqcc"], FteFlag::Dblstarexp));
        assert!(on(&["-std=frikqcc"], FteFlag::StateKeyword));
        assert!(on(&["-std=preqcc"], FteFlag::Hashonly));
        assert!(on(&["-std=reacc"], FteFlag::Acc));
        assert!(on(&["-std=qccx"], FteFlag::Qccx));
        assert!(!on(&["-std=fteqcc"], FteFlag::LongKeyword));
        assert!(!on(&["-Klong", "-std=qcc"], FteFlag::LongKeyword));
        assert!(on(&["-std=qcc", "-Kfor"], FteFlag::ForKeyword));
        assert!(!on(&["-Fqccx", "-std=bogus"], FteFlag::Qccx));
        let warnings = |args: &[&str], warning| {
            CommandLine::new(args)
                .warnings(WarningSettings::default())
                .action(warning)
        };
        assert_eq!(warnings(&["-std=qcc"], W::ParamWithNoName), A::Error);
        assert_eq!(warnings(&["-std=c"], W::SameNameAsGlobal), A::Ignore);
        assert_eq!(warnings(&["-std=c"], W::TooFewParams), A::Error);
        assert_eq!(warnings(&["-std=gmqcc"], W::AssignmentToConstant), A::Error);
        assert_eq!(warnings(&["-std=gmqcc"], W::PointlessStatement), A::Ignore);
        assert_eq!(warnings(&["-std=qccx"], W::LaxCast), A::Ignore);
        assert_eq!(
            warnings(&["-std=qcc", "-WF320"], W::ParamWithNoName),
            A::Warn
        );
        assert_eq!(warnings(&["-std=fteqcc"], W::SameNameAsGlobal), A::Warn);
    }

    // spec FTEF-14.72: a `-F` NAME no row has may be a compatibility flag, which `-K` is not; the
    // target is the standard one and then each `-T` that names one
    #[test]
    fn compatibility_flags_and_targets() {
        let on = |args: &[&str], flag| {
            CommandLine::new(args)
                .flags(FlagSettings::default())
                .is_on(flag)
        };
        assert!(on(&["-Fshort-logic"], FteFlag::LogicopsOpt));
        assert!(!on(&["-Kshort-logic"], FteFlag::LogicopsOpt));
        assert!(on(&["-FCORRECT-LOGIC"], FteFlag::Vectorlogic));
        assert!(!on(&["-Fno-correct-logic"], FteFlag::Ifvector));
        assert!(on(&["-Ffalse-empty-strings"], FteFlag::Ifstring));
        let target = |args: &[&str]| CommandLine::new(args).target();
        assert_eq!(target(&[]), TargetFormat::Standard);
        assert_eq!(target(&["-Thexen2"]), TargetFormat::Hexen2);
        assert_eq!(target(&["-TH2"]), TargetFormat::Hexen2);
        assert_eq!(target(&["-Tfte_5768"]), TargetFormat::Fte);
        assert_eq!(target(&["-Thexen2", "-Tid"]), TargetFormat::Standard);
        assert_eq!(target(&["-Thexen2", "-Tparse"]), TargetFormat::Hexen2);
        assert_eq!(target(&["-Thexen2", "-Tbogus"]), TargetFormat::Hexen2);
        assert_eq!(target(&["-h2"]), TargetFormat::Standard);
        assert_eq!(target(&["-thexen2"]), TargetFormat::Standard);
        assert_eq!(target(&["-o", "-Thexen2"]), TargetFormat::Standard);
    }

    // spec FTEF-14.72: `-F` and `-K` set the first row of the flag table by its abbreviation,
    // whether or not it may change mid-compile
    #[test]
    fn flag_options() {
        let on = |args: &[&str], flag| {
            CommandLine::new(args)
                .flags(FlagSettings::default())
                .is_on(flag)
        };
        assert!(!on(&[], FteFlag::Caseinsensitive));
        assert!(on(&["-Fcaseinsens"], FteFlag::Caseinsensitive));
        assert!(on(&["-fCASEINSENS"], FteFlag::Caseinsensitive));
        assert!(!on(
            &["-Fcaseinsens", "-Fno-caseinsens"],
            FteFlag::Caseinsensitive
        ));
        assert!(on(&["-Kcaseinsens"], FteFlag::Caseinsensitive));
        assert!(!on(&["-Kno-qcfuncs"], FteFlag::Qcfuncs));
        assert!(on(&["-Fassumeint"], FteFlag::AssumeInteger));
        assert!(!on(&["-Fbogus"], FteFlag::AssumeInteger));
    }
}
