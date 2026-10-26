// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! Comparing two differential traces and measuring statement coverage (DIFF-9 to DIFF-11).
//!
//! A differential run executes one scenario in two VMs and writes two traces
//! ([`qc_trace::Trace`], `docs/spec/trace.md`); this module decides whether they agree
//! (DIFF-9), classifies a disagreement as an expected divergence or a failure (DIFF-10), and
//! measures how much of a progs the trace exercised (DIFF-11). It reads values through each
//! trace's own header and string records (TRACE-7), because entity values, pointers and engine
//! strings differ between runners (DIFF-9.4).
//!
//! This is a format of this project, not a port: DIFF-9 to DIFF-11 are decisions, each made so
//! that two independent runners can agree on a scenario. The statements are in
//! `docs/spec/differential.md`.

use std::collections::BTreeMap;

use qc_progs::limits::LIMIT_MAX_EDICTS;
use qc_trace::{Changes, Record, StringValue, Trace};

/// The largest edict number an entity or pointer reading may name (DIFF-9.4): `LIMIT_MAX_EDICTS`
/// (`qc_progs::limits::LIMIT_MAX_EDICTS`, EDICT-1.8).
fn max_edicts() -> u64 {
    u64::try_from(LIMIT_MAX_EDICTS).unwrap_or(u64::MAX)
}

/// One change-carrying record and its position in the trace's record list.
///
/// A trace holds run, step, exit, error and string records mixed together (TRACE-4); DIFF-9
/// pairs only the change-carrying ones (TRACE-3), but reads string values at the record's
/// position in the full list (TRACE-7.3).
#[derive(Clone, Copy, Debug)]
struct Carrier<'a> {
    /// The record itself.
    record: &'a Record,
    /// Its index in the trace's `records`, so string values are read as of just after it
    /// (TRACE-7.3).
    index: usize,
}

/// The change-carrying records of a trace, in order, with their positions (TRACE-3, DIFF-9.1).
fn carriers(trace: &Trace) -> Vec<Carrier<'_>> {
    trace
        .records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.changes().is_some())
        .map(|(index, record)| Carrier { record, index })
        .collect()
}

/// Which kind of record this is, for the tag check of DIFF-9.2.
fn tag(record: &Record) -> u8 {
    match record {
        Record::Run { .. } => 1,
        Record::Step(_) => 2,
        Record::Exit { .. } => 3,
        Record::Error { .. } => 4,
        Record::String { .. } => 5,
    }
}

/// A global slot or an entity field slot (DIFF-9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Slot {
    /// A global slot number.
    Global(u32),
    /// A field slot of an edict: (edict number, field slot).
    Field(u32, u32),
}

/// How a pair of traces differs, when it does (DIFF-9.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Divergence {
    /// The headers disagree on a field that must match (DIFF-9.1): `progs_crc`, `entityfields`
    /// or `numstrings`.
    Header {
        /// What differs, for the report.
        field: &'static str,
    },
    /// One trace has fewer change-carrying records; `index` is the first the shorter one lacks
    /// (DIFF-9.5). `shorter_is_a` says which trace ran short.
    Length {
        /// The index (among change-carrying records) of the first missing record.
        index: usize,
        /// Whether trace A is the shorter one.
        shorter_is_a: bool,
    },
    /// The `index`-th change-carrying records disagree in their tag or header fields (DIFF-9.2).
    Record {
        /// The index among change-carrying records.
        index: usize,
        /// The statement of A's record, or -1 when it has none (a run or exit record).
        statement_a: i32,
        /// The statement of B's record, or -1 when it has none.
        statement_b: i32,
        /// What differs, for the report.
        what: &'static str,
    },
    /// The `index`-th records' change sets disagree on a slot's value (DIFF-9.3).
    Value {
        /// The index among change-carrying records.
        index: usize,
        /// The statement of A's record, or -1 when it has none.
        statement_a: i32,
        /// The statement of B's record, or -1 when it has none.
        statement_b: i32,
        /// The slot that differs.
        slot: Slot,
        /// A's value of the slot (its bits) at this record.
        a: u32,
        /// B's value of the slot at this record.
        b: u32,
    },
}

/// The statement of a record, for a divergence report: the statement of a step or error record,
/// or -1 for a run or exit record, which have none (TRACE-4).
fn record_statement(record: &Record) -> i32 {
    match record {
        Record::Step(step) => step.statement,
        Record::Error { statement, .. } => *statement,
        Record::Run { .. } | Record::Exit { .. } | Record::String { .. } => -1,
    }
}

