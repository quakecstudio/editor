// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, QCC_PR_CommandLinePrecompilerOptions, QCC_SetDefaultProperties

//! FTEQCC's command line, as far as it sets the warnings and the compiler flags
//! (`docs/spec/dialect-fteqcc.md` FTEF-14.71): the `-W`, `-F` and `-K` options, read as
//! `QCC_PR_CommandLinePrecompilerOptions` reads them, again each time `QCC_SetDefaultProperties`
//! resets the settings. Ported from FTEQCC's behaviour under ADR-0017's clean-port policy.

use crate::fte_pragma::FlagSettings;
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
    /// `QCC_CheckParm ("-Fqccx")` (F313 and F314 ignored) and the command line's `-W` options,
    /// in order; then, after `-Werror`, every warning not ignored is an error (FTEF-14.71).
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

    /// `flags` after the command line's `-F` and `-K` options, in order: `-FNAME` or `-KNAME`
    /// turns on, and `-Fno-NAME` or `-Kno-NAME` off, the first row of `compiler_flag[]` whose
    /// abbreviation is NAME in any case, whether or not it may change mid-compile.
    #[must_use]
    pub fn flags(&self, mut flags: FlagSettings) -> FlagSettings {
        for arg in self.options() {
            let Some(rest) = after(arg, b"-F").or_else(|| after(arg, b"-K")) else {
                continue;
            };
            let (name, on) = match after(rest, b"no-") {
                Some(name) => (name, false),
                None => (rest, true),
            };
            flags.set_named(name, on);
        }
        flags
    }
}

#[cfg(test)]
mod tests {
    use super::CommandLine;
    use crate::fte_pragma::FlagSettings;
    use crate::fte_tables::FteFlag;
    use crate::fte_warning::{FteWarning as W, WarningAction as A, WarningSettings};

    fn action(args: &[&str], warning: W) -> A {
        CommandLine::new(args)
            .warnings(WarningSettings::default())
            .action(warning)
    }

    // spec FTEF-14.71: `-W` options set the warnings in order, in any case, and `-Werror` makes
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

    // spec FTEF-14.71: `-F` and `-K` set the first row of the flag table by its abbreviation,
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
