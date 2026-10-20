// SPDX-License-Identifier: GPL-3.0-or-later

//! The call depth an error leaves behind: a runtime error resets it (VM-19.1, VM-19.3), a host
//! error does not (VM-2.1, BUILTIN-3.2), so the next run starts where the error left it
//! (DIFF-8.4).
//!
//! Each test runs a chain of 30 QC functions whose last calls builtin 1, which raises the error
//! under test 30 calls deep, then starts a second run whose function makes one QC call: after
//! a host error that call is the 32nd frame and fails with `stack overflow`, as id's engine
//! does in the oracle scenario `depth-kept`.

#![expect(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or overflow is a failed test"
)]

use qc_progs::{Progs, ReadOptions};

use super::harness::{self, Func, Stmt};
use crate::exec::{Builtins, Context, NoObserver, ServerState};
use crate::{ErrorKind, Machine, RunError};

/// The number of QC functions on the stack when builtin 1 raises its error.
const CHAIN: i16 = 30;
/// The global slot holding the function the chain's function `k` calls is `CALLEE_BASE + k`.
const CALLEE_BASE: i16 = 100;
/// The function the second run starts: it calls [`LEAF`] once and is done.
const SHUTDOWN: i32 = 32;
/// The function [`SHUTDOWN`] calls.
const LEAF: i32 = 33;
/// The global slot holding [`LEAF`].
const LEAF_SLOT: i16 = 140;

/// Which error builtin 1 raises.
#[derive(Clone, Copy)]
enum Raise {
    /// A builtin's host error, `error` or `objerror` (BUILTIN-3.2).
    Host,
    /// A nested `PR_ExecuteProgram` of function 0: `NULL function` (VM-2.1).
    NullRun,
    /// A builtin's runtime error, through `PR_RunError` (BUILTIN-3.1, VM-19.1).
    Runtime,
}

impl Builtins for Raise {
    /// The progs calls only builtin 1, so the number is not looked at.
    fn call(&mut self, _number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        match *self {
            Self::Host => Err(context.run_error(ErrorKind::Host {
                message: "deep".to_owned(),
            })),
            Self::NullRun => context.execute_program(self, 0),
            Self::Runtime => Err(context.run_error(ErrorKind::Builtin {
                message: "deep".to_owned(),
            })),
        }
    }

    fn engine_string(&self, _value: i32) -> Option<&[u8]> {
        None
    }
}

/// The progs: functions 1 to 30 each call the next, function 30 calls builtin 1 (function 31),
/// function 32 calls function 33, which is done.
fn chain_progs() -> Vec<u8> {
    let mut functions = Vec::new();
    let mut statements = Vec::new();
    let mut globals = Vec::new();
    for k in 1..=CHAIN {
        functions.push(Func {
            first_statement: i32::from(2 * k - 1),
            ..Func::default()
        });
        let slot = CALLEE_BASE + k;
        statements.push(Stmt::op("CALL0", slot, 0, 0));
        statements.push(Stmt::op("DONE", 0, 0, 0));
        globals.push((
            usize::try_from(slot).unwrap(),
            u32::try_from(k + 1).unwrap(),
        ));
    }
    // Function 31: builtin 1.
    functions.push(Func {
        first_statement: -1,
        ..Func::default()
    });
    // Function 32 at statement 61, function 33 at statement 63.
    functions.push(Func {
        first_statement: 61,
        ..Func::default()
    });
    statements.push(Stmt::op("CALL0", LEAF_SLOT, 0, 0));
    statements.push(Stmt::op("DONE", 0, 0, 0));
    functions.push(Func {
        first_statement: 63,
        ..Func::default()
    });
    statements.push(Stmt::op("DONE", 0, 0, 0));
    globals.push((
        usize::try_from(LEAF_SLOT).unwrap(),
        u32::try_from(LEAF).unwrap(),
    ));
    harness::ProgsBuilder::default()
        .numglobals(256)
        .functions(functions)
        .statements(&statements)
        .globals_init(&globals)
        .build()
}

