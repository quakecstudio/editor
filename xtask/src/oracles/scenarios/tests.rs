// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or match is a failed test"
)]

use std::collections::BTreeSet;

use qc_progs::crc::crc_block;
use qc_progs::{Progs, ReadOptions};
use qc_trace::{Changes, FieldChange, GlobalChange, Header, Record, Step, Text, Trace, encode};

use super::*;

#[test]
fn every_case_builds_a_progs_qc_progs_reads_and_has_a_unique_name() {
    let mut names = BTreeSet::new();
    let cases = cases::all();
    assert!(cases.len() > 150, "{}", cases.len());
    for case in &cases {
        assert!(names.insert(case.name.clone()), "{} twice", case.name);
        assert!(
            !case.name.is_empty()
                && case
                    .name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
            "{} is not a directory name",
            case.name
        );
        let bytes = case.program.bytes().unwrap();
        Progs::read_with(
            &bytes,
            &ReadOptions {
                check_header_crc: false,
            },
        )
        .unwrap();
        assert!(!case.checks.is_empty(), "{} checks nothing", case.name);
    }
}

#[test]
fn a_statement_that_does_not_parse_fails_the_build() {
    let mut p = Program::new();
    p.func("bad", &["NOT_AN_OPCODE 1 2 3".to_owned()]);
    assert!(p.bytes().unwrap_err().contains("NOT_AN_OPCODE"));
    let mut p = Program::new();
    p.body(999, &["DONE".to_owned()]);
    assert!(p.bytes().unwrap_err().contains("no function 999"));
    let mut p = Program::new();
    p.set_function_locals(999, 100, 1);
    assert!(p.bytes().is_err());
}

#[test]
fn a_program_has_noop_worldspawn_and_its_system_functions() {
    let mut p = Program::new();
    assert_eq!(p.next_function(), 3);
    for slot in global::MAIN..=global::SET_CHANGE_PARMS {
        assert_eq!(p.initial(slot), 1);
    }
    let progs = p.bytes().unwrap();
    let read = Progs::read(&progs).unwrap();
    assert_eq!(read.header().crc, qc_progs::PROGS_HEADER_CRC);
    assert_eq!(read.header().entityfields, 110);
    assert_eq!(read.header().numglobals, 128);
    let slot = p.float(2.5);
    assert_eq!(slot, 100);
    assert_eq!(p.initial(slot), 2.5_f32.to_bits());
    let v = p.vector([1.0, 2.0, 3.0]);
    assert_eq!(p.initial(v.saturating_add(2)), 3.0_f32.to_bits());
    let text = p.string("hello");
    let offset = p.initial(text);
    assert_eq!(p.table_text(offset), Some(&b"hello"[..]));
    assert_eq!(p.table_text(99_999), None);
    p.reserve_globals(300);
    assert_eq!(p.temp(), 300);
    assert_eq!(
        p.bytes()
            .map(|b| Progs::read(&b).unwrap().header().numglobals)
            .unwrap(),
        301
    );
    p.truncate_globals(91);
    assert_eq!(p.initial(95), 0);
}

#[test]
fn calls_store_their_arguments_then_call() {
    let mut p = Program::new();
    assert_eq!(
        p.call(120, &[Arg::F(100), Arg::V(101)]),
        ["STORE_F 100 4", "STORE_V 101 7", "CALL2 120"]
    );
    let code = p.write_field(28, field::HEALTH, 100);
    assert_eq!(code.len(), 2);
    assert!(code[0].starts_with("ADDRESS 28 "));
    assert!(code[1].starts_with("STOREP_F 100 "));
    assert!(p.write_vector_field(28, field::ORIGIN, 101)[1].starts_with("STOREP_V 101 "));
    assert!(p.read_field(28, field::HEALTH, 102)[0].starts_with("LOAD_F 28 "));
}

/// A trace with a 536-byte edict layout and 1000 bytes of strings.
fn trace(records: Vec<Record>) -> Trace {
    Trace {
        header: Header::new(0, 536, 96, 110, 1000).unwrap(),
        records,
    }
}

fn changes(globals: &[(u32, u32)], fields: &[(u32, u32, u32)]) -> Changes {
    Changes::new(
        globals
            .iter()
            .map(|&(slot, bits)| GlobalChange { slot, bits })
            .collect(),
        fields
            .iter()
            .map(|&(edict, field, bits)| FieldChange { edict, field, bits })
            .collect(),
    )
    .unwrap()
}

fn run(function: i32, globals: &[(u32, u32)]) -> Record {
    Record::Run {
        function,
        changes: changes(globals, &[]),
    }
}

fn step(function: i32, statement: i32, builtin: Option<u32>, fields: &[(u32, u32, u32)]) -> Record {
    Record::Step(Step {
        function,
        statement,
        opcode: 0,
        builtin,
        changes: changes(&[], fields),
    })
}

