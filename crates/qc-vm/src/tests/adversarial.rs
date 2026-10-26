// SPDX-License-Identifier: GPL-3.0-or-later

//! Adversarial tests for the P1-R1 readiness review (TEST-9).
//!
//! Written by an independent reviewer who did not write the VM, to attack combinations the
//! hand-written vectors in `docs/spec/vectors/vm/` do not exercise directly: a nested run's own
//! runaway counter (LIMIT-4.2), the machine state a `stack overflow` leaves behind (VM-19.6),
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
    // Every run ended and the depth is reset (VM-19.6).
    assert_eq!(machine.depth(), 0);
}

// spec VM-19.6: a runtime error sets the call depth to 0 but does NOT reset the local stack;
// the aborted functions keep their slots. The `stack_overflow` vector records
// `after: depth=0 localstack=0` because its recursive function has no locals; this pins the
// other half of VM-19.6, that a deep recursion whose functions DO have locals leaves the local
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
    // VM-19.6: depth is reset to 0, but the local stack is NOT, so the aborted frames' saved
    // locals remain and localstack_used is greater than 0.
    assert_eq!(machine.depth(), 0);
    assert!(
        machine.localstack_used() > 0,
        "the aborted frames' locals are kept (VM-19.6), got {}",
        machine.localstack_used()
    );
}

// spec VM-1.1, VM-19.6: the machine persists between runs, and an aborted run leaves the local
// stack dirty (VM-19.6). A new PR_ExecuteProgram on the same machine still starts from call
// depth 0 and runs correctly, its function saving its locals above the slots the aborted run
// left, and restoring them, so the dirty local stack does not leak into the new run's result.
#[test]
fn machine_reused_after_an_aborted_run_still_runs_cleanly() {
    // f1 (one local) calls f2 (one local), which hits a bad opcode; f3 (one local) stores 3.125.
    let bytes = harness::ProgsBuilder::default()
        .numglobals(256)
        .functions(vec![
            Func {
                first_statement: 1,
                parm_start: 200,
                locals: 1,
                ..Func::default()
            },
            Func {
                first_statement: 3,
                parm_start: 201,
                locals: 1,
                ..Func::default()
            },
            Func {
                first_statement: 4,
                parm_start: 202,
                locals: 1,
                ..Func::default()
            },
        ])
        .statements(&[
            Stmt::op("CALL0", 100, 0, 0),     // 1: f1 calls f2
            Stmt::op("DONE", 0, 0, 0),        // 2
            Stmt::raw(66, 0, 0, 0),           // 3: f2: Bad opcode 66
            Stmt::op("STORE_F", 102, 202, 0), // 4: f3: its local = 3.125
            Stmt::op("STORE_F", 202, 103, 0), // 5: slot 103 = its local
            Stmt::op("DONE", 0, 0, 0),        // 6
        ])
        .globals_init(&[(100, 2), (102, 0x4048_0000), (202, 0x4100_0000)]) // 3.125; 8.0
        .build();
    let progs = read(&bytes);
    let mut machine = Machine::new(&progs, 0).unwrap();
    let error = machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut NoBuiltins,
            &mut NoObserver,
            1,
        )
        .unwrap_err();
    assert_eq!(error.message(), "Bad opcode 66");
    assert_eq!(machine.depth(), 0);
    assert_eq!(machine.localstack_used(), 2, "two aborted frames' locals");

    // The same machine runs f3: it starts at depth 0, saves slot 202 (8.0) above the two dirty
    // slots, overwrites it, and on return restores it and the stack height.
    machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut NoBuiltins,
            &mut NoObserver,
            3,
        )
        .unwrap();
    assert_eq!(machine.globals()[103].as_f32(), 3.125);
    assert_eq!(
        machine.globals()[202].as_f32(),
        8.0,
        "f3's local is restored"
    );
    assert_eq!(machine.depth(), 0);
    assert_eq!(
        machine.localstack_used(),
        2,
        "the dirty slots are still in use"
    );
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

// spec VM-7.3: NoBuiltins has no engine and so no engine strings (EDICT-13.1): EQ_S, NE_S or
// NOT_S given a value outside the table, here -5 against the table's empty string, is the
// VM-7.3 run error, whatever text the value might have held
#[test]
fn reviewer_no_builtins_resolves_no_engine_string() {
    for op in ["EQ_S", "NE_S", "NOT_S"] {
        let bytes = harness::ProgsBuilder::default()
            .statements(&[Stmt::op(op, 100, 101, 110), Stmt::op("DONE", 0, 0, 0)])
            .build();
        let progs = read(&bytes);
        let mut machine = Machine::new(&progs, 0).unwrap();
        machine.set_global_test(100, Slot::from_i32(-5));
        machine.set_global_test(101, Slot::from_i32(0));
        let error = machine
            .execute_program(
                &progs,
                ServerState::Active,
                &mut NoBuiltins,
                &mut NoObserver,
                1,
            )
            .unwrap_err();
        match error.kind {
            ErrorKind::Divergence { statement, .. } => assert_eq!(statement, "VM-7.3", "{op}"),
            other => panic!("{op}: {other:?}"),
        }
    }
}

// spec VM-7.3: EQ_S and NE_S read an engine string's text when they run, so after a builtin
// rewrites the one temp buffer (BUILTIN-7.4) the same value compares by its new text: equal to
// the table string "one" before the call, and after it "two" against "one" is NE_S 't' - 'o'
#[test]
fn reviewer_an_engine_strings_text_is_read_at_each_comparison() {
    struct Dispatch {
        text: Vec<u8>,
    }
    impl Builtins for Dispatch {
        fn call(&mut self, number: u32, _context: &mut Context<'_, '_>) -> Result<(), RunError> {
            if number == 2 {
                self.text = b"two".to_vec();
            }
            Ok(())
        }
        fn engine_string(&self, value: i32) -> Option<&[u8]> {
            (value == -1).then_some(self.text.as_slice())
        }
    }

    // Strings: "\0one\0": "one" at 1.
    let bytes = harness::ProgsBuilder::default()
        .strings(b"\0one\0\0\0\0".to_vec())
        .functions(vec![
            Func {
                first_statement: 1,
                ..Func::default()
            }, // f1
            Func {
                first_statement: -2,
                ..Func::default()
            }, // f2: builtin 2
        ])
        .statements(&[
            Stmt::op("EQ_S", 100, 101, 110), // 1: engine -1 vs "one"
            Stmt::op("CALL0", 103, 0, 0),    // 2: builtin 2 rewrites the buffer
            Stmt::op("EQ_S", 100, 101, 111), // 3: now "two" vs "one"
            Stmt::op("NE_S", 100, 101, 112), // 4
            Stmt::op("DONE", 0, 0, 0),       // 5
        ])
        .build();
    let progs = read(&bytes);
    let mut machine = Machine::new(&progs, 0).unwrap();
    machine.set_global_test(100, Slot::from_i32(-1));
    machine.set_global_test(101, Slot::from_i32(1));
    machine.set_global_test(103, Slot::from_i32(2)); // f2 index
    let mut dispatch = Dispatch {
        text: b"one".to_vec(),
    };
    machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut dispatch,
            &mut NoObserver,
            1,
        )
        .unwrap();
    assert_eq!(machine.globals()[110].as_f32(), 1.0);
    assert_eq!(machine.globals()[111].as_f32(), 0.0);
    assert_eq!(machine.globals()[112].as_f32(), 5.0);
}