impl Divergence {
    /// The statement to report the divergence at (DIFF-9.5): A's statement where it has one,
    /// else B's, else -1.
    #[must_use]
    pub const fn statement(&self) -> i32 {
        match self {
            Self::Header { .. } | Self::Length { .. } => -1,
            Self::Record {
                statement_a,
                statement_b,
                ..
            }
            | Self::Value {
                statement_a,
                statement_b,
                ..
            } => {
                if *statement_a >= 0 {
                    *statement_a
                } else {
                    *statement_b
                }
            }
        }
    }
}

/// The result of comparing two traces (DIFF-9, DIFF-10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Comparison {
    /// The traces are equal (DIFF-9.1): every corresponding record agrees.
    Equal,
    /// The first divergence (DIFF-9.5), classified as a failure (DIFF-9.6). It is not one of
    /// the expected divergences of DIFF-10.1.
    Failure(Divergence),
}

/// Compares trace A against trace B (DIFF-9).
///
/// `initial_globals` is the progs file's value for each global slot (PROGS-12.1), the same for
/// both traces since they ran the same progs (DIFF-9.1); it is the default value of a global
/// that no change set up to a record lists (DIFF-9.3). A field that no change set lists
/// defaults to 0 (TRACE-3.4).
///
/// Returns [`Comparison::Equal`] when the traces agree, or [`Comparison::Failure`] with the
/// first divergence (DIFF-9.5). The expected divergences of DIFF-10.1 that this comparator can
/// see are not treated specially: the VM's own undefined cases (VM-19.7) end QuakeC Studio's
/// trace with an error record, and DIFF-9.2 compares that record's message, so an agreeing
/// error record compares equal and nothing after it exists to compare (DIFF-9.5, DIFF-10.1); the
/// NaN, edict layout and engine string readings of DIFF-10.2 are handled inside
/// [`equivalent`] (DIFF-9.4). Any divergence this returns is therefore a failure (DIFF-9.6,
/// DIFF-10.3).
#[must_use]
pub fn compare(a: &Trace, b: &Trace, initial_globals: &[u32]) -> Comparison {
    if let Some(divergence) = compare_headers(a, b) {
        return Comparison::Failure(divergence);
    }
    let carriers_a = carriers(a);
    let carriers_b = carriers(b);

    // DIFF-9.3: the latest value of each slot in each trace, updated as the records are walked.
    let mut values_a = SlotValues::new(initial_globals);
    let mut values_b = SlotValues::new(initial_globals);

    let pairs = carriers_a.len().min(carriers_b.len());
    for index in 0..pairs {
        let (Some(ca), Some(cb)) = (carriers_a.get(index), carriers_b.get(index)) else {
            // Unreachable: index is below both lengths.
            break;
        };
        let (ca, cb) = (*ca, *cb);
        // Apply both change sets before comparing, so each trace's value of a slot is the value
        // in the latest change set up to and including this record (DIFF-9.3).
        if let Some(changes) = ca.record.changes() {
            values_a.apply(changes);
        }
        if let Some(changes) = cb.record.changes() {
            values_b.apply(changes);
        }
        if let Some(divergence) = compare_record(index, ca.record, cb.record) {
            return Comparison::Failure(divergence);
        }
        if let Some(divergence) = compare_values(index, ca, cb, a, b, &values_a, &values_b) {
            return Comparison::Failure(divergence);
        }
    }

    if carriers_a.len() != carriers_b.len() {
        return Comparison::Failure(Divergence::Length {
            index: pairs,
            shorter_is_a: carriers_a.len() < carriers_b.len(),
        });
    }
    Comparison::Equal
}

/// The header checks of DIFF-9.1: `progs_crc`, `entityfields` and `numstrings` must match;
/// `edict_size` and `field_offset` may differ. The version is `TRACE_VERSION` for every trace
/// (TRACE-2.2) and is not part of [`qc_trace::Header`].
fn compare_headers(a: &Trace, b: &Trace) -> Option<Divergence> {
    let ha = a.header;
    let hb = b.header;
    if ha.progs_crc() != hb.progs_crc() {
        return Some(Divergence::Header { field: "progs_crc" });
    }
    if ha.entityfields() != hb.entityfields() {
        return Some(Divergence::Header {
            field: "entityfields",
        });
    }
    if ha.numstrings() != hb.numstrings() {
        return Some(Divergence::Header {
            field: "numstrings",
        });
    }
    None
}

