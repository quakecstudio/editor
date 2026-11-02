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
/// divergences of VM-19.7), and the trace is the one `PR_StackTrace` would print, innermost
/// first. The error ends every run at every level of nesting (VM-19.2); the machine's globals
/// and local stack keep the values they had when it happened (VM-19.6).
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
/// Implements VM-19.1 (the messages id's VM raises), VM-3.4 (`Bad opcode`), VM-14.7
/// (`Bad builtin call number`) and VM-19.7 (the divergences, each with the implementation's
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
    /// `Bad builtin call number` (VM-14.7, LIMIT-5.3): the builtin number is
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
    /// A divergence where id's C is undefined and this spec fixes an error (VM-19.7). The
    /// message is this VM's own.
    Divergence(Divergence),
    /// A runtime error a builtin raised with id's verbatim message (BUILTIN-3.1, DIFF-8.2).
    ///
    /// Unlike [`Divergence`](Self::Divergence), the message is printed byte for byte so a
    /// builtin error (`invalid entity`, `no precache: <m>`, `backwards mins/maxs`, and the
    /// fixed-answer errors `invalid string`, `invalid field`, `invalid lightstyle` of
    /// DIFF-6.6) appears in the trace exactly as both differential runners write it.
    Builtin {
        /// The message, exactly as it goes into the trace's error record (TRACE-4.8).
        message: String,
    },
    /// A host error a builtin raised, id's `Host_Error ("Program error")` called without
    /// `PR_RunError` (BUILTIN-3.2): `error` and `objerror`.
    ///
    /// Unlike a runtime error it leaves the call depth where it was (BUILTIN-3.2, DIFF-8.4).
    /// The message is the joined text the builtin was given (DIFF-8.2), not `Program error`.
    Host {
        /// The message, exactly as it goes into the trace's error record (DIFF-8.2).
        message: String,
    },
    /// A fatal error a builtin raised, id's `Sys_Error` (BUILTIN-3.3): the engine exits, so
    /// nothing after it runs and no shutdown runs QuakeC (DIFF-8.6).
    Fatal {
        /// The message `Sys_Error` is given, as it goes into the trace's error record (DIFF-8.2).
        message: String,
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
            Self::Divergence(divergence) => {
                !matches!(divergence, Divergence::StatementOutOfRange { .. })
            }
            _ => true,
        }
    }

    /// Whether the error went through id's `PR_RunError`, which sets the call depth to 0
    /// before `Host_Error` (VM-19.1, VM-19.6).
    ///
    /// Every runtime error does (VM-19.1, VM-19.7, BUILTIN-3.1). `PR_ExecuteProgram: NULL
    /// function` (VM-2.1, VM-2.2) and a builtin's host error (BUILTIN-3.2) call `Host_Error`
    /// directly, and a fatal error (BUILTIN-3.3) calls `Sys_Error`: none of them resets the
    /// depth, so after one the depth is still the depth at the error (DIFF-8.4).
    #[must_use]
    pub const fn resets_depth(&self) -> bool {
        match self {
            Self::NullRunFunction | Self::Host { .. } | Self::Fatal { .. } => false,
            Self::NullCallFunction
            | Self::StackOverflow
            | Self::LocalsStackOverflow
            | Self::Runaway
            | Self::BadBuiltin
            | Self::BadOpcode { .. }
            | Self::AssignmentToWorld
            | Self::Divergence(_)
            | Self::Builtin { .. } => true,
        }
    }

    /// Whether the error is fatal, id's `Sys_Error` (BUILTIN-3.3): the engine exits, so a host
    /// runs no shutdown after it (DIFF-8.6).
    #[must_use]
    pub const fn is_fatal(&self) -> bool {
        match self {
            Self::Fatal { .. } => true,
            Self::NullRunFunction
            | Self::NullCallFunction
            | Self::StackOverflow
            | Self::LocalsStackOverflow
            | Self::Runaway
            | Self::BadBuiltin
            | Self::BadOpcode { .. }
            | Self::AssignmentToWorld
            | Self::Divergence(_)
            | Self::Builtin { .. }
            | Self::Host { .. } => false,
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
            Self::Divergence(divergence) => write!(f, "{divergence}"),
            Self::Builtin { message } | Self::Host { message } | Self::Fatal { message } => {
                f.write_str(message)
            }
        }
    }
}

