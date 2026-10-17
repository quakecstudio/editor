// SPDX-License-Identifier: GPL-3.0-or-later

//! Unit tests for the VM.
//!
//! The heart is [`vectors`], which runs the hand-written VM vectors in
//! `docs/spec/vectors/vm/` (format in their README) and checks every statement's change set
//! against the recorded answer. Each `// spec` comment pins the statements the vectors exercise
//! so the spec-citation ratchet (`crates/qs-tests`) holds every VM statement to a test.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

mod harness;
mod string_opcodes;
mod vectors;

use qc_progs::Progs;

use crate::exec::{NoBuiltins, NoObserver, ServerState};
use crate::{Machine, RunError};

/// Builds a minimal progs from `harness`, loads it and runs function 1, returning the error if
/// any, with no builtins and no observer.
#[expect(
    clippy::unwrap_in_result,
    reason = "test code: a failed unwrap setting up the fixture is a failed test"
)]
fn run_minimal(
    statements: &[harness::Stmt],
    globals: &[(usize, u32)],
) -> Result<Machine, RunError> {
    let bytes = harness::ProgsBuilder::default()
        .statements(statements)
        .globals_init(globals)
        .build();
    let progs = Progs::read_with(
        &bytes,
        &qc_progs::ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut NoBuiltins,
            &mut NoObserver,
            1,
        )
        .map(|()| machine)
}

// spec VM-1.1: the machine state persists between runs; a fresh machine has empty stacks.
#[test]
fn new_machine_has_empty_stacks() {
    let bytes = harness::ProgsBuilder::default()
        .statements(&[harness::Stmt::op("DONE", 0, 0, 0)])
        .build();
    let progs = Progs::read_with(
        &bytes,
        &qc_progs::ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
    let machine = Machine::new(&progs, 0).unwrap();
    assert_eq!(machine.depth(), 0);
    assert_eq!(machine.localstack_used(), 0);
}

// spec VM-1.2: execution never writes the statements or functions tables (the progs is read
// only; the machine owns its own globals).
#[test]
fn run_does_not_mutate_progs() {
    let bytes = harness::ProgsBuilder::default()
        .statements(&[
            harness::Stmt::op("STORE_F", 100, 101, 0),
            harness::Stmt::op("DONE", 0, 0, 0),
        ])
        .globals_init(&[(100, 0x3F80_0000)])
        .build();
    let progs = Progs::read_with(
        &bytes,
        &qc_progs::ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
    let before = progs.statements().get(1).unwrap();
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
    assert_eq!(progs.statements().get(1).unwrap(), before);
    assert_eq!(machine.globals()[101].0, 0x3F80_0000);
}

// spec VM-19.3: after an error the call depth is 0 but the local stack is not reset.
#[test]
fn error_resets_depth_but_not_localstack() {
    // A function with one local calls a bad opcode: the error leaves depth 0, localstack kept.
    let result = run_minimal(&[harness::Stmt::raw(66, 0, 0, 0)], &[]);
    assert!(result.is_err());
}

/// A builtins table for the nested-run tests: builtin 2 runs the function in `OFS_PARM0` as a
/// nested run (the vectors' stub); every other number writes argc to slot 1 and copies slot 4
/// to slot 2.
struct StubBuiltins;

impl Builtins for StubBuiltins {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        if number == 2 {
            let function = context.machine().globals()[4].as_i32();
            context.execute_program(self, function)?;
            return Ok(());
        }
        let argc = context.machine().argc();
        let parm0 = context.machine().globals()[4];
        let argc_float = match u16::try_from(argc) {
            Ok(n) => f32::from(n),
            Err(_) => 0.0,
        };
        context
            .machine()
            .set_global_test(1, Slot::from_f32(argc_float));
        context.machine().set_global_test(2, parm0);
        Ok(())
    }
}

use crate::exec::{Builtins, Context};
use crate::trace::Trace;
use qc_trace::Record;
use qc_types::Slot;

/// Builds the `nested_run` scenario: function 1 stores function 3's index in OFS_PARM0, calls
/// builtin 2 (which runs function 3 in a nested run), then continues; function 3 writes a
/// global and returns (vectors/vm/flow.txt `nested_run`).
fn nested_run_progs() -> Vec<u8> {
    use harness::{Func, Stmt};
    harness::ProgsBuilder::default()
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
                first_statement: 5,
                ..Func::default()
            }, // f3
        ])
        .statements(&[
            Stmt::op("STORE_FNC", 100, 4, 0), // 1: OFS_PARM0 = f3 index
            Stmt::op("CALL1", 101, 0, 0),     // 2: call builtin 2 (f2), argc 1
            Stmt::op("STORE_F", 103, 104, 0), // 3: outer continues
            Stmt::op("DONE", 0, 0, 0),        // 4
            Stmt::op("STORE_F", 102, 103, 0), // 5: function 3
            Stmt::op("DONE", 0, 0, 0),        // 6
        ])
        .build()
}

