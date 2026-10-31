// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The compiler differential's scripted function calls (`docs/spec/differential.md` DIFF-12,
//! P2-09, COMP-4).
//!
//! Two progs compiled from one codebase, id's `qcc`'s and `qc-codegen`'s, are each loaded in
//! `qc-vm` with the mock engine and set up as one scenario's world: the map spawned, the
//! clients entered and the scenario's frames run (DIFF-12.17). Every QuakeC function of the first progs is then called, in
//! several rounds, from that world with a drawn context (DIFF-12.2), applied to both progs by
//! name so the two calls take the same input. What each call did is compared (DIFF-12.3): how
//! it ended, the builtins it called, the named globals and every edict's named fields read
//! through their types, and the mock engine's state. When the two progs have the same
//! statements, the statements each call executed must be the same too (DIFF-12.4). The
//! statements the first progs executed are its coverage (DIFF-12.5).
//!
//! Implements `docs/spec/differential.md` DIFF-12.2 to DIFF-12.5 and DIFF-12.17.

use qc_edict::EdictNumber;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use qc_builtins::{EngineProfile, MockBuiltins};
use qc_progs::{Def, Progs};
use qc_types::{DefType, Etype, Slot};
use qc_vm::{ErrorKind, Machine, Observer, RunError};

use crate::host::{Host, RunRefused};
use crate::runner::scenarios;
use crate::scenario::{self, Scenario};

mod draw;

use draw::{Context, Drawer, Rng};

/// `OFS_RETURN` (VM-14.1): the return value's first slot.
const OFS_RETURN: usize = 1;
/// `OFS_PARM0` (VM-14.1): the first parameter slot; parameter `n` starts at `4 + 3n`.
const OFS_PARM0: usize = 4;
/// The name `qcc` gives every immediate's definition (QC-25.3); such a definition is not a
/// named global and is not compared (DIFF-12.3).
const IMMEDIATE: &[u8] = b"IMMEDIATE";

/// A value read through its definition's type, comparable across two progs (DIFF-12.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// A float or pointer slot, or an entity or field slot that names none, by its bits.
    Bits(u32),
    /// An entity, by its edict's number: its bits are the number times the progs' edict size,
    /// which two progs with other fields do not share (DIFF-12.3).
    Entity(usize),
    /// A field, by its name: its bits are the field's slot, which two progs may lay out in
    /// another order (DIFF-12.3).
    FieldName(Vec<u8>),
    /// A vector's three slots, by their bits.
    Vector([u32; 3]),
    /// A string, by its text, a progs string or an engine string; `None` when it has none.
    Text(Option<Vec<u8>>),
    /// A function, by its name; `None` for function 0 or an index outside the table.
    Function(Option<Vec<u8>>),
}

/// Where two progs' calls first differ (DIFF-12.3, DIFF-12.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Divergence {
    /// The function called, by name, or `None` for the world's setup.
    pub function: Option<Vec<u8>>,
    /// The round of the call; 0 for the setup.
    pub round: u32,
    /// What differed.
    pub what: Difference,
}

/// What differed between two calls (DIFF-12.3, DIFF-12.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Difference {
    /// The two progs name different globals or fields (DIFF-12.3).
    Definitions,
    /// One call ended in an error and the other did not, or with another message.
    Ending(Option<String>, Option<String>),
    /// The builtins called, in order, by number.
    Builtins(Vec<u32>, Vec<u32>),
    /// A named global's value.
    Global(Vec<u8>, Value, Value),
    /// One edict is free, or past the last, after one call and in use after the other.
    Free(usize),
    /// A named field of an edict.
    Field(usize, Vec<u8>, Value, Value),
    /// The mock engine's state: precaches, buffers, strings, the random source.
    Engine,
    /// The two progs have the same statements but the calls executed different ones
    /// (DIFF-12.4).
    Statements,
    /// The value the call returned, read from the return slot's first word (DIFF-12.7).
    Return(Value, Value),
    /// The first progs has a QuakeC function of this name and the second has none (DIFF-12.7).
    Missing,
}

/// The result of one compiler differential run over two progs (DIFF-12.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The calls made to each progs.
    pub calls: u32,
    /// Every call whose two runs differed, in order.
    pub divergences: Vec<Divergence>,
    /// The distinct statements the first progs executed, its setup included (DIFF-12.5).
    pub executed: BTreeSet<i32>,
    /// The first progs' statement count less the dummy statement 0 (DIFF-11.1).
    pub total_statements: u32,
}

