// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use qc_progs::{Function, ProgsFile, write};
use qc_syntax::{Code, Dialect};
use qc_types::Slot;

use super::super::testing::{fake_oracles, fresh_dir, root};
use super::{
    Case, FIXTURE, Outcome, Tally, bound, cases, check, compare, expected_difference, oracle,
    profile, read_fixture,
};
use crate::exec::{Fake, Reply};

fn fte(source: &str) -> Case {
    Case {
        dialect: Dialect::Fteqcc,
        source: source.to_owned(),
    }
}

fn vanilla(source: &str) -> Case {
    Case {
        dialect: Dialect::Vanilla,
        source: source.to_owned(),
    }
}

fn bound_as(number: i32, name: &str) -> Outcome {
    Outcome::Bound {
        number,
        name: name.to_owned(),
    }
}

// spec FTE-23.14: every case of the cross product is recorded, and the profile's outcome is the
// pinned FTEQCC's or id's qcc's, or differs only as QC-27.9, FTE-23.11 and FTEF-13.8 say
#[test]
fn the_profile_agrees_with_the_recorded_oracles() {
    let recorded = read_fixture(&root()).unwrap();
    let cases = cases();
    let keys: Vec<String> = cases.iter().map(Case::key).collect();
    assert_eq!(keys.len(), recorded.len());
    assert!(keys.iter().all(|key| recorded.contains_key(key)));
    let tally = compare(&cases, &recorded);
    assert_eq!(tally.differ, 0, "{tally}");
    assert_eq!(
        tally.expected.keys().copied().collect::<Vec<_>>(),
        ["FTE-23.11", "FTEF-13.8", "QC-27.9"]
    );
    assert!(tally.agree > 6000, "{tally}");
}

// spec FTE-23.14: the cross product has both dialects, every prefix, value, suffix and
// declarator, and each pair with and without `g` first, each case once
#[test]
fn the_cross_product_holds_every_form_once() {
    let cases = cases();
    let mut keys: Vec<String> = cases.iter().map(Case::key).collect();
    let all = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), all);
    for source in [
        "void() f = #16d;\n",
        "void() f = # 5;\n",
        "void() f = #\"ab\" \"cd\";\n",
        "void() f = : \"ab\" \"cd\":ex_b;\n",
        "void f() = :16d;\n",
        "void() f #5;\n",
        "void() f = #5;\nvoid() f = #5:ex_a;\n",
        "void() g = #5:ex_a;\nvoid() f = #5:ex_a;\nvoid() f = #5:ex_a;\n",
    ] {
        assert!(cases.contains(&fte(source)), "{source:?}");
    }
    assert!(cases.contains(&vanilla("void() f = #5;\nvoid() f = #6;\n")));
    // QC-13.3: the vanilla dialect is given twice only the forms of `#number`.
    assert!(!cases.contains(&vanilla("void() f = :ex_a;\nvoid() f = :ex_a;\n")));
    assert!(cases.contains(&fte("void() f = :ex_a;\nvoid() f = :ex_a;\n")));
    assert!(cases.contains(&vanilla(&format!("void() f = :{};\n", "n".repeat(300)))));
}

// spec FTE-23.14: under FTEQCC the cross product has every character-constant escape, a macro
// name before every ASCII byte that is no part of a name and before UTF-8 characters, in the
// source, a body and an argument, and labels and builtins a macro's argument meets
#[test]
fn the_cross_product_holds_the_macro_and_escape_families() {
    let cases = cases();
    let body = |define: &str, statement: &str| {
        fte(&format!(
            "#define N 5\n{define}\nvoid() f = #5;\nvoid() g = {{ local float x; {statement} }};\n"
        ))
    };
    for escape in ["s", "{6z}", "u041", "\r\n"] {
        assert!(
            cases.contains(&fte(&format!("void() f = :'\\{escape}';\n"))),
            "{escape:?}"
        );
    }
    let mut words = 0;
    for byte in (1..=0x7f_u8).map(char::from) {
        let named = byte.is_ascii_alphanumeric() || byte == '_';
        let follower = format!("x = N{byte}{};", super::word_continuation(byte));
        assert_eq!(cases.contains(&body("", &follower)), !named, "{byte:?}");
        words += usize::from(!named);
    }
    assert_eq!(words, 64);
    assert!(cases.contains(&body("", "x = N\u{10000};")));
    assert!(cases.contains(&body("#define A(a) a!=5", "x = A(N);")));
    assert!(cases.contains(&body("#define V(...) __VA_ARGS__:", "V(lbl ) goto lbl;")));
    assert!(cases.contains(&fte("#define E(n) #5:n\nvoid() f = E(ex_a);\n")));
}

