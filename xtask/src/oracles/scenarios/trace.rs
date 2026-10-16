// SPDX-License-Identifier: GPL-3.0-or-later

//! Scenario tests of the trace the oracle writes: `trace.md` TRACE-1.3 and TRACE-3 to
//! TRACE-5, checked on the records the oracle's own C encoder wrote.

use qc_progs::crc::crc_block;

use super::cases::{done, f, spawn, start_frame, worldspawn};
use super::{
    Arg, Case, Check, Input, Program, RETURN, Val, WORLDSPAWN, Want, field, scenario,
    scenario_with_map, ty,
};

pub(super) fn all() -> Vec<Case> {
    vec![
        header(),
        change_sets(),
        new_and_unused_edicts(),
        unused_edicts(),
        locals_restored(),
        records_of_calls(),
        nested_runs(),
        runaway_loop(),
        no_record_after_an_error(),
        string_records(),
        binary_form(),
    ]
}

/// The bytes of a little-endian word.
fn word(value: u32) -> [u8; 4] {
    value.to_le_bytes()
}

// spec TRACE-1.3, TRACE-5.4: the header is written once the progs is loaded: the magic `QCTR`,
// then words for the version 1, the progs's CRC, the edict layout, `entityfields` and
// `numstrings`; the globals' values in the progs are what the first change set compares
// against, so a global the program never changes is never listed.
fn header() -> Case {
    let mut p = Program::new();
    let constant = p.float(5.0);
    let program = p.clone();
    let bytes = program.bytes().unwrap_or_default();
    let crc = u32::from(crc_block(&bytes));
    let mut prefix = b"QCTR".to_vec();
    prefix.extend(word(1));
    prefix.extend(word(crc));
    // The header's numstrings is the progs's, at byte 44 of its header.
    let numstrings = bytes
        .get(44..48)
        .and_then(|field| <[u8; 4]>::try_from(field).ok())
        .map_or(0, u32::from_le_bytes);
    Case::new("header", p, scenario("frames 0\n"))
        .check(Check::Prefix(prefix))
        .check(Check::Header {
            entityfields: 110,
            numstrings,
        })
        .check(Check::Never(Want::GlobalChanged(constant)))
}

// spec TRACE-3.1: a change set lists every global and field whose bits changed since the
// previous change-carrying record: the three slots of a vector store, and -0.0 over 0.0.
fn change_sets() -> Case {
    let mut p = Program::new();
    let v = p.vector([1.0, 2.0, 3.0]);
    let out = p.vector([0.0, 0.0, 0.0]);
    let minus_zero = p.int(0x8000_0000);
    let zero_slot = p.temp();
    let store_v = p.next_statement();
    let code = vec![
        format!("STORE_V {v} {out}"),
        format!("STORE_F {minus_zero} {zero_slot}"),
    ];
    worldspawn(&mut p, code);
    Case::new("change-sets", p, scenario("frames 0\n")).check(Check::Seq(vec![
        Want::All(vec![
            Want::Step(store_v),
            Want::Global(out, f(1.0)),
            Want::Global(out.saturating_add(1), f(2.0)),
            Want::Global(out.saturating_add(2), f(3.0)),
        ]),
        Want::All(vec![
            Want::Step(store_v.saturating_add(1)),
            Want::Global(zero_slot, Val::Bits(0x8000_0000)),
        ]),
    ]))
}

// spec TRACE-3.4: a newly spawned edict is compared against all-zero fields, so it is listed
// only once a field of it is not zero.
fn new_and_unused_edicts() -> Case {
    let mut p = Program::new();
    let e = p.temp();
    let nine = p.float(9.0);
    let mut code = spawn(&mut p, e);
    code.extend(p.write_field(e, field::HEALTH, nine));
    worldspawn(&mut p, code);
    Case::new("new-edicts", p, scenario("frames 0\n"))
        .check(Check::Seq(vec![
            Want::All(vec![
                Want::Builtin(14),
                Want::Global(RETURN, Val::Entity(2)),
            ]),
            Want::Field(2, field::HEALTH, f(9.0)),
        ]))
        .check(Check::Count(Want::EdictChanged(2), 1))
}