impl Report {
    /// The fraction of the first progs' statements executed (DIFF-12.5); 0 when it has none.
    #[must_use]
    pub fn coverage(&self) -> f64 {
        coverage(&self.executed, self.total_statements)
    }
}

/// DIFF-12.13: each QuakeC function of the progs `bytes` with statements `executed` left
/// unexecuted, a line `<unexecuted>\t<of>\t<name>` each, most unexecuted first, for finding
/// what a codebase's calls do not reach.
///
/// # Errors
///
/// [`CallsError::Refused`] when the progs is refused.
pub fn uncovered(bytes: &[u8], executed: &BTreeSet<i32>) -> Result<String, CallsError> {
    let progs =
        Progs::read(bytes).map_err(|e| CallsError::Refused(RunRefused::Progs(e.to_string())))?;
    let mut starts: Vec<(i32, Vec<u8>)> = progs
        .functions()
        .iter()
        .filter(|f| f.first_statement > 0)
        .map(|f| {
            let name = progs
                .string(f.s_name)
                .map(<[u8]>::to_vec)
                .unwrap_or_default();
            (f.first_statement, name)
        })
        .collect();
    starts.sort();
    let end = i32::try_from(progs.statements().len()).unwrap_or(i32::MAX);
    let mut rows: Vec<(usize, usize, String)> = Vec::new();
    for (k, (start, name)) in starts.iter().enumerate() {
        let next = starts.get(k.saturating_add(1)).map_or(end, |(s, _)| *s);
        let of = usize::try_from(next.saturating_sub(*start)).unwrap_or(0);
        let missed = (*start..next).filter(|s| !executed.contains(s)).count();
        if missed > 0 {
            rows.push((missed, of, String::from_utf8_lossy(name).into_owned()));
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.2.cmp(&b.2)));
    Ok(rows
        .iter()
        .map(|(missed, of, name)| format!("{missed}\t{of}\t{name}\n"))
        .collect())
}

/// The fraction `executed` is of `total` statements (DIFF-12.5); 0 when `total` is 0.
#[must_use]
pub fn coverage(executed: &BTreeSet<i32>, total: u32) -> f64 {
    if total == 0 {
        return 0.0;
    }
    let count = u32::try_from(executed.len()).unwrap_or(u32::MAX);
    f64::from(count) / f64::from(total)
}

/// The rounds of calls to each function in the CI subset (DIFF-12.6).
pub const FAST_ROUNDS: u32 = 32;
/// The rounds of calls to each function in the nightly run (DIFF-12.6).
pub const FULL_ROUNDS: u32 = 128;
/// The most rounds a function is called in, as a multiple of the rounds asked for, when its
/// body still has statements no call executed (DIFF-12.16).
pub const EXTRA_ROUNDS: u32 = 4;
/// The least fraction of the first progs' statements a codebase's calls must execute
/// (DIFF-12.5, TEST-6).
pub const COVERAGE_BAR: f64 = 0.9;

/// The compiler differential over one codebase: a [`Report`] for each world of
/// [`crate::runner::scenarios`], and their statements together (DIFF-12.5).
#[derive(Clone, Debug, PartialEq)]
pub struct CodebaseReport {
    /// Each scenario's name and report, in order.
    pub scenarios: Vec<(&'static str, Report)>,
    /// The distinct statements of the first progs executed in any scenario.
    pub executed: BTreeSet<i32>,
    /// The first progs' statement count less the dummy statement 0 (DIFF-11.1).
    pub total_statements: u32,
}

impl CodebaseReport {
    /// The divergences over every scenario.
    #[must_use]
    pub fn divergences(&self) -> usize {
        self.scenarios
            .iter()
            .map(|(_, report)| report.divergences.len())
            .sum()
    }

    /// The fraction of the first progs' statements executed in any scenario (DIFF-12.5).
    #[must_use]
    pub fn coverage(&self) -> f64 {
        coverage(&self.executed, self.total_statements)
    }

    /// Whether the codebase meets the bar of DIFF-12.5: no divergence, and at least
    /// [`COVERAGE_BAR`] of the statements executed.
    #[must_use]
    pub fn passes(&self) -> bool {
        self.divergences() == 0 && self.coverage() >= COVERAGE_BAR
    }