// spec FTE-23.14: under FTEQCC the cross product has every macro-expansion initialiser and
// statement: pastes, strings, empty arguments, recursion and argument lists past a chunk
#[test]
fn the_cross_product_holds_the_expansion_families() {
    let cases = cases();
    for (define, use_) in super::EXPANSION_INITIALISERS {
        let source = format!("{define}\nvoid() f = {use_};\n");
        assert!(cases.contains(&fte(&source)), "{source:?}");
    }
    for (define, statement) in super::EXPANSION_STATEMENTS {
        let source = format!(
            "#define N 5\n{define}\nvoid() f = #5;\nvoid() g = {{ local float x; {statement} }};\n"
        );
        assert!(cases.contains(&fte(&source)), "{source:?}");
    }
    assert_eq!(super::EXPANSION_INITIALISERS.len(), 102);
    assert_eq!(super::EXPANSION_STATEMENTS.len(), 18);
}

// spec FTE-23.14: under FTEQCC the cross product has the macro-text family: separators in an
// argument's character constant, comment or string, stored bodies, parameter lists, variadic
// macros, built-in constants' lines and `__FILE__`, and functions with no body
#[test]
fn the_cross_product_holds_the_macro_text_family() {
    let cases = cases();
    let family = super::macro_text_cases();
    for source in &family {
        assert!(cases.contains(&fte(source)), "{source:?}");
    }
    assert_eq!(family.len(), 188);
    for source in [
        "#define E(n) :n\nvoid() f = E(',');\n",
        "#define M(x) x\n#define K M /**/ (#5)\nvoid() f = K;\n",
        "#define E(n, n) n\nvoid() f = E(#5, 6);\n",
        "#define V(...) :__VA_COUNT__\nvoid() f = V(a, b);\n",
        "void() f = #5;\nvoid() g;\n",
        "#pragma file(ex_a:b)\nvoid() f = :__FILE__;\n",
    ] {
        assert!(family.iter().any(|s| s == source), "{source:?}");
    }
}

// spec FTE-23.14: an outcome is written and read back in one form; any other text is refused
#[test]
fn outcomes_round_trip() {
    for outcome in [
        bound_as(-5, "ex_a"),
        bound_as(0, "a b"),
        bound_as(0, ""),
        Outcome::Accepted,
        Outcome::Redeclared,
        Outcome::Rejected,
    ] {
        assert_eq!(Outcome::parse(&outcome.to_string()), Some(outcome));
    }
    for text in ["bound 5", "bound x f", "", "accepted 1", "rejected "] {
        assert_eq!(Outcome::parse(text), None, "{text}");
    }
}

// spec QC-27.9: the only expected difference is the vanilla profile's error for a builtin
// number not above 0, where id's qcc does not refuse; without that error the outcomes agree
#[test]
fn only_a_vanilla_number_not_above_zero_may_differ() {
    let case = vanilla("void() f = #0;\n");
    let pair = vanilla("void() f = #0;\nvoid() f = #5;\n");
    for (case, errors, oracle, expected) in [
        (
            &case,
            &[Code::E0507][..],
            Outcome::Accepted,
            Some("QC-27.9"),
        ),
        (&case, &[Code::E0507], Outcome::Redeclared, None),
        (&case, &[Code::E0507], Outcome::Rejected, None),
        (&case, &[Code::E0507], bound_as(0, "f"), None),
        (&case, &[Code::E0209], Outcome::Accepted, None),
        (&case, &[], Outcome::Accepted, None),
        (
            &pair,
            &[Code::E0507, Code::E0206],
            Outcome::Redeclared,
            Some("QC-27.9"),
        ),
        (&pair, &[Code::E0507, Code::E0206], Outcome::Rejected, None),
        (
            &pair,
            &[Code::E0507, Code::E0209],
            Outcome::Rejected,
            Some("QC-27.9"),
        ),
        (
            &pair,
            &[Code::E0507, Code::E0209],
            Outcome::Redeclared,
            None,
        ),
        (
            &fte("void() f = #0;\n"),
            &[Code::E0507],
            Outcome::Accepted,
            None,
        ),
    ] {
        assert_eq!(
            expected_difference(case, &Outcome::Rejected, errors, &oracle),
            expected,
            "{case:?} {errors:?} {oracle}"
        );
    }
}