fn exit() -> Record {
    Record::Exit {
        changes: Changes::default(),
    }
}

fn error(message: &str) -> Record {
    Record::Error {
        function: 3,
        statement: 7,
        message: Text::new(message.as_bytes()).unwrap(),
        changes: Changes::default(),
    }
}

fn string(value: u32, text: &str) -> Record {
    Record::String {
        value,
        text: Text::new(text.as_bytes()).unwrap(),
    }
}

/// A case with these checks, and the outcome of a run that wrote `trace` and exited 0.
fn outcome_of(trace: &Trace) -> Outcome {
    Outcome {
        status: Some(0),
        trace: Some(encode(trace)),
        ..Outcome::default()
    }
}

fn case(checks: Vec<Check>) -> Case {
    let mut case = Case::new("t", Program::new(), scenario(""));
    case.checks = checks;
    case
}

fn sample() -> Trace {
    trace(vec![
        string(2000, "hello"),
        run(2, &[(31, 1.0_f32.to_bits()), (34, 2000)]),
        step(2, 5, None, &[(1, 48, 9.0_f32.to_bits())]),
        step(2, 6, Some(14), &[(2, 10, 0x8000_0000)]),
        exit(),
        run(1, &[(28, 536)]),
        error("boom"),
    ])
}

#[test]
fn sequences_match_records_in_order_not_adjacent() {
    let trace = sample();
    let outcome = outcome_of(&trace);
    let ok = |check: Check| verify(&case(vec![check]), &outcome);
    ok(Check::Seq(vec![
        Want::String("hello"),
        Want::Run(2),
        Want::Builtin(14),
        Want::Run(1),
        Want::ErrorMessage("boom"),
    ]))
    .unwrap();
    let error = ok(Check::Seq(vec![Want::Run(1), Want::Run(2)])).unwrap_err();
    assert!(error.contains("after the first 1"), "{error}");
    ok(Check::Seq(vec![Want::All(vec![
        Want::Run(2),
        Want::Global(31, Val::F(1.0)),
        Want::GlobalChanged(34),
    ])]))
    .unwrap();
    assert!(ok(Check::Seq(vec![Want::All(Vec::new())])).is_err());
    ok(Check::Seq(vec![
        Want::All(vec![
            Want::StepIn(2, 5),
            Want::StepWithoutBuiltin(5),
            Want::Field(1, 48, Val::F(9.0)),
        ]),
        Want::All(vec![
            Want::FieldChanged(2, 10),
            Want::EdictChanged(2),
            Want::Step(6),
        ]),
        Want::Exit,
        Want::Error(3, 7, "boom"),
    ]))
    .unwrap();
    // One record is matched by one want: two wants need two records.
    assert!(ok(Check::Seq(vec![Want::StepIn(2, 5), Want::Step(5)])).is_err());
    assert!(ok(Check::Seq(vec![Want::StepWithoutBuiltin(6)])).is_err());
    assert!(ok(Check::Seq(vec![Want::Error(3, 8, "boom")])).is_err());
}

#[test]
fn counts_absences_and_the_last_record() {
    let trace = sample();
    let outcome = outcome_of(&trace);
    let ok = |check: Check| verify(&case(vec![check]), &outcome);
    ok(Check::Count(Want::Exit, 1)).unwrap();
    assert!(
        ok(Check::Count(Want::Exit, 2))
            .unwrap_err()
            .contains("expected 2")
    );
    ok(Check::Never(Want::Run(9))).unwrap();
    assert!(ok(Check::Never(Want::Run(2))).is_err());
    ok(Check::Last(Want::ErrorMessage("boom"))).unwrap();
    assert!(ok(Check::Last(Want::Exit)).is_err());
}

#[test]
fn values_are_read_through_the_trace() {
    let trace = sample();
    let outcome = outcome_of(&trace);
    let ok = |check: Check| verify(&case(vec![check]), &outcome);
    ok(Check::FinalGlobal(34, Val::Text("hello"))).unwrap();
    ok(Check::FinalGlobal(34, Val::EngineText("hello"))).unwrap();
    assert!(ok(Check::FinalGlobal(34, Val::Text("bye"))).is_err());
    ok(Check::FinalGlobal(28, Val::Entity(1))).unwrap();
    assert!(ok(Check::FinalGlobal(28, Val::Entity(2))).is_err());
    ok(Check::FinalField(2, 10, Val::Bits(0x8000_0000))).unwrap();
    ok(Check::FinalField(2, 11, Val::F(0.0))).unwrap();
    // A global the trace never changes keeps its value in the progs: noop's index.
    ok(Check::FinalGlobal(global::START_FRAME, Val::Bits(1))).unwrap();
    // The string table: offset 1 holds "noop", which no engine string is.
    let mut table = case(vec![Check::FinalGlobal(
        global::START_FRAME,
        Val::Text("noop"),
    )]);
    table.program.set(global::START_FRAME, 1);
    verify(&table, &outcome).unwrap();
    table.checks = vec![Check::FinalGlobal(
        global::START_FRAME,
        Val::EngineText("noop"),
    )];
    assert!(verify(&table, &outcome).is_err());
    // A value outside the table that no string record names is no text.
    let unknown = case(vec![Check::FinalGlobal(28, Val::Text("hello"))]);
    assert!(verify(&unknown, &outcome).is_err());
}