    /// The report's row of the results table (DIFF-12.14): the codebase, the oracle's progs and
    /// ours, the divergences, the statements executed, and whether it passes.
    #[must_use]
    pub fn summary_row(&self, codebase: &str, oracle: &str, ours: &str) -> String {
        format!(
            "| {codebase} | {oracle} | {ours} | {} | {} of {} ({:.1}%) | {} |\n",
            self.divergences(),
            self.executed.len(),
            self.total_statements,
            self.coverage() * 100.0,
            if self.passes() { "pass" } else { "fail" }
        )
    }
}

/// The compiler that wrote the first progs, which says how the two progs' global definitions
/// must agree (DIFF-12.3, DIFF-12.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Oracle {
    /// id's `qcc`: the two progs name the same globals in the same order (DIFF-12.3).
    Qcc,
    /// The pinned FTEQCC, which leaves out definitions no statement uses: its named globals
    /// are found in ours in the same order, and ours may name more (DIFF-12.11).
    Fteqcc,
}

impl Oracle {
    /// Each named global of `theirs` paired with the global of `ours` it is compared with, as
    /// indices into the two programs' globals, or `None` when the definitions do not agree
    /// (DIFF-12.3, DIFF-12.11).
    fn pair_globals(
        self,
        theirs: &Program<'_, '_>,
        ours: &Program<'_, '_>,
    ) -> Option<Vec<(usize, usize)>> {
        match self {
            Self::Qcc => theirs
                .globals
                .iter()
                .map(|g| &g.0)
                .eq(ours.globals.iter().map(|g| &g.0))
                .then(|| (0..theirs.globals.len()).map(|i| (i, i)).collect()),
            Self::Fteqcc => {
                let mut pairs = Vec::with_capacity(theirs.globals.len());
                let mut rest = ours
                    .globals
                    .iter()
                    .enumerate()
                    .filter(|(_, global)| !ours.is_local(global));
                for (index, global) in theirs.globals.iter().enumerate() {
                    let (name, _, ofs) = global;
                    // FTEQCC's `QCC_FinaliseDef` gives a symbol nothing uses no slot, so its
                    // definition is left at slot 0, the null global; and `QCC_WriteData`
                    // names no local of a function whose locals it overlaps.
                    if *ofs == 0 || theirs.is_local(global) {
                        continue;
                    }
                    let (found, _) = rest.find(|(_, ours)| ours.0 == *name)?;
                    pairs.push((index, found));
                }
                Some(pairs)
            }
        }
    }
}

/// Runs [`run_calls`] on `theirs` and `ours` from each world of [`crate::runner::scenarios`],
/// `rounds` rounds each (DIFF-12.17, DIFF-12.5); `oracle` compiled `theirs`.
///
/// # Errors
///
/// [`CallsError`] when a scenario cannot be read, a progs is refused or a world's setup fails.
pub fn run_codebase(
    oracle: Oracle,
    theirs: &[u8],
    ours: &[u8],
    rounds: u32,
    profile: EngineProfile,
) -> Result<CodebaseReport, CallsError> {
    let mut worlds = Vec::new();
    for spec in scenarios("") {
        let text = format!("progs progs.dat\n{}", spec.directives);
        let scenario = scenario::read(text.as_bytes(), Path::new("."))
            .map_err(|e| CallsError::Scenario(format!("{}: {e}", spec.name)))?;
        worlds.push((spec.name, scenario, spec.entities.unwrap_or_default()));
    }
    // The scenarios share nothing, so each runs on its own thread; their reports are taken in
    // the scenarios' order, so the result is the same as one after another.
    let reports: Vec<(&'static str, Result<Report, CallsError>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = worlds
            .iter()
            .map(|(name, scenario, entities)| {
                let handle = scope.spawn(move || {
                    run_calls(oracle, theirs, ours, scenario, entities, rounds, profile)
                });
                (*name, handle)
            })
            .collect();
        handles
            .into_iter()
            .map(|(name, handle)| {
                let report = handle
                    .join()
                    .unwrap_or_else(|_| Err(CallsError::Stopped(name.to_owned())));
                (name, report)
            })
            .collect()
    });
    let mut result = CodebaseReport {
        scenarios: Vec::new(),
        executed: BTreeSet::new(),
        total_statements: 0,
    };
    for (name, report) in reports {
        let report = report?;
        result.executed.extend(report.executed.iter().copied());
        result.total_statements = report.total_statements;
        result.scenarios.push((name, report));
    }
    Ok(result)
}

/// Why a run could not start (DIFF-12.17).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallsError {
    /// A scenario of [`crate::runner::scenarios`] could not be read.
    Scenario(String),
    /// A progs was refused by the reader or the VM (DIFF-3.1).
    Refused(RunRefused),
    /// The world's setup ended in an error in a progs, so there is no world to call functions
    /// in (DIFF-12.17).
    Setup(String),
    /// The run of the scenario named here stopped before it gave a report.
    Stopped(String),
}

