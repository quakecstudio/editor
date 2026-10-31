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

mod adversarial;
mod harness;
mod host_errors;
mod string_opcodes;
mod vectors;

use qc_progs::Progs;

use crate::exec::{Builtins, Context, NoBuiltins, NoObserver, ServerState};
use crate::machine::Frame;
use crate::{ErrorKind, Machine, RunError};

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

/// A machine for a progs of one `DONE` statement and `numglobals` globals.
fn machine_with_globals(numglobals: usize) -> Machine {
    let bytes = harness::ProgsBuilder::default()
        .statements(&[harness::Stmt::op("DONE", 0, 0, 0)])
        .numglobals(numglobals)
        .build();
    let progs = Progs::read_with(
        &bytes,
        &qc_progs::ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
    Machine::new(&progs, 1).unwrap()
}

// No spec statement: the differential restores a machine from a snapshot with `clone_from`
// (DIFF-12.2), which must leave every part of it equal to the snapshot's, the globals, the
// edicts, the stacks and the run state, whatever was done to it since.
#[test]
fn clone_from_restores_a_changed_machine() {
    let mut snapshot = machine_with_globals(128);
    snapshot.set_global(100, Slot::from_i32(5)).unwrap();
    let mut copy = snapshot.clone();
    assert_eq!(format!("{copy:?}"), format!("{snapshot:?}"));

    copy.set_global(101, Slot::from_i32(6)).unwrap();
    let spawned = copy.edicts_mut().alloc(1.0).unwrap();
    copy.edicts_mut()
        .set_field(spawned, 2, Slot::from_i32(7))
        .unwrap();
    copy.stack.push(Frame {
        statement: 9,
        function: 1,
    });
    copy.localstack[3] = Slot::from_i32(8);
    copy.localstack_used = 4;
    copy.xfunction = 1;
    copy.xstatement = 2;
    copy.argc = 3;
    copy.trace_flag = true;
    assert_ne!(format!("{copy:?}"), format!("{snapshot:?}"));

    copy.clone_from(&snapshot);
    assert_eq!(format!("{copy:?}"), format!("{snapshot:?}"));

    let mut other = machine_with_globals(300);
    other.clone_from(&snapshot);
    assert_eq!(format!("{other:?}"), format!("{snapshot:?}"));
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
    assert_eq!(machine.globals()[101].bits(), 0x3F80_0000);
}

// spec VM-19.6: after a runtime error the call depth is 0 but the local stack is not reset:
// the slots the aborted functions saved stay in use.
#[test]
fn error_resets_depth_but_not_localstack() {
    use harness::{Func, Stmt};
    // f1 (one local) calls f2 (one local), whose first statement is a bad opcode, so two
    // functions have saved one slot each when the error aborts the run.
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
        ])
        .statements(&[
            Stmt::op("CALL0", 100, 0, 0), // 1: f1 calls f2
            Stmt::op("DONE", 0, 0, 0),    // 2
            Stmt::raw(66, 0, 0, 0),       // 3: f2: Bad opcode 66
        ])
        .globals_init(&[(100, 2)])
        .build();
    let progs = Progs::read_with(
        &bytes,
        &qc_progs::ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
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
    assert_eq!(error.function, 2);
    assert_eq!(machine.depth(), 0);
    assert_eq!(machine.localstack_used(), 2);
}

use crate::trace::Trace;
use harness::StubBuiltins;
use qc_trace::Record;
use qc_types::Slot;

/// A dispatch that has every builtin number, as vkQuake's sends a number past its table to
/// `PF_Fixme`, and records the number it was called with in global 60.
struct Unlimited;

impl Builtins for Unlimited {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        let number = i32::try_from(number).unwrap();
        context
            .machine()
            .set_global_test(60, Slot::from_i32(number));
        Ok(())
    }

    fn engine_string(&self, _value: i32) -> Option<&[u8]> {
        None
    }

    fn limit(&self) -> Option<u32> {
        None
    }
}

/// A run of function 1, which calls builtin `number` (function 2) and returns.
fn call_builtin(number: i32, builtins: &mut impl Builtins) -> Result<Machine, RunError> {
    use harness::{Func, Stmt};
    let bytes = harness::ProgsBuilder::default()
        .functions(vec![
            Func {
                first_statement: 1,
                ..Func::default()
            },
            Func {
                first_statement: number.saturating_neg(),
                ..Func::default()
            },
        ])
        .statements(&[Stmt::op("CALL0", 100, 0, 0), Stmt::op("DONE", 0, 0, 0)])
        .globals_init(&[(100, 2)])
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
        .execute_program(&progs, ServerState::Active, builtins, &mut NoObserver, 1)
        .map(|()| machine)
}