// spec TRACE-3.4: the fields compared are those of the edicts in use: a write to an edict past
// them is in no change set (vector mode, three edicts in use).
fn unused_edicts() -> Case {
    let mut p = Program::new();
    let far = p.temp();
    let near = p.temp();
    let nine = p.float(9.0);
    let mut code = p.write_field(far, field::HEALTH, nine);
    code.extend(p.write_field(near, field::HEALTH, nine));
    let entry = p.func("entry", &done(code));
    Case::new(
        "unused-edicts",
        p,
        Input::Vector {
            init: format!(
                "edicts 3\nentry {entry}\nglobal {far} entity 5\nglobal {near} entity 2\n"
            ),
        },
    )
    .check(Check::Seq(vec![Want::Field(2, field::HEALTH, f(9.0))]))
    .check(Check::Never(Want::EdictChanged(5)))
}

// spec TRACE-3.5: the local stack is not in a change set; what it does shows in the globals:
// a function's local, saved on entry, is written back when it returns.
fn locals_restored() -> Case {
    let mut p = Program::new();
    let local = p.temp();
    let nine = p.float(9.0);
    let callee = p.func("with_local", &done(vec![format!("STORE_F {nine} {local}")]));
    p.set_function_locals(callee, local, 1);
    let target = p.fref(callee);
    worldspawn(&mut p, vec![format!("CALL0 {target}")]);
    Case::new("locals-restored", p, scenario("frames 0\n")).check(Check::Seq(vec![
        Want::Global(local, f(9.0)),
        Want::Global(local, f(0.0)),
    ]))
}

// spec TRACE-4.2, TRACE-4.3, TRACE-4.5: a step record names the function current when its
// statement started, so a `CALL` belongs to its caller; it has a builtin number exactly when
// the call was to a builtin, with the builtin's writes in its change set; `DONE` has its step
// record and the run ends with an exit record.
fn records_of_calls() -> Case {
    let mut p = Program::new();
    let callee_done = p.next_statement();
    let callee = p.func("callee", &done(Vec::new()));
    let target = p.fref(callee);
    let vlen = p.builtin("vlen", 12);
    let v = p.vector([0.0, 3.0, 4.0]);
    // The statements: CALL0 callee, STORE_V v 4, CALL1 vlen, DONE.
    let call_qc = p.next_statement();
    let call_builtin = call_qc.saturating_add(2);
    let done_statement = call_qc.saturating_add(3);
    let mut code = vec![format!("CALL0 {target}")];
    code.extend(p.call(vlen, &[Arg::V(v)]));
    let start = start_frame(&mut p, code);
    Case::new("records-of-calls", p, scenario("frames 0\n")).check(Check::Seq(vec![
        Want::Run(start),
        Want::All(vec![
            Want::StepIn(start, call_qc),
            Want::StepWithoutBuiltin(call_qc),
        ]),
        // A QC call is no new run: the callee's statements are steps of the same run.
        Want::StepIn(callee, callee_done),
        Want::All(vec![
            Want::StepIn(start, call_builtin),
            Want::Builtin(12),
            Want::Global(RETURN, f(5.0)),
        ]),
        Want::StepIn(start, done_statement),
        Want::Exit,
    ]))
}

// spec TRACE-4.4: a builtin's nested runs write their records while it runs, before the step
// record of the `CALL` that called it, which holds what the builtin changed afterwards (vector
// mode's builtin 2, docs/spec/vectors/vm/README.md).
fn nested_runs() -> Case {
    let mut p = Program::new();
    let marker = p.temp();
    let one = p.float(1.0);
    let inner = p.func("inner", &done(vec![format!("STORE_F {one} {marker}")]));
    let inner_ref = p.fref(inner);
    let nested = p.builtin("nested", 2);
    let call = p.next_statement().saturating_add(1);
    let outer = p.func(
        "outer",
        &done(vec![
            format!("STORE_F {inner_ref} 4"),
            format!("CALL1 {nested}"),
        ]),
    );
    Case::new(
        "nested-runs",
        p,
        Input::Vector {
            init: format!("edicts 3\nentry {outer}\n"),
        },
    )
    .check(Check::Seq(vec![
        Want::Run(outer),
        Want::Run(inner),
        Want::Global(marker, f(1.0)),
        Want::Exit,
        Want::All(vec![Want::StepIn(outer, call), Want::Builtin(2)]),
        Want::Exit,
    ]))
    // Vector mode reports the call depth and local stack after the run.
    .check(Check::Stdout("depth=0 localstack=0"))
}

