// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The trace the host builds around the VM's runs (TRACE-1 to TRACE-4).
//!
//! This is a format of this project, not a port (`trace.md`). [`TraceBuilder`] is a
//! [`qc_vm::Observer`]: the VM calls it for each run, statement, exit and error (TRACE-4.1 to
//! TRACE-4.8), and `qc-vm`'s own [`Trace`](qc_vm::trace::Trace) records the globals and entity
//! fields that changed since the previous change-carrying record (TRACE-3). The builder adds
//! string records (TRACE-4.10) for the engine strings the host makes (`mapname`, the world's
//! `model`, each client's `netname`) and for the `ftos`/`vtos` buffer when a builtin changes its
//! text (DIFF-3.9), and error records for a host error the frame loop raises outside a run
//! (DIFF-8.1).

use std::cell::Cell;

use qc_builtins::MockBuiltins;
use qc_progs::Progs;
use qc_trace::{Header, Record, Text, Trace, TraceError};
use qc_vm::trace::Trace as VmTrace;
use qc_vm::{Builtins, Context, Machine, Observer, RunError};

/// Builds a [`qc_trace::Trace`] from a whole scenario (TRACE-1.1).
///
/// Created after the progs is loaded and before the first run (TRACE-1.3), then passed to
/// every [`Machine::execute_program`](qc_vm::Machine::execute_program) call as the observer,
/// inside a `StringObserver`. The host calls [`TraceBuilder::string_set`] for each engine
/// string whose text is set, by the host or by a builtin (DIFF-3.9), and
/// [`TraceBuilder::host_error`] for a host error outside a run. [`TraceBuilder::finish`]
/// returns the header and records.
#[derive(Clone, Debug)]
pub struct TraceBuilder {
    /// The VM's trace, which writes every change-carrying record (TRACE-3, TRACE-4).
    trace: VmTrace,
    /// String records waiting to be flushed before the next change-carrying record (TRACE-4.10).
    pending_strings: Vec<Record>,
}

/// The `ftos`/`vtos` buffer's new value and text after a builtin call, waiting for the observer
/// to record it before the `CALL`'s step record (TRACE-4.10, DIFF-3.9).
///
/// [`StringBuiltins`] sets it after each builtin call and `StringObserver` takes it before
/// each record; the VM holds the two as separate borrows, so they share it. No mock builtin
/// starts a nested run, so a builtin call is always followed by its own step record.
pub(crate) type StringQueue = Cell<Option<(i32, Vec<u8>)>>;

/// The mock builtins, queueing the temp buffer's text when a builtin changes it (DIFF-3.9).
pub(crate) struct StringBuiltins<'a> {
    /// The mock engine's builtins (DIFF-6, DIFF-7).
    pub(crate) builtins: &'a mut MockBuiltins,
    /// Where the change goes.
    pub(crate) queue: &'a StringQueue,
}

impl Builtins for StringBuiltins<'_> {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        let result = self.builtins.call(number, context);
        // DIFF-3.9: `ftos` and `vtos` set the temp buffer's text (BUILTIN-7.4); the string
        // record goes before the CALL's step record, whose change set first shows the value.
        if let Some(change) = self.builtins.engine_mut().strings.take_temp_change() {
            self.queue.set(Some(change));
        }
        result
    }

    fn limit(&self) -> Option<u32> {
        self.builtins.limit()
    }

    fn engine_string(&self, value: i32) -> Option<&[u8]> {
        self.builtins.engine_string(value)
    }
}

/// The trace observer, recording a queued temp-buffer change before each record (DIFF-3.9),
/// as the oracle's `begin_change_record` writes the touched strings before every record.
pub(crate) struct StringObserver<'a> {
    /// The trace being built.
    pub(crate) trace: &'a mut TraceBuilder,
    /// The change [`StringBuiltins`] queued.
    pub(crate) queue: &'a StringQueue,
}

impl StringObserver<'_> {
    /// Moves a queued change into the trace's pending string records (TRACE-4.10).
    fn record_strings(&mut self) {
        if let Some((value, text)) = self.queue.take() {
            self.trace.string_set(value, text);
        }
    }
}