// spec FTE-23.11: a macro after `#` that leaves `f` initialised with a number is FTEQCC's
// "Cannot cast" error and no error here; only that case may differ so
// spec FTEF-13.8: a case FTEQCC never finishes may differ, as the profile ends
#[test]
fn fteqcc_differs_only_where_it_casts_or_hangs() {
    let number = fte("#define N 5\nvoid() f = #N;\n");
    let string = fte("#define N \"ex_a\"\nvoid() f = #N;\n");
    let plain = fte("void() f = 5;\n");
    let hangs = fte("void() f = :\"\\UFFFFFFFF\";\n");
    let accepted = Outcome::Accepted;
    let rejected = Outcome::Rejected;
    for (case, ours, errors, oracle, expected) in [
        (&number, &accepted, &[][..], &rejected, Some("FTE-23.11")),
        (&number, &accepted, &[Code::E0209], &rejected, None),
        (&number, &rejected, &[], &accepted, None),
        (&number, &bound_as(5, "f"), &[], &rejected, None),
        (&string, &accepted, &[], &rejected, None),
        (&plain, &accepted, &[], &rejected, None),
        (
            &hangs,
            &bound_as(0, "x"),
            &[],
            &Outcome::Hangs,
            Some("FTEF-13.8"),
        ),
        (&hangs, &bound_as(0, "x"), &[], &rejected, None),
        // A case that is not one FTEQCC never finishes is a failure when it times out.
        (&plain, &accepted, &[], &Outcome::Hangs, None),
    ] {
        assert_eq!(
            expected_difference(case, ours, errors, oracle),
            expected,
            "{case:?} {ours} {errors:?} {oracle}"
        );
    }
}

// spec FTE-23.10: the profile's binding is read from the preprocessor's text, so a macro after
// `#` or `:` binds what it stands for; a builtin's own long name is found by its name
#[test]
fn bindings_are_read_after_the_preprocessor() {
    assert_eq!(
        profile(&fte("#define N #5:ex_a\nvoid() f = N;\n")).0,
        bound_as(5, "ex_a")
    );
    assert_eq!(
        profile(&fte("#define N ex_a\nvoid() f = :N;\n")).0,
        bound_as(0, "ex_a")
    );
    assert_eq!(
        profile(&fte("#define N 5\nvoid() f = #N;\n")).0,
        Outcome::Accepted
    );
    let long = "n".repeat(254);
    assert_eq!(
        profile(&fte(&format!("void() {long} = #5;\n"))).0,
        bound_as(5, &long)
    );
    // QC-9.16: a builtin a macro body writes binds as written.
    assert_eq!(
        profile(&fte("#define B #5\nvoid() f = B;\n")).0,
        bound_as(5, "f")
    );
    for (source, name) in [
        ("void f() = #5;\n", "f"),
        ("void() n = #5;\n", "f"),
        ("void() nx = #5;\n", "f"),
        ("void() nn = #5;\n", "nn"),
        ("void() g = #5;\nvoid() f = #5;\n", "f"),
    ] {
        assert_eq!(super::function_name(&fte(source)), name, "{source:?}");
    }
}

// spec QC-13.4: id's qcc's `#0` is found from the source text, whatever the number's form
#[test]
fn a_vanilla_zero_is_read_from_the_source() {
    for (source, zero) in [
        ("void() f = #0;\n", true),
        ("void() f = # -0;\n", true),
        ("void() f = #0.0;\n", true),
        ("void() g = #0;\nvoid() f = #0;\nvoid() f = #5;\n", true),
        ("void() f = #5;\n", false),
        ("void() f = #0.5;\n", false),
        ("void() f = #-;\n", false),
        ("void() f;\n", false),
    ] {
        assert_eq!(super::vanilla_zero(&vanilla(source)), zero, "{source:?}");
    }
}