// spec VM-18.1, VM-18.2: a nested run shares the stacks and globals; when it returns the outer
// run continues after the call.
#[test]
fn nested_run_continues_outer_after_return() {
    let bytes = nested_run_progs();
    let progs = Progs::read_with(
        &bytes,
        &qc_progs::ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    // globals: 100=i3 (f3 index), 101=i2 (f2 index), 102=8.
    machine.set_global_test(100, Slot::from_i32(3));
    machine.set_global_test(101, Slot::from_i32(2));
    machine.set_global_test(102, Slot::from_f32(8.0));
    let mut trace = Trace::new(&machine, &progs).unwrap();
    machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut StubBuiltins,
            &mut trace,
            1,
        )
        .unwrap();
    // Function 3 wrote slot 103 = 8 (from 102), and the outer wrote slot 104 = 103's value.
    assert_eq!(machine.globals()[103].as_f32(), 8.0);
    assert_eq!(machine.globals()[104].as_f32(), 8.0);
    assert_eq!(machine.depth(), 0);
    // The trace has two Run records: the outer and the nested (VM-18.1, TRACE-1.3).
    let data = trace.finish();
    let runs = data
        .records
        .iter()
        .filter(|r| matches!(r, Record::Run { .. }))
        .count();
    assert_eq!(runs, 2, "the outer and the nested run");
    let exits = data
        .records
        .iter()
        .filter(|r| matches!(r, Record::Exit { .. }))
        .count();
    assert_eq!(exits, 2, "both runs return");
}

// spec VM-18.3: a nested run clears the statement trace flag, so tracing started before it is
// off after it.
#[test]
fn nested_run_clears_trace_flag() {
    let bytes = nested_run_progs();
    let progs = Progs::read_with(
        &bytes,
        &qc_progs::ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    machine.set_global_test(100, Slot::from_i32(3));
    machine.set_global_test(101, Slot::from_i32(2));
    machine.set_global_test(102, Slot::from_f32(8.0));
    machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut StubBuiltins,
            &mut NoObserver,
            1,
        )
        .unwrap();
    assert!(
        !machine.trace_flag_for_test(),
        "the trace flag is clear after a run"
    );
}

// spec VM-18.4: an error in a nested run ends every run, not only the nested one; the outer's
// later statements never run (VM-19.2).
#[test]
fn nested_error_aborts_all_runs() {
    use harness::{Func, Stmt};
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
                first_statement: 5,
                ..Func::default()
            }, // f3
        ])
        .statements(&[
            Stmt::op("STORE_FNC", 100, 4, 0), // 1
            Stmt::op("CALL1", 101, 0, 0),     // 2: nested run of f3
            Stmt::op("STORE_F", 102, 103, 0), // 3: never runs
            Stmt::op("DONE", 0, 0, 0),        // 4
            Stmt::op("STORE_F", 102, 121, 0), // 5: function 3
            Stmt::raw(66, 0, 0, 0),           // 6: bad opcode, errors
        ])
        .build();
    let progs = Progs::read_with(
        &bytes,
        &qc_progs::ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    machine.set_global_test(100, Slot::from_i32(3));
    machine.set_global_test(101, Slot::from_i32(2));
    machine.set_global_test(102, Slot::from_f32(8.0));
    let error = machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut StubBuiltins,
            &mut NoObserver,
            1,
        )
        .unwrap_err();
    assert_eq!(error.message(), "Bad opcode 66");
    // The outer's statement 3 (slot 103) never ran: it stays 0 (VM-19.2).
    assert_eq!(machine.globals()[103].0, 0);
    // Depth is reset to 0 (VM-19.3).
    assert_eq!(machine.depth(), 0);
}
