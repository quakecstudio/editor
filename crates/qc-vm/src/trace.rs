// SPDX-License-Identifier: GPL-3.0-or-later

//! The per-statement trace hook that builds a [`qc_trace::Trace`] (TRACE-4).
//!
//! [`Trace`] is an [`Observer`](crate::Observer) that records a run record, a step record per
//! statement, an exit record and, on failure, an error record (TRACE-4.1 to TRACE-4.8), each
//! change-carrying record holding the globals and entity fields that changed since the
//! previous one (TRACE-3). It names globals by slot, edicts by number and fields by field slot
//! (TRACE-1.4), so two VMs with different memory layouts produce the same trace. This module
//! is a format of this project, not a port.

use qc_edict::EDICT_HEADER_SIZE;
use qc_progs::Progs;
use qc_trace::{Changes, FieldChange, GlobalChange, Header, Record, Text, Trace as TraceData};
use qc_types::Slot;

use crate::error::RunError;
use crate::exec::Observer;
use crate::machine::Machine;

/// Builds a [`qc_trace::Trace`] from a run (TRACE-1.1).
///
/// Set up with [`Trace::new`] before the first run (TRACE-1.3), then passed to
/// [`Machine::execute_program`](crate::Machine::execute_program) as the observer. After the
/// run, [`Trace::finish`] returns the header and records.
#[derive(Clone, Debug)]
pub struct Trace {
    header: Header,
    records: Vec<Record>,
    /// The globals as of the last change-carrying record (TRACE-3.1).
    last_globals: Vec<Slot>,
    /// The fields of every edict in use as of the last change-carrying record (TRACE-3.1,
    /// TRACE-3.4).
    last_fields: Vec<Vec<Slot>>,
    entityfields: usize,
}

impl Trace {
    /// A trace hook for `progs`, with the header written after loading and before the first
    /// run (TRACE-1.3, TRACE-2).
    ///
    /// # Errors
    ///
    /// A [`qc_trace::TraceError`] when the header numbers break TRACE-2.3 (they cannot for a
    /// progs the VM loaded, since the edict layout already fits).
    pub fn new(machine: &Machine, progs: &Progs<'_>) -> Result<Self, qc_trace::TraceError> {
        let edict_size = u32::try_from(machine.edicts().layout().edict_size()).unwrap_or(0);
        let field_offset = u32::try_from(EDICT_HEADER_SIZE).unwrap_or(0);
        let entityfields_i = machine.edicts().layout().entityfields();
        let entityfields = u32::try_from(entityfields_i).unwrap_or(0);
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
            entityfields: entityfields_i,
        })
    }

    /// The trace built so far: the header and every record, in order (TRACE-1.1).
    #[must_use]
    pub fn finish(self) -> TraceData {
        TraceData {
            header: self.header,
            records: self.records,
        }
    }

    /// The records so far, for inspection.
    #[must_use]
    pub fn records(&self) -> &[Record] {
        &self.records
    }

    /// Computes the change set since the last change-carrying record and resets the baseline
    /// (TRACE-3.1, TRACE-3.2).
    fn changes(&mut self, machine: &Machine) -> Changes {
        let mut globals = Vec::new();
        for (slot, (now, before)) in machine
            .globals()
            .iter()
            .zip(self.last_globals.iter())
            .enumerate()
        {
            if now.bits() != before.bits() {
                globals.push(GlobalChange {
                    slot: u32::try_from(slot).unwrap_or(u32::MAX),
                    bits: now.bits(),
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
                    .unwrap_or(Slot::ZERO);
                if now.bits() != before.bits() {
                    fields.push(FieldChange {
                        edict: u32::try_from(edict).unwrap_or(u32::MAX),
                        field: u32::try_from(field).unwrap_or(u32::MAX),
                        bits: now.bits(),
                    });
                }
            }
        }
        self.last_globals = machine.globals().to_vec();
        self.last_fields = current;
        // Changes::new sorts and rejects duplicates; a VM never produces a duplicate, so an
        // error here is unreachable. Fall back to an empty change set rather than panic.
        Changes::new(globals, fields).unwrap_or_default()
    }
}

/// A snapshot of the fields of every edict in use (TRACE-3.4): edicts 0 to num_edicts - 1.
fn snapshot_fields(machine: &Machine) -> Vec<Vec<Slot>> {
    let mut out = Vec::new();
    for number in 0..machine.edicts().num_edicts() {
        let slots = machine.edicts().fields(number).unwrap_or(&[]);
        out.push(slots.to_vec());
    }
    out
}

impl Observer for Trace {
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
        // TRACE-4.2: a step record with the function and statement current when the statement
        // became current, the opcode, the builtin number if any, and the changes.
        let changes = self.changes(machine);
        self.records.push(Record::Step(qc_trace::Step {
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
        // TRACE-4.6 to TRACE-4.8: an error record with the function and statement PR_RunError
        // prints and the message, without a trailing newline.
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

impl Trace {
    /// The field slots each edict has (for callers inspecting the trace shape).
    #[must_use]
    pub const fn entityfields(&self) -> usize {
        self.entityfields
    }
}