// spec FTE-23.7: under FTEQCC a redeclaration is reported only when it is the only error, since
// FTEQCC reads the initialiser first; under the vanilla dialect when it is the first, since
// id's qcc stops at the `=`
#[test]
fn the_profile_classifies_its_errors_as_each_compiler_stops() {
    let twice_bad = "void() f = #5;\nvoid() f = #16d;\n";
    assert_eq!(profile(&fte(twice_bad)).0, Outcome::Rejected);
    assert_eq!(
        profile(&fte("void() f = #5;\nvoid() f = #6;\n")),
        (Outcome::Redeclared, vec![Code::E0206])
    );
    assert_eq!(
        profile(&vanilla("void() f = #5;\nvoid() f = #5x;\n")).0,
        Outcome::Redeclared
    );
    assert_eq!(
        profile(&vanilla("void() f = #0;\nvoid() f = #5;\n")),
        (Outcome::Rejected, vec![Code::E0507, Code::E0206])
    );
    assert_eq!(
        profile(&fte("void() f = #5:ex_a;\n")).0,
        bound_as(5, "ex_a")
    );
    assert_eq!(profile(&fte("void() f = #\"\";\n")).0, bound_as(0, "f"));
    assert_eq!(profile(&vanilla("void() f = #16;\n")).0, bound_as(16, "f"));
    // A program with no builtin `f` binds nothing.
    assert_eq!(profile(&fte("void() g = #5;\n")).0, Outcome::Accepted);
    assert_eq!(profile(&fte("float f;\n")).0, Outcome::Accepted);
}

/// A `progs.dat` whose functions after the null one are `functions`, each a builtin number
/// and a name.
fn progs_with(functions: &[(i32, &str)]) -> Vec<u8> {
    let mut strings = vec![0_u8];
    let mut records = vec![Function::default()];
    for (number, name) in functions {
        let s_name = i32::try_from(strings.len()).unwrap();
        // Each `char` one byte (Latin-1), as the oracle reads a name back.
        strings.extend(name.chars().map(|c| u8::try_from(u32::from(c)).unwrap()));
        strings.push(0);
        records.push(Function {
            first_statement: number.wrapping_neg(),
            s_name,
            ..Function::default()
        });
    }
    let file = ProgsFile {
        entityfields: 0,
        statements: vec![qc_progs::Statement::default()],
        functions: records,
        strings,
        globals: vec![Slot(0); 28],
        ..ProgsFile::default()
    };
    write(&file).unwrap()
}

fn read(bytes: &[u8]) -> qc_progs::Progs<'_> {
    let options = qc_progs::ReadOptions {
        check_header_crc: false,
    };
    qc_progs::Progs::read_with(bytes, &options).unwrap()
}

// spec FTE-23.14: `f`'s function is the first, or the second after `g`'s; FTEQCC writes every
// builtin's number as `-first_statement`, id's qcc only a nonzero one's (QC-13.4)
#[test]
fn bound_reads_f_s_function_by_its_place() {
    let bytes = progs_with(&[(5, "ex_g"), (-5, "ex_f")]);
    let progs = read(&bytes);
    assert_eq!(
        bound(&progs, &fte("void() f = #5;\n")),
        Some(bound_as(5, "ex_g"))
    );
    assert_eq!(
        bound(&progs, &fte("void() g = #5;\nvoid() f = #-5;\n")),
        Some(bound_as(-5, "ex_f"))
    );
    assert_eq!(
        bound(&progs, &vanilla("void() g = #5;\nvoid() f = #-5;\n")),
        Some(Outcome::Accepted)
    );
    assert_eq!(
        bound(&progs, &vanilla("void() f = #5;\n")),
        Some(bound_as(5, "ex_g"))
    );
    let zero = progs_with(&[(0, "f")]);
    assert_eq!(
        bound(&read(&zero), &vanilla("void() f = #0;\n")),
        Some(Outcome::Accepted)
    );
    assert_eq!(
        bound(&read(&zero), &fte("void() f = #0;\n")),
        Some(bound_as(0, "f"))
    );
    // FTE-23.11: FTEQCC makes no function for `f` initialised with a string; a function
    // missing before `f`'s place is no such case.
    let none = progs_with(&[]);
    assert_eq!(
        bound(&read(&none), &fte("void() f = #0;\n")),
        Some(Outcome::Accepted)
    );
    assert_eq!(bound(&read(&none), &vanilla("void() f = #5;\n")), None);
    let after_g = fte("void() g = #5;\nvoid() f = #5;\n");
    assert_eq!(bound(&read(&none), &after_g), None);
}