impl core::fmt::Display for CallsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Scenario(message) => write!(f, "bad scenario: {message}"),
            Self::Refused(refused) => write!(f, "progs refused: {refused}"),
            Self::Setup(message) => write!(f, "the world's setup failed: {message}"),
            Self::Stopped(name) => write!(f, "{name}: the run stopped before it finished"),
        }
    }
}

impl core::error::Error for CallsError {}

/// Calls every QuakeC function of `theirs`, which `oracle` compiled, and the function of the
/// same name in `ours`, in `rounds` rounds each, from the world `scenario` and `entities` set
/// up, and compares the two (DIFF-12).
///
/// # Errors
///
/// [`CallsError`] when a progs is refused or the world's setup fails in either.
pub fn run_calls(
    oracle: Oracle,
    theirs: &[u8],
    ours: &[u8],
    scenario: &Scenario,
    entities: &[u8],
    rounds: u32,
    profile: EngineProfile,
) -> Result<Report, CallsError> {
    let refused = |e: qc_progs::ReadError| CallsError::Refused(RunRefused::Progs(e.to_string()));
    let their_progs = Progs::read(theirs).map_err(refused)?;
    let our_progs = Progs::read(ours).map_err(refused)?;
    let mut their_world = World::new(&their_progs, scenario, entities, profile)?;
    let mut our_world = World::new(&our_progs, scenario, entities, profile)?;
    let pair = Pair::new(oracle, &their_world, &our_world);
    let total = u32::try_from(their_progs.statements().len())
        .unwrap_or(u32::MAX)
        .saturating_sub(1);
    let mut report = Report {
        calls: 0,
        divergences: Vec::new(),
        executed: their_world.setup_executed.clone(),
        total_statements: total,
    };
    if let Some(what) = pair.differ(
        &their_world,
        &their_world.setup,
        &our_world,
        &our_world.setup,
    ) {
        report.divergences.push(Divergence {
            function: None,
            round: 0,
            what,
        });
        return Ok(report);
    }

    let drawer = Drawer::new(&their_world);
    for (index, function) in their_progs.functions().iter().enumerate() {
        let (Some(name), Ok(their_index)) =
            (their_progs.string(function.s_name), i32::try_from(index))
        else {
            continue;
        };
        if function.first_statement <= 0 {
            continue;
        }
        // DIFF-12.7: a function the second progs does not have is a divergence, not a skip.
        let Some(&our_index) = our_world.program.functions.get(name) else {
            report.divergences.push(Divergence {
                function: Some(name.to_vec()),
                round: 0,
                what: Difference::Missing,
            });
            continue;
        };
        // The function's own constants depend on it alone, so they are read once.
        let pools = drawer.pools(function);
        // DIFF-12.16: while its body has statements no call executed and the last batch of
        // rounds reached one more, the function is called in another batch, up to
        // `EXTRA_ROUNDS` times `rounds` in all.
        let body = function.first_statement..drawer.body_end(function.first_statement);
        let reached =
            |executed: &BTreeSet<i32>| body.clone().filter(|s| executed.contains(s)).count();
        let whole = usize::try_from(body.end.saturating_sub(body.start)).unwrap_or(0);
        let batch = rounds.checked_shr(2).unwrap_or(0).max(1);
        let cap = rounds.saturating_mul(EXTRA_ROUNDS);
        let mut limit = rounds;
        let mut before = reached(&report.executed);
        let mut round = 0;
        while round < limit {
            let mut rng = Rng::new(name, round);
            let context = drawer.draw(&mut rng, (function, &pools), round);
            let theirs = their_world.call(their_index, &context);
            let ours = our_world.call(our_index, &context);
            report.calls = report.calls.saturating_add(1);
            report
                .executed
                .extend(theirs.executed.iter().map(|e| e.statement));
            if let Some(what) = pair.differ(&their_world, &theirs, &our_world, &ours) {
                report.divergences.push(Divergence {
                    function: Some(name.to_vec()),
                    round,
                    what,
                });
            }
            round = round.saturating_add(1);
            if round == limit && limit < cap {
                let now = reached(&report.executed);
                if now > before && now < whole {
                    limit = limit.saturating_add(batch).min(cap);
                }
                before = now;
            }
        }
    }
    Ok(report)
}

