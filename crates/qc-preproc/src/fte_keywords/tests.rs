// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Enabled, Keyword, KeywordEdit, Keywords};
use crate::FlagSettings;
use crate::fte_tables::FteFlag;

// spec FTEF-20.1: the keywords start as `compiler_flag[]` sets them, `kce` on, and each keyword
// flag is its own row
#[test]
fn defaults_are_compiler_flag_s() {
    let keywords = Keywords::default();
    for on in [
        Keyword::For,
        Keyword::Switch,
        Keyword::Const,
        Keyword::Var,
        Keyword::Int,
        Keyword::Unsigned,
        Keyword::Asm,
        Keyword::Optional,
    ] {
        assert!(keywords.has(on), "{on:?}");
    }
    for off in [
        Keyword::Long,
        Keyword::Double,
        Keyword::Thinktime,
        Keyword::Until,
        Keyword::Loop,
        Keyword::State,
        Keyword::Inout,
        Keyword::Using,
        Keyword::Accumulate,
    ] {
        assert!(!keywords.has(off), "{off:?}");
    }
    assert!(keywords.coexist() && !keywords.case_insensitive() && !keywords.hexen2_target());
    assert!(
        Keywords::NONE
            .with(Keyword::Using, true)
            .has(Keyword::Using)
    );
    assert!(!Keywords::NONE.with(Keyword::Asm, true).has(Keyword::Using));
    for keyword in Keyword::ALL {
        assert_eq!(Keyword::of_flag(keyword.flag()), Some(keyword));
        assert_eq!(
            keywords.has(keyword),
            FlagSettings::default().is_on(keyword.flag())
        );
    }
    assert_eq!(Keyword::of_flag(FteFlag::Qccx), None);
    // Each keyword's bit is its place in `ALL`.
    for (at, keyword) in (0_u32..).zip(Keyword::ALL) {
        assert_eq!(keyword.bit(), 1_u64 << at, "{keyword:?}");
    }
}

// spec FTEF-20.2: `__WORD` is the keyword in any case whether or not it is on; a bare word only
// when it is on, exactly, or in any case under `caseinsens`
#[test]
fn check_keyword_matches_as_fteqcc_does() {
    let keywords = Keywords::default();
    let by = Enabled::By;
    assert!(keywords.checks(by(Keyword::For), "for", "for"));
    assert!(!keywords.checks(by(Keyword::For), "for", "FOR"));
    assert!(keywords.checks(by(Keyword::For), "for", "__FoR"));
    assert!(!keywords.checks(by(Keyword::For), "for", "_for"));
    assert!(!keywords.checks(by(Keyword::Long), "long", "long"));
    assert!(keywords.checks(by(Keyword::Long), "long", "__long"));
    assert!(!keywords.checks(by(Keyword::Long), "long", "__longer"));
    assert!(keywords.checks(Enabled::Always, "return", "return"));
    assert!(!keywords.checks(Enabled::Never, "deprecated", "deprecated"));
    assert!(keywords.checks(Enabled::Never, "deprecated", "__DEPRECATED"));
    let off = keywords.with(Keyword::For, false);
    assert!(!off.checks(by(Keyword::For), "for", "for"));
    assert!(off.checks(by(Keyword::For), "for", "__for"));
    let any_case = keywords.with_case_insensitive(true);
    assert!(any_case.checks(by(Keyword::For), "for", "FOR"));
    assert!(any_case.checks(Enabled::Always, "if", "If"));
    assert!(!any_case.checks(by(Keyword::Long), "long", "LONG"));
    assert!(any_case.names("get", "GET") && !keywords.names("get", "GET"));
    assert!(keywords.names("get", "get"));
}

