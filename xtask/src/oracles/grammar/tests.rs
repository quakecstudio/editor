// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use super::generate::{PROGRAMS, program};
use super::{
    Case, FIXTURE, Outcome, cases, check, check_recorded, expected_difference, first_error,
    read_fixture,
};
use crate::exec::{Fake, Reply};
use crate::oracles::testing::{fake_oracles, fresh_dir, root};

// spec DIFF-13.4: the recorded programs replay against the profile without the oracle, and the
// profile agrees with each, or differs as DIFF-13.3 explains
#[test]
fn the_recorded_programs_replay_without_the_oracle() {
    assert!(check_recorded(&root()).unwrap());
}

// spec DIFF-13.1: the generator makes the same programs on every host, of every form it names,
// and mutates about a third of them
#[test]
fn the_programs_cover_the_forms_and_are_the_same_each_time() {
    assert_eq!(program(7), program(7));
    let all: Vec<String> = (0..PROGRAMS).map(program).collect();
    let text = all.concat();
    for form in [
        "__unused ",
        "__deprecated(\"old\") ",
        "const ",
        "__const ",
        "typedef ",
        "enum {",
        "enumflags {",
        "struct ",
        "union ",
        "class ",
        "virtual ",
        "nonvirtual ",
        "accessor ",
        "inline get",
        "set float",
        "switch (",
        "case ",
        " .. ",
        "default:",
        "for (",
        "do\n",
        "while (",
        "break;",
        "continue;",
        " ? ",
        "++;",
        "__noref ",
    ] {
        assert!(text.contains(form), "no program has `{form}`");
    }
    let recorded = read_fixture(&root()).unwrap();
    let refused = recorded
        .values()
        .filter(|(_, outcome)| *outcome != Outcome::Accepted)
        .count();
    assert!(
        (200..=500).contains(&refused),
        "{refused} programs are refused"
    );
}

// spec DIFF-13.2: FTEQCC's outcome is written and read back as the fixture holds it, and its
// first error is the first `t.qc:LINE: error...` line it prints
#[test]
fn outcomes_and_errors_are_read_as_written() {
    for outcome in [
        Outcome::Accepted,
        Outcome::Rejected(None),
        Outcome::Rejected(Some((12, "\"=\" - not a name".to_owned()))),
        Outcome::Hangs,
    ] {
        assert_eq!(Outcome::parse(&outcome.to_string()), Some(outcome));
    }
    assert_eq!(Outcome::parse("rejected at x: m"), None);
    assert_eq!(Outcome::parse("bogus"), None);
    let log = "t.qc:3: warning F334: X\nin function f (line 4),\nt.qc:5: error: Unknown value \
               \"a\".\nt.qc:6: error: later\n";
    assert_eq!(
        first_error(log),
        Some((5, "Unknown value \"a\".".to_owned()))
    );
    assert_eq!(first_error("compilation errors\n"), None);
    assert_eq!(Outcome::of_run(true, log), Outcome::Accepted);
    assert_eq!(Outcome::of_run(false, "nothing\n"), Outcome::Rejected(None));
}

// spec DIFF-13.2: the profile agrees when both accept, or both reject with FTEQCC's first
// message among the profile's, wherever the profile puts it
#[test]
fn agreement_is_on_acceptance_and_the_first_message() {
    let errors = vec![(1, "a".to_owned()), (9, "b".to_owned())];
    assert!(Outcome::Accepted.agrees(&[]));
    assert!(!Outcome::Accepted.agrees(&errors));
    assert!(Outcome::Rejected(None).agrees(&errors));
    assert!(!Outcome::Rejected(None).agrees(&[]));
    assert!(Outcome::Rejected(Some((4, "b".to_owned()))).agrees(&errors));
    assert!(!Outcome::Rejected(Some((9, "c".to_owned()))).agrees(&errors));
    assert!(!Outcome::Hangs.agrees(&[]));
}

// spec DIFF-13.3: a program that lost a `}` whose body the profile ended is the difference
// FTEF-18.34 explains; no other is expected
#[test]
fn only_a_lost_brace_is_expected() {
    let case = |program: &str| Case {
        key: "k".to_owned(),
        program: program.to_owned(),
    };
    let ended = vec![(3, "expected }, found float".to_owned())];
    assert_eq!(
        expected_difference(&case("class C {\nfloat a;\n"), &ended),
        Some("FTEF-18.34")
    );
    assert_eq!(expected_difference(&case("class C {\n};\n"), &ended), None);
    let other = vec![(3, "\";\" - not a name".to_owned())];
    assert_eq!(expected_difference(&case("class C {\n"), &other), None);
}

/// A fake FTEQCC that gives each program the outcome `recorded` holds for it, found by the
/// `t.qc` the run writes, as FTEQCC's own output would say it.
fn answering(recorded: &BTreeMap<String, Outcome>, command: &Command) -> Reply {
    let dir = command.get_current_dir().unwrap();
    let text = fs::read_to_string(Path::new(dir).join("t.qc")).unwrap();
    match recorded.get(&text) {
        Some(Outcome::Accepted) => Reply::exit(0),
        Some(Outcome::Rejected(Some((line, message)))) => Reply {
            code: 1,
            stdout: format!("t.qc:{line}: error: {message}\n"),
            ..Reply::default()
        },
        Some(Outcome::Hangs) => Reply {
            hangs: true,
            ..Reply::default()
        },
        Some(Outcome::Rejected(None)) | None => Reply::exit(1),
    }
}

// spec DIFF-13.4: the check runs every program through FTEQCC and passes when it gives what the
// fixture records; an outcome that differs from the recording fails it
#[test]
fn the_check_compares_the_oracle_with_the_recording() {
    let by_program: BTreeMap<String, Outcome> =
        read_fixture(&root()).unwrap().into_values().collect();
    let fake = Fake::new(|command: &Command| answering(&by_program, command));
    let oracles = fake_oracles(&fresh_dir("grammar-agree"), &fake);
    assert!(check(&root(), &oracles, false).unwrap());
    assert_eq!(fake.seen.lock().unwrap().len(), cases().len());
    let mut changed = by_program.clone();
    changed.insert(
        cases()[0].program.clone(),
        Outcome::Rejected(Some((1, "changed".to_owned()))),
    );
    let fake = Fake::new(|command: &Command| answering(&changed, command));
    let oracles = fake_oracles(&fresh_dir("grammar-differ"), &fake);
    assert!(!check(&root(), &oracles, false).unwrap());
    assert!(root().join(FIXTURE).is_file());
}
