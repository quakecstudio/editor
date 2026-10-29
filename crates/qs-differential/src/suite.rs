// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The end-to-end differential VM runner (`docs/spec/differential.md`, P1-13, FND-8).
//!
//! A differential run writes a scenario (DIFF-2) for a corpus progs, with the entities of a
//! corpus map, runs it in QuakeC Studio's runner ([`crate::run_scenario`]) and in the id VM
//! oracle, and compares the two traces statement by statement (DIFF-9), reporting the first
//! divergence with context (DIFF-9.5) and measuring how many of the progs' statements the
//! traces executed (DIFF-11). The acceptance of P1-13 is zero divergences on id 1.01 and the
//! rerelease; statement coverage is measured and reported, not gated at a flat 90%, because the
//! mock does not drive the running world (DIFF-5.8, DIFF-1.5, ADR-0013).
//!
//! This module is the oracle-independent half of the runner: it builds the entity lump of a
//! scenario from a `.map` (DIFF-2.5), runs QuakeC Studio's side, compares a pair of traces and
//! aggregates coverage over a codebase. How the oracle's trace is produced (running the oracle
//! binary `cargo xtask oracles` builds) is injected as a closure, so this logic is unit-tested
//! and mutation-tested without the C oracle. `xtask/src/differential.rs` wires it to the oracle
//! binary and the corpus.
//!
//! Implements `docs/spec/differential.md` DIFF-1 (the two runners meeting over the scenario and
//! trace), with the entity lump of DIFF-2.5 built from a `.map` by `qs-map`.

use std::collections::BTreeSet;

use qc_progs::Progs;
use qc_trace::{Trace, decode};
use qs_map::MapFile;

use crate::compare::{Comparison, Divergence, coverage};
use crate::host::{RunOutput, RunRefused};
use crate::scenario::Scenario;

/// The result of comparing one scenario's two traces (DIFF-9), with the records the coverage
/// of DIFF-11.2 counts (those before the first divergence).
#[derive(Clone, Debug)]
pub struct ScenarioResult {
    /// QuakeC Studio's trace of the scenario (DIFF-1.1), for coverage (DIFF-11.2).
    pub trace: Trace,
    /// How the two traces compared (DIFF-9): equal, or the first divergence (DIFF-9.5).
    pub comparison: Comparison,
    /// How many of QuakeC Studio's change-carrying records precede the first divergence, the
    /// records DIFF-11.2 counts for coverage; the whole trace when there is no divergence.
    pub records_before_divergence: usize,
}

impl ScenarioResult {
    /// Whether the two traces agree (DIFF-9.1); a divergence is a failure (DIFF-9.6).
    #[must_use]
    pub fn agrees(&self) -> bool {
        matches!(self.comparison, Comparison::Equal)
    }

    /// The first divergence, when the traces disagree (DIFF-9.5).
    #[must_use]
    pub fn divergence(&self) -> Option<&Divergence> {
        match &self.comparison {
            Comparison::Equal => None,
            Comparison::Failure(divergence) => Some(divergence),
        }
    }
}

/// Why an end-to-end run could not produce a comparable pair of traces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuiteError {
    /// QuakeC Studio's runner refused the scenario before writing a trace (DIFF-3.1).
    Refused(String),
    /// The oracle's trace bytes did not decode as a trace (`trace.md`).
    OracleTrace(String),
    /// A `.map` could not be read, so no entity lump could be built (DIFF-2.5).
    Map(String),
    /// The progs could not be read, so its statement count is unknown (DIFF-11.2).
    Progs(String),
}

impl core::fmt::Display for SuiteError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Refused(m) => write!(f, "QuakeC Studio refused the scenario: {m}"),
            Self::OracleTrace(m) => write!(f, "the oracle's trace did not decode: {m}"),
            Self::Map(m) => write!(f, "the map could not be read: {m}"),
            Self::Progs(m) => write!(f, "the progs could not be read: {m}"),
        }
    }
}

impl core::error::Error for SuiteError {}

impl From<RunRefused> for SuiteError {
    fn from(refused: RunRefused) -> Self {
        Self::Refused(refused.to_string())
    }
}