// spec FTEF-20.4: a definition named exactly as an enabled keyword of `QCC_PR_DummyDef`'s list
// names it, `break` only when it is no function and `float` only off a Hexen II target; with
// `kce` off it turns the keyword off
#[test]
fn definitions_named_as_keywords() {
    let keywords = Keywords::default();
    for (name, keyword) in [
        ("var", Keyword::Var),
        ("for", Keyword::For),
        ("switch", Keyword::Switch),
        ("case", Keyword::Case),
        ("local", Keyword::Local),
        ("default", Keyword::Default),
        ("goto", Keyword::Goto),
        ("break", Keyword::Break),
        ("continue", Keyword::Continue),
        ("string", Keyword::String),
        ("float", Keyword::Float),
        ("entity", Keyword::Entity),
        ("vector", Keyword::Vector),
        ("const", Keyword::Const),
        ("asm", Keyword::Asm),
    ] {
        assert_eq!(
            keywords.named_definition(name, false),
            Some(keyword),
            "{name}"
        );
    }
    assert_eq!(keywords.named_definition("break", true), None);
    assert_eq!(keywords.named_definition("for", true), Some(Keyword::For));
    assert_eq!(keywords.named_definition("thinktime", false), None);
    assert_eq!(keywords.named_definition("state", false), None);
    assert_eq!(
        keywords
            .with(Keyword::Thinktime, true)
            .named_definition("thinktime", false),
        Some(Keyword::Thinktime)
    );
    assert_eq!(
        keywords
            .with(Keyword::State, true)
            .named_definition("state", false),
        Some(Keyword::State)
    );
    assert_eq!(keywords.named_definition("FOR", false), None);
    assert_eq!(keywords.named_definition("__for", false), None);
    assert_eq!(keywords.named_definition("while", false), None);
    assert_eq!(keywords.named_definition("typedef", false), None);
    assert_eq!(
        keywords
            .with_hexen2_target(true)
            .named_definition("float", false),
        None
    );
    assert_eq!(
        keywords
            .with(Keyword::For, false)
            .named_definition("for", false),
        None
    );
    assert!(keywords.after_definition(Keyword::For).has(Keyword::For));
    let lone = keywords.with_coexist(false);
    assert!(!lone.after_definition(Keyword::For).has(Keyword::For));
    assert!(lone.after_definition(Keyword::For).has(Keyword::Switch));
}

// spec FTEF-20.1: a pragma's change sets only what it names, so a keyword a definition turned
// off stays off unless the pragma names it; the first mark holds the whole state
#[test]
fn edits_set_only_what_they_name() {
    let mut settings = FlagSettings::default();
    let (_, set) = settings.apply_pragma(b"disable for kce qccx");
    let edit = KeywordEdit::of_flags(&set, settings.keywords());
    assert!(!edit.is_empty());
    let defined = Keywords::default().with(Keyword::Switch, false);
    let after = edit.apply(defined);
    assert!(!after.has(Keyword::For) && !after.coexist());
    assert!(!after.has(Keyword::Switch));
    assert!(after.has(Keyword::Case));
    assert!(KeywordEdit::of_flags(&[FteFlag::Qccx], settings.keywords()).is_empty());
    // A flag named twice is set once.
    let twice = KeywordEdit::of_flags(&[FteFlag::ForKeyword, FteFlag::ForKeyword], Keywords::NONE);
    assert!(!twice.apply(Keywords::default()).has(Keyword::For));
    let any_case = KeywordEdit::of_flags(&[FteFlag::Caseinsensitive], Keywords::NONE);
    assert!(!any_case.is_empty());
    assert!(
        !any_case
            .apply(Keywords::default().with_case_insensitive(true))
            .case_insensitive()
    );
    assert_eq!(
        KeywordEdit::whole(Keywords::NONE).apply(Keywords::default()),
        Keywords::NONE
    );
    let hexen2 = KeywordEdit::hexen2_target(true).apply(Keywords::default());
    assert!(hexen2.hexen2_target() && hexen2.has(Keyword::For));
    // A keyword the edit turns on that is on stays on.
    let on = KeywordEdit::of_flags(&[FteFlag::ForKeyword], Keywords::default());
    assert!(on.apply(Keywords::default()).has(Keyword::For));
    assert!(
        !KeywordEdit::hexen2_target(false)
            .apply(hexen2)
            .hexen2_target()
    );
    let marks = [(0, Keywords::NONE), (5, Keywords::default())];
    assert_eq!(Keywords::at(&marks, 4), Keywords::NONE);
    assert_eq!(Keywords::at(&marks, 5), Keywords::default());
    assert_eq!(Keywords::at(&[], 9), Keywords::default());
}
