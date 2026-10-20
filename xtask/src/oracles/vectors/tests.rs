// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code: a failed unwrap, index or panic is a failed test"
)]

use super::{
    End, Head, HostAction, Known, Target, Value, Vector, Verdict, build_progs, init_text, matches,
    parse, parse_host_action, parse_strings, parse_value, verdict,
};
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use qc_progs::crc::crc_block;
use qc_trace::{Changes, GlobalChange, Header, Record, Step, Text, Trace, encode};

use super::super::testing::{fake_oracles, fresh_dir, last_arg, root};
use super::{check, run, vector_dir};
use crate::exec::{Fake, Reply};

fn vector(spec: &str) -> Vector {
    Vector {
        spec: vec![spec.to_owned()],
        ..Vector::default()
    }
}

// spec DIFF-10.1: a VM-9.2 vector that fails on a host that is not x86-64 is an expected
// divergence, reported as such; on x86-64 it is a failure.
#[test]
fn an_out_of_range_conversion_is_expected_to_diverge_only_off_x86() {
    let bitand = vector("VM-9.2");
    let failure = || Err("line 1".to_owned());
    assert_eq!(
        verdict(&bitand, failure(), false),
        Verdict::Expected("line 1".to_owned())
    );
    assert_eq!(
        verdict(&bitand, failure(), true),
        Verdict::Failed("line 1".to_owned())
    );
    assert_eq!(verdict(&bitand, Ok(()), false), Verdict::Passed);
}

// spec DIFF-10.3: any divergence DIFF-10.1 does not list is a failure, on every host.
#[test]
fn any_other_divergence_is_a_failure() {
    for x86_64 in [false, true] {
        assert_eq!(
            verdict(&vector("VM-4.1"), Err("line 2".to_owned()), x86_64),
            Verdict::Failed("line 2".to_owned())
        );
    }
}

// spec DIFF-10.2: NaN payloads, the edict layout and engine string numbers are no
// divergence: a value is compared through its reading.
#[test]
fn values_are_compared_through_their_readings() {
    let layouts = [
        Header::new(0, 516, 96, 105, 1000).unwrap(),
        Header::new(0, 528, 108, 105, 1000).unwrap(),
    ];
    for header in &layouts {
        let known = Known {
            header,
            engine_strings: [2000, 70000].into(),
        };
        let size = header.edict_size();
        assert!(matches(&known, Value::AnyNan, 0x7fc0_0000));
        assert!(matches(&known, Value::AnyNan, 0xffc0_0001));
        assert!(!matches(&known, Value::AnyNan, 0x7f80_0000));
        assert!(matches(&known, Value::Entity(2), size * 2));
        assert!(!matches(&known, Value::Entity(2), size * 3));
        let nextthink = size * 2 + header.field_offset() + 4 * 46;
        assert!(matches(&known, Value::Pointer(2, 46), nextthink));
        assert!(matches(&known, Value::EngineString, 2000));
        assert!(matches(&known, Value::EngineString, 70000));
        assert!(!matches(&known, Value::EngineString, 500));
        assert!(matches(&known, Value::Bits(0x8000_0000), 0x8000_0000));
        assert!(!matches(&known, Value::Bits(0x8000_0000), 0));
    }
}

#[test]
fn parses_values_as_the_readme_defines_them() {
    assert_eq!(parse_value("1.5", false).unwrap(), Value::Bits(0x3fc0_0000));
    assert_eq!(parse_value("-0", false).unwrap(), Value::Bits(0x8000_0000));
    assert_eq!(parse_value("nan", false).unwrap(), Value::AnyNan);
    assert_eq!(parse_value("nan", true).unwrap(), Value::Bits(0x7fc0_0000));
    assert_eq!(parse_value("i-1", false).unwrap(), Value::Bits(u32::MAX));
    assert_eq!(parse_value("e2", false).unwrap(), Value::Entity(2));
    assert_eq!(
        parse_value("p1.105", false).unwrap(),
        Value::Pointer(1, 105)
    );
    assert_eq!(parse_value("es", false).unwrap(), Value::EngineString);
    assert!(parse_value("es", true).is_err());
    assert!(parse_value("x", false).is_err());
}