#[test]
fn the_header_prefix_and_second_run_are_checked() {
    let program = Program::new();
    let crc = crc_block(&program.bytes().unwrap());
    let trace = Trace {
        header: Header::new(crc, 536, 96, 110, 1000).unwrap(),
        records: vec![exit()],
    };
    let mut outcome = outcome_of(&trace);
    let header = |entityfields| Check::Header {
        entityfields,
        numstrings: 1000,
    };
    verify(&case(vec![header(110)]), &outcome).unwrap();
    assert!(verify(&case(vec![header(105)]), &outcome).is_err());
    verify(&case(vec![Check::Prefix(b"QCTR".to_vec())]), &outcome).unwrap();
    assert!(verify(&case(vec![Check::Prefix(b"QCTX".to_vec())]), &outcome).is_err());
    assert!(verify(&case(vec![Check::SameTwice]), &outcome).is_err());
    outcome.again.clone_from(&outcome.trace);
    verify(&case(vec![Check::SameTwice]), &outcome).unwrap();
    outcome.stdout = "depth=0 localstack=0\n".to_owned();
    verify(&case(vec![Check::Stdout("depth=0")]), &outcome).unwrap();
    assert!(verify(&case(vec![Check::Stdout("depth=1")]), &outcome).is_err());
}

#[test]
fn exit_statuses_and_missing_traces_are_checked() {
    let refused = Outcome {
        status: Some(2),
        stderr: "vm-oracle: case.scn:2: frames takes an integer".to_owned(),
        ..Outcome::default()
    };
    verify(&case(vec![Check::Exit(2, "frames takes")]), &refused).unwrap();
    assert!(verify(&case(vec![Check::Exit(2, "map takes")]), &refused).is_err());
    assert!(verify(&case(vec![Check::Exit(3, "")]), &refused).is_err());
    // A status other than 0 needs an exit check.
    assert!(verify(&case(vec![Check::Never(Want::Exit)]), &refused).is_err());
    // A refusal writes no trace.
    let mut traced = refused;
    traced.trace = Some(encode(&trace(Vec::new())));
    let error = verify(&case(vec![Check::Exit(2, "")]), &traced).unwrap_err();
    assert!(error.contains("wrote a trace"), "{error}");
    // A check of the records needs a trace.
    let untraced = Outcome {
        status: Some(0),
        ..Outcome::default()
    };
    assert!(verify(&case(vec![Check::Never(Want::Exit)]), &untraced).is_err());
    assert!(verify(&case(vec![Check::Prefix(Vec::new())]), &untraced).is_err());
}

#[test]
fn a_trace_must_decode_strictly_and_encode_back() {
    let mut bytes = encode(&sample());
    let check = case(vec![Check::Never(Want::Run(9))]);
    let decoded = Outcome {
        status: Some(0),
        trace: Some(bytes.clone()),
        ..Outcome::default()
    };
    verify(&check, &decoded).unwrap();
    bytes.truncate(10);
    let truncated = Outcome {
        status: Some(0),
        trace: Some(bytes),
        ..Outcome::default()
    };
    assert!(
        verify(&check, &truncated)
            .unwrap_err()
            .contains("does not decode")
    );
}

#[test]
fn a_case_names_its_inputs() {
    match scenario("frames 1\n") {
        Input::Scenario { text, files } => {
            assert_eq!(text, b"progs progs.dat\nframes 1\n");
            assert!(files.is_empty());
        }
        other => panic!("{other:?}"),
    }
    match scenario_with_map("", "{}") {
        Input::Scenario { text, files } => {
            assert!(text.ends_with(b"entities map.ent\n"));
            assert_eq!(files, [("map.ent".to_owned(), b"{}".to_vec())]);
        }
        other => panic!("{other:?}"),
    }
    match raw_scenario(b"x", &[("a/b", b"c")]) {
        Input::Scenario { text, files } => {
            assert_eq!(text, b"x");
            assert_eq!(files, [("a/b".to_owned(), b"c".to_vec())]);
        }
        other => panic!("{other:?}"),
    }
}
