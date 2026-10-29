// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The compiler differential's scripted function calls (`docs/spec/differential.md` DIFF-12,
//! P2-09, COMP-4).
//!
//! Two progs compiled from one codebase, id's `qcc`'s and `qc-codegen`'s, are each loaded in
//! `qc-vm` with the mock engine and set up as one scenario's world: the map spawned and the
//! clients entered (DIFF-12.1). Every QuakeC function of the first progs is then called, in
//! several rounds, from that world with a drawn context (DIFF-12.2), applied to both progs by
//! name so the two calls take the same input. What each call did is compared (DIFF-12.3): how
//! it ended, the builtins it called, the named globals and every edict's named fields read
//! through their types, and the mock engine's state. When the two progs have the same
//! statements, the statements each call executed must be the same too (DIFF-12.4). The
//! statements the first progs executed are its coverage (DIFF-12.5).
//!
//! Implements `docs/spec/differential.md` DIFF-12.1 to DIFF-12.5.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use qc_builtins::{MockBuiltins, MockEngine};
use qc_progs::{Def, Progs};
use qc_types::{DefType, Etype, Slot};
use qc_vm::{Machine, Observer, RunError};

use crate::host::{Host, RunRefused};
use crate::runner::scenarios;
use crate::scenario::{self, Scenario};

mod draw;

use draw::{Context, Drawer, Rng};

/// `OFS_PARM0` (VM-14.1): the first parameter slot; parameter `n` starts at `4 + 3n`.
const OFS_PARM0: usize = 4;
/// The name `qcc` gives every immediate's definition (QC-25.3); such a definition is not a
/// named global and is not compared (DIFF-12.3).
const IMMEDIATE: &[u8] = b"IMMEDIATE";

/// A value read through its definition's type, comparable across two progs (DIFF-12.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// A float, entity, field or pointer slot, by its bits.
    Bits(u32),
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
}

/// Runs [`run_calls`] on `theirs` and `ours` from each world of [`crate::runner::scenarios`],
/// `rounds` rounds each (DIFF-12.1, DIFF-12.5).
///
/// # Errors
///
/// [`CallsError`] when a scenario cannot be read, a progs is refused or a world's setup fails.
pub fn run_codebase(theirs: &[u8], ours: &[u8], rounds: u32) -> Result<CodebaseReport, CallsError> {
    let mut result = CodebaseReport {
        scenarios: Vec::new(),
        executed: BTreeSet::new(),
        total_statements: 0,
    };
    for spec in scenarios("") {
        let text = format!("progs progs.dat\n{}", spec.directives);
        let scenario = scenario::read(text.as_bytes(), Path::new("."))
            .map_err(|e| CallsError::Scenario(format!("{}: {e}", spec.name)))?;
        let entities = spec.entities.unwrap_or_default();
        let report = run_calls(theirs, ours, &scenario, &entities, rounds)?;
        result.executed.extend(report.executed.iter().copied());
        result.total_statements = report.total_statements;
        result.scenarios.push((spec.name, report));
    }
    Ok(result)
}

/// Why a run could not start (DIFF-12.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallsError {
    /// A scenario of [`crate::runner::scenarios`] could not be read.
    Scenario(String),
    /// A progs was refused by the reader or the VM (DIFF-3.1).
    Refused(RunRefused),
    /// The world's setup ended in an error in a progs, so there is no world to call functions
    /// in (DIFF-12.1).
    Setup(String),
}

impl core::fmt::Display for CallsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Scenario(message) => write!(f, "bad scenario: {message}"),
            Self::Refused(refused) => write!(f, "progs refused: {refused}"),
            Self::Setup(message) => write!(f, "the world's setup failed: {message}"),
        }
    }
}

impl core::error::Error for CallsError {}

