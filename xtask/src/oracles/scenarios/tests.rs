// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap, index or match is a failed test"
)]

use std::collections::{BTreeMap, BTreeSet};

use qc_progs::crc::crc_block;
use qc_progs::{Progs, ReadOptions};
use qc_trace::{Changes, FieldChange, GlobalChange, Header, Record, Step, Text, Trace, encode};

use super::super::testing::{args, fake_oracles, fresh_dir, last_arg};
use super::*;
use crate::exec::{Fake, Reply};

/// `xtask/fixtures/oracles/scenario-unrecorded.json`: the cases `record.py` left out because
/// their traces are longer than its `MAX_SCENARIO_TRACE`, as it wrote them.
fn unrecorded_cases() -> Vec<String> {
    let path =
        super::super::testing::root().join("xtask/fixtures/oracles/scenario-unrecorded.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn unhex(value: &serde_json::Value) -> Option<Vec<u8>> {
    let hex = value.as_str()?;
    Some(
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(hex.get(i..i.saturating_add(2)).unwrap(), 16).unwrap())
            .collect(),
    )
}

/// `xtask/fixtures/oracles/scenario-outcomes.json`: what the VM oracle gave for each case
/// (`record.py`), by name, with the repository's path put back in its messages, and the
/// digest of the inputs it was given.
fn recorded_outcomes() -> BTreeMap<String, (String, Outcome)> {
    let root = super::super::testing::root();
    let path = root.join("xtask/fixtures/oracles/scenario-outcomes.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let text = |value: &serde_json::Value| {
        let root = root.display().to_string();
        value.as_str().unwrap().replace("<root>", &root)
    };
    let mut outcomes = BTreeMap::new();
    for (name, entry) in json.as_object().unwrap() {
        let status = entry["status"]
            .as_i64()
            .map(|code| i32::try_from(code).unwrap());
        let outcome = Outcome {
            status,
            stdout: text(&entry["stdout"]),
            stderr: text(&entry["stderr"]),
            trace: unhex(&entry["trace"]),
            again: unhex(&entry["again"]),
        };
        let inputs = entry["inputs"].as_str().unwrap().to_owned();
        outcomes.insert(name.clone(), (inputs, outcome));
    }
    outcomes
}

/// What `record.py` digests: the case's progs.dat, then its scenario or init file.
fn inputs_digest(case: &Case) -> String {
    let mut bytes = match &case.progs {
        Some(progs) => progs.clone(),
        None => case.program.bytes().unwrap(),
    };
    match &case.input {
        Input::Scenario { text, .. } | Input::Relative(text) => bytes.extend_from_slice(text),
        Input::Vector { init } => bytes.extend_from_slice(init.as_bytes()),
        Input::Args(_) => {}
    }
    super::super::sha256::hex_digest(&bytes)
}

#[test]
fn every_case_passes_on_what_the_oracle_gave() {
    let mut outcomes = recorded_outcomes();
    let mut unrecorded = Vec::new();
    for case in cases::all() {
        match outcomes.remove(&case.name) {
            Some((inputs, outcome)) => {
                assert_eq!(
                    inputs_digest(&case),
                    inputs,
                    "{}: the case changed since record.py ran",
                    case.name
                );
                if let Err(problem) = verify(&case, &outcome) {
                    panic!("{}: {problem}", case.name);
                }
            }
            None => unrecorded.push(case.name),
        }
    }
    assert!(outcomes.is_empty(), "no case for {:?}", outcomes.keys());
    // The outcomes were recorded on macOS, which has no `/dev/full` and so not these cases.
    if cfg!(target_os = "linux") {
        unrecorded.retain(|name| !name.ends_with("trace-to-a-full-device"));
    }
    unrecorded.sort();
    assert_eq!(unrecorded, unrecorded_cases());
}

#[test]
fn the_progs_keeps_the_header_crc_the_program_gives() {
    let mut p = Program::new();
    p.crc = 1234;
    let bytes = p.bytes().unwrap();
    let options = ReadOptions {
        check_header_crc: false,
    };
    assert_eq!(
        Progs::read_with(&bytes, &options).unwrap().header().crc,
        1234
    );
}

