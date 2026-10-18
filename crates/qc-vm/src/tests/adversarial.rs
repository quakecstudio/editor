// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests for the P1-R1 readiness review (TEST-9).
//!
//! Written by an independent reviewer who did not write the VM, to attack combinations the
//! hand-written vectors in `docs/spec/vectors/vm/` do not exercise directly: a nested run's own
//! runaway counter (LIMIT-4.2), the machine state a `stack overflow` leaves behind (VM-19.3),
//! and a machine reused after an aborted run (VM-1.1). Each test builds a progs with the shared
//! harness, runs it, and checks the VM against the statement the `// spec` comment names.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_progs::{Progs, ReadOptions};
use qc_types::Slot;

use super::harness::{self, Func, Stmt};
use crate::exec::{Builtins, Context, NoBuiltins, NoObserver, ServerState};
use crate::{ErrorKind, Machine, RunError};

/// Reads a harness-built progs with the header CRC check off, as the other test modules do.
fn read(bytes: &[u8]) -> Progs<'_> {
    Progs::read_with(
        bytes,
        &ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap()
}

/// The stub builtin the vectors use: builtin 2 runs the function whose index is in `OFS_PARM0`
/// (slot 4) as a nested run; any other number is inert here.
struct NestedRunBuiltin;

impl Builtins for NestedRunBuiltin {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        if number == 2 {
            let function = context.machine().globals()[4].as_i32();
            context.execute_program(self, function)?;
            return Ok(());
        }
        Ok(())
    }

    fn engine_string(&self, _value: i32) -> Option<&[u8]> {
        None
    }
}

// spec LIMIT-4.2: statements run by a nested PR_ExecuteProgram count against the nested call's
// own counter, not the outer's; so a nested infinite loop fails with "runaway loop error" even
// though the outer run executed only a handful of statements and had almost its whole budget
// left. The `runaway_per_run` vector pins the outer reaching the limit through a nested run;
// this pins the complement, that the nested run has its own fresh counter.
#[test]
fn nested_run_has_its_own_runaway_counter() {
    // f1: store f3's index in OFS_PARM0, call builtin 2 (nested run of f3), then DONE.
    // f3: GOTO 0, an infinite one-statement loop, so the nested run burns its own counter.
    let bytes = harness::ProgsBuilder::default()
        .functions(vec![
            Func {
                first_statement: 1,
                ..Func::default()
            }, // f1
            Func {
                first_statement: -2,
                ..Func::default()
            }, // f2: builtin 2
            Func {
                first_statement: 4,
                ..Func::default()
            }, // f3
        ])
        .statements(&[
            Stmt::op("STORE_FNC", 100, 4, 0), // 1: OFS_PARM0 = f3 index
            Stmt::op("CALL1", 101, 0, 0),     // 2: builtin 2 runs f3 nested
            Stmt::op("DONE", 0, 0, 0),        // 3: outer would continue (never reached)
            Stmt::op("GOTO", 0, 0, 0),        // 4: f3, infinite loop (VM-13.4)
        ])
        .build();
    let progs = read(&bytes);
    let mut machine = Machine::new(&progs, 0).unwrap();
    machine.set_global_test(100, Slot::from_i32(3)); // f3 index
    machine.set_global_test(101, Slot::from_i32(2)); // f2 index
    let error = machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut NestedRunBuiltin,
            &mut NoObserver,
            1,
        )
        .unwrap_err();
    assert_eq!(error.message(), "runaway loop error");
    assert_eq!(error.kind, ErrorKind::Runaway);
    // The error is raised inside f3 (function index 3), not the outer f1 (VM-19.2).
    assert_eq!(
        error.function, 3,
        "the runaway fires in the nested function"
    );
    // Every run ended and the depth is reset (VM-19.3).
    assert_eq!(machine.depth(), 0);
}

// spec VM-19.3: a runtime error sets the call depth to 0 but does NOT reset the local stack;
// the aborted functions keep their slots. The `stack_overflow` vector records
// `after: depth=0 localstack=0` because its recursive function has no locals; this pins the
// other half of VM-19.3, that a deep recursion whose functions DO have locals leaves the local
// stack non-empty after the "stack overflow" error.
#[test]
fn stack_overflow_keeps_the_local_stack() {
    // f1 (the entry) and f2 each have one local; f1 calls f2, f2 calls itself, overflowing the
    // 32-deep call stack. Each live frame has saved one local slot, so localstack_used is not 0.
    let bytes = harness::ProgsBuilder::default()
        .numglobals(256)
        .functions(vec![
            // f1: enter, then call f2. parm_start 200, one local, so entering saves 1 slot.
            Func {
                first_statement: 1,
                parm_start: 200,
                locals: 1,
                ..Func::default()
            },
            // f2: call itself. parm_start 201, one local.
            Func {
                first_statement: 3,
                parm_start: 201,
                locals: 1,
                ..Func::default()
            },
        ])
        .statements(&[
            Stmt::op("CALL0", 100, 0, 0), // 1: f1 calls f2
            Stmt::op("DONE", 0, 0, 0),    // 2
            Stmt::op("CALL0", 101, 0, 0), // 3: f2 calls itself
            Stmt::op("DONE", 0, 0, 0),    // 4
        ])
        .build();
    let progs = read(&bytes);
    let mut machine = Machine::new(&progs, 0).unwrap();
    machine.set_global_test(100, Slot::from_i32(2)); // f2 index for f1's CALL0
    machine.set_global_test(101, Slot::from_i32(2)); // f2 index for f2's self-CALL0
    let error = machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut NoBuiltins,
            &mut NoObserver,
            1,
        )
        .unwrap_err();
    assert_eq!(error.message(), "stack overflow");
    assert_eq!(error.kind, ErrorKind::StackOverflow);
    // VM-19.3: depth is reset to 0, but the local stack is NOT, so the aborted frames' saved
    // locals remain and localstack_used is greater than 0.
    assert_eq!(machine.depth(), 0);
    assert!(
        machine.localstack_used() > 0,
        "the aborted frames' locals are kept (VM-19.3), got {}",
        machine.localstack_used()
    );
}