#[test]
fn parses_strings_with_escapes() {
    let strings = parse_strings(r#""abc" "" "\x80\x7fz""#).unwrap();
    assert_eq!(
        strings,
        vec![b"abc".to_vec(), Vec::new(), vec![0x80, 0x7f, b'z']]
    );
    assert!(parse_strings(r#""open"#).is_err());
}

// spec TRACE-4.3: a CALL step that called a builtin records `-first_statement` of the function
// it called; only numbers 1 to 78 are callable builtins (BUILTIN-1.1), and a call to QC code
// records none.
#[test]
fn a_call_expects_the_builtin_it_calls() {
    let text = "== v\nfunctions:\n  2: first=0\n  3: builtin=79\n  4: builtin=78\n  5: first=1\n\
                code:\n  CALL0 120\n  ADD_F 1 2 3\n";
    let v = parse("t.txt", text).unwrap().pop().unwrap();
    let call = |function: u32| {
        let globals = BTreeMap::from([(120, function)]);
        super::expected_builtin(&v, &globals, 1)
    };
    assert_eq!(call(2), None);
    assert_eq!(call(3), None);
    assert_eq!(call(4), Some(78));
    assert_eq!(call(5), None);
    assert_eq!(call(9), None);
    let globals = BTreeMap::from([(120, 4)]);
    assert_eq!(super::expected_builtin(&v, &globals, 2), None);
    assert_eq!(super::expected_builtin(&v, &globals, 0), None);
    assert_eq!(super::expected_builtin(&v, &globals, 3), None);
}

/// Function 1 starts at statement 1 unless the vector declares it
/// (`docs/spec/vectors/vm/README.md`).
#[test]
fn a_function_starts_where_the_vector_says() {
    let v = parse("t.txt", "== v\nfunctions:\n  2: first=5\n")
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(super::first_statement(&v, 2), 5);
    assert_eq!(super::first_statement(&v, 1), 1);
    assert_eq!(super::first_statement(&v, 3), 0);
    assert_eq!(super::first_statement(&v, -1), 0);
}

#[test]
fn parses_a_vector() {
    let text = "== v\nspec: VM-1.1\nglobals: 100=e1\nfields: e1.30=2\noracle: no\n\
                code:\n  STATE 100 101\n  66\nexpect:\n  1 STATE e1.30=nan\n  error\n";
    let vectors = parse("t.txt", text).unwrap();
    let v = vectors.first().unwrap();
    assert!(!v.oracle);
    assert!(!v.engine_builtins);
    assert_eq!(v.code.len(), 2);
    assert_eq!(v.initial.first().unwrap().0, Target::Global(100));
    assert_eq!(
        v.lines.first().unwrap().changes.first().unwrap().0,
        Target::Field(1, 30)
    );
    assert_eq!(v.end, Some(End::Error(None)));
    assert!(parse("t.txt", "loose\n").is_err());
    assert!(parse("t.txt", "== v\nwhat: 1\n").is_err());
    let engine = parse("t.txt", "== v\nbuiltins: engine\n").unwrap();
    assert!(engine.first().unwrap().engine_builtins);
    assert!(parse("t.txt", "== v\nbuiltins: stubs\n").is_err());
}

#[test]
fn parses_host_actions() {
    assert_eq!(
        parse_host_action("clock 1.2000000000000002").unwrap(),
        HostAction::Clock(1.200_000_000_000_000_2)
    );
    assert_eq!(parse_host_action("run 3").unwrap(), HostAction::Run(3));
    assert_eq!(
        parse_host_action("think e1 0.1").unwrap(),
        HostAction::Think(1, 0.1)
    );
    assert_eq!(
        parse_host_action("push e2 0.1").unwrap(),
        HostAction::Push(2, 0.1)
    );
    assert_eq!(
        parse_host_action("physics 0.1").unwrap(),
        HostAction::Physics(0.1, 1)
    );
    assert_eq!(
        parse_host_action("frame 0.1 100").unwrap(),
        HostAction::Frame(0.1, 100)
    );
    assert_eq!(
        parse_host_action("edict -1 100").unwrap(),
        HostAction::Edict(-1, 100)
    );
    for bad in [
        "",
        "sleep 1",
        "clock",
        "clock soon",
        "think 1 0.1",
        "push e1",
        "frame 0.1 0",
        "physics 0.1 -1",
        "edict 1 -1",
        "run 1 2",
    ] {
        assert!(parse_host_action(bad).is_err(), "{bad:?} parsed");
    }
}

#[test]
fn parses_a_host_vector() {
    let text = "== v\nspec: DIFF-5.6\nedicts: 2\nmaxclients: 1\nseed: 7\n\
                datagram: 64 255\nhost:\n  clock 1\n  run 1\ncode:\n  DONE\n\
                expect:\n  run 1 31=1\n  1 DONE\n  done\n";
    let vectors = parse("t.txt", text).unwrap();
    let v = vectors.first().unwrap();
    assert_eq!(v.edicts, Some(2));
    assert_eq!(v.maxclients, Some(1));
    assert_eq!(v.seed, Some(7));
    assert_eq!(v.datagram, Some(vec![64, 255]));
    assert_eq!(v.host, [HostAction::Clock(1.0), HostAction::Run(1)]);
    assert_eq!(v.lines.first().unwrap().head, Head::Run(1));
    assert_eq!(
        v.lines.last().unwrap().head,
        Head::Step {
            statement: 1,
            opcode: 0
        }
    );
    let init = init_text(v);
    assert!(init.starts_with("edicts 2\nmaxclients 1\nseed 7\n"));
    assert!(init.contains("host clock 1\nhost run 1\n"));
    assert!(parse("t.txt", "== v\ndatagram: 256\n").is_err());
    assert!(parse("t.txt", "== v\nhost:\n  wait\n").is_err());
    assert!(parse("t.txt", "== v\nexpect:\n  run x\n").is_err());
    let plain = parse("t.txt", "== v\ncode:\n  DONE\n").unwrap();
    assert!(init_text(plain.first().unwrap()).starts_with("edicts 3\nentry 1\n"));
}

#[test]
fn rejects_values_outside_32_bits() {
    assert_eq!(
        parse_value("0xffffffff", true).unwrap(),
        Value::Bits(u32::MAX)
    );
    assert!(parse_value("0x100000000", true).is_err());
    assert_eq!(
        parse_value("i-2147483648", true).unwrap(),
        Value::Bits(0x8000_0000)
    );
    assert!(parse_value("i2147483648", true).is_err());
    assert!(parse_value("p1", true).is_err());
    assert!(parse_value("e", true).is_err());
    assert_eq!(parse_value("inf", true).unwrap(), Value::Bits(0x7f80_0000));
}

#[test]
fn rejects_malformed_string_escapes_and_trailing_text() {
    assert!(parse_strings(r#""\x4""#).is_err());
    assert!(parse_strings(r#""\xzz""#).is_err());
    assert!(parse_strings(r#""a" b"#).is_err());
    assert_eq!(parse_strings("").unwrap(), Vec::<Vec<u8>>::new());
    let hash = parse("t.txt", "== v\nstrings: \"a#b\"\n").unwrap();
    assert_eq!(hash.first().unwrap().strings, vec![b"a#b".to_vec()]);
}

#[test]
fn keeps_a_quoted_error_message_and_a_bare_error() {
    let quoted = parse("t.txt", "== v\nexpect:\n  error \"NULL function\"\n").unwrap();
    assert_eq!(
        quoted.first().unwrap().end,
        Some(End::Error(Some("NULL function".to_owned())))
    );
    let bare = parse("t.txt", "== v\nexpect:\n  error\n").unwrap();
    assert_eq!(bare.first().unwrap().end, Some(End::Error(None)));
}

#[test]
fn keeps_a_well_formed_after_line() {
    let vectors = parse("t.txt", "== v\nafter: depth=0 localstack=2\n").unwrap();
    assert_eq!(vectors.first().unwrap().after, Some((0, 2)));
    assert!(parse("t.txt", "== v\nafter: stack=2\n").is_err());
}

#[test]
fn refuses_a_malformed_error_line() {
    for bad in ["error \"open", "errors", "error \"a # b\""] {
        let text = format!("== v\nexpect:\n  {bad}\n");
        assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
    }
    for bad in ["error \"a\" b", "error \"a\"b\"", "error x", "error \""] {
        let text = format!("== v\nexpect:\n  {bad}\n");
        assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
    }
    let empty = parse("t.txt", "== v\nexpect:\n  error \"\"\n").unwrap();
    assert_eq!(
        empty.first().unwrap().end,
        Some(End::Error(Some(String::new())))
    );
}

#[test]
fn refuses_a_malformed_after_line() {
    for bad in ["depth=x localstack=2", "depth=0", "localstack=2"] {
        let text = format!("== v\nafter: {bad}\n");
        assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
    }
    for bad in [
        "",
        "localstack=2 depth=0",
        "depth=0 localstack=2 depth=1",
        "depth=0 localstack=y",
        "depth=0 localstack=",
        "depth0 localstack=2",
        "depth=99999999999 localstack=2",
    ] {
        let text = format!("== v\nafter: {bad}\n");
        assert!(parse("t.txt", &text).is_err(), "{bad:?} parsed");
    }
}

#[test]
fn parses_every_vector_file() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(super::VECTORS);
    let mut count = 0_usize;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "txt") {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).unwrap();
            count = count.saturating_add(parse(&name, &text).unwrap().len());
        }
    }
    assert!(count > 0, "no vectors in {}", dir.display());
}

#[test]
fn refuses_text_after_the_operands() {
    assert!(parse("t.txt", "== v\ncode:\n  ADD_F 1 2 3 4\n").is_err());
    assert!(parse("t.txt", "== v\ncode:\n  ADD_F 1 2 3\n").is_ok());
}

#[test]
fn rejects_bad_operands_and_function_fields() {
    assert!(parse("t.txt", "== v\ncode:\n  ADD_F 1 x 3\n").is_err());
    assert!(parse("t.txt", "== v\ncode:\n  NOT_AN_OPCODE 1\n").is_err());
    assert!(parse("t.txt", "== v\nfunctions:\n  2: first=1 colour=3\n").is_err());
    assert!(parse("t.txt", "== v\nfunctions:\n  2: builtin=-2147483648\n").is_err());
    assert!(parse("t.txt", "== v\nfunctions:\n  x: first=1\n").is_err());
}

// spec LIMIT-1.5: a vector may ask for up to LIMIT_MAX_DEF_SLOT + 1 global slots and function
// indices below LIMIT_QCC_MAX_FUNCTIONS; one past either is malformed, refused before the
// progs is built.
#[test]
fn bounds_the_global_count_and_function_indices() {
    let globals = parse("t.txt", "== v\nnumglobals: 65536\n").unwrap();
    assert_eq!(build_progs(globals.first().unwrap()).map(|p| p.is_empty()), Ok(false));
    assert!(parse("t.txt", "== v\nnumglobals: 65537\n").is_err());
    assert!(parse("t.txt", "== v\nnumglobals: 0\n").is_ok());
    let functions = parse("t.txt", "== v\nfunctions:\n  8191: first=1\n").unwrap();
    assert!(build_progs(functions.first().unwrap()).is_ok());
    assert!(parse("t.txt", "== v\nfunctions:\n  8192: first=1\n").is_err());
}

// spec LIMIT-1.5: the xtask-vectors fuzz target's out-of-memory input (Fuzz run 39332527448),
// `numglobals:01111111111`, which built a progs of 1111111111 global slots; it is now refused.
#[test]
fn regression_fuzz_numglobals_out_of_memory() {
    let input = b"== %\x00\x00,3\x0a\x0a\x0anumglobals:01111111111 \x0a\x0a \x0a== \x00 \x0a \x0a \x0a \x0a== \x00 \x0a\x0a== \x00 \x0a\x0a \x0a== \x00 \x0a \x0a== \x00 == \x00  \x0a \x0a== \x00 \x0a==  \x00 \x0a \x0a \x0a \x0a== \x00 \x0a\x0a== \x00 \x0a \x0a== \x00 \x0a\x0a== \x00 \x0a\x0a \x0a== \x00 \x0a \x0a== \x0011111111111r\x02\x00;\x00\x00\x00\x00\x00\x00";
    let text = std::str::from_utf8(input).unwrap();
    let error = parse("fuzz.txt", text).unwrap_err();
    assert!(error.contains("bad numglobals"), "{error}");
}

// --- Running vectors, against a fake that replays what the VM oracle wrote ---

/// `xtask/fixtures/oracles/vector-traces.json`: the trace and standard output of each vector
/// (`record.py`), by `<file>/<vector>`.
fn recorded() -> BTreeMap<String, (Vec<u8>, String)> {
    let path = root().join("xtask/fixtures/oracles/vector-traces.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut recorded = BTreeMap::new();
    for (key, entry) in json.as_object().unwrap() {
        let hex = entry["trace"].as_str().unwrap();
        let trace = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(hex.get(i..i + 2).unwrap(), 16).unwrap())
            .collect();
        let stdout = entry["stdout"].as_str().unwrap().to_owned();
        recorded.insert(key.clone(), (trace, stdout));
    }
    recorded
}

/// The fake VM oracle: writes the recorded trace and output of the vector whose directory
/// the trace goes to, or nothing for a vector with none recorded.
fn replay(recorded: &BTreeMap<String, (Vec<u8>, String)>, command: &Command) -> Reply {
    let trace = last_arg(command);
    let dir = trace.parent().unwrap();
    let key = format!(
        "{}/{}",
        dir.parent().unwrap().file_name().unwrap().to_string_lossy(),
        dir.file_name().unwrap().to_string_lossy()
    );
    match recorded.get(&key) {
        Some((bytes, stdout)) => Reply {
            stdout: stdout.clone(),
            files: vec![(trace, bytes.clone())],
            ..Reply::default()
        },
        None => Reply::default(),
    }
}

fn all_vectors() -> Vec<Vector> {
    let dir = root().join(super::VECTORS);
    let mut vectors = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "txt"))
        .collect();
    files.sort();
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        vectors.extend(parse(&name, &std::fs::read_to_string(&path).unwrap()).unwrap());
    }
    vectors
}

// The vector checker read against what the VM oracle wrote for every vector (recorded on
// macOS arm64, where the two VM-9.2 vectors diverge as DIFF-10.1 says).
#[test]
fn every_recorded_trace_passes_its_vector() {
    let work = fresh_dir("vectors-replay");
    let recorded = recorded();
    let fake = Fake::new(|command: &Command| replay(&recorded, command));
    let oracles = fake_oracles(&work, &fake);
    let mut replayed = 0_usize;
    for vector in all_vectors().iter().filter(|vector| vector.oracle) {
        let key = vector_dir(Path::new(""), vector);
        let key = key.to_string_lossy().replace('\\', "/");
        let result = run(&oracles, &work, vector);
        if !recorded.contains_key(&key) {
            assert!(result.is_err(), "{key} has no trace to read");
        } else if ["bitwise/bitand_out_of_range", "bitwise/bitor_out_of_range"]
            .contains(&key.as_str())
        {
            assert!(result.unwrap_err().contains("line 1"), "{key}");
            replayed = replayed.saturating_add(1);
        } else {
            result.unwrap_or_else(|e| panic!("{key}: {e}"));
            replayed = replayed.saturating_add(1);
        }
    }
    assert_eq!(replayed, recorded.len());
    assert!(work.join("flow/bad_opcode/stdout.txt").is_file());
}

#[test]
fn checking_every_vector_counts_the_ones_without_a_trace_as_failures() {
    let work = fresh_dir("vectors-check");
    let recorded = recorded();
    let fake = Fake::new(|command: &Command| replay(&recorded, command));
    let oracles = fake_oracles(&work, &fake);
    assert!(!check(&root(), &oracles).unwrap());
    let seen = fake.seen.borrow();
    assert!(seen.len() > 250, "{}", seen.len());
    assert!(
        seen.iter()
            .all(|line| line.starts_with("vm-oracle vector "))
    );
}

/// The vector every failure test below runs: one `STORE_F`, then `DONE`.
fn simple() -> Vector {
    let text = "== simple\nglobals: 100=1\ncode:\n  STORE_F 100 101\n  DONE\nexpect:\n  \
                1 STORE_F 101=1\n  2 DONE\n  done\nafter: depth=0 localstack=0\ndatagram: 7\n";
    parse("t.txt", text).unwrap().pop().unwrap()
}

fn header(vector: &Vector) -> Header {
    let progs = super::build_progs(vector).unwrap();
    let numstrings =
        u32::try_from(qc_progs::Progs::read(&progs).unwrap().header().numstrings).unwrap();
    Header::new(crc_block(&progs), 548, 108, 110, numstrings).unwrap()
}

fn step(statement: i32, opcode: u16, globals: &[(u32, u32)]) -> Record {
    Record::Step(Step {
        function: 1,
        statement,
        opcode,
        builtin: None,
        changes: Changes::new(
            globals
                .iter()
                .map(|&(slot, bits)| GlobalChange { slot, bits })
                .collect(),
            Vec::new(),
        )
        .unwrap(),
    })
}

fn run_record(function: i32) -> Record {
    Record::Run {
        function,
        changes: Changes::default(),
    }
}

fn exit() -> Record {
    Record::Exit {
        changes: Changes::default(),
    }
}

/// The trace of `simple` as the VM oracle writes it.
fn simple_records() -> Vec<Record> {
    vec![
        run_record(1),
        step(1, 31, &[(101, 0x3f80_0000)]),
        step(2, 0, &[]),
        exit(),
    ]
}

/// What `run` makes of a fake oracle that writes `trace` and `stdout` and exits with `code`.
fn outcome(
    name: &str,
    vector: &Vector,
    trace: Option<Vec<u8>>,
    stdout: &str,
    code: i32,
) -> Result<(), String> {
    let work = fresh_dir(name);
    let fake = Fake::new(|command: &Command| Reply {
        code,
        stdout: stdout.to_owned(),
        files: trace
            .iter()
            .map(|bytes| (last_arg(command), bytes.clone()))
            .collect(),
        ..Reply::default()
    });
    let oracles = fake_oracles(&work, &fake);
    run(&oracles, &work, vector)
}

fn encoded(vector: &Vector, records: Vec<Record>) -> Option<Vec<u8>> {
    Some(encode(&Trace {
        header: header(vector),
        records,
    }))
}

const GOOD_STDOUT: &str = "depth=0 localstack=0\ndatagram=07\n";

#[test]
fn a_run_passes_when_the_trace_and_report_match() {
    let v = simple();
    outcome(
        "vectors-pass",
        &v,
        encoded(&v, simple_records()),
        GOOD_STDOUT,
        0,
    )
    .unwrap();
}

#[test]
fn a_run_fails_when_the_oracle_fails_or_writes_no_trace_it_can_read() {
    let v = simple();
    let good = encoded(&v, simple_records());
    let failed = outcome("vectors-exit", &v, good, GOOD_STDOUT, 2).unwrap_err();
    assert!(failed.contains("the VM oracle failed"), "{failed}");
    assert!(outcome("vectors-no-trace", &v, None, GOOD_STDOUT, 0).is_err());
    let garbage = outcome(
        "vectors-garbage",
        &v,
        Some(b"QCTX".to_vec()),
        GOOD_STDOUT,
        0,
    );
    assert!(garbage.unwrap_err().contains("does not decode"));
    let work = fresh_dir("vectors-cannot-run");
    let fake = Fake::new(|_: &Command| Reply {
        cannot_run: true,
        ..Reply::default()
    });
    let error = run(&fake_oracles(&work, &fake), &work, &v).unwrap_err();
    assert!(error.contains("cannot run the VM oracle"), "{error}");
}

#[test]
fn a_run_fails_on_a_header_that_does_not_describe_the_progs() {
    let v = simple();
    let right = header(&v);
    let crc = Some(encode(&Trace {
        header: Header::new(right.progs_crc() ^ 1, 548, 108, 110, right.numstrings()).unwrap(),
        records: simple_records(),
    }));
    let error = outcome("vectors-crc", &v, crc, GOOD_STDOUT, 0).unwrap_err();
    assert!(error.contains("not the file's CRC"), "{error}");
    let fields = Some(encode(&Trace {
        header: Header::new(right.progs_crc(), 548, 108, 105, right.numstrings()).unwrap(),
        records: simple_records(),
    }));
    let error = outcome("vectors-fields", &v, fields, GOOD_STDOUT, 0).unwrap_err();
    assert!(error.contains("does not describe the progs"), "{error}");
}

#[test]
fn a_run_fails_on_the_wrong_depth_or_datagram() {
    let v = simple();
    let good = encoded(&v, simple_records());
    let deeper = "depth=1 localstack=0\ndatagram=07\n";
    let depth = outcome("vectors-depth", &v, good.clone(), deeper, 0);
    assert!(depth.unwrap_err().contains("after the run"));
    let empty = "depth=0 localstack=0\ndatagram=\n";
    let datagram = outcome("vectors-datagram", &v, good, empty, 0);
    assert!(datagram.unwrap_err().contains("the datagram holds"));
}

#[test]
fn a_run_fails_on_records_out_of_order_or_unlike_the_vector() {
    let v = simple();
    let stored = || step(1, 31, &[(101, 0x3f80_0000)]);
    let exit_with_changes = Record::Exit {
        changes: Changes::new(vec![GlobalChange { slot: 5, bits: 1 }], Vec::new()).unwrap(),
    };
    let cases: [(&str, Vec<Record>, &str); 8] = [
        (
            "vectors-two-runs",
            [simple_records(), vec![run_record(1), exit()]].concat(),
            "a run after the entry run",
        ),
        (
            "vectors-step-outside",
            vec![stored()],
            "a step outside a run",
        ),
        (
            "vectors-exit-outside",
            vec![exit()],
            "an exit outside a run",
        ),
        (
            "vectors-exit-changes",
            vec![run_record(1), exit_with_changes],
            "an exit record with changes",
        ),
        (
            "vectors-unended",
            vec![run_record(1), stored()],
            "ends inside a run",
        ),
        ("vectors-no-run", Vec::new(), "has none"),
        (
            "vectors-wrong-value",
            vec![
                run_record(1),
                step(1, 31, &[(101, 0)]),
                step(2, 0, &[]),
                exit(),
            ],
            "line 1",
        ),
        (
            "vectors-short",
            vec![run_record(1), stored(), exit()],
            "lines in the trace",
        ),
    ];
    for (name, records, message) in cases {
        let error = outcome(name, &v, encoded(&v, records), GOOD_STDOUT, 0).unwrap_err();
        assert!(error.contains(message), "{name}: {error}");
    }
}

#[test]
fn a_run_reads_an_error_and_its_message() {
    let text = "== failing\ncode:\n  CALL0 100\nexpect:\n  1 CALL0\n  error \"NULL function\"\n";
    let v = parse("t.txt", text).unwrap().pop().unwrap();
    let error_record = |message: &str| Record::Error {
        function: 1,
        statement: 1,
        message: Text::new(message.as_bytes()).unwrap(),
        changes: Changes::default(),
    };
    let traced = |message: &str| encoded(&v, vec![run_record(1), error_record(message)]);
    outcome("vectors-error", &v, traced("NULL function"), "", 0).unwrap();
    let other = outcome("vectors-other-error", &v, traced("other"), "", 0);
    assert!(other.unwrap_err().contains("ended with"));
    let finished = encoded(&v, vec![run_record(1), step(1, 51, &[]), exit()]);
    assert!(outcome("vectors-done-not-error", &v, finished, "", 0).is_err());
}

#[test]
fn a_partial_vector_counts_its_statements() {
    let text = "== partial\ncode:\n  STORE_F 100 101\n  DONE\nsteps: 2\nexpect:\n  ...\n  \
                2 DONE\n  done\n";
    let v = parse("t.txt", text).unwrap().pop().unwrap();
    outcome("vectors-partial", &v, encoded(&v, simple_records()), "", 0).unwrap();
    let three = parse("t.txt", &text.replace("steps: 2", "steps: 3"))
        .unwrap()
        .pop()
        .unwrap();
    let error = outcome(
        "vectors-partial-count",
        &three,
        encoded(&three, simple_records()),
        "",
        0,
    );
    assert!(error.unwrap_err().contains("statements executed"));
}

#[test]
fn a_step_must_name_the_builtin_its_call_calls() {
    let text = "== builtin\nglobals: 100=i2\nfunctions:\n  2: builtin=5\ncode:\n  CALL0 100\n  \
                DONE\nexpect:\n  1 CALL0\n  2 DONE\n  done\n";
    let v = parse("t.txt", text).unwrap().pop().unwrap();
    let records = vec![run_record(1), step(1, 51, &[]), step(2, 0, &[]), exit()];
    let error = outcome("vectors-builtin", &v, encoded(&v, records), "", 0).unwrap_err();
    assert!(error.contains("TRACE-4.3"), "{error}");
}

#[test]
fn a_line_names_its_statement_and_a_run_none() {
    let v = Vector::default();
    let step = super::step_line(&v, 5, BTreeMap::new());
    assert_eq!(super::statement_of(&step), 5);
    let run = super::Line {
        head: Head::Run(3),
        changes: BTreeMap::new(),
    };
    assert_eq!(super::statement_of(&run), 0);
}