/// What is known of the two progs before any call (DIFF-12.3, DIFF-12.4).
struct Pair {
    /// Whether the two have the same statements, so the calls must execute the same ones.
    same_statements: bool,
    /// Whether the two have the same definitions, functions and strings, so two states agree
    /// when their slots hold the same bits.
    same_tables: bool,
    /// Each named global of the first progs with the one of the second it is compared with,
    /// as indices into [`Program::globals`], or `None` when the definitions do not agree.
    globals: Option<Vec<(usize, usize)>>,
    /// The compiler of the first progs.
    oracle: Oracle,
}

impl Pair {
    fn new(oracle: Oracle, a: &World<'_, '_, '_>, b: &World<'_, '_, '_>) -> Self {
        let (theirs, ours) = (a.program.progs, b.program.progs);
        let their_names = theirs.functions().iter().map(|f| f.s_name);
        let globals = a
            .program
            .fields
            .keys()
            .eq(b.program.fields.keys())
            .then(|| oracle.pair_globals(&a.program, &b.program))
            .flatten();
        Self {
            oracle,
            globals,
            same_statements: theirs.statements().iter().eq(ours.statements().iter()),
            same_tables: theirs.global_defs().iter().eq(ours.global_defs().iter())
                && theirs.field_defs().iter().eq(ours.field_defs().iter())
                && theirs.strings() == ours.strings()
                && their_names.eq(ours.functions().iter().map(|f| f.s_name)),
        }
    }

    /// The first difference between the two calls just made, or `None` (DIFF-12.3, DIFF-12.4).
    fn differ(
        &self,
        their_world: &World<'_, '_, '_>,
        theirs: &Outcome,
        our_world: &World<'_, '_, '_>,
        ours: &Outcome,
    ) -> Option<Difference> {
        if theirs.error != ours.error {
            return Some(Difference::Ending(theirs.error.clone(), ours.error.clone()));
        }
        // DIFF-12.11: where a loop is stopped depends on how many statements each pass takes,
        // which another compiler's progs need not share.
        let runaway = |outcome: &Outcome| outcome.kind == Some(ErrorKind::Runaway);
        if self.oracle == Oracle::Fteqcc && runaway(theirs) && runaway(ours) {
            return None;
        }
        let builtins = |outcome: &Outcome| -> Vec<u32> {
            outcome.executed.iter().filter_map(|e| e.builtin).collect()
        };
        let (their_builtins, our_builtins) = (builtins(theirs), builtins(ours));
        if their_builtins != our_builtins {
            return Some(Difference::Builtins(their_builtins, our_builtins));
        }
        if self.same_statements && theirs.executed != ours.executed {
            return Some(Difference::Statements);
        }
        if their_world.host.builtins.engine() != our_world.host.builtins.engine() {
            return Some(Difference::Engine);
        }
        if self.same_tables && same_slots(&their_world.host.machine, &our_world.host.machine) {
            return None;
        }
        let Some(globals) = &self.globals else {
            return Some(Difference::Definitions);
        };
        return_difference(their_world, our_world)
            .or_else(|| state_difference(their_world, our_world, globals))
    }
}

/// DIFF-12.7: the two calls' return values differ. The return slot's type is not in a version
/// 6 progs, so its first word is read as each type a function returns in one word, and the
/// two agree when any reading does: the same bits, the same text, the same edict or the same
/// function. A vector's other two words are compared only through the state they leave.
fn return_difference(a: &World<'_, '_, '_>, b: &World<'_, '_, '_>) -> Option<Difference> {
    let readings = |world: &World<'_, '_, '_>| {
        [
            world.global(DefType::Known(Etype::Float), OFS_RETURN),
            world.global(DefType::Known(Etype::String), OFS_RETURN),
            world.global(DefType::Known(Etype::Entity), OFS_RETURN),
            world.global(DefType::Known(Etype::Function), OFS_RETURN),
        ]
    };
    let (theirs, ours) = (readings(a), readings(b));
    let agree = theirs
        .iter()
        .zip(&ours)
        .any(|(t, o)| t == o && !matches!(t, Value::Text(None) | Value::Function(None)));
    (!agree).then(|| {
        let [their_bits, ..] = theirs;
        let [our_bits, ..] = ours;
        Difference::Return(their_bits, our_bits)
    })
}

/// Whether two machines hold the same bits in every global and in every edict's fields, with
/// the same edicts free.
fn same_slots(a: &Machine, b: &Machine) -> bool {
    let (ea, eb) = (a.edicts(), b.edicts());
    a.globals() == b.globals()
        && ea.num_edicts() == eb.num_edicts()
        && (0..ea.num_edicts()).all(|n| {
            ea.is_free(EdictNumber::new(n)) == eb.is_free(EdictNumber::new(n))
                && ea.fields(EdictNumber::new(n)) == eb.fields(EdictNumber::new(n))
        })
}