/// The tag and header-field checks of DIFF-9.2 for one pair of corresponding records.
fn compare_record(index: usize, a: &Record, b: &Record) -> Option<Divergence> {
    let statement_a = record_statement(a);
    let statement_b = record_statement(b);
    let diverge = |what: &'static str| {
        Some(Divergence::Record {
            index,
            statement_a,
            statement_b,
            what,
        })
    };
    if tag(a) != tag(b) {
        return diverge("tag");
    }
    match (a, b) {
        (Record::Run { function: fa, .. }, Record::Run { function: fb, .. }) => {
            if fa != fb {
                return diverge("function");
            }
        }
        (Record::Step(sa), Record::Step(sb)) => {
            if sa.function != sb.function {
                return diverge("function");
            }
            if sa.statement != sb.statement {
                return diverge("statement");
            }
            if sa.opcode != sb.opcode {
                return diverge("opcode");
            }
            if sa.builtin != sb.builtin {
                return diverge("builtin");
            }
        }
        (
            Record::Error {
                function: fa,
                statement: ta,
                message: ma,
                ..
            },
            Record::Error {
                function: fb,
                statement: tb,
                message: mb,
                ..
            },
        ) => {
            if fa != fb {
                return diverge("function");
            }
            if ta != tb {
                return diverge("statement");
            }
            if ma.as_bytes() != mb.as_bytes() {
                return diverge("message");
            }
        }
        // Exit records carry only a change set (TRACE-4.5), checked by compare_values. Mixed
        // tags are caught by the tag check above.
        _ => {}
    }
    None
}

/// The value check of DIFF-9.3: every slot listed in either record's change set is equivalent
/// in the two traces (DIFF-9.4).
fn compare_values(
    index: usize,
    ca: Carrier<'_>,
    cb: Carrier<'_>,
    a: &Trace,
    b: &Trace,
    values_a: &SlotValues,
    values_b: &SlotValues,
) -> Option<Divergence> {
    let mut slots = Vec::new();
    if let Some(changes) = ca.record.changes() {
        collect_slots(changes, &mut slots);
    }
    if let Some(changes) = cb.record.changes() {
        collect_slots(changes, &mut slots);
    }
    slots.sort_unstable();
    slots.dedup();

    for slot in slots {
        let va = values_a.value(slot);
        let vb = values_b.value(slot);
        // Read string values as of just after each record (TRACE-7.3): a string record written
        // before the change-carrying record is in scope.
        if !equivalent(
            va,
            vb,
            a,
            b,
            ca.index.saturating_add(1),
            cb.index.saturating_add(1),
        ) {
            return Some(Divergence::Value {
                index,
                statement_a: record_statement(ca.record),
                statement_b: record_statement(cb.record),
                slot,
                a: va,
                b: vb,
            });
        }
    }
    None
}

/// Adds every slot listed in `changes` to `slots` (DIFF-9.3).
fn collect_slots(changes: &Changes, slots: &mut Vec<Slot>) {
    for global in changes.globals() {
        slots.push(Slot::Global(global.slot));
    }
    for field in changes.fields() {
        slots.push(Slot::Field(field.edict, field.field));
    }
}

/// Whether value `va` of trace A and `vb` of trace B are equivalent (DIFF-9.4).
///
/// `before_a` and `before_b` are the record positions in each trace at which to read a string
/// value: the position just after the record being compared, so a string record written before
/// it is in scope (TRACE-7.3).
fn equivalent(va: u32, vb: u32, a: &Trace, b: &Trace, before_a: usize, before_b: usize) -> bool {
    // DIFF-9.4, first rule: if each is an engine string of its trace, equivalent exactly when
    // the latest texts are the same bytes.
    let sa = a.string_value(before_a, va);
    let sb = b.string_value(before_b, vb);
    if let (StringValue::Engine(ta), StringValue::Engine(tb)) = (sa, sb) {
        return ta.as_bytes() == tb.as_bytes();
    }
    // Otherwise: equal bits.
    if va == vb {
        return true;
    }
    // Both NaNs (VM-4.3), whatever the sign and payload.
    if is_nan_bits(va) && is_nan_bits(vb) {
        return true;
    }
    let limit = max_edicts();
    // Both name the same edict n below LIMIT_MAX_EDICTS (TRACE-7.1).
    if let (Some(na), Some(nb)) = (a.header.entity_number(va), b.header.entity_number(vb))
        && na == nb
        && u64::from(na) < limit
    {
        return true;
    }
    // Both name the same field slot of the same edict n below LIMIT_MAX_EDICTS (TRACE-7.2).
    if let (Some((ea, fa)), Some((eb, fb))) =
        (a.header.pointer_target(va), b.header.pointer_target(vb))
        && ea == eb
        && fa == fb
        && u64::from(ea) < limit
    {
        return true;
    }
    false
}