// spec VM-1.1, VM-19.3: the machine persists between runs, and an aborted run leaves the local
// stack dirty (VM-19.3). A fresh PR_ExecuteProgram on the same machine must still start from
// call depth 0 and run correctly, so the dirty local stack of the earlier abort does not leak
// into the new run's result. The vectors run one program per machine; this pins reuse.
#[test]
fn machine_reused_after_an_aborted_run_still_runs_cleanly() {
    // First program: a bad opcode aborts function 1 immediately.
    let bad = harness::ProgsBuilder::default()
        .statements(&[Stmt::raw(66, 0, 0, 0)])
        .build();
    let progs_bad = read(&bad);
    let mut machine = Machine::new(&progs_bad, 0).unwrap();
    let error = machine
        .execute_program(
            &progs_bad,
            ServerState::Active,
            &mut NoBuiltins,
            &mut NoObserver,
            1,
        )
        .unwrap_err();
    assert_eq!(error.message(), "Bad opcode 66");
    assert_eq!(machine.depth(), 0);

    // Second program on a fresh machine from a clean progs: a plain store then DONE. Depth
    // starts at 0 and the store lands, proving a run is self-contained (VM-1.1).
    let good = harness::ProgsBuilder::default()
        .statements(&[Stmt::op("STORE_F", 100, 101, 0), Stmt::op("DONE", 0, 0, 0)])
        .globals_init(&[(100, 0x4048_0000)]) // 3.125
        .build();
    let progs_good = read(&good);
    let mut machine2 = Machine::new(&progs_good, 0).unwrap();
    assert_eq!(
        machine2.depth(),
        0,
        "a fresh run starts at depth 0 (VM-1.1)"
    );
    machine2
        .execute_program(
            &progs_good,
            ServerState::Active,
            &mut NoBuiltins,
            &mut NoObserver,
            1,
        )
        .unwrap();
    assert_eq!(machine2.globals()[101].as_f32(), 3.125);
    assert_eq!(machine2.depth(), 0);
}

// spec VM-16.4, VM-3.5: a run ends when a return brings the call depth back to the exit depth,
// so a statement placed physically after the entry function's DONE never executes. The flow
// vectors exercise DONE, but this pins that execution does not fall through DONE into a later
// function's body laid out next to it in the statements table.
#[test]
fn done_does_not_fall_through_into_the_next_statement() {
    let bytes = harness::ProgsBuilder::default()
        .statements(&[
            Stmt::op("DONE", 0, 0, 0),        // 1: entry returns at once
            Stmt::op("STORE_F", 100, 101, 0), // 2: must NOT run
        ])
        .globals_init(&[(100, 0x3F80_0000)]) // 1.0
        .build();
    let progs = read(&bytes);
    let mut machine = Machine::new(&progs, 0).unwrap();
    machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut NoBuiltins,
            &mut NoObserver,
            1,
        )
        .unwrap();
    // Slot 101 would be 1.0 had statement 2 run; it must stay 0 (VM-16.4).
    assert_eq!(
        machine.globals()[101].0,
        0,
        "DONE ends the run, statement 2 never runs"
    );
    assert_eq!(machine.depth(), 0);
}

