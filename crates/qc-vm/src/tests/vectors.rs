// SPDX-License-Identifier: GPL-3.0-or-later

//! The VM vector runner (`docs/spec/vectors/vm/`, format in its README).
//!
//! It parses each vector, builds a progs.dat from its tables, runs it with the stub builtins
//! the README describes and the trace hook, and checks that every statement's change set
//! matches the recorded answer. The `// spec` comments pin the statements the vectors cover.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "test code: a failed unwrap, index, overflow or cast is a failed test"
)]

use qc_edict::EdictNumber;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use qc_progs::{Progs, ReadOptions};
use qc_trace::Record;
use qc_types::Slot;

use crate::Machine;
use crate::exec::ServerState;
use crate::trace::Trace;

use super::harness::{Func, ProgsBuilder, Stmt, StubBuiltins, opcode_number};

/// A parsed vector.
#[derive(Debug, Default)]
struct Vector {
    name: String,
    globals: Vec<(usize, String)>,
    fields: Vec<(usize, usize, String)>,
    strings_line: Option<String>,
    numglobals: Option<usize>,
    maxclients: usize,
    edicts: usize,
    server_loading: bool,
    uses_engine_builtins: bool,
    has_host: bool,
    entry: i32,
    functions: BTreeMap<usize, FuncSpec>,
    code: Vec<Stmt>,
    expect: Vec<String>,
    steps: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
enum FuncSpec {
    Qc(Func),
    Builtin(i32),
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Parses every `.txt` vector file that this crate runs: the five VM files (arith, compare,
/// bitwise, store, flow). The engine-builtin files (builtins, math, edicts, frames) need
/// `qc-builtins` and the simulation (P1-16, P1-31, P1-32) and are not run here.
fn vm_vector_files() -> Vec<PathBuf> {
    [
        "arith.txt",
        "compare.txt",
        "bitwise.txt",
        "store.txt",
        "flow.txt",
    ]
    .iter()
    .map(|name| repo_root().join("docs/spec/vectors/vm").join(name))
    .collect()
}

fn parse_file(text: &str) -> Vec<Vector> {
    let mut vectors: Vec<Vector> = Vec::new();
    let mut block = "";
    for raw in text.lines() {
        let line = if raw.starts_with("strings:") {
            raw
        } else {
            raw.split('#').next().unwrap_or("")
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix("== ") {
            vectors.push(Vector {
                name: name.trim().to_owned(),
                entry: 1,
                edicts: 3,
                ..Vector::default()
            });
            block = "";
            continue;
        }
        let v = vectors.last_mut().expect("vector before a line");
        if let Some(body) = line.strip_prefix("  ") {
            match block {
                "code" => v.code.push(parse_statement(body.trim())),
                "expect" => v.expect.push(body.trim().to_owned()),
                "functions" => parse_function_line(v, body.trim()),
                "host" => v.has_host = true,
                _ => panic!("indented line outside a block: {body}"),
            }
            continue;
        }
        let (key, rest) = line.split_once(':').expect("key: value");
        let rest = rest.trim();
        match key {
            "code" | "expect" | "functions" | "host" => block = key,
            "spec" | "after" | "oracle" => block = "",
            "seed" | "datagram" => block = "",
            "globals" => {
                for pair in rest.split_whitespace() {
                    let (slot, value) = pair.split_once('=').unwrap();
                    v.globals.push((slot.parse().unwrap(), value.to_owned()));
                }
            }
            "fields" => {
                for pair in rest.split_whitespace() {
                    let (lhs, value) = pair.split_once('=').unwrap();
                    let (edict, field) = parse_field_ref(lhs);
                    v.fields.push((edict, field, value.to_owned()));
                }
            }
            "strings" => v.strings_line = Some(rest.to_owned()),
            "numglobals" => v.numglobals = Some(rest.parse().unwrap()),
            "edicts" => v.edicts = rest.parse().unwrap(),
            "maxclients" => v.maxclients = rest.parse().unwrap(),
            "server" => v.server_loading = rest == "loading",
            "builtins" => v.uses_engine_builtins = rest == "engine",
            "entry" => v.entry = rest.parse().unwrap(),
            "steps" => v.steps = Some(rest.parse().unwrap()),
            other => panic!("unknown key {other}"),
        }
    }
    vectors
}

fn parse_statement(body: &str) -> Stmt {
    let mut parts = body.split_whitespace();
    let op = parts.next().unwrap();
    let a = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let b = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let c = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let number = match op.parse::<u16>() {
        Ok(number) => number,
        Err(_) => match opcode_number(op) {
            Some(number) => number,
            None => panic!("statement `{body}`: `{op}` is not a vanilla opcode name or number"),
        },
    };
    Stmt::raw(number, a, b, c)
}

// No spec statement: this pins the vector reader, so that a misspelt opcode in a vector file
// fails naming the statement instead of running some other opcode.
#[test]
fn a_statement_names_its_opcode_by_name_or_number() {
    let add = parse_statement("ADD_F 1 2 3");
    assert_eq!((add.op, add.a, add.b, add.c), (6, 1, 2, 3));
    assert_eq!(parse_statement("66").op, 66);
    assert_eq!(opcode_number("ADD_X"), None);
}

#[test]
#[should_panic(expected = "statement `ADD_X 1 2 3`: `ADD_X` is not a vanilla opcode name")]
fn a_misspelt_opcode_fails_naming_the_statement() {
    parse_statement("ADD_X 1 2 3");
}

fn parse_function_line(v: &mut Vector, body: &str) {
    let (index, rest) = body.split_once(':').unwrap();
    let index: usize = index.trim().parse().unwrap();
    if let Some(builtin) = field_value(rest, "builtin") {
        v.functions.insert(index, FuncSpec::Builtin(builtin));
        return;
    }
    let mut func = Func {
        first_statement: field_value(rest, "first").unwrap_or(0),
        parm_start: field_value(rest, "parm_start").unwrap_or(0),
        locals: field_value(rest, "locals").unwrap_or(0),
        ..Func::default()
    };
    if let Some(parms) = token_after(rest, "parms=") {
        let sizes: Vec<u8> = parms.split(',').map(|s| s.parse().unwrap()).collect();
        func.numparms = sizes.len() as i32;
        for (i, size) in sizes.iter().enumerate() {
            if i < 8 {
                func.parm_size[i] = *size;
            }
        }
    }
    v.functions.insert(index, FuncSpec::Qc(func));
}

/// The signed value of `key=<n>` in a whitespace-separated line.
fn field_value(line: &str, key: &str) -> Option<i32> {
    token_after(line, &format!("{key}=")).and_then(|v| v.parse().ok())
}

/// The token following the prefix `key` (which ends in `=`), up to the next whitespace.
fn token_after<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace()
        .find_map(|tok| tok.strip_prefix(key))
}

/// `e<n>.<field>` to an (edict, field) pair.
fn parse_field_ref(lhs: &str) -> (usize, usize) {
    let rest = lhs.strip_prefix('e').unwrap();
    let (edict, field) = rest.split_once('.').unwrap();
    (edict.parse().unwrap(), field.parse().unwrap())
}

/// Parses a vector value to its 32-bit pattern (README, "Values"). Engine-specific forms
/// (`e<n>`, `p<n>.<f>`) need the layout, so they are left to [`resolve_value`].
fn parse_value(text: &str) -> u32 {
    if let Some(hex) = text.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).unwrap();
    }
    match text {
        "nan" => return 0x7FC0_0000,
        "inf" => return f32::INFINITY.to_bits(),
        "-inf" => return f32::NEG_INFINITY.to_bits(),
        "-0" => return 0x8000_0000,
        _ => {}
    }
    if let Some(int) = text.strip_prefix('i') {
        return (int.parse::<i32>().unwrap()).cast_unsigned();
    }
    text.parse::<f32>().unwrap().to_bits()
}

/// Resolves an expected value, including the engine-specific forms, against a machine's layout.
fn resolve_value(text: &str, machine: &Machine) -> ExpectedValue {
    if let Some(rest) = text.strip_prefix('e') {
        if let Ok(number) = rest.parse::<i32>() {
            let bits = machine
                .edicts()
                .layout()
                .entity_value(EdictNumber::from_i32(number).unwrap())
                .unwrap()
                .bits();
            return ExpectedValue::Exact(bits.cast_unsigned());
        } else if rest == "s" {
            return ExpectedValue::AnyEngineString;
        }
    }
    if let Some(rest) = text.strip_prefix('p') {
        let (edict, field) = rest.split_once('.').unwrap();
        let bits = machine
            .edicts()
            .layout()
            .pointer_value(
                EdictNumber::new(edict.parse().unwrap()),
                field.parse().unwrap(),
            )
            .unwrap()
            .bits();
        return ExpectedValue::Exact(bits.cast_unsigned());
    }
    if text == "nan" {
        return ExpectedValue::AnyNan;
    }
    ExpectedValue::Exact(parse_value(text))
}

/// Resolves an initial value (README, "Values"), with `nan` as 0x7FC00000 (the initial-value
/// form) and the engine-specific forms via the layout.
fn resolve_init(text: &str, machine: &Machine) -> u32 {
    if let Some(rest) = text.strip_prefix('e')
        && let Ok(number) = rest.parse::<i32>()
    {
        let bits = machine
            .edicts()
            .layout()
            .entity_value(EdictNumber::from_i32(number).unwrap())
            .unwrap()
            .bits();
        return bits.cast_unsigned();
    }
    if let Some(rest) = text.strip_prefix('p')
        && let Some((edict, field)) = rest.split_once('.')
        && let (Ok(e), Ok(f)) = (edict.parse::<i32>(), field.parse::<usize>())
    {
        let bits = machine
            .edicts()
            .layout()
            .pointer_value(EdictNumber::from_i32(e).unwrap(), f)
            .unwrap()
            .bits();
        return bits.cast_unsigned();
    }
    parse_value(text)
}

#[derive(Clone, Copy, Debug)]
enum ExpectedValue {
    Exact(u32),
    AnyNan,
    AnyEngineString,
}

impl ExpectedValue {
    fn matches(self, bits: u32) -> bool {
        match self {
            Self::Exact(expected) => expected == bits,
            // VM-4.3: every NaN is equal to every other for comparison.
            Self::AnyNan => f32::from_bits(bits).is_nan(),
            // An engine-made string is matched only by the trace's string records, which the
            // core VM does not produce; no core vector expects `es`.
            Self::AnyEngineString => true,
        }
    }
}

/// One expected record parsed from an `expect:` line.
#[derive(Debug)]
enum Expected {
    /// A statement: its index and the changed globals and fields.
    Step {
        statement: i32,
        globals: Vec<(u32, String)>,
        fields: Vec<(u32, u32, String)>,
    },
    Done,
    Error(Option<String>),
    Ellipsis,
}

fn parse_expect(line: &str) -> Expected {
    if line == "..." {
        return Expected::Ellipsis;
    }
    if line == "done" {
        return Expected::Done;
    }
    if line == "error" {
        return Expected::Error(None);
    }
    if let Some(rest) = line.strip_prefix("error \"") {
        let message = rest.strip_suffix('"').unwrap().to_owned();
        return Expected::Error(Some(message));
    }
    // `<statement> <OP> <changes...>`: skip the statement index and the opcode name.
    let mut parts = line.split_whitespace();
    let statement: i32 = parts.next().unwrap().parse().unwrap();
    let _op = parts.next();
    let mut globals = Vec::new();
    let mut fields = Vec::new();
    for change in parts {
        let (lhs, value) = change.split_once('=').unwrap();
        if let Some(rest) = lhs.strip_prefix('e') {
            let (edict, field) = rest.split_once('.').unwrap();
            fields.push((
                edict.parse().unwrap(),
                field.parse().unwrap(),
                value.to_owned(),
            ));
        } else {
            globals.push((lhs.parse().unwrap(), value.to_owned()));
        }
    }
    Expected::Step {
        statement,
        globals,
        fields,
    }
}

/// Builds and runs one vector, returning the trace and the error if any.
fn run_vector(v: &Vector) -> (qc_trace::Trace, Option<crate::RunError>, Machine) {
    let numglobals = v.numglobals.unwrap_or(128);
    // Build the function table: indices named in `functions:` override the defaults; function
    // 1 defaults to first=1 with no parameters.
    let max_index = v.functions.keys().copied().max().unwrap_or(1).max(1);
    let mut functions: Vec<Func> = (1..=max_index)
        .map(|i| {
            if i == 1 {
                Func {
                    first_statement: 1,
                    ..Func::default()
                }
            } else {
                Func::default()
            }
        })
        .collect();
    for (index, spec) in &v.functions {
        if *index == 0 {
            continue;
        }
        let slot = index - 1;
        match spec {
            FuncSpec::Qc(func) => functions[slot] = *func,
            FuncSpec::Builtin(number) => {
                functions[slot] = Func {
                    first_statement: -number,
                    ..Func::default()
                };
            }
        }
    }

    let strings = build_strings(v.strings_line.as_deref());

    let bytes = ProgsBuilder::default()
        .numglobals(numglobals)
        .statements(&v.code)
        .functions(functions)
        .strings(strings)
        .build();
    let progs = Progs::read_with(
        &bytes,
        &ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap_or_else(|e| panic!("vector {} failed to load: {e}", v.name));
    let mut machine = Machine::new(&progs, v.maxclients).unwrap();
    // The vector env has `edicts` edicts in use (default 3); grow num_edicts to match by
    // allocating the ones past maxclients + 1. Each starts zeroed and in use (EDICT-1.8).
    while machine.edicts().num_edicts() < v.edicts {
        machine.edicts_mut().alloc(0.0).unwrap();
    }
    for (slot, value) in &v.globals {
        machine.set_global_test(*slot as i32, Slot::from_bits(resolve_init(value, &machine)));
    }
    for (edict, field, value) in &v.fields {
        let bits = resolve_init(value, &machine);
        machine
            .edicts_mut()
            .set_field(EdictNumber::new(*edict), *field, Slot::from_bits(bits))
            .unwrap();
    }

    let server = if v.server_loading {
        ServerState::Loading
    } else {
        ServerState::Active
    };
    let mut trace = Trace::new(&machine, &progs).unwrap();
    let result = machine.execute_program(&progs, server, &mut StubBuiltins, &mut trace, v.entry);
    let error = result.err();
    (trace.finish(), error, machine)
}

/// Lays out the strings after the empty one, each NUL-terminated (README, "strings:").
fn build_strings(line: Option<&str>) -> Vec<u8> {
    let mut bytes = vec![0_u8];
    let Some(line) = line else {
        return bytes;
    };
    for token in split_quoted(line) {
        bytes.extend(unescape(&token));
        bytes.push(0);
    }
    bytes
}

/// Splits a `strings:` line into its quoted tokens.
fn split_quoted(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = line.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c == '"' {
            chars.next();
            let mut token = String::new();
            for ch in chars.by_ref() {
                if ch == '"' {
                    break;
                }
                token.push(ch);
            }
            tokens.push(token);
        } else {
            chars.next();
        }
    }
    tokens
}

/// Expands `\xNN` escapes in a string literal (README, "strings:").
fn unescape(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'x') {
            chars.next();
            let hi = chars.next().unwrap();
            let lo = chars.next().unwrap();
            let byte = u8::from_str_radix(&format!("{hi}{lo}"), 16).unwrap();
            out.push(byte);
        } else {
            let mut buf = [0_u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    out
}

/// Checks one vector: the step records match the expected changes, and the ending matches.
fn check_vector(v: &Vector) {
    let (trace, error, machine) = run_vector(v);
    let expectations: Vec<Expected> = v.expect.iter().map(|l| parse_expect(l)).collect();
    let ellipsis = expectations.iter().any(|e| matches!(e, Expected::Ellipsis));

    // The step records of the trace, in order.
    let steps: Vec<&qc_trace::Step> = trace
        .records
        .iter()
        .filter_map(|r| match r {
            Record::Step(step) => Some(step),
            _ => None,
        })
        .collect();

    // The ending: the last expectation is done or error.
    let ending = expectations
        .last()
        .expect("an expect block ends with done or error");
    match ending {
        Expected::Done => assert!(
            error.is_none(),
            "vector {} expected done but errored: {:?}",
            v.name,
            error.as_ref().map(|e| e.message())
        ),
        Expected::Error(message) => {
            let e = error
                .as_ref()
                .unwrap_or_else(|| panic!("vector {} expected an error", v.name));
            if let Some(expected) = message {
                assert_eq!(&e.message(), expected, "vector {} error message", v.name);
            }
        }
        _ => panic!(
            "vector {} expect block does not end with done or error",
            v.name
        ),
    }

    // The statement expectations, in order (skipping the ellipsis and the ending).
    let step_expects: Vec<&Expected> = expectations
        .iter()
        .filter(|e| matches!(e, Expected::Step { .. }))
        .collect();

    // The trace events that map to a statement line: every step record, and, for an error
    // An error raised by a statement that ran (bad opcode, a call or address failure, an
    // enter-time overflow from a CALL, a divergence) is an executed statement with no step
    // record (TRACE-4.6): it adds one to the statements executed and is listed as the last
    // statement line. A runaway error fires the guard before the statement runs, and points
    // back to the last statement, which already has a step (VM-19.1), so it adds nothing.
    let error_is_executed = error
        .as_ref()
        .is_some_and(|e| e.statement != -1 && e.kind.is_executed_statement());
    let error_record = trace.records.iter().find_map(|r| match r {
        Record::Error {
            statement, changes, ..
        } => Some((*statement, changes)),
        _ => None,
    });

    if ellipsis {
        // Runaway and overflow vectors list only the last statements; check the executed-
        // statement count (step records plus an executed error) and the tail. The step
        // expectation lines are matched against the last step records; the error, when it is
        // an executed statement, is counted but not listed as a tail line here.
        if let Some(total) = v.steps {
            let executed = steps.len() + usize::from(error_is_executed);
            assert_eq!(
                executed, total,
                "vector {} executed-statement count",
                v.name
            );
        }
        let tail = &steps[steps.len().saturating_sub(step_expects.len())..];
        for (expected, step) in step_expects.iter().zip(tail.iter()) {
            check_event(v, expected, step.statement, &step.changes, &machine);
        }
        return;
    }

    // Build the event list: the step records, then the executed error record if any.
    let mut events: Vec<(i32, &qc_trace::Changes)> =
        steps.iter().map(|s| (s.statement, &s.changes)).collect();
    if error_is_executed && let Some((statement, changes)) = error_record {
        events.push((statement, changes));
    }

    assert_eq!(
        events.len(),
        step_expects.len(),
        "vector {} expected {} statement lines, got {}",
        v.name,
        step_expects.len(),
        events.len()
    );
    for (expected, (statement, changes)) in step_expects.iter().zip(events.iter()) {
        check_event(v, expected, *statement, changes, &machine);
    }
}

fn check_event(
    v: &Vector,
    expected: &Expected,
    statement_index: i32,
    changes: &qc_trace::Changes,
    machine: &Machine,
) {
    let Expected::Step {
        statement,
        globals,
        fields,
    } = expected
    else {
        return;
    };
    assert_eq!(
        statement_index, *statement,
        "vector {} statement index",
        v.name
    );

    // Compare the global changes as a map slot -> bits.
    let actual_globals: BTreeMap<u32, u32> =
        changes.globals().iter().map(|g| (g.slot, g.bits)).collect();
    assert_eq!(
        actual_globals.len(),
        globals.len(),
        "vector {} step {}: expected {} global changes, got {:?}",
        v.name,
        statement,
        globals.len(),
        actual_globals
    );
    for (slot, value) in globals {
        let bits = *actual_globals.get(slot).unwrap_or_else(|| {
            panic!(
                "vector {} step {}: no change to global {slot}",
                v.name, statement
            )
        });
        assert!(
            resolve_value(value, machine).matches(bits),
            "vector {} step {}: global {slot} expected {value}, got {bits:#010x}",
            v.name,
            statement
        );
    }

    let actual_fields: BTreeMap<(u32, u32), u32> = changes
        .fields()
        .iter()
        .map(|f| ((f.edict, f.field), f.bits))
        .collect();
    assert_eq!(
        actual_fields.len(),
        fields.len(),
        "vector {} step {}: expected {} field changes, got {:?}",
        v.name,
        statement,
        fields.len(),
        actual_fields
    );
    for (edict, field, value) in fields {
        let bits = *actual_fields.get(&(*edict, *field)).unwrap_or_else(|| {
            panic!(
                "vector {} step {}: no change to field e{edict}.{field}",
                v.name, statement
            )
        });
        assert!(
            resolve_value(value, machine).matches(bits),
            "vector {} step {}: field e{edict}.{field} expected {value}, got {bits:#010x}",
            v.name,
            statement
        );
    }
}

/// Runs every VM vector this crate owns and checks each against its recorded answer.
///
/// This pins the whole of `vm.md`: every statement VM-1 to VM-19 and the VM limits LIMIT-2 to
/// LIMIT-5 is cited below so the ratchet holds it to this test.
// spec VM-1.1, VM-1.2, VM-1.3, VM-1.4: machine state, operands and the out-of-range error.
// spec VM-2.1, VM-2.2, VM-2.3, VM-2.4: starting a run and its errors.
// spec VM-3.1, VM-3.2, VM-3.3, VM-3.4, VM-3.5, VM-3.6: the statement loop and the opcode table.
// spec VM-4.1, VM-4.2, VM-4.3, VM-4.4: float arithmetic.
// spec VM-5.1, VM-5.2, VM-5.3, VM-5.4: vector arithmetic.
// spec VM-6.1, VM-6.2, VM-6.3, VM-6.4: float and vector comparisons.
// spec VM-7.1, VM-7.2, VM-7.3, VM-7.4: string, entity and function comparisons.
// spec VM-8.1, VM-8.2, VM-8.3, VM-8.4, VM-8.5, VM-8.6, VM-8.7: logical operators.
// spec VM-9.1, VM-9.2, VM-9.3: bitwise operators.
// spec VM-10.1, VM-10.2, VM-10.3, VM-10.4: global stores.
// spec VM-11.1, VM-11.2, VM-11.3, VM-11.4: entity field loads.
// spec VM-12.1, VM-12.2, VM-12.3, VM-12.4, VM-12.5, VM-12.6: addresses and pointer stores.
// spec VM-13.1, VM-13.2, VM-13.3, VM-13.4, VM-13.5: jumps.
// spec VM-14.1, VM-14.2, VM-14.3, VM-14.4, VM-14.5, VM-14.6: calls.
// spec VM-15.1, VM-15.2, VM-15.3, VM-15.4, VM-15.5, VM-15.6, VM-15.7: entering a function.
// spec VM-16.1, VM-16.2, VM-16.3, VM-16.4, VM-16.5, VM-16.6: returning.
// spec VM-17.1, VM-17.2, VM-17.3, VM-17.4: STATE.
// spec VM-18.1, VM-18.2, VM-18.3, VM-18.4: nested runs.
// spec VM-19.1, VM-19.2, VM-19.6, VM-19.7, VM-19.5: errors.
// spec LIMIT-2.1, LIMIT-2.2, LIMIT-2.3: QC call depth.
// spec LIMIT-3.1: the local stack.
// spec LIMIT-4.1, LIMIT-4.2: the runaway loop guard.
// spec LIMIT-5.1, LIMIT-5.2, LIMIT-5.3: builtin numbers.
#[test]
fn every_vm_vector_matches_its_recorded_answer() {
    let mut count = 0_usize;
    for path in vm_vector_files() {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for v in parse_file(&text) {
            if v.has_host || v.uses_engine_builtins {
                continue;
            }
            // Vectors that start a nested run (the stub builtin 2) present the CALL line
            // before the nested run's statements (vectors' README), the reverse of the trace
            // record order (TRACE-4.4). Reconciling the two presentations is the differential
            // runner's job (P1-13); VM-18 is covered here by the dedicated nested-run tests.
            let nests = v
                .functions
                .values()
                .any(|f| matches!(f, FuncSpec::Builtin(2)));
            if nests {
                continue;
            }
            check_vector(&v);
            count += 1;
        }
    }
    assert!(count >= 100, "ran only {count} VM vectors");
}
