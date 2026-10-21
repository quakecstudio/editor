// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::collections::BTreeMap;
use std::process::Command;

use qc_progs::{ProgsFile, Statement, write};

use super::super::testing::{fake_oracles, fresh_dir, root};
use std::borrow::Cow;

use super::{
    Case, Outcome, agree, all_cases, check, compare, differences, first_error, read_fixture,
};
use crate::exec::{Fake, Reply};

// spec QC-25.1: qc-codegen writes the progs id's qcc writes for every recorded program, or
// refuses it with qcc's message, but where a QC-27 statement says otherwise.
#[test]
fn qc_codegen_agrees_with_the_recorded_qcc_outcomes() {
    let recorded = read_fixture(&root()).unwrap();
    assert_eq!(recorded.len(), all_cases().len(), "every case is recorded");
    assert!(compare(&recorded, None).unwrap());
}

#[test]
fn every_case_has_its_own_name() {
    let cases = all_cases();
    let mut names: Vec<&str> = cases.iter().map(|c| c.name.as_ref()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), cases.len());
}

#[test]
fn an_outcome_reads_back_as_written() {
    for outcome in [
        Outcome::Progs("ab12".to_owned()),
        Outcome::Error("type mismatch for =".to_owned()),
        Outcome::Crash,
    ] {
        assert_eq!(Outcome::parse(&outcome.to_string()), Some(outcome));
    }
    assert_eq!(Outcome::parse("bound 3"), None);
    assert_eq!(Outcome::parse("progs"), None);
}

#[test]
fn qccs_first_error_is_read_from_its_log() {
    let files = [("t.qc", ""), ("b.qc", "")];
    assert_eq!(
        first_error(
            "compiling t.qc\nt.qc:3:type mismatch for /\nt.qc:4:x\n",
            &files
        ),
        Some("type mismatch for /".to_owned())
    );
    assert_eq!(
        first_error("b.qc:12:not a function\n", &files),
        Some("not a function".to_owned())
    );
    assert_eq!(
        first_error("function g was not defined\n", &files),
        Some("function g was not defined".to_owned())
    );
    assert_eq!(
        first_error(
            "\n************ ERROR ************\nbad parm order\n",
            &files
        ),
        Some("bad parm order".to_owned())
    );
    assert_eq!(first_error("t.qc:x:no line\nt.qc::none\n", &files), None);
    assert_eq!(first_error("compiling t.qc\n", &files), None);
}

#[test]
fn outcomes_agree_on_the_same_progs_or_a_shared_message() {
    let progs = |d: &str| Outcome::Progs(d.to_owned());
    let error = |m: &str| Outcome::Error(m.to_owned());
    assert!(agree(&progs("a"), &[], &progs("a")));
    assert!(!agree(&progs("a"), &[], &progs("b")));
    let messages = ["x".to_owned(), "y".to_owned()];
    assert!(agree(&error("x"), &messages, &error("y")));
    assert!(!agree(&error("x"), &messages, &error("z")));
    assert!(!agree(&progs("a"), &[], &error("x")));
    assert!(!agree(&error("x"), &messages, &Outcome::Crash));
}

#[test]
fn differences_name_the_first_entry_of_each_table_that_differs() {
    let file = |op: u32, crc: i32| {
        write(&ProgsFile {
            crc,
            statements: vec![
                Statement::default(),
                Statement {
                    op,
                    ..Statement::default()
                },
            ],
            strings: vec![0, 0, 0, 0],
            ..ProgsFile::default()
        })
        .unwrap()
    };
    assert!(
        differences(&file(1, 5927), &file(1, 5927))
            .unwrap()
            .is_empty()
    );
    let found = differences(&file(2, 1), &file(1, 5927)).unwrap();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("crc 1 against 5927"));
    assert!(found[1].starts_with("statement 1:"));
    assert!(differences(b"short", &file(1, 5927)).is_err());
}

/// A fake qcc that writes `progs` for every case, or prints `log` and writes nothing.
fn qcc(progs: Option<Vec<u8>>, log: &'static str) -> impl Fn(&Command) -> Reply {
    move |command: &Command| {
        let dir = command.get_current_dir().unwrap().to_path_buf();
        Reply {
            stdout: log.to_owned(),
            files: progs
                .iter()
                .map(|p| (dir.join("progs.dat"), p.clone()))
                .collect(),
            ..Reply::default()
        }
    }
}

#[test]
fn a_qcc_that_disagrees_with_the_recording_fails_the_check() {
    let dir = fresh_dir("codegen-check");
    let fake = Fake::new(qcc(None, "t.qc:1:something else\n"));
    let oracles = fake_oracles(&dir.join("work"), &fake);
    assert!(!check(&root(), &oracles, false).unwrap());
    let seen = fake.seen.borrow();
    let cases = all_cases();
    assert_eq!(
        seen.iter().filter(|l| l.starts_with("qcc")).count(),
        cases.len()
    );
    let src = std::fs::read_to_string(
        dir.join("work/codegen")
            .join(cases[0].name.as_ref())
            .join("progs.src"),
    );
    assert_eq!(src.unwrap(), "progs.dat\nt.qc\n");
}

#[test]
fn a_case_that_crashes_qcc_is_a_crash() {
    let dir = fresh_dir("codegen-crash");
    let fake = Fake::new(qcc(None, "compiling t.qc\n"));
    let oracles = fake_oracles(&dir.join("work"), &fake);
    let case = Case {
        name: Cow::Borrowed("c"),
        files: &[("b.qc", "float b;\n")],
        source: Cow::Borrowed("float a;\n"),
        divergence: None,
    };
    assert_eq!(super::oracle(&oracles, &case).unwrap(), Outcome::Crash);
    let src = std::fs::read_to_string(dir.join("work/codegen/c/progs.src")).unwrap();
    assert_eq!(src, "progs.dat\nt.qc\nb.qc\n");
}

#[test]
fn a_listed_divergence_that_agrees_fails() {
    let mut recorded = BTreeMap::new();
    for case in &all_cases() {
        recorded.insert(case.name.to_string(), super::ours(case).outcome);
    }
    assert!(
        !compare(&recorded, None).unwrap(),
        "the diverging cases agree with themselves"
    );
}