// spec VM-14.7: id's table ends below LIMIT_NUM_BUILTINS, so a call of 79 is `Bad builtin
// call number` and 78 reaches the dispatch; a dispatch with no limit is called with any number
#[test]
fn the_dispatch_decides_which_builtin_numbers_exist() {
    let bad = call_builtin(79, &mut harness::StubBuiltins).unwrap_err();
    assert_eq!(bad.kind, ErrorKind::BadBuiltin);
    assert!(call_builtin(78, &mut harness::StubBuiltins).is_ok());
    assert_eq!(
        harness::StubBuiltins.limit(),
        Some(crate::LIMIT_NUM_BUILTINS)
    );
    for number in [79, 233, 1_000_000] {
        let machine = call_builtin(number, &mut Unlimited).unwrap();
        assert_eq!(machine.global(60).unwrap().as_i32(), number);
    }
}

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

// spec VM-2.3: a run clears the statement trace flag when it starts
// spec VM-18.3: a nested run clears the statement trace flag, so tracing started before it is
// off after it.
#[test]
fn nested_run_clears_trace_flag() {
    /// Turns tracing on, then runs function 3 nested, as `traceon(); f3();` from a builtin.
    struct TraceThenNest {
        before_nested: Option<bool>,
        after_nested: Option<bool>,
    }
    impl Builtins for TraceThenNest {
        fn call(&mut self, _number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
            context.machine().set_tracing(true);
            self.before_nested = Some(context.machine_view().tracing());
            context.execute_program(self, 3)?;
            self.after_nested = Some(context.machine_view().tracing());
            Ok(())
        }
        fn engine_string(&self, _value: i32) -> Option<&[u8]> {
            None
        }
    }
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
    machine.set_tracing(true);
    assert!(machine.tracing());
    let mut builtins = TraceThenNest {
        before_nested: None,
        after_nested: None,
    };
    machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut builtins,
            &mut NoObserver,
            1,
        )
        .unwrap();
    assert_eq!(builtins.before_nested, Some(true), "set_tracing set it");
    assert_eq!(
        builtins.after_nested,
        Some(false),
        "the nested run cleared it"
    );
    assert!(
        !machine.tracing(),
        "nothing set it again after the nested run"
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
    assert_eq!(machine.globals()[103].bits(), 0);
    // Depth is reset to 0 (VM-19.6).
    assert_eq!(machine.depth(), 0);
}

// ===== accessor, Display, opcode-detail and trace coverage (P1-65) =====

mod coverage {
    //! Tests that pin the VM's accessors, error Display, opcode details and the trace
    //! observer's reads, so the readiness mutation score reaches the ADR-0009 bar. Each cites
    //! the statement it pins.

    use qc_edict::{EdictLayout, EdictStore};
    use qc_progs::{Progs, ReadOptions};
    use qc_types::Slot;

    use super::harness::{self, Func, Stmt};
    use crate::exec::{NoBuiltins, NoObserver, Observer, ServerState};
    use crate::trace::Trace;
    use crate::{ErrorKind, Machine, MachineError, RunError};