// spec TRACE-4.7: for `runaway loop error` the error record names the statement before the one
// that was not run: a `GOTO 0` that jumps to itself.
fn runaway_loop() -> Case {
    let mut p = Program::new();
    let statement = p.next_statement();
    let start = start_frame(&mut p, vec!["GOTO 0".to_owned()]);
    Case::new("runaway-loop", p, scenario("frames 1\n"))
        .check(Check::Last(Want::Error(
            start,
            statement,
            "runaway loop error",
        )))
        .check(Check::Count(Want::Step(statement), 99_999))
}

// spec TRACE-4.9: after an error record no record follows for the runs that were active: no
// exit record for the failing function or for the run that called it.
fn no_record_after_an_error() -> Case {
    let mut p = Program::new();
    let null = p.int(0);
    let failing_call = p.next_statement();
    let failing = p.func("failing", &done(vec![format!("CALL0 {null}")]));
    let target = p.fref(failing);
    let start = start_frame(&mut p, vec![format!("CALL0 {target}")]);
    Case::new("no-record-after-an-error", p, scenario("frames 1\n"))
        .check(Check::Seq(vec![Want::Run(start)]))
        .check(Check::Last(Want::Error(
            failing,
            failing_call,
            "NULL function",
        )))
}

// spec TRACE-4.10: a string record comes when an engine string's text is set or changes, not
// again for the same text.
fn string_records() -> Case {
    let mut p = Program::new();
    let ftos = p.builtin("ftos", 26);
    let one = p.float(1.0);
    let two = p.float(2.0);
    let mut code = p.call(ftos, &[Arg::F(one)]);
    code.extend(p.call(ftos, &[Arg::F(one)]));
    code.extend(p.call(ftos, &[Arg::F(two)]));
    worldspawn(&mut p, code);
    Case::new("string-records", p, scenario("frames 0\n"))
        .check(Check::Count(Want::String("1"), 1))
        .check(Check::Seq(vec![
            Want::String("1"),
            Want::Builtin(26),
            Want::Builtin(26),
            Want::String("2"),
            Want::Builtin(26),
        ]))
}

// spec TRACE-5.1, TRACE-5.2, TRACE-5.3, TRACE-5.5, TRACE-5.6, TRACE-5.7: the oracle's encoder
// writes every record kind, words, uvars of one to three bytes (a global slot above 16383, an
// edict above 127), a svar of -1 and text, in a stream `qc-trace`'s strict decoder reads and
// encodes back to the same bytes.
fn binary_form() -> Case {
    let mut p = Program::new();
    let high = 20_000_u32;
    p.reserve_globals(high.saturating_add(1));
    p.field_def("message", ty::STRING, field::MESSAGE);
    let one = p.float(1.0);
    let e = p.temp();
    let mut code = Vec::new();
    for _ in 0..130 {
        code.extend(spawn(&mut p, e));
    }
    code.extend(p.write_field(e, field::HEALTH, one));
    code.extend(p.write_field(e, field::MOVETYPE, one));
    code.push(format!("STORE_F {one} {high}"));
    worldspawn(&mut p, code);
    let map = "{\n\"classname\" \"worldspawn\"\n\"message\" \"text\"\n}\n";
    Case::new("binary-form", p, scenario_with_map("frames 1\n", map))
        .check(Check::Seq(vec![
            Want::String("text"),
            Want::Run(WORLDSPAWN),
            Want::Field(131, field::HEALTH, f(1.0)),
            Want::Global(high, f(1.0)),
            Want::Exit,
        ]))
        .check(Check::Last(Want::Error(
            0,
            -1,
            "SV_Physics: bad movetype 1",
        )))
}