/// Whether `bits` is an IEEE-754 single NaN: exponent all ones, mantissa non-zero (VM-4.3).
fn is_nan_bits(bits: u32) -> bool {
    f32::from_bits(bits).is_nan()
}

/// The latest value of each slot a trace has written, with the progs' global defaults
/// (DIFF-9.3). A global not yet written reads its progs file value (PROGS-12.1); a field not
/// yet written reads 0 (TRACE-3.4).
struct SlotValues {
    globals: Vec<u32>,
    overrides: BTreeMap<Slot, u32>,
}

impl SlotValues {
    /// A value map whose global defaults are `initial_globals` (PROGS-12.1).
    fn new(initial_globals: &[u32]) -> Self {
        Self {
            globals: initial_globals.to_vec(),
            overrides: BTreeMap::new(),
        }
    }

    /// Applies a change set, recording the new value of each listed slot (DIFF-9.3).
    fn apply(&mut self, changes: &Changes) {
        for global in changes.globals() {
            self.overrides
                .insert(Slot::Global(global.slot), global.bits);
        }
        for field in changes.fields() {
            self.overrides
                .insert(Slot::Field(field.edict, field.field), field.bits);
        }
    }

    /// The latest value of `slot`: the value in the latest change set that listed it, or the
    /// default (DIFF-9.3).
    fn value(&self, slot: Slot) -> u32 {
        if let Some(bits) = self.overrides.get(&slot) {
            return *bits;
        }
        match slot {
            Slot::Global(index) => usize::try_from(index)
                .ok()
                .and_then(|i| self.globals.get(i))
                .copied()
                .unwrap_or(0),
            Slot::Field(_, _) => 0,
        }
    }
}

/// Statement coverage of a trace (DIFF-11).
pub mod coverage {
    use std::collections::BTreeSet;

    use qc_trace::{Record, Trace};

    /// The distinct statement indices a trace executed, among the first `records_before`
    /// records (DIFF-11.1, DIFF-11.2).
    ///
    /// A statement is executed when a step record has its index (DIFF-11.1). `records_before`
    /// is how many of the trace's records to count, so a caller counts only the records before
    /// the first divergence of a comparison (DIFF-11.2); pass `trace.records.len()` to count
    /// the whole trace.
    #[must_use]
    pub fn executed(trace: &Trace, records_before: usize) -> BTreeSet<i32> {
        trace
            .records
            .iter()
            .take(records_before)
            .filter_map(|record| match record {
                Record::Step(step) => Some(step.statement),
                _ => None,
            })
            .collect()
    }

    /// The statement coverage of a progs over a set of traces (DIFF-11.2): the number of
    /// distinct statements executed in any trace divided by `numstatements - 1`.
    ///
    /// `numstatements` is the progs' statement count; its statements are 1 to
    /// `numstatements - 1` (DIFF-11.1). `traces` gives each trace with the count of records to
    /// consider (the records before its first divergence, DIFF-11.2). Returns 0.0 when the
    /// progs has no statements to cover.
    #[must_use]
    pub fn fraction<'a>(
        numstatements: u32,
        traces: impl IntoIterator<Item = (&'a Trace, usize)>,
    ) -> f64 {
        let total = numstatements.saturating_sub(1);
        if total == 0 {
            return 0.0;
        }
        let mut distinct = BTreeSet::new();
        for (trace, records_before) in traces {
            for statement in executed(trace, records_before) {
                // Statement 0 is reserved (DIFF-11.1); count only 1 to numstatements - 1.
                if statement >= 1 && statement.cast_unsigned() <= total {
                    distinct.insert(statement);
                }
            }
        }
        let covered = u32::try_from(distinct.len()).unwrap_or(u32::MAX);
        f64::from(covered) / f64::from(total)
    }
}

#[cfg(test)]
#[path = "compare/tests.rs"]
mod tests;