/// What the fake compilers write for each outcome: FTEQCC's and qcc's own words.
fn reply(dir: &Path, fteqcc: bool, case: &Case, outcome: &Outcome) -> Reply {
    let failed = |line: &str| Reply {
        code: 1,
        stdout: format!(
            "compiling t.qc\nt.qc:1: warning: w\n{line}\nt.qc:1:    f is defined here\n"
        ),
        ..Reply::default()
    };
    match outcome {
        Outcome::Hangs => Reply {
            hangs: true,
            ..Reply::default()
        },
        Outcome::Rejected => failed("t.qc:1: error: Bad builtin immediate"),
        Outcome::Redeclared if fteqcc => failed("t.qc:2: error: incompatible redeclaration"),
        Outcome::Redeclared => failed("t.qc:2:f redeclared"),
        // FTEQCC accepts with no builtin only a function it makes none for (FTE-23.11).
        Outcome::Accepted if fteqcc => Reply {
            files: vec![(dir.join("progs.dat"), progs_with(&[]))],
            ..Reply::exit(0)
        },
        Outcome::Accepted => Reply {
            files: vec![(dir.join("progs.dat"), b"not a progs".to_vec())],
            ..Reply::exit(0)
        },
        Outcome::Bound { number, name } => {
            let mut functions = vec![(*number, name.as_str())];
            if case.source.starts_with("void() g") {
                functions.insert(0, (1, "g"));
            }
            Reply {
                files: vec![(dir.join("progs.dat"), progs_with(&functions))],
                ..Reply::exit(0)
            }
        }
    }
}

/// Fake compilers that give each case the outcome `recorded` has for it.
fn answering(recorded: BTreeMap<String, Outcome>) -> impl Fn(&Command) -> Reply {
    move |command: &Command| {
        let dir = command.get_current_dir().unwrap().to_path_buf();
        let fteqcc = command.get_program() == "fteqcc";
        assert_eq!(command.get_args().count(), usize::from(fteqcc));
        assert_eq!(
            fs::read(dir.join("progs.src")).unwrap(),
            b"progs.dat\nt.qc\n"
        );
        assert!(!dir.join("progs.dat").exists());
        let case = Case {
            dialect: if fteqcc {
                Dialect::Fteqcc
            } else {
                Dialect::Vanilla
            },
            source: fs::read_to_string(dir.join("t.qc")).unwrap(),
        };
        reply(&dir, fteqcc, &case, &recorded[&case.key()])
    }
}

/// A repository whose fixture is `fixture`.
fn repository(name: &str, fixture: &BTreeMap<String, Outcome>) -> PathBuf {
    let dir = fresh_dir(name);
    let path = dir.join(FIXTURE);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let raw: BTreeMap<&String, String> = fixture.iter().map(|(k, v)| (k, v.to_string())).collect();
    fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();
    dir
}

// spec FTE-23.14: the check runs every case through the oracle of its dialect and passes when
// the oracles give what the fixture records and the profile agrees
#[test]
fn check_passes_when_the_oracles_give_the_recording() {
    let recorded = read_fixture(&root()).unwrap();
    let repo = repository("builtins-agree", &recorded);
    let fake = Fake::new(answering(recorded.clone()));
    let oracles = fake_oracles(&repo.join("work"), &fake);
    assert!(check(&repo, &oracles, false).unwrap());
    assert_eq!(fake.seen.borrow().len(), recorded.len());
    assert!(fake.seen.borrow().iter().any(|line| line == "fteqcc -Tid"));
    assert!(fake.seen.borrow().iter().any(|line| line == "qcc"));
}