/// The entity lump of a scenario (DIFF-2.5), built from a `.map` by removing its brushes.
///
/// DIFF-2.5 reads the entities file as `ED_LoadFromFile` reads a BSP entity lump (EDICT-8):
/// the same `{ "key" "value" ... }` syntax a `.map` holds, without the brushes (BSP-2). This
/// writes each entity's key and value pairs, in file order, as a quoted pair per line, and
/// gives each brush entity after the world its model number `*1`, `*2`, ... in file order, as
/// qbsp does (MAP-6.2), so `setmodel` finds the submodel the scenario's `submodels` declares.
/// A `model` pair a brush entity already holds is replaced; a brush entity with no `model` pair
/// gains one. The world (the first entity, MAP-6.1) keeps its pairs unchanged.
///
/// # Errors
///
/// [`SuiteError::Map`] when `qs-map` cannot parse the `.map`.
pub fn entity_lump(map_bytes: &[u8]) -> Result<Vec<u8>, SuiteError> {
    let map = MapFile::parse(map_bytes).map_err(|e| SuiteError::Map(format!("{e:?}")))?;
    // MAP-6.2: brush entities after the world are models *1, *2, ... in file order.
    let mut model_number = 0_usize;
    let mut out = Vec::new();
    for (index, entity) in map.entities.iter().enumerate() {
        let is_brush_entity = index > 0 && entity.has_brushes();
        let model = if is_brush_entity {
            model_number = model_number.saturating_add(1);
            Some(format!("*{model_number}"))
        } else {
            None
        };
        out.push(b'{');
        out.push(b'\n');
        let mut wrote_model = false;
        for pair in entity.pairs() {
            let key = pair.key();
            if model.is_some() && key == b"model" {
                // A brush entity's model is the submodel qbsp assigns, not the .map's value.
                write_pair(&mut out, key, model.as_deref().unwrap_or("").as_bytes());
                wrote_model = true;
            } else {
                write_pair(&mut out, key, pair.value());
            }
        }
        if let Some(model) = &model
            && !wrote_model
        {
            write_pair(&mut out, b"model", model.as_bytes());
        }
        out.push(b'}');
        out.push(b'\n');
    }
    Ok(out)
}

/// Writes one `"key" "value"` pair, each already a BSP entity-lump token (EDICT-8.6): the
/// bytes are written as they are, inside double quotes, since a `.map` key or value holds no
/// double quote of its own (MAP-2.3).
fn write_pair(out: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    out.push(b'"');
    out.extend_from_slice(key);
    out.extend_from_slice(b"\" \"");
    out.extend_from_slice(value);
    out.extend_from_slice(b"\"\n");
}

/// The progs file's value for every global slot (PROGS-12.1), the initial globals the
/// comparator reads for a slot no change set lists (DIFF-9.3).
///
/// # Errors
///
/// [`SuiteError::Progs`] when `qc-progs` refuses the progs.
pub fn initial_globals(progs_bytes: &[u8]) -> Result<Vec<u32>, SuiteError> {
    let progs = Progs::read(progs_bytes).map_err(|e| SuiteError::Progs(format!("{e:?}")))?;
    Ok(progs.globals().iter().map(|slot| slot.bits()).collect())
}

/// The number of statements a progs has, `numstatements` (PROGS-2.1); its statements are 1 to
/// `numstatements - 1` (DIFF-11.1).
///
/// # Errors
///
/// [`SuiteError::Progs`] when `qc-progs` refuses the progs.
pub fn numstatements(progs_bytes: &[u8]) -> Result<u32, SuiteError> {
    let progs = Progs::read(progs_bytes).map_err(|e| SuiteError::Progs(format!("{e:?}")))?;
    Ok(progs.header().numstatements.cast_unsigned())
}

/// The number of QuakeC Studio change-carrying records before the first divergence of
/// `comparison` (DIFF-11.2): all of them when the traces agree, and otherwise the records
/// before the first diverging change-carrying record, which itself is not counted.
#[must_use]
pub fn records_before(trace: &Trace, comparison: &Comparison) -> usize {
    match comparison {
        Comparison::Equal => trace.records.len(),
        Comparison::Failure(divergence) => records_before_carrier(trace, carrier_index(divergence)),
    }
}

/// The index among change-carrying records at which a divergence occurs (DIFF-9.5): the
/// record index for a record or value divergence, the missing index for a length divergence,
/// and 0 for a header divergence, where nothing was compared.
fn carrier_index(divergence: &Divergence) -> usize {
    match divergence {
        Divergence::Header { .. } => 0,
        Divergence::Length { index, .. }
        | Divergence::Record { index, .. }
        | Divergence::Value { index, .. } => *index,
    }
}

/// The number of records in `trace` before the `carrier`-th change-carrying record, so
/// [`coverage::executed`] counts the step records of the agreeing prefix and not the record
/// the VMs disagree on (DIFF-11.2). A `carrier` at or past the number of change-carrying
/// records, where this trace is the shorter, counts the whole trace.
fn records_before_carrier(trace: &Trace, carrier: usize) -> usize {
    let mut carriers = 0_usize;
    for (position, record) in trace.records.iter().enumerate() {
        if record.changes().is_some() {
            if carriers == carrier {
                return position;
            }
            carriers = carriers.saturating_add(1);
        }
    }
    trace.records.len()
}

