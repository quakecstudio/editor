// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_exec.c, PR_ExecuteProgram, PR_EnterFunction, PR_LeaveFunction

//! The statement loop, entering and leaving functions, and calls.
//!
//! Implements VM-2 (starting a run), VM-3 (the statement loop), VM-14 (calls), VM-15
//! (entering), VM-16 (returning) and VM-18 (nested runs), the body of id's
//! `PR_ExecuteProgram`, `PR_EnterFunction` and `PR_LeaveFunction`.

use qc_progs::opcode::Opcode;
use qc_progs::{Function, Progs, Statement};
use qc_types::{PROGS_OFS_PARM0, PROGS_PARM_SLOTS};

use crate::error::{Divergence, ErrorKind, RunError, StackEntry};
use crate::machine::{Frame, Machine};
use crate::ops::{self, Control};

/// Whether the server is active or loading, which `ADDRESS` on the world checks (VM-12.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerState {
    /// `ss_loading`: a map is loading and spawn functions run; `ADDRESS` on the world
    /// succeeds (VM-12.2).
    Loading,
    /// `ss_active`: the server is running; `ADDRESS` on the world fails (VM-12.2).
    Active,
}

/// The builtin dispatch id's `pr_builtins` table provides (VM-14.4).
///
/// A `CALL` to a function whose `first_statement` is negative calls builtin `-first_statement`
/// (PROGS-10.2). The VM has already checked the number is 1 to
/// [`LIMIT_NUM_BUILTINS`](crate::LIMIT_NUM_BUILTINS) - 1 (VM-14.4) and set `pr_argc`
/// (VM-14.1); the implementation reads the parameter slots and `OFS_RETURN` through the
/// [`Machine`] and may start nested runs through [`Context`].
pub trait Builtins {
    /// Calls builtin `number` with argument count `argc` (already in [`Machine::argc`]).
    ///
    /// The implementation reads its arguments from the parameter slots `OFS_PARM0` up and
    /// writes its result to `OFS_RETURN` (PROGS-18). It returns `Ok(())` on success; an
    /// `Err(RunError)` ends every run as a builtin error does (VM-19.1).
    ///
    /// # Errors
    ///
    /// Whatever error the builtin raises; it ends every active run (VM-19.2).
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError>;

    /// The text of the engine-allocated string `value`, or `None` when `value` is not one.
    ///
    /// Implements PROGS-11.6 and EDICT-13.1: at run time a string value can point outside the
    /// string table, at text the engine allocated (`mapname`, `netname`, the world's `model`,
    /// an `ftos`/`vtos` buffer). The string opcodes `EQ_S`, `NE_S` and `NOT_S` (VM-7.3,
    /// VM-8.3) resolve such a value by asking the dispatch that owns the engine's strings for
    /// its text, so a comparison of an engine-made string reads its bytes instead of failing.
    /// The value is represented in a range that cannot be a string-table offset (EDICT-13.3).
    ///
    /// This is a required method so every dispatch decides it deliberately: one that makes no
    /// engine strings returns `None` for every value, which makes any string value outside the
    /// table the VM-19.7 runtime error (VM-7.3); one that allocates engine strings returns the
    /// text it holds for `value`. There is no default, so a dispatch that owns engine strings
    /// cannot silently fall back to `None` and fail every comparison of one at run time.
    fn engine_string(&self, value: i32) -> Option<&[u8]>;
}

/// A [`Builtins`] that has no builtins: every call fails as `Bad builtin call number` would be
/// unreachable here, so it is used only when a progs makes no builtin calls.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoBuiltins;

impl Builtins for NoBuiltins {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        Err(
            context.run_error(ErrorKind::Divergence(Divergence::UnregisteredBuiltin {
                number,
            })),
        )
    }

    /// `NoBuiltins` has no engine, so it makes no engine strings and resolves none (EDICT-13.1).
    fn engine_string(&self, _value: i32) -> Option<&[u8]> {
        None
    }
}

/// What a trace hook observes as a run proceeds (TRACE-4).
///
/// The VM calls these in the order the events happen so an observer can build a trace without
/// the VM depending on the trace format. The [`trace`](crate::trace) module has the observer
/// that builds a [`qc_trace::Trace`].
pub trait Observer {
    /// `PR_ExecuteProgram` was called with this function index (TRACE-4.1).
    fn run(&mut self, machine: &Machine, function: i32);
    /// A statement finished without an error (TRACE-4.2). `builtin` is the number a `CALL`
    /// called, when it called a builtin (TRACE-4.3).
    fn step(
        &mut self,
        machine: &Machine,
        function: i32,
        statement: i32,
        opcode: u16,
        builtin: Option<u32>,
    );
    /// `PR_ExecuteProgram` returned to its caller (TRACE-4.5).
    fn exit(&mut self, machine: &Machine);
    /// A runtime error ended the run (TRACE-4.6).
    fn error(&mut self, machine: &Machine, error: &RunError);
}