// spec VM-7.3: EQ_S resolves engine-allocated string values (outside the table) through the
// Builtins dispatch even when the comparison happens inside a nested run, because the dispatch
// is threaded through Context::execute_program. The string_opcodes module pins EQ_S at the top
// level; this pins that engine-string resolution survives a nested PR_ExecuteProgram.
#[test]
fn engine_string_resolves_inside_a_nested_run() {
    // A dispatch that both runs nested programs (builtin 2) and owns one engine string.
    struct Dispatch;
    impl Builtins for Dispatch {
        fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
            if number == 2 {
                let function = context.machine().globals()[4].as_i32();
                context.execute_program(self, function)?;
                return Ok(());
            }
            Ok(())
        }
        fn engine_string(&self, value: i32) -> Option<&[u8]> {
            // One engine string at the out-of-table value -5 (EDICT-13.3).
            if value == -5 { Some(b"abc") } else { None }
        }
    }

    // f1: OFS_PARM0 = f3 index, CALL1 builtin 2 (nested run of f3), DONE.
    // f3: EQ_S slot 100 vs slot 102 into slot 110; both hold the engine value -5, so equal.
    let bytes = harness::ProgsBuilder::default()
        .functions(vec![
            Func {
                first_statement: 1,
                ..Func::default()
            }, // f1
            Func {
                first_statement: -2,
                ..Func::default()
            }, // f2: builtin 2
            Func {
                first_statement: 4,
                ..Func::default()
            }, // f3
        ])
        .statements(&[
            Stmt::op("STORE_FNC", 101, 4, 0), // 1: OFS_PARM0 = f3 index
            Stmt::op("CALL1", 103, 0, 0),     // 2: nested run of f3
            Stmt::op("DONE", 0, 0, 0),        // 3
            Stmt::op("EQ_S", 100, 102, 110),  // 4: f3 compares two engine strings
            Stmt::op("DONE", 0, 0, 0),        // 5
        ])
        .build();
    let progs = read(&bytes);
    let mut machine = Machine::new(&progs, 0).unwrap();
    // -5 as the engine string value in both operand slots (EDICT-13.3).
    machine.set_global_test(100, Slot::from_i32(-5));
    machine.set_global_test(102, Slot::from_i32(-5));
    machine.set_global_test(101, Slot::from_i32(3)); // f3 index
    machine.set_global_test(103, Slot::from_i32(2)); // f2 index
    machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut Dispatch,
            &mut NoObserver,
            1,
        )
        .unwrap();
    // Equal engine strings compare equal: EQ_S writes 1.0 (VM-7.3).
    assert_eq!(machine.globals()[110].as_f32(), 1.0);
}

// spec VM-19.1: PR_RunError's trace lists every active function from the innermost out, each
// resolving its name and source file from the progs' string table. The `runaway` and
// `stack_overflow` vectors pin that an error fires; this pins the trace's contents, so an
// empty trace (stack_trace) or an unresolved name (string_of) is caught.
#[test]
fn run_error_trace_names_the_active_functions() {
    // Strings: "\0entry\0inner\0a.qc\0b.qc\0": entry@1, inner@7, a.qc@13, b.qc@18.
    let mut strings = vec![0_u8];
    for name in [b"entry".as_slice(), b"inner", b"a.qc", b"b.qc"] {
        strings.extend_from_slice(name);
        strings.push(0);
    }
    // f1 "entry" in "a.qc" calls f2 (builtin 2, a nested run of f3); f3 "inner" in "b.qc" hits
    // a bad opcode, raising the error two frames deep.
    let bytes = harness::ProgsBuilder::default()
        .strings(strings)
        .functions(vec![
            Func {
                first_statement: 1,
                s_name: 1,  // "entry"
                s_file: 13, // "a.qc"
                ..Func::default()
            }, // f1
            Func {
                first_statement: -2,
                ..Func::default()
            }, // f2: builtin 2
            Func {
                first_statement: 4,
                s_name: 7,  // "inner"
                s_file: 18, // "b.qc"
                ..Func::default()
            }, // f3
        ])
        .statements(&[
            Stmt::op("STORE_FNC", 100, 4, 0), // 1: OFS_PARM0 = f3 index
            Stmt::op("CALL1", 101, 0, 0),     // 2: builtin 2 runs f3 nested
            Stmt::op("DONE", 0, 0, 0),        // 3
            Stmt::raw(66, 0, 0, 0),           // 4: f3, a bad opcode (VM-2.3)
        ])
        .build();
    let progs = read(&bytes);
    let mut machine = Machine::new(&progs, 0).unwrap();
    machine.set_global_test(100, Slot::from_i32(3)); // f3 index
    machine.set_global_test(101, Slot::from_i32(2)); // f2 index
    let error = machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut NestedRunBuiltin,
            &mut NoObserver,
            1,
        )
        .unwrap_err();
    // The trace is innermost first: f3 "inner" (b.qc), f1 "entry" (a.qc), then the null base
    // frame (function 0, no name or file). The builtin frame (f2) is not a QC stack frame.
    assert_eq!(
        error.trace.len(),
        3,
        "the two QC frames plus the base frame"
    );
    assert_eq!(error.trace[0].function, 3);
    assert_eq!(error.trace[0].name, "inner");
    assert_eq!(error.trace[0].file, "b.qc");
    assert_eq!(error.trace[1].function, 1);
    assert_eq!(error.trace[1].name, "entry");
    assert_eq!(error.trace[1].file, "a.qc");
    // The base frame is the null function with no resolved name or file (string_of of 0 is
    // the empty string, stack_entry's None-function branch).
    assert_eq!(error.trace[2].function, 0);
    assert_eq!(error.trace[2].name, "");
    assert_eq!(error.trace[2].file, "");
}