/// Runs the chain with builtin 1 raising `raise` and checks the error's kind; returns the
/// machine, the depth the error left and the result of the second run, of [`SHUTDOWN`].
fn error_then_run(raise: Raise, kind: &ErrorKind) -> (Machine, usize, Result<(), RunError>) {
    let bytes = chain_progs();
    let progs = Progs::read_with(
        &bytes,
        &ReadOptions {
            check_header_crc: false,
        },
    )
    .unwrap();
    let mut machine = Machine::new(&progs, 0).unwrap();
    let mut builtins = raise;
    let error = machine
        .execute_program(
            &progs,
            ServerState::Active,
            &mut builtins,
            &mut NoObserver,
            1,
        )
        .unwrap_err();
    assert_eq!(&error.kind, kind);
    let depth = machine.depth();
    let second = machine.execute_program(
        &progs,
        ServerState::Active,
        &mut builtins,
        &mut NoObserver,
        SHUTDOWN,
    );
    (machine, depth, second)
}

// spec BUILTIN-3.2, DIFF-8.4: a builtin's host error raised 30 calls deep leaves the depth at
// 30, so the next run's first QC call reaches LIMIT_MAX_STACK_DEPTH and fails with stack
// overflow.
#[test]
fn a_host_error_keeps_the_depth() {
    let host = ErrorKind::Host {
        message: "deep".to_owned(),
    };
    let (machine, depth, second) = error_then_run(Raise::Host, &host);
    assert_eq!(depth, 30);
    let error = second.unwrap_err();
    assert_eq!(error.kind, ErrorKind::StackOverflow);
    // The overflow fires entering LEAF, before it becomes current: the function is the caller.
    assert_eq!(error.function, SHUTDOWN);
    // The overflow is a runtime error: it resets the depth (VM-19.1).
    assert_eq!(machine.depth(), 0);
}

// spec VM-2.1: PR_ExecuteProgram's NULL function goes straight to Host_Error, so a NULL run
// started 30 calls deep leaves the depth at 30 and the next run's first QC call overflows.
#[test]
fn a_null_run_function_keeps_the_depth() {
    let (_, depth, second) = error_then_run(Raise::NullRun, &ErrorKind::NullRunFunction);
    assert_eq!(depth, 30);
    assert_eq!(second.unwrap_err().kind, ErrorKind::StackOverflow);
}

// spec VM-19.1, VM-19.3: a runtime error 30 calls deep resets the depth to 0, so the next run
// makes its call and returns, back at depth 0.
#[test]
fn a_runtime_error_resets_the_depth() {
    let runtime = ErrorKind::Builtin {
        message: "deep".to_owned(),
    };
    let (machine, depth, second) = error_then_run(Raise::Runtime, &runtime);
    assert_eq!(depth, 0);
    assert!(second.is_ok(), "{second:?}");
    assert_eq!(machine.depth(), 0);
}

// spec VM-2.1, BUILTIN-3.2: the errors that call Host_Error or Sys_Error directly keep the
// depth; every error through PR_RunError resets it (VM-19.1), and only Sys_Error is fatal.
#[test]
fn which_errors_reset_the_depth() {
    let keep = [
        ErrorKind::NullRunFunction,
        ErrorKind::Host {
            message: String::new(),
        },
        ErrorKind::Fatal {
            message: String::new(),
        },
    ];
    for kind in &keep {
        assert!(!kind.resets_depth(), "{kind:?}");
    }
    let reset = [
        ErrorKind::NullCallFunction,
        ErrorKind::StackOverflow,
        ErrorKind::LocalsStackOverflow,
        ErrorKind::Runaway,
        ErrorKind::BadBuiltin,
        ErrorKind::BadOpcode { opcode: 70 },
        ErrorKind::AssignmentToWorld,
        ErrorKind::Divergence {
            statement: "VM-3.3",
            detail: String::new(),
        },
        ErrorKind::Builtin {
            message: String::new(),
        },
    ];
    for kind in &reset {
        assert!(kind.resets_depth(), "{kind:?}");
        assert!(!kind.is_fatal(), "{kind:?}");
    }
    let fatal: Vec<bool> = keep.iter().map(ErrorKind::is_fatal).collect();
    assert_eq!(fatal, [false, false, true]);
}
