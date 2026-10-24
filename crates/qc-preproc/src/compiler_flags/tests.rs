// SPDX-License-Identifier: GPL-3.0-or-later

use qc_lexer::LexFlag;

use super::{CompilerFlags, FlagEdit, Priorities};
use crate::fte_tables::FteFlag;
use crate::{CommandLine, FlagSettings};

// spec FTEF-21.1: the flags kept are the lexer's eight, `cpriority`, `assumevar`, `subscope` and
// `attributes`, with the casts' (FTEF-22); any other is read as off and set to nothing
// spec FTEF-21.18: a flag only the compiler's output reads is not kept
#[test]
fn the_kept_flags() {
    let all = [
        FteFlag::Qcfuncs,
        FteFlag::Qccx,
        FteFlag::Utf8strings,
        FteFlag::Ilp32,
        FteFlag::Dblstarexp,
        FteFlag::AssumeInteger,
        FteFlag::Macroinstrings,
        FteFlag::AssumeDouble,
        FteFlag::Cpriority,
        FteFlag::Assumevar,
        FteFlag::PrSubscopedlocals,
        FteFlag::Attributes,
        FteFlag::Laxcasts,
        FteFlag::Typeexplicit,
        FteFlag::Undefwordsize,
    ];
    for flag in all {
        let on = CompilerFlags::default().with(flag, true);
        assert!(on.has(flag), "{flag:?}");
        assert!(!on.with(flag, false).has(flag), "{flag:?}");
        for other in all.iter().filter(|&&other| other != flag) {
            assert!(
                !on.has(*other) || *other == FteFlag::Qcfuncs,
                "{flag:?} {other:?}"
            );
        }
    }
    for flag in [FteFlag::Iffloat, FteFlag::Boundchecks] {
        let on = CompilerFlags::default().with(flag, true);
        assert_eq!(on, CompilerFlags::default());
        assert!(!on.has(flag));
    }
    // The casts' flags are none of the lexer's.
    let casts = CompilerFlags::default()
        .with(FteFlag::Laxcasts, true)
        .with(FteFlag::Typeexplicit, true)
        .with(FteFlag::Undefwordsize, true);
    assert_eq!(casts.lex(), CompilerFlags::default().lex());
    let lex = CompilerFlags::default()
        .with(FteFlag::Dblstarexp, true)
        .with(FteFlag::Assumevar, true)
        .lex();
    assert!(lex.has(LexFlag::Ssp) && lex.has(LexFlag::QcFuncs));
    assert!(!lex.has(LexFlag::AssumeInt));
}

// spec FTEF-21.1: a pragma's edit sets only the flags it names, the first edit the whole state
#[test]
fn edits_set_what_they_name() {
    let now = CompilerFlags::default()
        .with(FteFlag::Qccx, true)
        .with(FteFlag::Utf8strings, true);
    let edit = FlagEdit::of_flags(&[FteFlag::Qccx], now);
    let before = CompilerFlags::default().with(FteFlag::Qcfuncs, false);
    let after = edit.apply(before);
    assert!(after.has(FteFlag::Qccx));
    assert!(!after.has(FteFlag::Utf8strings));
    assert!(!after.has(FteFlag::Qcfuncs));
    assert!(FlagEdit::of_flags(&[FteFlag::Iffloat], now).is_empty());
    assert!(!edit.is_empty());
    assert_eq!(FlagEdit::whole(now).apply(before), now);
}

// spec FTEF-21.4: a pragma that names `cpriority` sets the priorities by it, at once, and any
// other leaves them; a flag set otherwise does not
#[test]
fn pragmas_naming_cpriority_set_the_priorities() {
    let mut settings = FlagSettings::default();
    assert_eq!(settings.priorities(), Priorities::Qc);
    let _ = settings.apply_pragma(b"enable cpriority");
    assert_eq!(settings.priorities(), Priorities::C);
    assert_eq!(settings.compiler_flags().priorities(), Priorities::C);
    let _ = settings.apply_pragma(b"enable qcfuncs");
    assert_eq!(settings.priorities(), Priorities::C);
    settings.set(FteFlag::Cpriority, false);
    assert_eq!(settings.priorities(), Priorities::C);
    let _ = settings.apply_pragma(b"disable cpriority");
    assert_eq!(settings.priorities(), Priorities::Qc);
    let edit = FlagEdit::of_flags(&[FteFlag::Cpriority], settings.compiler_flags());
    let c = CompilerFlags::default().with_priorities(Priorities::C);
    assert_eq!(edit.apply(c).priorities(), Priorities::Qc);
    let other = FlagEdit::of_flags(&[FteFlag::Qcfuncs], settings.compiler_flags());
    assert_eq!(other.apply(c).priorities(), Priorities::C);
}

// spec FTEF-21.4: a compile starts with the priorities `cpriority` has once the command line is
// read
#[test]
fn the_command_line_sets_the_first_priorities() {
    let read = |options: &[&str]| {
        let line = CommandLine::new(options);
        let mut flags = line.flags(FlagSettings::default());
        flags.prioritise();
        flags.priorities()
    };
    assert_eq!(read(&[]), Priorities::Qc);
    assert_eq!(read(&["-Fcpriority"]), Priorities::C);
    assert_eq!(read(&["-std=c"]), Priorities::C);
    assert_eq!(read(&["-std=c", "-Fno-cpriority"]), Priorities::Qc);
    let kept = FlagSettings::default().with_priorities(Priorities::C);
    assert_eq!(kept.priorities(), Priorities::C);
    assert!(!kept.is_on(FteFlag::Cpriority));
}