/// An [`Observer`] that records nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoObserver;

impl Observer for NoObserver {
    fn run(&mut self, _machine: &Machine, _function: i32) {}
    fn step(&mut self, _m: &Machine, _f: i32, _s: i32, _o: u16, _b: Option<u32>) {}
    fn exit(&mut self, _machine: &Machine) {}
    fn error(&mut self, _machine: &Machine, _error: &RunError) {}
}

/// The context a builtin runs in: the machine, the progs, the server state, the trace
/// observer and a way to start nested runs (VM-18).
///
/// It does not hold the [`Builtins`] dispatch: a builtin that starts a nested run passes its
/// own `self` back to [`Context::execute_program`], so the one trait object is threaded
/// through without a double mutable borrow.
pub struct Context<'a, 'p> {
    pub(crate) machine: &'a mut Machine,
    pub(crate) progs: &'a Progs<'p>,
    pub(crate) server: ServerState,
    pub(crate) observer: &'a mut dyn Observer,
}

impl Context<'_, '_> {
    /// The machine, for reading parameter slots and writing `OFS_RETURN`.
    #[must_use]
    pub fn machine(&mut self) -> &mut Machine {
        self.machine
    }

    /// The machine, read only: for a builtin that reads edicts or globals while it holds
    /// other state of its own borrowed.
    #[must_use]
    pub fn machine_view(&self) -> &Machine {
        self.machine
    }

    /// The loaded progs.
    #[must_use]
    pub fn progs(&self) -> &Progs<'_> {
        self.progs
    }

    /// The server state (VM-12.2).
    #[must_use]
    pub fn server(&self) -> ServerState {
        self.server
    }

    /// Starts a nested run of function `function`, id's `PR_ExecuteProgram` called from a
    /// builtin (VM-18).
    ///
    /// The nested run shares the call stack, call depth, local stack and globals with the
    /// outer run and has its own runaway counter and exit depth (VM-18.1). The builtin passes
    /// its own dispatch as `builtins` so the nested run's calls reach the same table.
    ///
    /// # Errors
    ///
    /// Whatever error the nested run raises; it ends every run (VM-18.4).
    pub fn execute_program(
        &mut self,
        builtins: &mut dyn Builtins,
        function: i32,
    ) -> Result<(), RunError> {
        run_inner(
            self.machine,
            self.progs,
            self.server,
            builtins,
            self.observer,
            function,
        )
    }

    /// Builds a [`RunError`] for an error a builtin raises, with the current stack trace.
    #[must_use]
    pub fn run_error(&self, kind: ErrorKind) -> RunError {
        self.machine.run_error(self.progs, kind)
    }
}

impl Machine {
    /// Runs QuakeC function `function`, id's `PR_ExecuteProgram` (VM-2 to VM-19).
    ///
    /// Starts a run (VM-2.3): sets a fresh runaway counter (LIMIT-4.1), clears the trace flag,
    /// records the exit depth, enters the function (VM-15) and runs the statement loop (VM-3)
    /// until a return brings the call depth back to the exit depth (VM-16.4). Builtins are
    /// dispatched through `builtins` (VM-14.4); `observer` records the run as a trace
    /// (TRACE-4).
    ///
    /// On a runtime error, id's `PR_RunError` sets the call depth to 0 but does not reset the
    /// local stack (VM-19.6); this VM does the same, so after one [`depth`](Machine::depth) is
    /// 0 and [`localstack_used`](Machine::localstack_used) keeps the aborted functions' slots.
    /// After a host error (`PR_ExecuteProgram: NULL function`, VM-2.1, or a builtin's, BUILTIN-3.2)
    /// or a fatal error (BUILTIN-3.3), which do not go through `PR_RunError`, the depth is
    /// still the depth at the error ([`ErrorKind::resets_depth`]), so a later run starts there
    /// (DIFF-8.4).
    ///
    /// # Errors
    ///
    /// A [`RunError`] on any runtime error (VM-19); the machine's globals keep the values they
    /// had when it happened (VM-19.6).
    pub fn execute_program(
        &mut self,
        progs: &Progs<'_>,
        server: ServerState,
        builtins: &mut dyn Builtins,
        observer: &mut dyn Observer,
        function: i32,
    ) -> Result<(), RunError> {
        // The outermost run emits the one error record (TRACE-4.6); a nested run started by a
        // builtin calls run_inner directly, so the record is written once (TRACE-4.9).
        let error = match run_inner(self, progs, server, builtins, observer, function) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        // VM-19.1: PR_RunError prints the current statement, the stack and the message, then
        // sets the depth to 0 ("dump the stack so host_error can shutdown functions"); the
        // local stack is not reset (VM-19.6). VM-2.1's NULL function and a builtin's host error
        // call Host_Error directly, and a fatal error Sys_Error, so the depth stays where the
        // error left it (BUILTIN-3.2, DIFF-8.4).
        observer.error(self, &error);
        if error.kind.resets_depth() {
            self.stack.clear();
        }
        Err(error)
    }
}

