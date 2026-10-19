// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The trace the host builds around the VM's runs (TRACE-1 to TRACE-4).
//!
//! This is a format of this project, not a port (`trace.md`). [`TraceBuilder`] is a
//! [`qc_vm::Observer`]: the VM calls it for each run, statement, exit and error (TRACE-4.1 to
//! TRACE-4.8), and it records the globals and entity fields that changed since the previous
//! change-carrying record (TRACE-3). It differs from `qc-vm`'s own
//! [`Trace`](qc_vm::trace::Trace) only in also emitting string records (TRACE-4.10) for the
//! engine strings the host makes (`mapname`, the world's `model`, each client's `netname`),
//! and error records for a host error the frame loop raises outside a run (DIFF-8.1). The
//! change-set algorithm is the same: snapshot the globals and every in-use edict's fields, and
//! list what differs.

use std::cell::RefCell;
use std::collections::BTreeMap;

use qc_builtins::{MockBuiltins, MockEngine};
use qc_edict::EDICT_HEADER_SIZE;
use qc_progs::Progs;
use qc_trace::{Changes, FieldChange, GlobalChange, Header, Record, Step, Text, Trace, TraceError};
use qc_types::Slot;
use qc_vm::{Builtins, Context, Machine, Observer, RunError};

/// Builds a [`qc_trace::Trace`] from a whole scenario (TRACE-1.1).
///
/// Created after the progs is loaded and before the first run (TRACE-1.3), then passed to
/// every [`Machine::execute_program`](qc_vm::Machine::execute_program) call as the observer,
/// inside a [`StringObserver`]. The host calls [`TraceBuilder::string_made`] for each engine
/// string whose text is set, by the host or by a builtin (DIFF-3.9), and
/// [`TraceBuilder::host_error`] for a host error outside a run. [`TraceBuilder::finish`]
/// returns the header and records.
#[derive(Clone, Debug)]
pub struct TraceBuilder {
    header: Header,
    records: Vec<Record>,
    /// The globals as of the last change-carrying record (TRACE-3.1).
    last_globals: Vec<Slot>,
    /// The fields of every edict in use as of the last change-carrying record (TRACE-3.1,
    /// TRACE-3.4).
    last_fields: Vec<Vec<Slot>>,
    /// String records waiting to be flushed before the next change-carrying record (TRACE-4.10).
    pending_strings: Vec<Record>,
    /// The text of the latest string record written for each engine string value, so a text
    /// set again unchanged writes no record (DIFF-3.9).
    written_strings: BTreeMap<i32, Vec<u8>>,
}

/// The engine strings a builtin set during a run, by value and text, waiting for the observer
/// to record them before the change-carrying record of the `CALL` (TRACE-4.10, DIFF-3.9).
///
/// [`StringBuiltins`] fills it after each builtin call and [`StringObserver`] empties it into
/// the [`TraceBuilder`] before each record; the VM holds the two as separate borrows, so they
/// share it.
pub(crate) type StringQueue = RefCell<Vec<(i32, Vec<u8>)>>;

/// The mock builtins, queueing every engine string a builtin sets (DIFF-3.9).
pub(crate) struct StringBuiltins<'a> {
    /// The mock engine's builtins (DIFF-6, DIFF-7).
    pub(crate) builtins: &'a mut MockBuiltins<MockEngine>,
    /// Where the changed engine strings go.
    pub(crate) queue: &'a StringQueue,
}

impl Builtins for StringBuiltins<'_> {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        let result = self.builtins.call(number, context);
        // DIFF-3.9: `ftos` and `vtos` set the temp buffer's text (BUILTIN-7.4); the string
        // record goes before the CALL's step record, whose change set first shows the value.
        let changed = self.builtins.engine_mut().strings.take_changed();
        if let Ok(mut queue) = self.queue.try_borrow_mut() {
            queue.extend(changed);
        }
        result
    }

    fn engine_string(&self, value: i32) -> Option<&[u8]> {
        self.builtins.engine_string(value)
    }
}

/// The trace observer, recording the queued engine strings before each record (DIFF-3.9).
pub(crate) struct StringObserver<'a> {
    /// The trace being built.
    pub(crate) trace: &'a mut TraceBuilder,
    /// The engine strings [`StringBuiltins`] queued.
    pub(crate) queue: &'a StringQueue,
}