/// Runs one scenario end to end and compares the two traces (DIFF-1, DIFF-9).
///
/// QuakeC Studio's trace comes from [`crate::run_scenario`] with `progs_bytes` and
/// `entities`; the oracle's trace bytes come from `oracle`, a closure that runs the id VM
/// oracle on the same scenario and returns its trace file (`xtask` runs the oracle binary).
/// The two are compared by DIFF-9 with the progs file's globals as the initial values
/// (DIFF-9.3), and the records before the first divergence are counted for coverage
/// (DIFF-11.2).
///
/// # Errors
///
/// [`SuiteError`] when QuakeC Studio refuses the scenario (DIFF-3.1), the oracle's trace does
/// not decode, or the progs cannot be read.
pub fn run_and_compare(
    scenario: &Scenario,
    progs_bytes: &[u8],
    entities: &[u8],
    oracle: impl FnOnce() -> Result<Vec<u8>, SuiteError>,
) -> Result<ScenarioResult, SuiteError> {
    let RunOutput { trace: ours, .. } = crate::run_scenario(scenario, progs_bytes, entities)?;
    let oracle_bytes = oracle()?;
    let theirs = decode(&oracle_bytes).map_err(|e| SuiteError::OracleTrace(format!("{e:?}")))?;
    let initial = initial_globals(progs_bytes)?;
    // DIFF-9.5: comparing QuakeC Studio (A) against the oracle (B), so a reported divergence's
    // "A" is ours and "B" is the oracle's.
    let comparison = crate::compare(&ours, &theirs, &initial);
    let records_before_divergence = records_before(&ours, &comparison);
    Ok(ScenarioResult {
        trace: ours,
        comparison,
        records_before_divergence,
    })
}

/// How a codebase did over its scenarios (DIFF-9, DIFF-11): the acceptance of P1-13.
#[derive(Clone, Debug, Default)]
pub struct CodebaseReport {
    /// The codebase id, for the report.
    pub id: String,
    /// `numstatements - 1`, the statements the coverage fraction divides by (DIFF-11.2).
    pub total_statements: u32,
    /// The distinct statements executed before any divergence, across every scenario
    /// (DIFF-11.2).
    pub executed: BTreeSet<i32>,
    /// How many scenarios ran.
    pub scenarios: usize,
    /// How many scenarios diverged (a failure, DIFF-9.6); zero is the acceptance of P1-13.
    pub divergences: usize,
}

impl CodebaseReport {
    /// A fresh report for a codebase with `numstatements` statements (PROGS-2.1).
    #[must_use]
    pub fn new(id: &str, numstatements: u32) -> Self {
        Self {
            id: id.to_owned(),
            total_statements: numstatements.saturating_sub(1),
            executed: BTreeSet::new(),
            scenarios: 0,
            divergences: 0,
        }
    }

    /// Records one scenario's result into the report: its executed statements (before any
    /// divergence, DIFF-11.2) and whether it diverged (DIFF-9.6).
    pub fn record(&mut self, result: &ScenarioResult) {
        self.scenarios = self.scenarios.saturating_add(1);
        if !result.agrees() {
            self.divergences = self.divergences.saturating_add(1);
        }
        for statement in coverage::executed(&result.trace, result.records_before_divergence) {
            // DIFF-11.1: statements are 1 to numstatements - 1; statement 0 is reserved.
            if statement >= 1 && statement.cast_unsigned() <= self.total_statements {
                self.executed.insert(statement);
            }
        }
    }

    /// Records that a scenario's run failed (a refused scenario or a bad oracle trace): it
    /// counts as a scenario that diverged (DIFF-9.6), so a run that cannot be compared fails
    /// the codebase rather than being silently skipped.
    pub fn count_error(&mut self) {
        self.scenarios = self.scenarios.saturating_add(1);
        self.divergences = self.divergences.saturating_add(1);
    }

    /// The statement coverage of the codebase (DIFF-11.2): distinct executed statements over
    /// `numstatements - 1`, 0.0 when the progs has no statements.
    #[must_use]
    pub fn coverage(&self) -> f64 {
        if self.total_statements == 0 {
            return 0.0;
        }
        let covered = u32::try_from(self.executed.len()).unwrap_or(u32::MAX);
        f64::from(covered) / f64::from(self.total_statements)
    }

    /// Whether the codebase meets its differential bar (P1-13, ADR-0013): zero divergences
    /// over at least one scenario. Statement coverage ([`Self::coverage`]) is measured and
    /// reported but not gated here, because the mock does not drive the running world
    /// (DIFF-5.8, DIFF-1.5), so most of a Quake progs is unreachable under it.
    #[must_use]
    pub fn passes(&self) -> bool {
        self.scenarios > 0 && self.divergences == 0
    }
}

#[cfg(test)]
#[path = "suite/tests.rs"]
mod tests;