/// id's `PR_ExecuteProgram` body: one run, nested or not (VM-2, VM-3, VM-18). Returns the
/// error as a value without observing it or resetting the depth;
/// [`Machine::execute_program`] does that once.
pub(crate) fn run_inner(
    machine: &mut Machine,
    progs: &Progs<'_>,
    server: ServerState,
    builtins: &mut dyn Builtins,
    observer: &mut dyn Observer,
    function: i32,
) -> Result<(), RunError> {
    observer.run(machine, function);

    // VM-2.1, VM-2.2: fnum of 0, negative, or >= numfunctions fails with NULL function,
    // before any statement, with the run's function and statement -1 (TRACE-4.7).
    let callee = function_at(progs, function).ok_or_else(|| {
        before_first_statement(machine, progs, function, ErrorKind::NullRunFunction)
    })?;

    // VM-2.4: a builtin passed to PR_ExecuteProgram would start at a negative statement index,
    // undefined in C; this spec fails before any statement runs.
    if callee.first_statement < 0 {
        return Err(before_first_statement(
            machine,
            progs,
            function,
            ErrorKind::Divergence(Divergence::RunBuiltin),
        ));
    }

    // VM-2.3: the run's exit depth is the depth before entering, its own runaway counter and
    // its own cleared trace flag.
    let exit_depth = machine.stack.len();
    let mut runaway = crate::LIMIT_RUNAWAY;
    machine.trace_flag = false;

    // VM-15: enter the function. An error entering the run's own function is before its first
    // statement, so its function is the run's and its statement is -1 (TRACE-4.7).
    enter_function(machine, progs, function, &callee).map_err(|mut error| {
        error.function = function;
        error.statement = -1;
        error
    })?;

    loop {
        // VM-3.1: move to the next statement, count against runaway; the statement that
        // brings it to 0 fails without executing.
        machine.xstatement = machine.xstatement.wrapping_add(1);
        runaway = runaway.wrapping_sub(1);
        if runaway == 0 {
            return Err(machine.run_error(progs, ErrorKind::Runaway));
        }

        // VM-3.3: a statement index outside 0..numstatements fails before it executes.
        let statements = progs.statements();
        let Some(statement) = statement_at(&statements, machine.xstatement) else {
            return Err(machine.divergence(
                progs,
                Divergence::StatementOutOfRange {
                    statement: machine.xstatement,
                },
            ));
        };

        let step_function = machine.xfunction;
        let step_statement = machine.xstatement;

        // VM-3.4: an opcode above 65 fails with Bad opcode.
        let Some(opcode) = Opcode::from_number(statement.op) else {
            return Err(machine.run_error(
                progs,
                ErrorKind::BadOpcode {
                    opcode: statement.op,
                },
            ));
        };

        let control = ops::execute_statement(
            machine, progs, server, builtins, observer, opcode, &statement,
        )?;
        match control {
            Control::Next { builtin } => {
                observer.step(
                    machine,
                    step_function,
                    step_statement,
                    opcode.number(),
                    builtin,
                );
            }
            Control::Jump(target) => {
                observer.step(
                    machine,
                    step_function,
                    step_statement,
                    opcode.number(),
                    None,
                );
                // VM-13: the jump sets the next statement to target - 1, so the loop's +1
                // lands on it. A jump out of range is caught on the next iteration (VM-3.3,
                // VM-13.5).
                machine.xstatement = target.wrapping_sub(1);
            }
            Control::Return => {
                // VM-16: leave the function (copy the return value, restore the locals, pop
                // the frame) before the step record, so the restore is in the RETURN step's
                // change set (TRACE-3.1); id does the leave as part of OP_DONE.
                leave_function(machine, progs);
                observer.step(
                    machine,
                    step_function,
                    step_statement,
                    opcode.number(),
                    None,
                );
                if machine.stack.len() == exit_depth {
                    observer.exit(machine);
                    return Ok(());
                }
            }
        }
    }
}

