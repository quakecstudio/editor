// SPDX-License-Identifier: GPL-3.0-or-later

//! String-opcode resolution of engine-allocated strings (VM-7.3, VM-8.3).
//!
//! `EQ_S`, `NE_S` and `NOT_S` resolve a string value that is a string-table offset against the
//! table and a value outside the table against the engine-allocated strings a [`Builtins`]
//! dispatch owns (PROGS-11.6, EDICT-13.1, EDICT-13.3). These tests drive the opcodes through a
//! small [`Builtins`] that returns engine text for negative values, as the mock engine does,
//! and pin the behaviour against id's `PR_GetString`, which resolves both.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_progs::Progs;
use qc_types::Slot;

use super::harness::{self, Stmt};
use crate::exec::{Context, NoObserver, ServerState};
use crate::{Builtins, ErrorKind, Machine, RunError};

/// A [`Builtins`] that holds engine-allocated strings by their negative value, as the mock
/// engine represents them (EDICT-13.3). `call` is never reached: these progs make no `CALL`.
#[derive(Clone, Debug, Default)]
struct EngineStringBuiltins {
    /// Each engine string's value and text, resolved by [`Builtins::engine_string`].
    entries: Vec<(i32, Vec<u8>)>,
}

impl EngineStringBuiltins {
    /// Adds an engine string at `value` (a value that cannot be a table offset) with `text`.
    fn with(mut self, value: i32, text: &[u8]) -> Self {
        self.entries.push((value, text.to_vec()));
        self
    }
}

impl Builtins for EngineStringBuiltins {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        Err(context.run_error(ErrorKind::Divergence {
            statement: "VM-14.4",
            detail: format!("no builtin {number} in this test"),
        }))
    }

    fn engine_string(&self, value: i32) -> Option<&[u8]> {
        self.entries
            .iter()
            .find(|(key, _)| *key == value)
            .map(|(_, text)| text.as_slice())
    }
}

/// Runs the statements with globals preset to the given bit patterns and the given engine
/// strings, returning the machine (or the error), as the vectors do but with a dispatch that
/// owns engine strings.
#[expect(
    clippy::unwrap_in_result,
    reason = "test code: a failed unwrap setting up the fixture is a failed test"
)]
fn run_with_engine(
    statements: &[Stmt],
    globals: &[(usize, u32)],
    builtins: &mut EngineStringBuiltins,
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
        .execute_program(&progs, ServerState::Active, builtins, &mut NoObserver, 1)
        .map(|()| machine)
}

/// The slot `c` holds after the run, as its raw bits.
fn result_bits(machine: &Machine, slot: i32) -> u32 {
    machine.global(slot).unwrap().bits()
}

// spec VM-7.3: EQ_S resolves two engine-allocated string values (outside the table) to their
// text; equal texts compare equal, so EQ_S writes 1.0.
#[test]
fn eq_s_equal_engine_strings_are_equal() {
    // Two distinct engine-string values holding the same text (as two map keys might).
    let mut builtins = EngineStringBuiltins::default()
        .with(-1, b"e1m1")
        .with(-2, b"e1m1");
    let machine = run_with_engine(
        &[
            // EQ_S slot 100, slot 101 -> slot 102, then DONE.
            Stmt::op("EQ_S", 100, 101, 102),
            Stmt::op("DONE", 0, 0, 0),
        ],
        // (-1) and (-2) as u32 bit patterns in slots 100 and 101.
        &[
            (100, (-1_i32).cast_unsigned()),
            (101, (-2_i32).cast_unsigned()),
        ],
        &mut builtins,
    )
    .unwrap();
    // VM-7.1: equal strings write 1.0.
    assert_eq!(result_bits(&machine, 102), Slot::from_f32(1.0).bits());
}

// spec VM-7.3: EQ_S resolves engine-allocated strings with different text to not equal, so it
// writes 0.0 rather than failing.
#[test]
fn eq_s_different_engine_strings_are_not_equal() {
    let mut builtins = EngineStringBuiltins::default()
        .with(-1, b"player")
        .with(-2, b"monster");
    let machine = run_with_engine(
        &[Stmt::op("EQ_S", 100, 101, 102), Stmt::op("DONE", 0, 0, 0)],
        &[
            (100, (-1_i32).cast_unsigned()),
            (101, (-2_i32).cast_unsigned()),
        ],
        &mut builtins,
    )
    .unwrap();
    // VM-7.1: different strings write 0.0.
    assert_eq!(result_bits(&machine, 102), Slot::from_f32(0.0).bits());
}