/// The first named global or field whose value differs between the two worlds, the globals
/// compared in the pairs `globals` gives (DIFF-12.3, DIFF-12.11).
fn state_difference(
    a: &World<'_, '_, '_>,
    b: &World<'_, '_, '_>,
    globals: &[(usize, usize)],
) -> Option<Difference> {
    let (pa, pb) = (&a.program, &b.program);
    for (theirs, ours) in globals {
        let (Some((name, their_type, their_ofs)), Some((_, our_type, our_ofs))) =
            (pa.globals.get(*theirs), pb.globals.get(*ours))
        else {
            return Some(Difference::Definitions);
        };
        let theirs = a.global(*their_type, *their_ofs);
        let ours = b.global(*our_type, *our_ofs);
        if theirs != ours {
            return Some(Difference::Global(name.clone(), theirs, ours));
        }
    }
    let (ea, eb) = (a.host.machine.edicts(), b.host.machine.edicts());
    for n in 0..ea.num_edicts().max(eb.num_edicts()) {
        let free = ea.is_free(EdictNumber::new(n));
        if free != eb.is_free(EdictNumber::new(n)) {
            return Some(Difference::Free(n));
        }
        if free != Some(false) {
            continue;
        }
        for ((name, (their_type, their_ofs)), (our_type, our_ofs)) in
            pa.fields.iter().zip(pb.fields.values())
        {
            let theirs = a.field(n, *their_type, *their_ofs);
            let ours = b.field(n, *our_type, *our_ofs);
            if theirs != ours {
                return Some(Difference::Field(n, name.clone(), theirs, ours));
            }
        }
    }
    None
}

/// One statement a call executed (DIFF-12.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Executed {
    function: i32,
    statement: i32,
    builtin: Option<u32>,
}

/// An observer that keeps the statements a call executes (DIFF-12.4).
#[derive(Default)]
struct Statements {
    executed: Vec<Executed>,
}

impl Observer for Statements {
    fn run(&mut self, _machine: &Machine, _function: i32) {}

    fn step(
        &mut self,
        _machine: &Machine,
        function: i32,
        statement: i32,
        _opcode: u16,
        builtin: Option<u32>,
    ) {
        self.executed.push(Executed {
            function,
            statement,
            builtin,
        });
    }

    fn exit(&mut self, _machine: &Machine) {}

    fn error(&mut self, _machine: &Machine, _error: &RunError) {}
}

/// A named global definition: its name, type and slot.
/// FTEQCC's `ev_struct` and `ev_union`, `progslib.h`'s `etype_t` (DIFF-12.10).
const EV_STRUCT: u16 = 14;
const EV_UNION: u16 = 15;

type Global = (Vec<u8>, DefType, usize);

/// A progs' named definitions and functions (DIFF-12.3).
struct Program<'a, 'p> {
    progs: &'a Progs<'p>,
    /// The named globals, in definition order.
    globals: Vec<Global>,
    /// The slot of each name a context writes by name: its first float or string global
    /// outside every function's locals, else its first (DIFF-12.11).
    global_slots: BTreeMap<Vec<u8>, usize>,
    /// The named fields, by name: the type and slot of the first definition of each.
    fields: BTreeMap<Vec<u8>, (DefType, usize)>,
    /// The QuakeC functions, by name: the index of the first of each name.
    functions: BTreeMap<Vec<u8>, i32>,
    /// The slots of the QuakeC functions' parameters and locals, from `parm_start` for
    /// `locals` slots, as start and end.
    locals: Vec<(usize, usize)>,
}