impl StringObserver<'_> {
    /// Moves the queued engine strings into the trace's pending string records (TRACE-4.10).
    fn record_strings(&mut self) {
        let changed = self
            .queue
            .try_borrow_mut()
            .map(|mut queue| core::mem::take(&mut *queue))
            .unwrap_or_default();
        for (value, text) in changed {
            self.trace.string_made(value, text);
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
        let edict_size = u32::try_from(machine.edicts().layout().edict_size()).unwrap_or(0);
        let field_offset = u32::try_from(EDICT_HEADER_SIZE).unwrap_or(0);
        let entityfields = u32::try_from(machine.edicts().layout().entityfields()).unwrap_or(0);
        let numstrings = u32::try_from(progs.strings().len()).unwrap_or(0);
        let header = Header::new(
            progs.file_crc(),
            edict_size,
            field_offset,
            entityfields,
            numstrings,
        )?;
        Ok(Self {
            header,
            records: Vec::new(),
            last_globals: machine.globals().to_vec(),
            last_fields: snapshot_fields(machine),
            pending_strings: Vec::new(),
            written_strings: BTreeMap::new(),
        })
    }

    /// The trace built so far: the header and every record, in order (TRACE-1.1).
    ///
    /// Any string records still pending are flushed; they carry no change set, so trailing
    /// ones are harmless (TRACE-4.10).
    #[must_use]
    pub fn finish(mut self) -> Trace {
        let mut records = self.records;
        records.append(&mut self.pending_strings);
        Trace {
            header: self.header,
            records,
        }
    }

    /// The records so far, for a test to inspect (TRACE-4).
    #[must_use]
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// The trace's header (TRACE-2).
    #[must_use]
    pub const fn header(&self) -> Header {
        self.header
    }

    /// Records that engine string `value` now holds `text` (TRACE-4.10): the host made it, or a
    /// builtin set its text (DIFF-3.9).
    ///
    /// The string record is written before the next change-carrying record (the one whose
    /// change set may first show the value), as the oracle writes it (`trace.c`,
    /// `begin_change_record`). A text the same as the latest record's for `value` writes none,
    /// as the oracle's `qs_emit_string_records` skips it (DIFF-3.9).
    pub fn string_made(&mut self, value: i32, text: Vec<u8>) {
        if self.written_strings.get(&value) == Some(&text) {
            return;
        }
        self.written_strings.insert(value, text.clone());
        if let Ok(text) = Text::new(text) {
            self.pending_strings.push(Record::String {
                value: value.cast_unsigned(),
                text,
            });
        }
    }

    /// Writes an error record for a host error raised outside any run (DIFF-8.1): a bad
    /// movetype in the physics frame, whose function is 0 and statement -1.
    pub fn host_error(&mut self, function: i32, statement: i32, message: &str) {
        self.flush_strings();
        let message = Text::new(message.as_bytes().to_vec()).unwrap_or_default();
        self.records.push(Record::Error {
            function,
            statement,
            message,
            // A host error outside a run carries the host's own changes since the previous
            // record (TRACE-3.1); the physics loop writes no globals or fields between the
            // last record and the bad movetype, so the change set is empty.
            changes: Changes::default(),
        });
    }

    /// Flushes the pending string records before a change-carrying record (TRACE-4.10).
    fn flush_strings(&mut self) {
        self.records.append(&mut self.pending_strings);
    }

    /// The change set since the last change-carrying record, resetting the baseline
    /// (TRACE-3.1, TRACE-3.2), after flushing any pending string records (TRACE-4.10).
    fn changes(&mut self, machine: &Machine) -> Changes {
        self.flush_strings();
        let mut globals = Vec::new();
        for (slot, (now, before)) in machine
            .globals()
            .iter()
            .zip(self.last_globals.iter())
            .enumerate()
        {
            if now.0 != before.0 {
                globals.push(GlobalChange {
                    slot: u32::try_from(slot).unwrap_or(u32::MAX),
                    bits: now.0,
                });
            }
        }
        let mut fields = Vec::new();
        let current = snapshot_fields(machine);
        for (edict, slots) in current.iter().enumerate() {
            let previous = self.last_fields.get(edict);
            for (field, now) in slots.iter().enumerate() {
                let before = previous
                    .and_then(|p| p.get(field))
                    .copied()
                    .unwrap_or(Slot(0));
                if now.0 != before.0 {
                    fields.push(FieldChange {
                        edict: u32::try_from(edict).unwrap_or(u32::MAX),
                        field: u32::try_from(field).unwrap_or(u32::MAX),
                        bits: now.0,
                    });
                }
            }
        }
        self.last_globals = machine.globals().to_vec();
        self.last_fields = current;
        // A VM never produces a duplicate; fall back to an empty change set rather than panic.
        Changes::new(globals, fields).unwrap_or_default()
    }
}

/// A snapshot of the fields of every edict in use (TRACE-3.4): edicts 0 to num_edicts - 1.
fn snapshot_fields(machine: &Machine) -> Vec<Vec<Slot>> {
    (0..machine.edicts().num_edicts())
        .map(|number| machine.edicts().fields(number).unwrap_or(&[]).to_vec())
        .collect()
}

impl Observer for TraceBuilder {
    fn run(&mut self, machine: &Machine, function: i32) {
        // TRACE-4.1: a run record with the changes since the previous record.
        let changes = self.changes(machine);
        self.records.push(Record::Run { function, changes });
    }

    fn step(
        &mut self,
        machine: &Machine,
        function: i32,
        statement: i32,
        opcode: u16,
        builtin: Option<u32>,
    ) {
        // TRACE-4.2: a step record with the function, statement, opcode, builtin and changes.
        let changes = self.changes(machine);
        self.records.push(Record::Step(Step {
            function,
            statement,
            opcode,
            builtin,
            changes,
        }));
    }

    fn exit(&mut self, machine: &Machine) {
        // TRACE-4.5: an exit record, normally with no changes.
        let changes = self.changes(machine);
        self.records.push(Record::Exit { changes });
    }

    fn error(&mut self, machine: &Machine, error: &RunError) {
        // TRACE-4.6 to TRACE-4.8: an error record with PR_RunError's function and statement and
        // the message, without a trailing newline.
        let changes = self.changes(machine);
        let message = Text::new(error.message().into_bytes()).unwrap_or_default();
        self.records.push(Record::Error {
            function: error.function,
            statement: error.statement,
            message,
            changes,
        });
    }
}