// spec VM-7.3: NE_S resolves an engine-allocated string value against a table offset and
// writes strcmp's byte difference; equal text (offset 0 and an empty engine string) is 0.0.
#[test]
fn ne_s_engine_string_against_table_offset() {
    // slot 100 is table offset 0 (the empty string); slot 101 is an empty engine string.
    let mut builtins = EngineStringBuiltins::default().with(-1, b"");
    let machine = run_with_engine(
        &[Stmt::op("NE_S", 100, 101, 102), Stmt::op("DONE", 0, 0, 0)],
        &[(100, 0), (101, (-1_i32).cast_unsigned())],
        &mut builtins,
    )
    .unwrap();
    // VM-7.2: equal strings (both empty) write 0.0.
    assert_eq!(result_bits(&machine, 102), Slot::from_f32(0.0).bits());
}

// spec VM-8.3: NOT_S on a non-empty engine-allocated string writes 0.0 (it is true), reading
// its text rather than failing (EDICT-13.2).
#[test]
fn not_s_nonempty_engine_string_is_false() {
    let mut builtins = EngineStringBuiltins::default().with(-1, b"start");
    let machine = run_with_engine(
        &[Stmt::op("NOT_S", 100, 0, 102), Stmt::op("DONE", 0, 0, 0)],
        &[(100, (-1_i32).cast_unsigned())],
        &mut builtins,
    )
    .unwrap();
    // VM-8.3: a non-empty string writes 0.0.
    assert_eq!(result_bits(&machine, 102), Slot::from_f32(0.0).bits());
}

// spec VM-8.3: NOT_S on an empty engine-allocated string writes 1.0 (its first byte is NUL).
#[test]
fn not_s_empty_engine_string_is_true() {
    let mut builtins = EngineStringBuiltins::default().with(-1, b"");
    let machine = run_with_engine(
        &[Stmt::op("NOT_S", 100, 0, 102), Stmt::op("DONE", 0, 0, 0)],
        &[(100, (-1_i32).cast_unsigned())],
        &mut builtins,
    )
    .unwrap();
    // VM-8.3: an empty string writes 1.0.
    assert_eq!(result_bits(&machine, 102), Slot::from_f32(1.0).bits());
}

// spec VM-7.3: a string value that is neither a table offset nor an engine-allocated string
// fails with the VM-19.7 runtime error rather than reading out-of-bounds memory.
#[test]
fn eq_s_unknown_value_is_runtime_error() {
    // The dispatch holds only -1; slot 101 names -5, which is no engine string.
    let mut builtins = EngineStringBuiltins::default().with(-1, b"known");
    let error = run_with_engine(
        &[Stmt::op("EQ_S", 100, 101, 102), Stmt::op("DONE", 0, 0, 0)],
        &[
            (100, (-1_i32).cast_unsigned()),
            (101, (-5_i32).cast_unsigned()),
        ],
        &mut builtins,
    )
    .unwrap_err();
    match error.kind {
        ErrorKind::Divergence { statement, .. } => assert_eq!(statement, "VM-7.3"),
        other => panic!("expected a VM-7.3 divergence, got {other:?}"),
    }
}

// spec VM-8.3: NOT_S on a value that is neither a table offset nor an engine string fails with
// the VM-19.7 runtime error.
#[test]
fn not_s_unknown_value_is_runtime_error() {
    let mut builtins = EngineStringBuiltins::default();
    let error = run_with_engine(
        &[Stmt::op("NOT_S", 100, 0, 102), Stmt::op("DONE", 0, 0, 0)],
        &[(100, (-9_i32).cast_unsigned())],
        &mut builtins,
    )
    .unwrap_err();
    match error.kind {
        ErrorKind::Divergence { statement, .. } => assert_eq!(statement, "VM-7.3"),
        other => panic!("expected a VM-7.3 divergence, got {other:?}"),
    }
}