/// id's `PR_EnterFunction` (VM-15).
pub(crate) fn enter_function(
    machine: &mut Machine,
    progs: &Progs<'_>,
    function: i32,
    callee: &Function,
) -> Result<(), RunError> {
    // VM-15.1: push a frame of the current statement and function, add one to the depth, and
    // fail with stack overflow when the new depth reaches LIMIT_MAX_STACK_DEPTH.
    machine.stack.push(Frame {
        statement: machine.xstatement,
        function: machine.xfunction,
    });
    if machine.stack.len() >= crate::LIMIT_MAX_STACK_DEPTH {
        return Err(machine.run_error(progs, ErrorKind::StackOverflow));
    }

    // VM-15.6: a negative locals is undefined in C; this spec fails.
    let locals = usize::try_from(callee.locals).map_err(|_| {
        machine.divergence(
            progs,
            Divergence::NegativeLocals {
                locals: callee.locals,
            },
        )
    })?;

    // VM-15.2: fail with locals stack overflow before saving anything when the slots in use
    // plus locals exceed LIMIT_LOCALSTACK_SIZE.
    let new_used = machine
        .localstack_used
        .checked_add(locals)
        .filter(|used| *used <= crate::LIMIT_LOCALSTACK_SIZE);
    let Some(new_used) = new_used else {
        return Err(machine.run_error(progs, ErrorKind::LocalsStackOverflow));
    };

    // VM-15.2: save the callee's locals slots, from parm_start up, onto the local stack.
    for offset in 0..locals {
        let slot = add_i32(callee.parm_start, offset, progs, machine)?;
        let value = ops::read_slot(machine, progs, slot)?;
        if let Some(cell) = machine
            .localstack
            .get_mut(machine.localstack_used.wrapping_add(offset))
        {
            *cell = value;
        }
    }
    machine.localstack_used = new_used;

    // VM-15.3: copy the parameters, packed, after the save, so the saved values are the
    // caller's.
    copy_parameters(machine, progs, callee)?;

    // VM-15.5: the callee becomes the current function; execution continues at its
    // first_statement (the loop's +1 lands there, so set xstatement to first_statement - 1).
    machine.xfunction = function;
    machine.xstatement = callee.first_statement.wrapping_sub(1);
    Ok(())
}

/// VM-15.3: copy the call parameters into the callee's local slots, packed by `parm_size`.
fn copy_parameters(
    machine: &mut Machine,
    progs: &Progs<'_>,
    callee: &Function,
) -> Result<(), RunError> {
    // VM-15.4: numparms of 0 or less copies nothing.
    let numparms = usize::try_from(callee.numparms).unwrap_or(0);
    let mut dest = callee.parm_start;
    for i in 0..numparms {
        // Parameter i starts at OFS_PARM0 + 3*i (PROGS-18.1); parm_size[i] slots are copied.
        let size = callee.parm_size.get(i).copied().unwrap_or(0);
        let parm_base = i32::from(PROGS_OFS_PARM0)
            .wrapping_add(i32::from(PROGS_PARM_SLOTS).wrapping_mul(i32::try_from(i).unwrap_or(0)));
        for j in 0..usize::from(size) {
            let src = parm_base.wrapping_add(i32::try_from(j).unwrap_or(0));
            let value = ops::read_slot(machine, progs, src)?;
            ops::write_slot(machine, progs, dest, value)?;
            dest = dest.wrapping_add(1);
        }
    }
    Ok(())
}

/// id's `PR_LeaveFunction` (VM-16.3): restore the callee's locals and pop the frame.
///
/// The callee is the current function (`pr_xfunction`); its `locals` slots are restored from
/// the top of the local stack to `parm_start` up, `localstack_used` drops by `locals`, and the
/// frame's saved function and statement become current again.
pub(crate) fn leave_function(machine: &mut Machine, progs: &Progs<'_>) {
    // The callee is the current function. A QC function always has a non-negative locals
    // (VM-15.6) and its index is valid, so these lookups succeed; fall back to 0 rather than
    // panic for a frame that somehow has no function.
    let current = function_record(progs, machine.xfunction);
    let locals = current
        .and_then(|f| usize::try_from(f.locals).ok())
        .unwrap_or(0);
    let parm_start = current.map_or(0, |f| f.parm_start);

    // VM-16.3: drop the slots in use by locals, then restore from there up.
    machine.localstack_used = machine.localstack_used.saturating_sub(locals);
    for offset in 0..locals {
        if let Some(value) = machine
            .localstack
            .get(machine.localstack_used.wrapping_add(offset))
            .copied()
        {
            let slot = parm_start.wrapping_add(i32::try_from(offset).unwrap_or(i32::MAX));
            // A restore to a slot outside the globals cannot happen: the save checked it
            // (VM-1.4); ignore a failure rather than panic.
            if machine.set_global(slot, value).is_err() {
                // Unreachable: the matching save checked this slot. Leave it unchanged.
            }
        }
    }

    // VM-16.3: pop the frame; the saved function and statement become current.
    if let Some(frame) = machine.stack.pop() {
        machine.xstatement = frame.statement;
        machine.xfunction = frame.function;
    }
}