impl Observer for StringObserver<'_> {
    fn run(&mut self, machine: &Machine, function: i32) {
        self.record_strings();
        self.trace.run(machine, function);
    }

    fn step(
        &mut self,
        machine: &Machine,
        function: i32,
        statement: i32,
        opcode: u16,
        builtin: Option<u32>,
    ) {
        self.record_strings();
        self.trace
            .step(machine, function, statement, opcode, builtin);
    }

    fn exit(&mut self, machine: &Machine) {
        self.record_strings();
        self.trace.exit(machine);
    }

    fn error(&mut self, machine: &Machine, error: &RunError) {
        self.record_strings();
        self.trace.error(machine, error);
    }
}

impl TraceBuilder {
    /// A trace hook for `progs`, with the header written after loading and before the first
    /// run (TRACE-1.3, TRACE-2).
    ///
    /// # Errors
    ///
    /// A [`TraceError`] when the header numbers break TRACE-2.3 (they cannot for a progs the VM
    /// loaded).
    pub fn new(machine: &Machine, progs: &Progs<'_>) -> Result<Self, TraceError> {
        Ok(Self {
            trace: VmTrace::new(machine, progs)?,
            pending_strings: Vec::new(),
        })
    }

    /// The trace built so far: the header and every record, in order (TRACE-1.1).
    ///
    /// Any string records still pending are flushed; they carry no change set, so trailing
    /// ones are harmless (TRACE-4.10).
    #[must_use]
    pub fn finish(mut self) -> Trace {
        self.flush_strings();
        self.trace.finish()
    }

    /// Drops the records so far, pending strings included, for a run whose records are not
    /// kept (DIFF-12.7).
    pub(crate) fn clear(&mut self) {
        self.trace.clear();
        self.pending_strings.clear();
    }

    /// The records so far, for a test to inspect (TRACE-4).
    #[must_use]
    pub fn records(&self) -> &[Record] {
        self.trace.records()
    }

    /// The trace's header (TRACE-2).
    #[must_use]
    pub const fn header(&self) -> Header {
        self.trace.header()
    }

    /// Records that engine string `value` now holds `text` (TRACE-4.10): the host made it, or a
    /// builtin set its text (DIFF-3.9).
    ///
    /// The string record is written before the next change-carrying record (the one whose
    /// change set may first show the value), as the oracle writes it (`trace.c`,
    /// `begin_change_record`).
    pub fn string_set(&mut self, value: i32, text: Vec<u8>) {
        if let Ok(text) = Text::new(text) {
            self.pending_strings.push(Record::String {
                value: value.cast_unsigned(),
                text,
            });
        }
    }

    /// Writes an error record for an error the host raises outside any run (DIFF-8.1): a bad
    /// movetype in the physics frame, or an error while spawning the map, whose function is 0
    /// and statement -1.
    ///
    /// The record carries the host's own changes since the previous record (TRACE-3.1), as the
    /// oracle's `qs_trace_error` does with `put_changes`: while spawning, the world's fields,
    /// the globals and the keys already applied.
    pub fn host_error(&mut self, machine: &Machine, function: i32, statement: i32, message: &str) {
        self.flush_strings();
        self.trace
            .error_record(machine, function, statement, message.as_bytes().to_vec());
    }

    /// Flushes the pending string records before a change-carrying record (TRACE-4.10).
    fn flush_strings(&mut self) {
        for record in self.pending_strings.drain(..) {
            self.trace.push(record);
        }
    }
}

impl Observer for TraceBuilder {
    fn run(&mut self, machine: &Machine, function: i32) {
        self.flush_strings();
        self.trace.run(machine, function);
    }

    fn step(
        &mut self,
        machine: &Machine,
        function: i32,
        statement: i32,
        opcode: u16,
        builtin: Option<u32>,
    ) {
        self.flush_strings();
        self.trace
            .step(machine, function, statement, opcode, builtin);
    }

    fn exit(&mut self, machine: &Machine) {
        self.flush_strings();
        self.trace.exit(machine);
    }

    fn error(&mut self, machine: &Machine, error: &RunError) {
        self.flush_strings();
        self.trace.error(machine, error);
    }
}