    fn read(bytes: &[u8]) -> Progs<'_> {
        Progs::read_with(
            bytes,
            &ReadOptions {
                check_header_crc: false,
            },
        )
        .unwrap()
    }

    /// Runs one program with the given builtins and observer, returning the machine and result.
    fn run(bytes: &[u8]) -> (Machine, Result<(), RunError>) {
        let progs = read(bytes);
        let mut machine = Machine::new(&progs, 0).unwrap();
        let result = machine.execute_program(
            &progs,
            ServerState::Active,
            &mut NoBuiltins,
            &mut NoObserver,
            1,
        );
        (machine, result)
    }

    // spec VM-19.1: a RunError's Display is its kind's message (the text PR_RunError prints),
    // not an empty string.
    #[test]
    fn run_error_display_is_the_message() {
        let bytes = harness::ProgsBuilder::default()
            .statements(&[Stmt::raw(66, 0, 0, 0)])
            .build();
        let (_m, result) = run(&bytes);
        let error = result.unwrap_err();
        assert_eq!(format!("{error}"), "Bad opcode 66");
        assert_eq!(error.message(), "Bad opcode 66");
    }

    // spec EDICT-2.4, EDICT-5.1: a MachineError forwards its inner error's message through
    // Display (not the Default empty string).
    #[test]
    fn machine_error_display_forwards() {
        // A layout error: too few globals.
        let layout_err = EdictLayout::new(105, 10).unwrap_err();
        let machine_err = MachineError::Layout(layout_err);
        assert_eq!(format!("{machine_err}"), layout_err.to_string());
        assert!(!machine_err.to_string().is_empty());
        // An edict error: maxclients out of range.
        let layout = EdictLayout::new(105, 128).unwrap();
        let edict_err = EdictStore::new(layout, 17).unwrap_err();
        let machine_err2 = MachineError::Edict(edict_err);
        assert_eq!(format!("{machine_err2}"), edict_err.to_string());
    }

    // spec VM-14.7: NoBuiltins::call fails every builtin call with the VM-14.7 divergence, so a
    // progs that calls a builtin under NoBuiltins is a runtime error (not a silent Ok).
    #[test]
    fn no_builtins_call_errors() {
        // Function 2 is builtin 7; function 1 calls it.
        let bytes = harness::ProgsBuilder::default()
            .functions(vec![
                Func {
                    first_statement: 1,
                    ..Func::default()
                },
                Func {
                    first_statement: -7,
                    ..Func::default()
                },
            ])
            .statements(&[
                Stmt::op("CALL0", 100, 0, 0), // call function 2 (builtin 7)
                Stmt::op("DONE", 0, 0, 0),
            ])
            .globals_init(&[(100, 2)]) // function 2 index
            .build();
        let (_m, result) = run(&bytes);
        let error = result.unwrap_err();
        assert!(
            matches!(error.kind, ErrorKind::Divergence(d) if d.statement() == "VM-14.7"),
            "NoBuiltins fails a builtin call with VM-14.7, got {:?}",
            error.kind
        );
    }

    // spec VM-1.4: set_global3 writes all three slots when they are in range and reads back;
    // an out-of-range triple writes nothing and errors.
    #[test]
    fn set_global3_writes_and_bounds_check() {
        let bytes = harness::ProgsBuilder::default()
            .statements(&[Stmt::op("DONE", 0, 0, 0)])
            .build();
        let progs = read(&bytes);
        let mut machine = Machine::new(&progs, 0).unwrap();
        machine
            .set_global3(
                100,
                [
                    Slot::from_f32(1.0),
                    Slot::from_f32(2.0),
                    Slot::from_f32(3.0),
                ],
            )
            .unwrap();
        assert_eq!(machine.global3(100).unwrap()[0].as_f32(), 1.0);
        assert_eq!(machine.global3(100).unwrap()[1].as_f32(), 2.0);
        assert_eq!(machine.global3(100).unwrap()[2].as_f32(), 3.0);
        // A slot past the globals (128 slots, 0..127) errors and writes nothing.
        assert!(machine.set_global3(200, [Slot::ZERO; 3]).is_err());
    }

    /// An observer that records the deepest call depth seen during a run.
    #[derive(Default)]
    struct MaxDepth {
        max: usize,
    }

    impl Observer for MaxDepth {
        fn run(&mut self, machine: &Machine, _function: i32) {
            self.max = self.max.max(machine.depth());
        }
        fn step(&mut self, machine: &Machine, _f: i32, _s: i32, _o: u16, _b: Option<u32>) {
            self.max = self.max.max(machine.depth());
        }
        fn exit(&mut self, _machine: &Machine) {}
        fn error(&mut self, _machine: &Machine, _error: &RunError) {}
    }

    // spec LIMIT-2.1: depth() reports the active QC frame count; inside a called function it is
    // greater than 0 (a mutant returning a constant 0 is caught by observing mid-run).
    #[test]
    fn depth_is_positive_inside_a_run() {
        // f1 calls f2; f2 does a store then returns. During f2 the depth is 2.
        let bytes = harness::ProgsBuilder::default()
            .functions(vec![
                Func {
                    first_statement: 1,
                    ..Func::default()
                },
                Func {
                    first_statement: 3,
                    ..Func::default()
                },
            ])
            .statements(&[
                Stmt::op("CALL0", 100, 0, 0),     // 1: f1 calls f2
                Stmt::op("DONE", 0, 0, 0),        // 2
                Stmt::op("STORE_F", 101, 102, 0), // 3: f2 body
                Stmt::op("DONE", 0, 0, 0),        // 4
            ])
            .globals_init(&[(100, 2)]) // f2 index
            .build();
        let progs = read(&bytes);
        let mut machine = Machine::new(&progs, 0).unwrap();
        let mut observer = MaxDepth::default();
        machine
            .execute_program(
                &progs,
                ServerState::Active,
                &mut NoBuiltins,
                &mut observer,
                1,
            )
            .unwrap();
        assert!(
            observer.max >= 2,
            "depth reaches 2 inside f2, got {}",
            observer.max
        );
        assert_eq!(machine.depth(), 0);
    }

    // spec TRACE-4.1, TRACE-3.4: the Trace observer records the run and its changes, and
    // reports the edict field count; records() is non-empty after a run and entityfields() is
    // the progs' field count.
    #[test]
    fn trace_records_and_entityfields() {
        let bytes = harness::ProgsBuilder::default()
            .statements(&[Stmt::op("STORE_F", 100, 101, 0), Stmt::op("DONE", 0, 0, 0)])
            .globals_init(&[(100, 0x3F80_0000)]) // 1.0
            .build();
        let progs = read(&bytes);
        let mut machine = Machine::new(&progs, 0).unwrap();
        let mut trace = Trace::new(&machine, &progs).unwrap();
        machine
            .execute_program(&progs, ServerState::Active, &mut NoBuiltins, &mut trace, 1)
            .unwrap();
        assert!(
            !trace.records().is_empty(),
            "a run records at least its run and step records"
        );
        // The default harness has 110 entity fields.
        assert_eq!(trace.entityfields(), 110);
    }

    // spec VM-9.1: OR and AND of two integer-valued floats are a bitwise OR and AND; with
    // operands that overlap in some bits, OR differs from XOR, pinning `a | b`.
    #[test]
    fn bitwise_or_and() {
        // 0b1100 | 0b1010 = 0b1110 (14); XOR would be 0b0110 (6).
        let bytes = harness::ProgsBuilder::default()
            .statements(&[Stmt::op("BITOR", 100, 101, 102), Stmt::op("DONE", 0, 0, 0)])
            .globals_init(&[(100, 0x4140_0000), (101, 0x4120_0000)]) // 12.0, 10.0
            .build();
        let (machine, result) = run(&bytes);
        result.unwrap();
        assert_eq!(machine.globals()[102].as_f32(), 14.0);

        // 12 & 10 = 8.
        let bytes2 = harness::ProgsBuilder::default()
            .statements(&[Stmt::op("BITAND", 100, 101, 102), Stmt::op("DONE", 0, 0, 0)])
            .globals_init(&[(100, 0x4140_0000), (101, 0x4120_0000)])
            .build();
        let (machine2, result2) = run(&bytes2);
        result2.unwrap();
        assert_eq!(machine2.globals()[102].as_f32(), 8.0);
    }

    // spec VM-7.1, VM-7.2: EQ_S and NE_S compare strings by content; two equal strings compare
    // equal and two different ones unequal, and a string compared with itself is equal. Pins
    // strcmp's (None, None) terminal arm and its equality logic.
    #[test]
    fn string_compare_by_content() {
        // strings: "\0ab\0ab\0abc\0": ab@1, ab@4, abc@7.
        let mut strings = vec![0_u8];
        for s in [b"ab".as_slice(), b"ab", b"abc"] {
            strings.extend_from_slice(s);
            strings.push(0);
        }
        // EQ_S slot100 vs slot101 (both "ab" at different offsets) -> equal (1).
        let bytes = harness::ProgsBuilder::default()
            .strings(strings)
            .statements(&[
                Stmt::op("EQ_S", 100, 101, 103),
                Stmt::op("EQ_S", 100, 102, 104), // "ab" vs "abc" -> 0
                Stmt::op("NE_S", 100, 101, 105), // "ab" vs "ab" -> 0.0 (equal)
                Stmt::op("NE_S", 100, 102, 106), // "ab" vs "abc" -> -99 (NUL - 'c'), VM-7.2
                Stmt::op("DONE", 0, 0, 0),
            ])
            .globals_init(&[(100, 1), (101, 4), (102, 7)])
            .build();
        let (machine, result) = run(&bytes);
        result.unwrap();
        assert_eq!(machine.globals()[103].as_f32(), 1.0, "ab == ab");
        assert_eq!(machine.globals()[104].as_f32(), 0.0, "ab != abc");
        // VM-7.2: NE_S writes strcmp's byte difference: 0 for equal, else first differing byte.
        assert_eq!(machine.globals()[105].as_f32(), 0.0, "ab NE ab is 0");
        assert_eq!(
            machine.globals()[106].as_f32(),
            -99.0,
            "ab NE abc is NUL-'c'"
        );
    }

    // spec VM-6.2: GT is a strict greater-than, so equal operands compare false (not true); a
    // `>` to `>=` mutant is caught by the equal case.
    #[test]
    fn float_gt_is_strict() {
        let bytes = harness::ProgsBuilder::default()
            .statements(&[Stmt::op("GT", 100, 101, 102), Stmt::op("DONE", 0, 0, 0)])
            .globals_init(&[(100, 0x4040_0000), (101, 0x4040_0000)]) // 3.0, 3.0
            .build();
        let (machine, result) = run(&bytes);
        result.unwrap();
        assert_eq!(machine.globals()[102].as_f32(), 0.0, "3 > 3 is false");
    }
}