/// Function `index`, or `None` for an index of 0, negative, or >= numfunctions (VM-2.1).
pub(crate) fn function_at(progs: &Progs<'_>, index: i32) -> Option<Function> {
    if index <= 0 {
        return None;
    }
    function_record(progs, index)
}

/// The functions-table record at `index`, function 0 included; `None` outside the table.
fn function_record(progs: &Progs<'_>, index: i32) -> Option<Function> {
    let i = usize::try_from(index).ok()?;
    if i >= progs.functions().len() {
        return None;
    }
    progs.functions().get(i)
}

/// Statement `index`, or `None` outside 0..numstatements (VM-3.3).
fn statement_at(statements: &qc_progs::Table<'_, Statement>, index: i32) -> Option<Statement> {
    let i = usize::try_from(index).ok()?;
    statements.get(i)
}

/// `base + offset` as an `i32`, failing with a VM-1.4 global error (overflow is out of range).
fn add_i32(
    base: i32,
    offset: usize,
    progs: &Progs<'_>,
    machine: &Machine,
) -> Result<i32, RunError> {
    i32::try_from(offset)
        .ok()
        .and_then(|o| base.checked_add(o))
        .ok_or_else(|| {
            machine.divergence(progs, Divergence::GlobalOffsetOutOfRange { base, offset })
        })
}

/// Turns a [`GlobalAccessError`](crate::GlobalAccessError) into a VM-1.4 runtime error.
pub(crate) fn global_error(
    machine: &Machine,
    progs: &Progs<'_>,
    error: crate::GlobalAccessError,
) -> RunError {
    machine.divergence(progs, Divergence::GlobalOutOfRange { slot: error.slot })
}

impl Machine {
    /// Builds a [`RunError`] for an error raised by the current statement (VM-19.1): the
    /// current function and statement and the stack trace innermost first.
    pub(crate) fn run_error(&self, progs: &Progs<'_>, kind: ErrorKind) -> RunError {
        RunError {
            kind,
            function: self.xfunction,
            statement: self.xstatement,
            trace: stack_trace(self, progs),
        }
    }

    /// Builds the [`RunError`] for a divergence the current statement raised (VM-19.7).
    pub(crate) fn divergence(&self, progs: &Progs<'_>, divergence: Divergence) -> RunError {
        self.run_error(progs, ErrorKind::Divergence(divergence))
    }
}

/// Builds a [`RunError`] for an error before a run's first statement (VM-2.1, VM-2.2, VM-2.4):
/// the function is the run's and the statement is -1 (TRACE-4.7).
fn before_first_statement(
    machine: &Machine,
    progs: &Progs<'_>,
    function: i32,
    kind: ErrorKind,
) -> RunError {
    RunError {
        kind,
        function,
        statement: -1,
        trace: stack_trace(machine, progs),
    }
}

/// The QC stack trace id's `PR_StackTrace` prints, innermost first (VM-19.1).
fn stack_trace(machine: &Machine, progs: &Progs<'_>) -> Vec<StackEntry> {
    let mut trace = Vec::new();
    // The innermost frame is the current function and statement.
    trace.push(stack_entry(progs, machine.xfunction, machine.xstatement));
    for frame in machine.stack.iter().rev() {
        trace.push(stack_entry(progs, frame.function, frame.statement));
    }
    trace
}

/// One stack-trace entry, resolving the function's name and file from the string table.
fn stack_entry(progs: &Progs<'_>, function: i32, statement: i32) -> StackEntry {
    let (name, file) = match function_record(progs, function) {
        Some(f) => (string_of(progs, f.s_name), string_of(progs, f.s_file)),
        None => (String::new(), String::new()),
    };
    StackEntry {
        function,
        statement,
        name,
        file,
    }
}

/// The string at `offset`, lossily as UTF-8 for a trace entry, or empty.
fn string_of(progs: &Progs<'_>, offset: i32) -> String {
    match progs.string(offset) {
        Some(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        None => String::new(),
    }
}