/// Calls every QuakeC function of `theirs` and the function of the same name in `ours`, in
/// `rounds` rounds each, from the world `scenario` and `entities` set up, and compares the two
/// (DIFF-12).
///
/// # Errors
///
/// [`CallsError`] when a progs is refused or the world's setup fails in either.
pub fn run_calls(
    theirs: &[u8],
    ours: &[u8],
    scenario: &Scenario,
    entities: &[u8],
    rounds: u32,
) -> Result<Report, CallsError> {
    let refused = |e: qc_progs::ReadError| CallsError::Refused(RunRefused::Progs(e.to_string()));
    let their_progs = Progs::read(theirs).map_err(refused)?;
    let our_progs = Progs::read(ours).map_err(refused)?;
    let mut their_world = World::new(&their_progs, scenario, entities)?;
    let mut our_world = World::new(&our_progs, scenario, entities)?;
    let pair = Pair::new(&their_progs, &our_progs);
    let total = u32::try_from(their_progs.statements().len())
        .unwrap_or(u32::MAX)
        .saturating_sub(1);
    let mut report = Report {
        calls: 0,
        divergences: Vec::new(),
        executed: their_world.setup_executed.clone(),
        total_statements: total,
    };
    let setup = Outcome::default();
    if let Some(what) = pair.differ(&their_world, &setup, &our_world, &setup) {
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
        // The setup compared the named globals, which name every function (DIFF-12.3), so
        // the second progs has one of this name.
        let Some(&our_index) = our_world.program.functions.get(name) else {
            continue;
        };
        for round in 0..rounds {
            let mut rng = Rng::new(name, round);
            let context = drawer.draw(&mut rng, function, round);
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
}

impl Pair {
    fn new(theirs: &Progs<'_>, ours: &Progs<'_>) -> Self {
        let their_names = theirs.functions().iter().map(|f| f.s_name);
        Self {
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
        state_difference(their_world, our_world)
    }
}

/// Whether two machines hold the same bits in every global and in every edict's fields, with
/// the same edicts free.
fn same_slots(a: &Machine, b: &Machine) -> bool {
    let (ea, eb) = (a.edicts(), b.edicts());
    a.globals() == b.globals()
        && ea.num_edicts() == eb.num_edicts()
        && (0..ea.num_edicts())
            .all(|n| ea.is_free(n) == eb.is_free(n) && ea.fields(n) == eb.fields(n))
}

/// The first named global or field whose value differs between the two worlds (DIFF-12.3).
fn state_difference(a: &World<'_, '_, '_>, b: &World<'_, '_, '_>) -> Option<Difference> {
    let (pa, pb) = (&a.program, &b.program);
    let same_globals = pa
        .globals
        .iter()
        .map(|g| &g.0)
        .eq(pb.globals.iter().map(|g| &g.0));
    if !same_globals || !pa.fields.keys().eq(pb.fields.keys()) {
        return Some(Difference::Definitions);
    }
    for ((name, their_type, their_ofs), (_, our_type, our_ofs)) in
        pa.globals.iter().zip(&pb.globals)
    {
        let theirs = a.global(*their_type, *their_ofs);
        let ours = b.global(*our_type, *our_ofs);
        if theirs != ours {
            return Some(Difference::Global(name.clone(), theirs, ours));
        }
    }
    let (ea, eb) = (a.host.machine.edicts(), b.host.machine.edicts());
    for n in 0..ea.num_edicts().max(eb.num_edicts()) {
        let free = ea.is_free(n);
        if free != eb.is_free(n) {
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

/// A progs' named definitions and functions (DIFF-12.3).
struct Program<'a, 'p> {
    progs: &'a Progs<'p>,
    /// The named globals, in definition order: name, type and slot.
    globals: Vec<(Vec<u8>, DefType, usize)>,
    /// The named fields, by name: the type and slot of the first definition of each.
    fields: BTreeMap<Vec<u8>, (DefType, usize)>,
    /// The QuakeC functions, by name: the index of the first of each name.
    functions: BTreeMap<Vec<u8>, i32>,
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
        for (index, function) in progs.functions().iter().enumerate() {
            if let (Some(name), Ok(index)) = (progs.string(function.s_name), i32::try_from(index))
                && function.first_statement > 0
            {
                functions.entry(name.to_vec()).or_insert(index);
            }
        }
        Self {
            progs,
            globals,
            fields,
            functions,
        }
    }
}

/// How one call went in one progs (DIFF-12.3, DIFF-12.4).
#[derive(Default)]
struct Outcome {
    /// The run error's message, or `None` when the call returned.
    error: Option<String>,
    /// The statements it executed, in order.
    executed: Vec<Executed>,
}

/// One progs' world after the setup, kept to restore before each call (DIFF-12.1).
struct World<'a, 'p, 's> {
    program: Program<'a, 'p>,
    host: Host<'s>,
    machine: Machine,
    builtins: MockBuiltins<MockEngine>,
    /// The statements the setup executed (DIFF-12.5).
    setup_executed: BTreeSet<i32>,
}

impl<'a, 'p, 's> World<'a, 'p, 's> {
    /// Loads `progs` and sets up the world of `scenario` and `entities`: the map spawned and
    /// the clients entered (DIFF-12.1).
    fn new(
        progs: &'a Progs<'p>,
        scenario: &'s Scenario,
        entities: &[u8],
    ) -> Result<Self, CallsError> {
        let mut host = Host::new(scenario, progs).map_err(CallsError::Refused)?;
        host.start_map(progs, entities)
            .map_err(|e| CallsError::Setup(format!("{e:?}")))?;
        for slot in 0..scenario.clients {
            host.client_enters(progs, slot)
                .map_err(|e| CallsError::Setup(e.message()))?;
        }
        let setup_executed = host
            .trace
            .records()
            .iter()
            .filter_map(|record| match record {
                qc_trace::Record::Step(step) => Some(step.statement),
                qc_trace::Record::Run { .. }
                | qc_trace::Record::Exit { .. }
                | qc_trace::Record::Error { .. }
                | qc_trace::Record::String { .. } => None,
            })
            .collect();
        Ok(Self {
            machine: host.machine.clone(),
            builtins: host.builtins.clone(),
            program: Program::new(progs),
            host,
            setup_executed,
        })
    }

    /// The edicts in use after the setup, the world first.
    fn in_use(&self) -> Vec<usize> {
        let edicts = self.machine.edicts();
        (0..edicts.num_edicts())
            .filter(|&n| edicts.is_free(n) == Some(false))
            .collect()
    }

    /// Restores the world, applies `context` and calls `function` (DIFF-12.2).
    fn call(&mut self, function: i32, context: &Context) -> Outcome {
        self.host.machine.clone_from(&self.machine);
        self.host.builtins.clone_from(&self.builtins);
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
                .unwrap_or(Slot(0))
        })
    }

    /// The value of edict `edict`'s field of type `def_type` at slot `ofs` (DIFF-12.3).
    fn field(&self, edict: usize, def_type: DefType, ofs: usize) -> Value {
        let edicts = self.host.machine.edicts();
        self.value(def_type, ofs, |n| edicts.field(edict, n).unwrap_or(Slot(0)))
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
                slot(ofs).0,
                slot(ofs.saturating_add(1)).0,
                slot(ofs.saturating_add(2)).0,
            ]),
            DefType::Known(
                Etype::Void | Etype::Float | Etype::Entity | Etype::Field | Etype::Pointer,
            )
            | DefType::Unknown(_) => Value::Bits(slot(ofs).0),
        }
    }
}

#[cfg(test)]
mod tests;
