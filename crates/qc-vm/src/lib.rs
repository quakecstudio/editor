// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_exec.c, PR_ExecuteProgram, PR_EnterFunction, PR_LeaveFunction, PR_RunError, PR_StackTrace

//! The QuakeC virtual machine: id's `PR_ExecuteProgram` as a no-panic interpreter.
//!
//! [`Machine`] holds the state id's `pr_exec.c` keeps between runs (`vm.md` VM-1.1): the
//! globals, the edicts ([`qc_edict`]), the call stack, the local stack, the current function
//! and statement and the argument count of the last call. [`Machine::execute_program`] is id's
//! `PR_ExecuteProgram`: it runs one QC function, decrements a per-run runaway counter
//! (LIMIT-4), dispatches every vanilla opcode (VM-4 to VM-17), enters and leaves QC functions
//! (VM-15, VM-16), calls builtins through a [`Builtins`] dispatch (VM-14), and fails with a
//! [`RunError`] value, never a panic, on any input (VM-19).
//!
//! The VM runs untrusted `progs.dat`, so every operand, global slot, field slot, jump target
//! and call index is bounds- and type-checked exactly as the spec fixes id's undefined C
//! (VM-1.4, VM-2.2, VM-2.4, VM-3.3, VM-7.3, VM-11.4, VM-12.5, VM-14.3, VM-15.6): id reads or
//! writes out of bounds, this VM returns an error before changing any state (VM-19.7).
//!
//! A [`Trace`](trace::Trace) hook records what each statement did in the shared trace format
//! ([`qc_trace`], `trace.md`), so the differential runner can compare this VM against the id
//! VM oracle statement by statement.
//!
//! Implements `docs/spec/vm.md` (VM-1 to VM-19) and the VM limits of `docs/spec/limits.md`
//! (LIMIT-2 to LIMIT-5). The file format, opcode numbers and value encodings are in
//! [`qc_progs`] and [`qc_types`]; the edict store is [`qc_edict`].

pub mod convert;
mod error;
mod exec;
mod machine;
mod ops;
pub mod trace;

#[cfg(test)]
mod tests;

pub use error::{Divergence, ErrorKind, FieldFault, RunError, StackEntry};
pub use exec::{Builtins, Context, NoBuiltins, NoObserver, Observer, ServerState};
pub use machine::{GlobalAccessError, Machine, MachineError};

/// Slots of the local stack shared by every active QC function (VM-15.2).
///
/// `LIMIT_LOCALSTACK_SIZE` in LIMIT-3.1, re-exported from [`qc_progs::limits`].
pub use qc_progs::limits::LIMIT_LOCALSTACK_SIZE;
/// QC call depth at which entering a function fails with `stack overflow` (VM-15.1).
///
/// `LIMIT_MAX_STACK_DEPTH` in LIMIT-2.1, re-exported from [`qc_progs::limits`].
pub use qc_progs::limits::LIMIT_MAX_STACK_DEPTH;
/// Entries in id's builtin table, numbers 0 to 78; a higher number fails a call (VM-14.7).
///
/// `LIMIT_NUM_BUILTINS` in LIMIT-5.1, re-exported from [`qc_progs::limits`].
pub use qc_progs::limits::LIMIT_NUM_BUILTINS;
/// The runaway counter one [`Machine::execute_program`] call starts with, id's `runaway`.
///
/// `LIMIT_RUNAWAY` in LIMIT-4.1, re-exported from [`qc_progs::limits`].
pub use qc_progs::limits::LIMIT_RUNAWAY;