impl<'a, 'p> Program<'a, 'p> {
    fn new(progs: &'a Progs<'p>) -> Self {
        let named = |def: &Def| {
            progs
                .string(def.s_name)
                .filter(|name| !name.is_empty() && *name != IMMEDIATE)
                .map(<[u8]>::to_vec)
                .zip(usize::try_from(def.ofs).ok())
        };
        let mut globals = Vec::new();
        for def in progs.global_defs().iter() {
            // DIFF-12.10: a struct or union element is compared by its members.
            if matches!(def.def_type(), DefType::Unknown(EV_STRUCT | EV_UNION)) {
                continue;
            }
            if let Some((name, ofs)) = named(&def) {
                globals.push((name, def.def_type(), ofs));
            }
        }
        let mut fields = BTreeMap::new();
        for def in progs.field_defs().iter() {
            if let Some((name, ofs)) = named(&def) {
                fields.entry(name).or_insert((def.def_type(), ofs));
            }
        }
        let mut functions = BTreeMap::new();
        let mut locals = Vec::new();
        for function in progs.functions().iter() {
            if let (true, Ok(start), Ok(count)) = (
                function.first_statement > 0,
                usize::try_from(function.parm_start),
                usize::try_from(function.locals),
            ) {
                locals.push((start, start.saturating_add(count)));
            }
        }
        for (index, function) in progs.functions().iter().enumerate() {
            if let (Some(name), Ok(index)) = (progs.string(function.s_name), i32::try_from(index))
                && function.first_statement > 0
            {
                functions.entry(name.to_vec()).or_insert(index);
            }
        }
        let mut program = Self {
            progs,
            globals,
            global_slots: BTreeMap::new(),
            fields,
            functions,
            locals,
        };
        // DIFF-12.11: a context writes a global by name, a float or a string, to its first
        // float or string definition outside every function's locals, else to its first.
        let scalar = |g: &&Global| matches!(g.1, DefType::Known(Etype::Float | Etype::String));
        let mut global_slots = BTreeMap::new();
        for global in program
            .globals
            .iter()
            .filter(scalar)
            .filter(|g| !program.is_local(g))
        {
            global_slots.entry(global.0.clone()).or_insert(global.2);
        }
        for (name, _, ofs) in program.globals.iter().filter(scalar) {
            global_slots.entry(name.clone()).or_insert(*ofs);
        }
        program.global_slots = global_slots;
        program
    }

    /// Whether `global` is a parameter or local of a QuakeC function: a global in its locals
    /// that is not the definition of the QuakeC function of its name, as a state's
    /// `[frame, next]` declares `next` while the body is compiled (QC-13.9).
    fn is_local(&self, (name, def_type, ofs): &Global) -> bool {
        let declares_function = *def_type == DefType::Known(Etype::Function)
            && self.functions.get(name).is_some_and(|index| {
                self.progs
                    .globals()
                    .get(*ofs)
                    .is_some_and(|slot| u32::try_from(*index) == Ok(slot.bits()))
            });
        !declares_function
            && self
                .locals
                .iter()
                .any(|(start, end)| (*start..*end).contains(ofs))
    }
}

/// How one call went in one progs (DIFF-12.3, DIFF-12.4).
#[derive(Default)]
struct Outcome {
    /// The run error's message, or `None` when the call returned.
    error: Option<String>,
    /// The run error's kind, or `None` when the call returned.
    kind: Option<ErrorKind>,
    /// The statements it executed, in order.
    executed: Vec<Executed>,
}

/// One progs' world after the setup, kept to restore before each call (DIFF-12.17).
struct World<'a, 'p, 's> {
    program: Program<'a, 'p>,
    host: Host<'s>,
    machine: Machine,
    builtins: MockBuiltins,
    /// The statements the setup executed (DIFF-12.5).
    setup_executed: BTreeSet<i32>,
    /// How the setup went: the statements and builtins it executed, compared with the other
    /// progs' setup as two calls are (DIFF-12.17).
    setup: Outcome,
}

impl<'a, 'p, 's> World<'a, 'p, 's> {
    /// Loads `progs` and sets up the world of `scenario` and `entities`: the map spawned, the
    /// clients entered and the scenario's frames run with its inputs (DIFF-12.17).
    fn new(
        progs: &'a Progs<'p>,
        scenario: &'s Scenario,
        entities: &[u8],
        profile: EngineProfile,
    ) -> Result<Self, CallsError> {
        let mut host = Host::new(scenario, progs).map_err(CallsError::Refused)?;
        host.builtins.engine_mut().profile = profile;
        host.start_map(progs, entities)
            .map_err(|e| CallsError::Setup(format!("{e:?}")))?;
        for slot in 0..scenario.clients {
            host.client_enters(progs, slot)
                .map_err(|e| CallsError::Setup(e.message()))?;
        }
        for frame in 1..=scenario.frames {
            host.server_frame(progs, scenario.frametime, frame)
                .map_err(|e| CallsError::Setup(e.message()))?;
        }
        let executed: Vec<Executed> = host
            .trace
            .records()
            .iter()
            .filter_map(|record| match record {
                qc_trace::Record::Step(step) => Some(Executed {
                    function: step.function,
                    statement: step.statement,
                    builtin: step.builtin,
                }),
                qc_trace::Record::Run { .. }
                | qc_trace::Record::Exit { .. }
                | qc_trace::Record::Error { .. }
                | qc_trace::Record::String { .. } => None,
            })
            .collect();
        let setup_executed = executed.iter().map(|e| e.statement).collect();
        host.trace.clear();
        Ok(Self {
            machine: host.machine.clone(),
            builtins: host.builtins.clone(),
            program: Program::new(progs),
            host,
            setup_executed,
            setup: Outcome {
                error: None,
                kind: None,
                executed,
            },
        })
    }