// spec FTE-23.14: an oracle that gives another outcome than the fixture fails the check, and
// `--record` writes what the oracles gave
#[test]
fn check_fails_on_a_stale_recording_and_records_anew() {
    let recorded = read_fixture(&root()).unwrap();
    let key = fte("void() f = #5;\n").key();
    let mut stale = recorded.clone();
    stale.insert(key.clone(), bound_as(6, "f"));
    let repo = repository("builtins-stale", &stale);
    let fake = Fake::new(answering(recorded.clone()));
    let oracles = fake_oracles(&repo.join("work"), &fake);
    assert!(!check(&repo, &oracles, false).unwrap());
    assert!(check(&repo, &oracles, true).unwrap());
    assert_eq!(read_fixture(&repo).unwrap(), recorded);
    // A recording of a case the cross product no longer makes is stale too.
    let mut extra = recorded.clone();
    extra.insert("fteqcc|gone".to_owned(), Outcome::Rejected);
    let repo = repository("builtins-extra", &extra);
    let oracles = fake_oracles(&repo.join("work"), &fake);
    assert!(!check(&repo, &oracles, false).unwrap());
}

// spec FTE-23.14: an oracle whose outcome the profile does not share fails the check, even
// when the recording has it
#[test]
fn check_fails_when_the_profile_differs() {
    let mut recorded = read_fixture(&root()).unwrap();
    recorded.insert(fte("void() f = #16d;\n").key(), bound_as(16, "f"));
    let repo = repository("builtins-differ", &recorded);
    let fake = Fake::new(answering(recorded.clone()));
    let oracles = fake_oracles(&repo.join("work"), &fake);
    assert!(!check(&repo, &oracles, false).unwrap());
    let tally = compare(&cases(), &recorded);
    assert_eq!(tally.differ, 1);
    let mut missing = recorded;
    missing.remove(&fte("void() f = #5;\n").key());
    assert_eq!(compare(&cases(), &missing).differ, 2);
}

// spec FTE-23.14: a compiler that cannot run, a progs it does not write, and an FTEQCC progs
// qc-progs refuses are errors of the check, not outcomes
#[test]
fn oracle_failures_are_errors() {
    let dir = fresh_dir("builtins-failures");
    let case = fte("void() f = #5;\n");
    let cannot = Fake::new(|_: &Command| Reply {
        cannot_run: true,
        ..Reply::default()
    });
    let err = oracle(&fake_oracles(&dir, &cannot), &dir, &case).unwrap_err();
    assert!(err.contains("cannot run fteqcc"), "{err}");
    let err = oracle(
        &fake_oracles(&dir, &cannot),
        &dir,
        &vanilla("void() f = #5;\n"),
    )
    .unwrap_err();
    assert!(err.contains("cannot run qcc"), "{err}");
    let silent = Fake::new(|_: &Command| Reply::exit(0));
    let err = oracle(&fake_oracles(&dir, &silent), &dir, &case).unwrap_err();
    assert!(err.contains("progs.dat"), "{err}");
    let garbage = dir.join("progs.dat");
    let refused = Fake::new(move |_: &Command| Reply {
        files: vec![(garbage.clone(), b"not a progs".to_vec())],
        ..Reply::exit(0)
    });
    let err = oracle(&fake_oracles(&dir, &refused), &dir, &case).unwrap_err();
    assert!(err.contains("qc-progs refuses"), "{err}");
    assert!(err.starts_with("fteqcc \"void() f = #5;\\n\": "), "{err}");
    assert_eq!(
        oracle(
            &fake_oracles(&dir, &refused),
            &dir,
            &vanilla("void() f = #5;\n")
        )
        .unwrap(),
        Outcome::Accepted
    );
    // QC-13.4: whatever id's qcc wrote for `#0` is not recorded.
    let heap = dir.join("progs.dat");
    let uninitialised = Fake::new(move |_: &Command| Reply {
        files: vec![(heap.clone(), progs_with(&[(7, "f")]))],
        ..Reply::exit(0)
    });
    for (case, outcome) in [
        (vanilla("void() f = #0;\n"), Outcome::Accepted),
        (vanilla("void() f = #-0;\n"), Outcome::Accepted),
        (vanilla("void() f = #7;\n"), bound_as(7, "f")),
        (fte("void() f = #0;\n"), bound_as(7, "f")),
    ] {
        assert_eq!(
            oracle(&fake_oracles(&dir, &uninitialised), &dir, &case).unwrap(),
            outcome,
            "{case:?}"
        );
    }
    let empty = dir.join("progs.dat");
    let no_function = Fake::new(move |_: &Command| Reply {
        files: vec![(empty.clone(), progs_with(&[]))],
        ..Reply::exit(0)
    });
    let after_g = fte("void() g = #5;\nvoid() f = #5;\n");
    let err = oracle(&fake_oracles(&dir, &no_function), &dir, &after_g).unwrap_err();
    assert!(err.contains("no function f"), "{err}");
}

