// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_exec.c, PR_RunError, PR_StackTrace

//! Runtime errors and the QuakeC stack trace they carry.
//!
//! Implements VM-19: a runtime error from the VM, like one from a builtin, goes through id's
//! `PR_RunError`, which prints the current statement, the call stack from the innermost
//! function out, then the message, and then ends every run (VM-19.1, VM-19.2). This VM returns
//! that information as a [`RunError`] value instead of printing it and aborting the process.

use core::fmt;

/// One frame of the QuakeC stack trace id's `PR_StackTrace` prints, innermost first.
///
/// Implements VM-19.1: `PR_StackTrace` prints each active function as `<file> : <function>`,
/// the names resolved through the progs' string table. The indices are kept so a caller can
/// resolve the names however it formats the trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackEntry {
    /// The function's index in the functions table.
    pub function: i32,
    /// The statement current in that function when the error happened or it called deeper.
    pub statement: i32,
    /// The function's name, resolved from the progs' string table, or empty when it has none.
    pub name: String,
    /// The function's source file name, resolved from the string table, or empty.
    pub file: String,
}

/// A runtime error that ended every active run, id's `PR_RunError` then `Host_Error`.
///
/// Implements VM-19: the kind names which error it is (the messages of VM-19.1 and the
/// divergences of VM-19.4), and the trace is the one `PR_StackTrace` would print, innermost
/// first. The error ends every run at every level of nesting (VM-19.2); the machine's globals
/// and local stack keep the values they had when it happened (VM-19.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunError {
    /// Which error it is and its message.
    pub kind: ErrorKind,
    /// The function current when the error was raised, or the run's function for an error
    /// before its first statement (VM-2.1, VM-2.2, VM-2.4).
    pub function: i32,
    /// The statement current when the error was raised, or -1 for an error before the run's
    /// first statement (TRACE-4.7).
    pub statement: i32,
    /// The QC call stack, innermost first, as `PR_StackTrace` prints it.
    pub trace: Vec<StackEntry>,
}

impl RunError {
    /// The error's message, as id prints it or as this spec fixes it for a divergence.
    #[must_use]
    pub fn message(&self) -> String {
        self.kind.to_string()
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl core::error::Error for RunError {}

/// Which runtime error ended the run, with id's message or this spec's for a divergence.
///
/// Implements VM-19.1 (the messages id's VM raises), VM-3.4 (`Bad opcode`), VM-14.4
/// (`Bad builtin call number`) and VM-19.4 (the divergences, each with the implementation's
/// own message, since id has none).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// `PR_ExecuteProgram: NULL function` (VM-2.1, VM-2.2): the run's function index is 0,
    /// negative or not less than `numfunctions`.
    NullRunFunction,
    /// `NULL function` (VM-14.2): a `CALL` named function index 0.
    NullCallFunction,
    /// `stack overflow` (VM-15.1): entering made the call depth reach `LIMIT_MAX_STACK_DEPTH`.
    StackOverflow,
    /// `PR_ExecuteProgram: locals stack overflow` (VM-15.2): the local stack cannot hold the
    /// callee's locals.
    LocalsStackOverflow,
    /// `runaway loop error` (VM-3.1): the run's runaway counter reached 0.
    Runaway,
    /// `Bad builtin call number` (VM-14.4, LIMIT-5.3): the builtin number is
    /// `LIMIT_NUM_BUILTINS` or more, or has no positive value.
    BadBuiltin,
    /// `Bad opcode <op>` (VM-3.4): the opcode is above 65 (`OP_BITOR`).
    BadOpcode {
        /// The opcode as stored, printed as an unsigned decimal (PROGS-7.4).
        opcode: u32,
    },
    /// `assignment to world entity` (VM-12.2): `ADDRESS` on the world while the server is
    /// active.
    AssignmentToWorld,
    /// A divergence where id's C is undefined and this spec fixes an error (VM-19.4). The
    /// message is this VM's own.
    Divergence {
        /// The spec statement that fixes this error.
        statement: &'static str,
        /// What went wrong, as this VM phrases it.
        detail: String,
    },
}

impl ErrorKind {
    /// Whether this error was raised by a statement that ran (an opcode failure, a call or
    /// address error, an enter-time overflow from a `CALL`, a divergence raised while
    /// executing a statement), as opposed to the runaway guard (which fires before the next
    /// statement and points back to the last one) or the out-of-range fetch of VM-3.3 (where
    /// the statement does not exist). An executed statement with no step record counts as one
    /// more statement executed and is listed as the last statement line (TRACE-4.6, and the
    /// vectors' README `steps:`).
    #[must_use]
    pub fn is_executed_statement(&self) -> bool {
        match self {
            Self::Runaway => false,
            Self::Divergence { statement, .. } => *statement != "VM-3.3",
            _ => true,
        }
    }
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NullRunFunction => f.write_str("PR_ExecuteProgram: NULL function"),
            Self::NullCallFunction => f.write_str("NULL function"),
            Self::StackOverflow => f.write_str("stack overflow"),
            Self::LocalsStackOverflow => f.write_str("PR_ExecuteProgram: locals stack overflow"),
            Self::Runaway => f.write_str("runaway loop error"),
            Self::BadBuiltin => f.write_str("Bad builtin call number"),
            Self::BadOpcode { opcode } => write!(f, "Bad opcode {opcode}"),
            Self::AssignmentToWorld => f.write_str("assignment to world entity"),
            Self::Divergence { statement, detail } => write!(f, "{detail} ({statement})"),
        }
    }
}