    /// The edicts in use after the setup, the world first.
    fn in_use(&self) -> Vec<usize> {
        let edicts = self.machine.edicts();
        (0..edicts.num_edicts())
            .filter(|&n| edicts.is_free(EdictNumber::new(n)) == Some(false))
            .collect()
    }

    /// Restores the world, applies `context` and calls `function` (DIFF-12.2).
    fn call(&mut self, function: i32, context: &Context) -> Outcome {
        self.host.machine.clone_from(&self.machine);
        self.host.builtins.clone_from(&self.builtins);
        // A call's trace records are never read; dropping them keeps a run's memory flat.
        self.host.trace.clear();
        context.apply(&mut self.host, &self.program);
        let mut observer = Statements::default();
        let result = self.host.machine.execute_program(
            self.program.progs,
            context.server,
            &mut self.host.builtins,
            &mut observer,
            function,
        );
        Outcome {
            kind: result.as_ref().err().map(|e| e.kind.clone()),
            error: result.err().map(|e| e.message()),
            executed: observer.executed,
        }
    }

    /// The value of the global of type `def_type` at slot `ofs` (DIFF-12.3).
    fn global(&self, def_type: DefType, ofs: usize) -> Value {
        let machine = &self.host.machine;
        self.value(def_type, ofs, |n| {
            i32::try_from(n)
                .ok()
                .and_then(|n| machine.global(n).ok())
                .unwrap_or(Slot::ZERO)
        })
    }

    /// The value of edict `edict`'s field of type `def_type` at slot `ofs` (DIFF-12.3).
    fn field(&self, edict: usize, def_type: DefType, ofs: usize) -> Value {
        let edicts = self.host.machine.edicts();
        self.value(def_type, ofs, |n| {
            edicts
                .field(EdictNumber::new(edict), n)
                .unwrap_or(Slot::ZERO)
        })
    }

    /// The value of type `def_type` starting at slot `ofs` of `slot` (DIFF-12.3): a string by
    /// its text, an engine string's or the progs', a function by its name, a vector by its
    /// three slots, anything else by its bits.
    fn value(&self, def_type: DefType, ofs: usize, slot: impl Fn(usize) -> Slot) -> Value {
        let progs = self.program.progs;
        match def_type {
            DefType::Known(Etype::String) => {
                let value = slot(ofs).as_i32();
                let text = self
                    .host
                    .builtins
                    .engine()
                    .strings
                    .text(value)
                    .or_else(|| progs.string(value));
                Value::Text(text.map(<[u8]>::to_vec))
            }
            DefType::Known(Etype::Function) => Value::Function(
                usize::try_from(slot(ofs).as_i32())
                    .ok()
                    .filter(|&n| n != 0)
                    .and_then(|n| progs.functions().get(n))
                    .and_then(|f| progs.string(f.s_name))
                    .map(<[u8]>::to_vec),
            ),
            DefType::Known(Etype::Vector) => Value::Vector([
                slot(ofs).bits(),
                slot(ofs.saturating_add(1)).bits(),
                slot(ofs.saturating_add(2)).bits(),
            ]),
            DefType::Known(Etype::Entity) => {
                let bits = slot(ofs).bits();
                let size = self.host.machine.edicts().layout().edict_size();
                usize::try_from(i32::from_ne_bytes(bits.to_ne_bytes()))
                    .ok()
                    .zip(usize::try_from(size).ok().filter(|&size| size > 0))
                    .filter(|(value, size)| value.checked_rem(*size) == Some(0))
                    .and_then(|(value, size)| value.checked_div(size))
                    .map_or(Value::Bits(bits), Value::Entity)
            }
            DefType::Known(Etype::Field) => {
                let bits = slot(ofs).bits();
                usize::try_from(bits)
                    .ok()
                    .and_then(|slot| {
                        self.program
                            .fields
                            .iter()
                            .find(|(_, (_, field))| *field == slot)
                            .map(|(name, _)| name.clone())
                    })
                    .map_or(Value::Bits(bits), Value::FieldName)
            }
            DefType::Known(Etype::Void | Etype::Float | Etype::Pointer) | DefType::Unknown(_) => {
                Value::Bits(slot(ofs).bits())
            }
        }
    }
}

#[cfg(test)]
mod tests;