// spec FTE-23.14: an oracle killed at its time limit hangs, an FTEQCC with nothing to write
// accepts, and one that writes no progs for another reason is an error of the check
#[test]
fn oracles_that_hang_or_write_nothing() {
    let dir = fresh_dir("builtins-hang");
    let case = fte("void() f = :\"\\UFFFFFFFF\";\n");
    let hangs = Fake::new(|_: &Command| Reply {
        hangs: true,
        ..Reply::default()
    });
    assert_eq!(
        oracle(&fake_oracles(&dir, &hangs), &dir, &case).unwrap(),
        Outcome::Hangs
    );
    let says = |stdout: &'static str| {
        Fake::new(move |_: &Command| Reply {
            stdout: stdout.to_owned(),
            ..Reply::exit(0)
        })
    };
    let zero = fte("#define N 0\nvoid() f = #N;\n");
    assert_eq!(
        oracle(
            &fake_oracles(&dir, &says("nothing to write\n")),
            &dir,
            &zero
        )
        .unwrap(),
        Outcome::Accepted
    );
    let err = oracle(&fake_oracles(&dir, &says("done\n")), &dir, &zero).unwrap_err();
    assert!(
        err.contains("No such file") || err.contains("cannot find"),
        "{err}"
    );
    let vanilla_nothing = vanilla("void() f = #5;\n");
    assert!(
        oracle(
            &fake_oracles(&dir, &says("nothing to write\n")),
            &dir,
            &vanilla_nothing
        )
        .is_err()
    );
}

// spec FTE-23.14: a refusal is a redeclaration only when the first error the compiler reports at
// a line of the file is its redeclaration error; warnings and "defined here" notes are skipped
#[test]
fn oracle_reads_the_first_error() {
    let dir = fresh_dir("builtins-errors");
    for (fteqcc, stdout, outcome) in [
        (
            true,
            "t.qc:1: warning: x\nt.qc:2: error: incompatible redeclaration\n",
            Outcome::Redeclared,
        ),
        (
            true,
            "t.qc:2:    const void() f is defined here\nt.qc:2: error: Bad builtin immediate\n\
             t.qc:3: error: incompatible redeclaration\n",
            Outcome::Rejected,
        ),
        (true, "t.qc:2:f redeclared\n", Outcome::Rejected),
        (false, "t.qc:2:f redeclared\n", Outcome::Redeclared),
        (
            false,
            "t.qc:1:Bad builtin immediate\nt.qc:2:f redeclared\n",
            Outcome::Rejected,
        ),
        (false, "compilation errors\n", Outcome::Rejected),
    ] {
        let fake = Fake::new(move |_: &Command| Reply {
            code: 1,
            stderr: stdout.to_owned(),
            ..Reply::default()
        });
        let case = Case {
            dialect: if fteqcc {
                Dialect::Fteqcc
            } else {
                Dialect::Vanilla
            },
            source: "void() f = #5;\n".to_owned(),
        };
        assert_eq!(
            oracle(&fake_oracles(&dir, &fake), &dir, &case).unwrap(),
            outcome,
            "{stdout}"
        );
    }
}

// spec FTE-23.14: a fixture that is not JSON, or holds an outcome in no known form, is refused
#[test]
fn a_bad_fixture_is_refused() {
    let dir = fresh_dir("builtins-bad-fixture");
    assert!(
        read_fixture(&dir)
            .unwrap_err()
            .contains("builtin-outcomes.json")
    );
    let path = dir.join(FIXTURE);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "[").unwrap();
    assert!(read_fixture(&dir).is_err());
    fs::write(&path, "{\"fteqcc|x\": \"maybe\"}").unwrap();
    assert!(
        read_fixture(&dir)
            .unwrap_err()
            .contains("bad outcome `maybe`")
    );
}

// spec FTE-23.14: the tally names each statement an expected difference is recorded under
#[test]
fn the_tally_prints_each_count() {
    let mut tally = Tally {
        agree: 3,
        differ: 1,
        ..Tally::default()
    };
    tally.expected.insert("QC-27.9", 2);
    assert_eq!(
        tally.to_string(),
        "3 agree with the profile, 2 differ as QC-27.9 says, 1 differ"
    );
}