/// A runtime error where id's C is undefined and this spec fixes one (VM-19.7), one variant
/// per statement that fixes it.
///
/// Its `Display` is the VM's own message followed by the statement in parentheses, the text
/// that reaches a trace's error record (TRACE-4.8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Divergence {
    /// VM-1.4: an operand names a global slot outside the globals.
    GlobalOutOfRange {
        /// The slot.
        slot: i32,
    },
    /// VM-1.4: a vector operand's base plus a component offset overflows.
    GlobalOffsetOutOfRange {
        /// The operand's slot.
        base: i32,
        /// The component offset.
        offset: usize,
    },
    /// VM-2.4: `PR_ExecuteProgram` was given a builtin.
    RunBuiltin,
    /// VM-3.3: the next statement index is outside the statements.
    StatementOutOfRange {
        /// The statement index.
        statement: i32,
    },
    /// VM-7.3: a string operand names neither the string table nor an engine string.
    BadString {
        /// The string value.
        value: i32,
    },
    /// VM-11.4: a `LOAD` whose entity or field is out of range.
    Load(FieldFault),
    /// VM-12.5: an `ADDRESS` or `STOREP` whose entity, field or pointer is out of range.
    Pointer(FieldFault),
    /// VM-14.3: a `CALL` names a function index outside the functions.
    CallOutOfRange {
        /// The function index.
        index: i32,
    },
    /// VM-14.7: a builtin number the host has no builtin for.
    UnregisteredBuiltin {
        /// The builtin number.
        number: u32,
    },
    /// VM-15.6: the function being entered has a negative `locals`.
    NegativeLocals {
        /// The function's `locals`.
        locals: i32,
    },
    /// VM-17.4: `STATE`'s `self` does not name an edict.
    StateSelf,
}

impl Divergence {
    /// The spec statement that fixes this error.
    #[must_use]
    pub const fn statement(self) -> &'static str {
        match self {
            Self::GlobalOutOfRange { .. } | Self::GlobalOffsetOutOfRange { .. } => "VM-1.4",
            Self::RunBuiltin => "VM-2.4",
            Self::StatementOutOfRange { .. } => "VM-3.3",
            Self::BadString { .. } => "VM-7.3",
            Self::Load(_) => "VM-11.4",
            Self::Pointer(_) => "VM-12.5",
            Self::CallOutOfRange { .. } => "VM-14.3",
            Self::UnregisteredBuiltin { .. } => "VM-14.7",
            Self::NegativeLocals { .. } => "VM-15.6",
            Self::StateSelf => "VM-17.4",
        }
    }
}

impl fmt::Display for Divergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GlobalOutOfRange { slot } => write!(f, "global slot {slot} out of range"),
            Self::GlobalOffsetOutOfRange { base, offset } => {
                write!(f, "global slot {base} + {offset} out of range")
            }
            Self::RunBuiltin => f.write_str("PR_ExecuteProgram on a builtin"),
            Self::StatementOutOfRange { statement } => {
                write!(f, "statement index {statement} out of range")
            }
            Self::BadString { value } => write!(
                f,
                "string value {value} is neither a table offset nor an engine string"
            ),
            Self::Load(fault) | Self::Pointer(fault) => write!(f, "{fault}"),
            Self::CallOutOfRange { index } => write!(f, "call function index {index} out of range"),
            Self::UnregisteredBuiltin { number } => write!(f, "no builtin {number} is registered"),
            Self::NegativeLocals { locals } => write!(f, "function locals {locals} is negative"),
            Self::StateSelf => f.write_str("self does not name an edict"),
        }?;
        write!(f, " ({})", self.statement())
    }
}

/// What was out of range in a field access (VM-11.4, VM-12.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldFault {
    /// The entity value does not name an edict.
    NotAnEdict,
    /// The field index is outside the edict's fields.
    FieldOutOfRange,
    /// The edict has no slot at the field index.
    FieldSlotMissing,
    /// A vector field's last slot is outside the edict's fields.
    VectorFieldOutOfRange,
    /// The pointer value is not representable.
    PointerOutOfRange,
    /// The pointer does not name a field slot of an edict.
    PointerNotAField,
    /// A vector pointer's last slot is outside the edict's fields.
    VectorPointerOutOfRange,
}

impl fmt::Display for FieldFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotAnEdict => "entity value does not name an edict",
            Self::FieldOutOfRange => "field index out of range",
            Self::FieldSlotMissing => "field slot missing",
            Self::VectorFieldOutOfRange => "vector field slot out of range",
            Self::PointerOutOfRange => "pointer out of range",
            Self::PointerNotAField => "pointer does not name a field slot",
            Self::VectorPointerOutOfRange => "vector pointer out of range",
        })
    }
}