#[test]
fn a_function_reference_holds_the_function_index() {
    let mut p = Program::new();
    let slot = p.fref(7);
    assert_eq!(p.initial(slot), 7);
}

// spec DIFF-3.1: each header refusal changes the word its name says: a table before the file
// has a negative offset, and a misaligned one an offset inside the file that is not a multiple
// of its record's alignment.
#[test]
fn each_header_refusal_breaks_what_its_name_says() {
    let good = Program::new().bytes().unwrap();
    let word = |bytes: &[u8], offset: usize| {
        i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    };
    let cases: BTreeMap<String, Case> = cases::all()
        .into_iter()
        .map(|case| (case.name.clone(), case))
        .collect();
    let progs = |name: &str| cases[name].progs.clone().unwrap();
    assert!(word(&progs("progs-statements-before-the-file"), 8) < 0);
    let len = i32::try_from(good.len()).unwrap();
    for (name, offset) in [
        ("progs-statements-misaligned", 8),
        ("progs-globaldefs-misaligned", 16),
        ("progs-fielddefs-misaligned", 24),
        ("progs-functions-misaligned", 32),
        ("progs-globals-misaligned", 48),
    ] {
        let value = word(&progs(name), offset);
        assert!(value > 0 && value < len, "{name}: {value}");
        assert_eq!(
            value,
            word(&good, offset) - if offset == 8 { 1 } else { 2 },
            "{name}"
        );
    }
    assert_eq!(word(&progs("progs-negative-entityfields"), 56), -1);
}

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
fn a_record_matches_only_when_every_part_matches() {
    let program = Program::new();
    let reader = Reader {
        program: &program,
        trace: None,
    };
    let matches = |record: &Record, want: Want| reader.matches(0, record, &want);
    let s = step(4, 7, None, &[(2, 10, 5)]);
    assert!(matches(&s, Want::StepIn(4, 7)));
    assert!(!matches(&s, Want::StepIn(4, 8)));
    assert!(!matches(&s, Want::StepIn(5, 7)));
    assert!(matches(&s, Want::Field(2, 10, Val::Bits(5))));
    assert!(!matches(&s, Want::Field(3, 10, Val::Bits(5))));
    assert!(!matches(&s, Want::Field(2, 11, Val::Bits(5))));
    assert!(!matches(&s, Want::Field(2, 10, Val::Bits(6))));
    let r = run(4, &[(30, 9)]);
    assert!(matches(&r, Want::Global(30, Val::Bits(9))));
    assert!(!matches(&r, Want::Global(31, Val::Bits(9))));
    assert!(!matches(&r, Want::Global(30, Val::Bits(8))));
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
    let error = verify(&case(vec![Check::Never(Want::Exit)]), &untraced).unwrap_err();
    assert_eq!(error, "no trace was written");
    let error = verify(&case(vec![Check::SameTwice]), &untraced).unwrap_err();
    assert_eq!(error, "no trace was written");
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

// --- Running cases, against a fake VM oracle ---

#[test]
fn checking_runs_every_case_and_counts_the_failures() {
    let work = fresh_dir("scenarios-check");
    let fake = Fake::new(|_: &std::process::Command| Reply::exit(0));
    let oracles = fake_oracles(&work, &fake);
    // A run that exits 0 and writes no trace fails every case that reads the trace.
    assert!(!check(&oracles, None).unwrap());
    let cases = all_cases();
    let twice = cases
        .iter()
        .filter(|case| case.checks.contains(&Check::SameTwice))
        .count();
    assert_eq!(fake.seen.lock().unwrap().len(), cases.len() + twice);
    assert!(check(&oracles, Some(&["no-such-case"])).is_err());
    assert!(
        check(&oracles, Some(&[][..])).is_ok(),
        "no names is every case"
    );
}

#[test]
fn a_refusal_passes_an_oracle_that_refuses_with_its_message() {
    let work = fresh_dir("scenarios-refusal");
    let fake = Fake::new(|_: &std::process::Command| Reply {
        code: 2,
        stderr: "vm-oracle: case.scn:2: frames takes an integer".to_owned(),
        ..Reply::default()
    });
    let oracles = fake_oracles(&work, &fake);
    assert!(check(&oracles, Some(&["frames-too-many"])).unwrap());
    let seen = fake.seen.lock().unwrap();
    assert!(seen[0].starts_with("vm-oracle scenario "), "{}", seen[0]);
    assert!(seen[0].ends_with("trace.bin"), "{}", seen[0]);
    assert!(work.join("scenarios/frames-too-many/case.scn").is_file());
    assert!(work.join("scenarios/frames-too-many/progs.dat").is_file());
}

#[test]
fn a_case_reads_the_trace_the_oracle_wrote() {
    let work = fresh_dir("scenarios-trace");
    let records = vec![
        run(WORLDSPAWN, &[]),
        exit(),
        run(NOOP, &[]),
        exit(),
        run(NOOP, &[]),
        exit(),
    ];
    let fake = Fake::new(|command: &std::process::Command| Reply {
        files: vec![(last_arg(command), encode(&trace(records.clone())))],
        ..Reply::default()
    });
    let oracles = fake_oracles(&work, &fake);
    assert!(check(&oracles, Some(&["default-entities"])).unwrap());
    // A map file is written where the scenario names it.
    assert!(!check(&oracles, Some(&["entities-file"])).unwrap());
    assert!(work.join("scenarios/entities-file/map.ent").is_file());
    // A second run, for a case that compares two, writes the trace again.
    assert!(!check(&oracles, Some(&["deterministic"])).unwrap());
    assert_eq!(fake.seen.lock().unwrap().len(), 4);
}

#[test]
fn each_input_reaches_the_oracle_as_its_mode_says() {
    let work = fresh_dir("scenarios-modes");
    let fake = Fake::new(|command: &std::process::Command| {
        let cwd = command.get_current_dir().map(Path::to_path_buf);
        let args = args(command);
        let first = args.first().map(|arg| arg.to_string_lossy().into_owned());
        match first.as_deref() {
            Some("vector") => Reply {
                code: 2,
                stderr: "too short".to_owned(),
                ..Reply::default()
            },
            Some("--console") => Reply {
                stderr: "SERVER ERROR".to_owned(),
                files: vec![(last_arg(command), encode(&trace(vec![error("shown")])))],
                ..Reply::default()
            },
            Some("scenario")
                if cwd
                    .as_ref()
                    .is_some_and(|dir| dir.ends_with("relative-scenario-path")) =>
            {
                assert_eq!(args[1], PathBuf::from("case.scn"));
                Reply::exit(4)
            }
            _ => Reply {
                code: 2,
                stderr: "usage: vm-oracle".to_owned(),
                ..Reply::default()
            },
        }
    });
    let oracles = fake_oracles(&work, &fake);
    assert!(check(&oracles, Some(&["vector-progs-refused"])).unwrap());
    assert!(check(&oracles, Some(&["console-copied"])).unwrap());
    assert!(check(&oracles, Some(&["bad-command-line"])).unwrap());
    assert!(!check(&oracles, Some(&["relative-scenario-path"])).unwrap());
    // A trace in a missing directory: the oracle cannot open it, as the fake says.
    assert!(!check(&oracles, Some(&["unwritable-trace"])).unwrap());
}

#[test]
fn a_trace_on_a_device_is_neither_removed_nor_read() {
    let work = fresh_dir("scenarios-device");
    let device = work.join("full-device");
    std::fs::write(&device, b"kept").unwrap();
    let fake = Fake::new(|_: &std::process::Command| Reply {
        code: 3,
        stderr: "vm-oracle: cannot write".to_owned(),
        ..Reply::default()
    });
    let oracles = fake_oracles(&work, &fake);
    let mut case = Case::new("device", Program::new(), scenario("frames 1\n"))
        .check(Check::Exit(3, "cannot write"));
    let leaked: &'static str = Box::leak(device.display().to_string().into_boxed_str());
    case.trace = TracePath::Device(leaked);
    let outcome = super::run(&oracles, &work, &case).unwrap();
    assert_eq!(outcome.trace, None);
    verify(&case, &outcome).unwrap();
    assert_eq!(std::fs::read(&device).unwrap(), b"kept");
}
