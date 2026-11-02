// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The context each scripted call is made in (`docs/spec/differential.md` DIFF-12.2): `self`,
//! `other`, the parameters, and values for some fields and variables, drawn from the first
//! progs' world by a generator seeded with the function's name and the round, then applied to
//! each progs by name.
//!
//! Implements DIFF-12.2.

use std::collections::{BTreeMap, BTreeSet};

use qc_progs::opcode::Opcode;
use qc_progs::{Function, Progs, Statement};
use qc_types::{DefType, Etype, Slot};
use qc_vm::ServerState;

use super::{OFS_PARM0, Program, World};
use crate::host::Host;

/// `MAX_PARMS` (VM-14.1): the parameters a call passes in the parameter slots.
const MAX_PARMS: usize = 8;
/// Texts a string is drawn from besides the progs' own (DIFF-12.2).
const TEXTS: [&[u8]; 3] = [b"", b"player", b"1"];

/// A value drawn for a field, a variable or a parameter, applied to each progs the same way
/// (DIFF-12.2).
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Draw {
    Float(f32),
    Vector([f32; 3]),
    /// An edict, by number.
    Entity(usize),
    /// A new engine string holding the text.
    Text(Vec<u8>),
}

impl Draw {
    /// The slots this value writes, making the engine string a text needs.
    fn slots(&self, host: &mut Host<'_>) -> Vec<Slot> {
        match self {
            Self::Float(value) => vec![Slot::from_f32(*value)],
            Self::Vector(values) => values.iter().map(|v| Slot::from_f32(*v)).collect(),
            Self::Entity(edict) => vec![Slot::from_i32(host.entity_value(*edict))],
            Self::Text(text) => vec![Slot::from_i32(host.make_string(text.clone()))],
        }
    }
}

/// The input of one call (DIFF-12.2).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Context {
    /// The edict `self` is set to.
    pub(super) self_edict: usize,
    /// The edict `other` is set to.
    pub(super) other_edict: usize,
    /// Field writes, applied in order: edict, field name, value.
    pub(super) fields: Vec<(usize, Vec<u8>, Draw)>,
    /// Variable writes, applied in order: name, value.
    pub(super) globals: Vec<(Vec<u8>, Draw)>,
    /// The parameters, first to last.
    pub(super) parms: Vec<Draw>,
    /// The server state the call runs in (VM-12.2, BUILTIN-10.1).
    pub(super) server: ServerState,
}

impl Context {
    /// Writes this context into `host`'s world, finding each field and global by its name in
    /// `program`; a name `program` lacks is skipped.
    pub(super) fn apply(&self, host: &mut Host<'_>, program: &Program<'_, '_>) {
        host.set_self(self.self_edict);
        host.set_other(self.other_edict);
        for (edict, name, draw) in &self.fields {
            if let Some(&(_, slot)) = program.fields.get(name) {
                for (offset, bits) in draw.slots(host).into_iter().enumerate() {
                    host.set_field(*edict, slot.saturating_add(offset), bits);
                }
            }
        }
        for (name, draw) in &self.globals {
            if let Some(&ofs) = program.global_slots.get(name) {
                write_globals(host, ofs, draw);
            }
        }
        for (n, draw) in self.parms.iter().enumerate() {
            write_globals(host, n.saturating_mul(3).saturating_add(OFS_PARM0), draw);
        }
        // DIFF-12.12: the slots past the drawn parameters are cleared, so neither call reads
        // what earlier calls of its progs left there.
        let past = self.parms.len().saturating_mul(3).saturating_add(OFS_PARM0);
        let end = MAX_PARMS.saturating_mul(3).saturating_add(OFS_PARM0);
        for slot in past..end {
            if let Ok(slot) = i32::try_from(slot) {
                host.set_global_slot(slot, Slot::ZERO);
            }
        }
    }
}

/// Writes `draw` to the globals from slot `ofs`.
fn write_globals(host: &mut Host<'_>, ofs: usize, draw: &Draw) {
    for (offset, bits) in draw.slots(host).into_iter().enumerate() {
        if let Ok(slot) = i32::try_from(ofs.saturating_add(offset)) {
            host.set_global_slot(slot, bits);
        }
    }
}

/// What a function's statements name (DIFF-12.2, DIFF-12.21): the constants a value is drawn
/// from, float constants by their bits and texts, and the fields and variables drawn for the
/// call.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Pools {
    pub(super) numbers: Vec<u32>,
    pub(super) texts: Vec<Vec<u8>>,
    /// The names of the fields whose field globals the statements name (DIFF-12.21).
    pub(super) fields: Vec<Vec<u8>>,
    /// The names of the float and string variables the statements name: globals whose value
    /// in the progs is 0 (DIFF-12.21).
    pub(super) variables: Vec<Vec<u8>>,
    /// The values the statements compare each named field, variable or parameter with
    /// (DIFF-12.22), by name.
    pub(super) compared: ComparedByName,
    /// The pairs of names the statements compare with each other (DIFF-12.22), the second
    /// drawn to the first's value.
    pub(super) pairs: Pairs,
}

/// The compared values of a function's statements, by name (DIFF-12.22).
type ComparedByName = BTreeMap<Vec<u8>, Compared>;
/// The pairs of names a function's statements compare with each other (DIFF-12.22).
type Pairs = Vec<(Vec<u8>, Vec<u8>)>;

/// The values a function's statements compare one name with (DIFF-12.22).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Compared {
    /// Float constants, by their bits; a truth test adds 0 and 1, a bit test its mask and 0.
    pub(super) numbers: BTreeSet<u32>,
    /// String constants; a truth test adds `""`.
    pub(super) texts: BTreeSet<Vec<u8>>,
    /// Whether an entity is tested for being `world`.
    pub(super) world: bool,
}

impl Compared {
    /// A value drawn from these for a value of type `def_type`, or `None` when there is none
    /// of that type.
    fn draw(&self, rng: &mut Rng, def_type: DefType) -> Option<Draw> {
        match drawable(def_type)? {
            Etype::Float => {
                let numbers: Vec<u32> = self.numbers.iter().copied().collect();
                rng.pick(&numbers)
                    .map(|bits| Draw::Float(f32::from_bits(*bits)))
            }
            Etype::String => {
                let texts: Vec<&Vec<u8>> = self.texts.iter().collect();
                rng.pick(&texts).map(|text| Draw::Text((*text).clone()))
            }
            Etype::Entity => self.world.then_some(Draw::Entity(0)),
            Etype::Vector | Etype::Void | Etype::Field | Etype::Function | Etype::Pointer => None,
        }
    }
}

/// What an operand of a statement is, for the compared values (DIFF-12.22).
enum Operand {
    /// A field a `LOAD` read into the slot, a variable, or a parameter, by name.
    Named(Vec<u8>),
    /// A global whose value is 0 and which no `IMMEDIATE` names, `FALSE` or an uninitialised
    /// variable, by name: compared with as a name, and an ordering against it draws around 0.
    Zero(Vec<u8>),
    /// A float constant, by its bits.
    Number(u32),
    /// A string constant.
    Text(Vec<u8>),
    /// Anything else: a temporary, a vector, a function.
    Other,
}

/// Records that `name` is compared with `other`: a constant among its values, a name a pair.
fn record(compared: &mut ComparedByName, pairs: &mut Pairs, name: Vec<u8>, other: &Operand) {
    let entry = compared.entry(name.clone()).or_default();
    match other {
        Operand::Number(bits) => {
            entry.numbers.insert(*bits);
        }
        Operand::Text(text) => {
            entry.texts.insert(text.clone());
        }
        Operand::Named(second) | Operand::Zero(second) => {
            if *second != name {
                pairs.push((name, second.clone()));
            }
        }
        Operand::Other => {}
    }
}

/// The compared values and pairs of the statements `statements` (DIFF-12.22): a field read by
/// a `LOAD` into a slot, a float or string variable, or a parameter in `parms`, compared with
/// a constant by `EQ`, `NE`, `LT`, `GT`, `LE` or `GE`, masked by `BITAND` or `BITOR`, or
/// tested by `IF`, `IFNOT` or `NOT`; a `STORE` carries the name to the slot stored to.
fn mine_compared(
    progs: &Progs<'_>,
    (types, names): (&BTreeMap<u32, DefType>, &BTreeMap<u32, Vec<u8>>),
    parms: std::ops::Range<i32>,
    statements: impl Iterator<Item = Statement>,
) -> (ComparedByName, Pairs) {
    let mut compared = ComparedByName::new();
    let mut pairs = Pairs::new();
    let mut loaded: BTreeMap<i32, Vec<u8>> = BTreeMap::new();
    let operand = |loaded: &BTreeMap<i32, Vec<u8>>, slot: i32| -> Operand {
        if let Some(name) = loaded.get(&slot) {
            return Operand::Named(name.clone());
        }
        let (Ok(ofs), Ok(index)) = (u32::try_from(slot), usize::try_from(slot)) else {
            return Operand::Other;
        };
        let Some(value) = progs.globals().get(index) else {
            return Operand::Other;
        };
        let name = names.get(&ofs);
        let immediate = name.is_some_and(|name| name == b"IMMEDIATE");
        match types.get(&ofs) {
            Some(DefType::Known(Etype::Float)) => {
                if parms.contains(&slot) {
                    name.cloned().map_or(Operand::Other, Operand::Named)
                } else if value.bits() == 0 && !immediate && ofs != 0 {
                    name.cloned().map_or(Operand::Other, Operand::Zero)
                } else {
                    Operand::Number(value.bits())
                }
            }
            Some(DefType::Known(Etype::String)) => {
                if parms.contains(&slot) || (value.bits() == 0 && !immediate && ofs != 0) {
                    name.cloned().map_or(Operand::Other, Operand::Named)
                } else {
                    progs
                        .string(value.as_i32())
                        .map_or(Operand::Other, |text| Operand::Text(text.to_vec()))
                }
            }
            Some(DefType::Known(Etype::Entity)) if parms.contains(&slot) => {
                name.cloned().map_or(Operand::Other, Operand::Named)
            }
            _ => Operand::Other,
        }
    };
    for statement in statements {
        let Some(op) = Opcode::from_number(statement.op) else {
            continue;
        };
        let (a, b, c) = (statement.a, statement.b, statement.c);
        match op {
            Opcode::LoadF
            | Opcode::LoadV
            | Opcode::LoadS
            | Opcode::LoadEnt
            | Opcode::LoadFld
            | Opcode::LoadFnc => {
                let field = u32::try_from(b)
                    .ok()
                    .filter(|ofs| types.get(ofs) == Some(&DefType::Known(Etype::Field)))
                    .and_then(|ofs| names.get(&ofs).cloned());
                match field {
                    Some(name) => {
                        loaded.insert(c, name);
                    }
                    None => {
                        loaded.remove(&c);
                    }
                }
            }
            Opcode::StoreF | Opcode::StoreS | Opcode::StoreEnt => match operand(&loaded, a) {
                Operand::Named(name) | Operand::Zero(name) => {
                    loaded.insert(b, name);
                }
                Operand::Number(_) | Operand::Text(_) | Operand::Other => {
                    loaded.remove(&b);
                }
            },
            Opcode::EqF
            | Opcode::EqS
            | Opcode::NeF
            | Opcode::NeS
            | Opcode::Le
            | Opcode::Ge
            | Opcode::Lt
            | Opcode::Gt
            | Opcode::BitAnd
            | Opcode::BitOr => {
                let (first, second) = (operand(&loaded, a), operand(&loaded, b));
                let masked = matches!(op, Opcode::BitAnd | Opcode::BitOr);
                let ordered = matches!(op, Opcode::Le | Opcode::Ge | Opcode::Lt | Opcode::Gt);
                for (name, other) in [(&first, &second), (&second, &first)] {
                    let (Operand::Named(name) | Operand::Zero(name)) = name else {
                        continue;
                    };
                    record(&mut compared, &mut pairs, name.clone(), other);
                    if masked {
                        record(&mut compared, &mut pairs, name.clone(), &Operand::Number(0));
                    }
                    // An ordering with a constant is satisfied strictly on either side of it,
                    // so the name takes one less and one more too; against a zero global such
                    // as `FALSE`, it takes 0 and its neighbours.
                    let around: &[f32] = match other {
                        Operand::Number(bits) if ordered => {
                            let middle = f32::from_bits(*bits);
                            &[middle - 1.0, middle + 1.0]
                        }
                        Operand::Zero(_) if ordered => &[-1.0, 0.0, 1.0],
                        Operand::Number(_)
                        | Operand::Named(_)
                        | Operand::Zero(_)
                        | Operand::Text(_)
                        | Operand::Other => &[],
                    };
                    for value in around {
                        record(
                            &mut compared,
                            &mut pairs,
                            name.clone(),
                            &Operand::Number(value.to_bits()),
                        );
                    }
                }
                loaded.remove(&c);
            }
            Opcode::If | Opcode::IfNot | Opcode::NotF | Opcode::NotS | Opcode::NotEnt => {
                if let Operand::Named(name) | Operand::Zero(name) = operand(&loaded, a) {
                    let entry = compared.entry(name).or_default();
                    entry.numbers.insert(0);
                    entry.numbers.insert(1.0_f32.to_bits());
                    entry.texts.insert(Vec::new());
                    entry.world = true;
                }
                if matches!(op, Opcode::NotF | Opcode::NotS | Opcode::NotEnt) {
                    loaded.remove(&c);
                }
            }
            Opcode::EqE | Opcode::NeE => {
                if let Operand::Named(name) | Operand::Zero(name) = operand(&loaded, a) {
                    compared.entry(name).or_default().world = true;
                }
                if let Operand::Named(name) | Operand::Zero(name) = operand(&loaded, b) {
                    compared.entry(name).or_default().world = true;
                }
                loaded.remove(&c);
            }
            _ => {
                loaded.remove(&c);
            }
        }
    }
    (compared, pairs)
}

impl Pools {
    /// What the global slots `slots` name: the constants, each slot with a value other than 0
    /// in the progs, read through the type of the first definition at it, a float or a
    /// string; the fields, each slot a field definition names; and the variables, each slot
    /// outside 0 with the value 0 that a float or string definition names.
    pub(super) fn of(
        progs: &Progs<'_>,
        (types, names): (&BTreeMap<u32, DefType>, &BTreeMap<u32, Vec<u8>>),
        slots: impl Iterator<Item = i32>,
    ) -> Self {
        let mut numbers = BTreeSet::new();
        let mut texts = BTreeSet::new();
        let mut fields = BTreeSet::new();
        let mut variables = BTreeSet::new();
        for slot in slots {
            let (Ok(ofs), Ok(index)) = (u32::try_from(slot), usize::try_from(slot)) else {
                continue;
            };
            let Some(value) = progs.globals().get(index) else {
                continue;
            };
            let name = || names.get(&ofs).cloned();
            match (types.get(&ofs), value.bits()) {
                (Some(DefType::Known(Etype::Field)), _) => {
                    fields.extend(name());
                }
                (Some(DefType::Known(Etype::Float | Etype::String)), 0) => {
                    if ofs != 0 {
                        variables.extend(name());
                    }
                }
                (Some(DefType::Known(Etype::Float)), bits) => {
                    numbers.insert(bits);
                }
                (Some(DefType::Known(Etype::String)), _) => {
                    if let Some(text) = progs.string(value.as_i32()) {
                        texts.insert(text.to_vec());
                    }
                }
                (Some(_) | None, _) => {}
            }
        }
        Self {
            numbers: numbers.into_iter().collect(),
            texts: texts.into_iter().collect(),
            fields: fields.into_iter().collect(),
            variables: variables.into_iter().collect(),
            compared: BTreeMap::new(),
            pairs: Vec::new(),
        }
    }
}

/// What a context is drawn from: the first progs' world after the setup (DIFF-12.2).
pub(super) struct Drawer<'a, 'p> {
    progs: &'a Progs<'p>,
    /// The constants of the whole progs, and [`TEXTS`].
    all: Pools,
    /// The type of the first global definition at each slot.
    types: BTreeMap<u32, DefType>,
    /// The name of the first global definition at each slot.
    names: BTreeMap<u32, Vec<u8>>,
    /// Each QuakeC function's first statement and the statement after its last (DIFF-11.3).
    bodies: BTreeMap<i32, i32>,
    /// The edicts in use, the world first.
    edicts: Vec<usize>,
    /// The scenario's client slots; client `n` is edict `n`.
    clients: u64,
    /// The named globals a value may be drawn for: float and string globals whose value in
    /// the progs is 0, so variables rather than constants, outside slot 0 and in a slot no
    /// other float or string definition names, as FTEQCC's merged constants share one
    /// (DIFF-12.2, DIFF-12.11).
    variables: BTreeMap<Vec<u8>, DefType>,
    /// The named fields of a type a value is drawn for.
    fields: BTreeMap<Vec<u8>, DefType>,
    /// For each QuakeC function by its first statement, the edicts in use whose function
    /// fields (`think`, `touch`, `use` and the rest) hold it (DIFF-12.15).
    owners: BTreeMap<i32, Vec<usize>>,
}

/// DIFF-12.15: each QuakeC function, by its first statement, with the edicts in use whose
/// named fields of function type hold it, in edict order.
pub(super) fn owners(world: &World<'_, '_, '_>, progs: &Progs<'_>) -> BTreeMap<i32, Vec<usize>> {
    let functions: Vec<i32> = progs
        .functions()
        .iter()
        .map(|f| f.first_statement)
        .collect();
    let slots: Vec<usize> = world
        .program
        .fields
        .values()
        .filter(|(def_type, _)| *def_type == DefType::Known(Etype::Function))
        .map(|(_, slot)| *slot)
        .collect();
    let edicts = world.machine.edicts();
    let mut owners: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    for &edict in &world.in_use() {
        for &slot in &slots {
            let Some(value) = edicts.field(qc_edict::EdictNumber::new(edict), slot) else {
                continue;
            };
            let Some(&first) = usize::try_from(value.bits())
                .ok()
                .and_then(|index| functions.get(index))
            else {
                continue;
            };
            if first > 0 {
                let list = owners.entry(first).or_default();
                if list.last() != Some(&edict) {
                    list.push(edict);
                }
            }
        }
    }
    owners
}

impl<'a, 'p> Drawer<'a, 'p> {
    pub(super) fn new(world: &World<'a, 'p, '_>) -> Self {
        let progs = world.program.progs;
        let scalar = |def_type| matches!(drawable(def_type), Some(Etype::Float | Etype::String));
        let mut named_by: BTreeMap<u32, usize> = BTreeMap::new();
        for def in progs.global_defs().iter() {
            if scalar(def.def_type()) {
                let count = named_by.entry(def.ofs).or_default();
                *count = count.saturating_add(1);
            }
        }
        let variables = world
            .program
            .globals
            .iter()
            .filter(|(_, def_type, ofs)| {
                let alone = u32::try_from(*ofs)
                    .is_ok_and(|ofs| ofs != 0 && named_by.get(&ofs).copied() == Some(1));
                scalar(*def_type)
                    && alone
                    && progs
                        .globals()
                        .get(*ofs)
                        .is_some_and(|slot| slot.bits() == 0)
            })
            .map(|(name, def_type, _)| (name.clone(), *def_type))
            .collect();
        let fields = world
            .program
            .fields
            .iter()
            .filter(|(_, (def_type, _))| drawable(*def_type).is_some())
            .map(|(name, (def_type, _))| (name.clone(), *def_type))
            .collect();
        let mut types = BTreeMap::new();
        let mut names = BTreeMap::new();
        for def in progs.global_defs().iter() {
            types.entry(def.ofs).or_insert(def.def_type());
            if let Some(name) = progs.string(def.s_name) {
                names.entry(def.ofs).or_insert_with(|| name.to_vec());
            }
        }
        let owners = owners(world, progs);
        let slots = progs
            .global_defs()
            .iter()
            .filter_map(|def| i32::try_from(def.ofs).ok());
        let mut all = Pools::of(progs, (&types, &names), slots);
        all.texts.extend(TEXTS.iter().map(|text| text.to_vec()));
        Self {
            progs,
            all,
            types,
            names,
            bodies: bodies(progs),
            edicts: world.in_use(),
            owners,
            clients: u64::from(world.host.scenario.clients),
            variables,
            fields,
        }
    }

    /// The statement after the last of the body that starts at `first` (DIFF-11.3).
    pub(super) fn body_end(&self, first: i32) -> i32 {
        self.bodies.get(&first).copied().unwrap_or(first)
    }

    /// The constants the statements of `function` name as operands (DIFF-12.2).
    pub(super) fn pools(&self, function: Function) -> Pools {
        let first = function.first_statement;
        let end = self.bodies.get(&first).copied().unwrap_or(first);
        let statements = self.progs.statements();
        let body = || (first..end).filter_map(|n| statements.get(usize::try_from(n).ok()?));
        let slots = body().flat_map(|s| [s.a, s.b, s.c]);
        let mut pools = Pools::of(self.progs, (&self.types, &self.names), slots);
        let parm_words: i32 = function
            .parm_size
            .iter()
            .take(
                usize::try_from(function.numparms)
                    .unwrap_or(0)
                    .min(MAX_PARMS),
            )
            .map(|size| i32::from(*size))
            .sum();
        let parms = function.parm_start..function.parm_start.saturating_add(parm_words);
        let (compared, pairs) =
            mine_compared(self.progs, (&self.types, &self.names), parms, body());
        pools.compared = compared;
        pools.pairs = pairs;
        pools
    }

    /// An edict in use.
    fn edict(&self, rng: &mut Rng) -> usize {
        rng.pick(&self.edicts).copied().unwrap_or(0)
    }

    /// A client's edict one time in `one_in`, else an edict in use.
    fn edict_or_client(&self, rng: &mut Rng, one_in: u64) -> usize {
        if self.clients > 0 && rng.below(one_in) == 0 {
            return usize::try_from(rng.below(self.clients))
                .unwrap_or(0)
                .saturating_add(1);
        }
        self.edict(rng)
    }

    /// A value of type `def_type`, or `None` for a type no value is drawn for; `own` holds
    /// the called function's constants.
    fn value(&self, rng: &mut Rng, own: &Pools, def_type: DefType) -> Option<Draw> {
        Some(match drawable(def_type)? {
            Etype::Vector => Draw::Vector([
                self.float(rng, own),
                self.float(rng, own),
                self.float(rng, own),
            ]),
            Etype::Entity => Draw::Entity(self.edict_or_client(rng, 3)),
            Etype::String => {
                let pool = if rng.below(2) == 0 {
                    &own.texts
                } else {
                    &self.all.texts
                };
                Draw::Text(rng.pick(pool).cloned().unwrap_or_default())
            }
            Etype::Float | Etype::Void | Etype::Field | Etype::Function | Etype::Pointer => {
                Draw::Float(self.float(rng, own))
            }
        })
    }

    /// A value for the field, variable or parameter `name` of type `def_type`: one time in
    /// two a value the called function's statements compare it with, when there is one of
    /// its type (DIFF-12.22), else as [`Drawer::value`] draws it.
    fn named_value(
        &self,
        rng: &mut Rng,
        own: &Pools,
        name: &[u8],
        def_type: DefType,
    ) -> Option<Draw> {
        if rng.below(2) == 0
            && let Some(compared) = own.compared.get(name)
            && let Some(draw) = compared.draw(rng, def_type)
        {
            return Some(draw);
        }
        self.value(rng, own, def_type)
    }

    /// A float: a third of the time one of the called function's constants, a third one of the
    /// progs', else one of [`Rng::float`]'s.
    fn float(&self, rng: &mut Rng, own: &Pools) -> f32 {
        let pool = match rng.below(3) {
            0 => &own.numbers,
            1 => &self.all.numbers,
            _ => return rng.float(),
        };
        rng.pick(pool)
            .map_or_else(|| rng.float(), |bits| f32::from_bits(*bits))
    }

    /// The context for round `round` of the call to `function` (DIFF-12.2). Round 0 of each
    /// four leaves the fields and variables as the setup left them; rounds 1, 2 and 3 draw
    /// about 1 in 16, 1 in 4 and 1 in 2 of the fields of `self`, `other`, each entity
    /// parameter and, one time in four, the world, and a quarter as many of the variables.
    /// Every eighth round runs with the server loading.
    pub(super) fn draw(
        &self,
        rng: &mut Rng,
        (function, own): (Function, &Pools),
        round: u32,
    ) -> Context {
        // DIFF-12.15: one time in two `self` is an edict that holds the function in a field,
        // as the engine calls a `think` or `touch` on its own edict.
        let self_edict = match self.owners.get(&function.first_statement) {
            Some(owners) if rng.below(2) == 0 => rng.pick(owners).copied().unwrap_or(0),
            _ => self.edict_or_client(rng, 4),
        };
        let other_edict = self.edict_or_client(rng, 2);
        let share = match round % 4 {
            0 => 0,
            1 => 4,
            2 => 16,
            _ => 32,
        };
        let parms = self.parms(rng, own, function);
        let mut edicts = vec![self_edict, other_edict];
        for parm in &parms {
            if let Draw::Entity(edict) = parm {
                edicts.push(*edict);
            }
        }
        if rng.below(4) == 0 {
            edicts.push(0);
        }
        edicts.sort_unstable();
        edicts.dedup();
        let mut fields = Vec::new();
        for edict in edicts.iter().copied() {
            for (field, def_type) in &self.fields {
                if rng.below(64) < share
                    && let Some(draw) = self.value(rng, own, *def_type)
                {
                    fields.push((edict, field.clone(), draw));
                }
            }
        }
        let mut globals = Vec::new();
        for (name, def_type) in &self.variables {
            if rng.below(256) < share
                && let Some(draw) = self.value(rng, own, *def_type)
            {
                globals.push((name.clone(), draw));
            }
        }
        // DIFF-12.21: the fields and variables the function's statements name are drawn one
        // time in two each, after the share's draws, so a call reaches the branches its own
        // conditions guard.
        if share > 0 {
            for edict in &edicts {
                for name in &own.fields {
                    if rng.below(2) == 0
                        && let Some(def_type) = self.fields.get(name)
                        && let Some(draw) = self.named_value(rng, own, name, *def_type)
                    {
                        fields.push((*edict, name.clone(), draw));
                    }
                }
            }
            for name in &own.variables {
                if rng.below(2) == 0
                    && let Some(def_type) = self.variables.get(name)
                    && let Some(draw) = self.named_value(rng, own, name, *def_type)
                {
                    globals.push((name.clone(), draw));
                }
            }
            // DIFF-12.22: a name compared with another takes its value one time in two.
            for (first, second) in &own.pairs {
                if rng.below(2) != 0 {
                    continue;
                }
                let found: Vec<(usize, Draw)> = fields
                    .iter()
                    .filter(|(_, name, _)| name == first)
                    .map(|(edict, _, draw)| (*edict, draw.clone()))
                    .collect();
                for (edict, draw) in found {
                    if self.fields.contains_key(second) {
                        fields.push((edict, second.clone(), draw));
                    }
                }
                if let Some((_, draw)) = globals.iter().rev().find(|(name, _)| name == first)
                    && self.variables.contains_key(second)
                {
                    globals.push((second.clone(), draw.clone()));
                }
            }
        }
        let server = if round % 8 == 7 {
            ServerState::Loading
        } else {
            ServerState::Active
        };
        Context {
            self_edict,
            other_edict,
            fields,
            globals,
            parms,
            server,
        }
    }

    /// The parameters of `function`, each drawn by the type of its definition (DIFF-12.2).
    fn parms(&self, rng: &mut Rng, own: &Pools, function: Function) -> Vec<Draw> {
        let count = usize::try_from(function.numparms)
            .unwrap_or(0)
            .min(MAX_PARMS);
        let mut parms = Vec::new();
        let mut ofs = u32::try_from(function.parm_start).unwrap_or(0);
        for size in function.parm_size.iter().take(count) {
            let def_type = match size {
                3 => DefType::Known(Etype::Vector),
                _ => self
                    .types
                    .get(&ofs)
                    .copied()
                    .unwrap_or(DefType::Known(Etype::Float)),
            };
            let draw = match self.names.get(&ofs) {
                Some(name) => self.named_value(rng, own, name, def_type),
                None => self.value(rng, own, def_type),
            };
            parms.push(draw.unwrap_or(Draw::Float(0.0)));
            ofs = ofs.saturating_add(u32::from(*size));
        }
        parms
    }
}

/// Each QuakeC function's first statement, with the statement after its last: the next larger
/// first statement, or `numstatements` (DIFF-11.3).
fn bodies(progs: &Progs<'_>) -> BTreeMap<i32, i32> {
    let mut starts: Vec<i32> = progs
        .functions()
        .iter()
        .map(|f| f.first_statement)
        .filter(|&first| first > 0)
        .collect();
    starts.sort_unstable();
    starts.dedup();
    let end = i32::try_from(progs.statements().len()).unwrap_or(i32::MAX);
    starts
        .iter()
        .zip(starts.iter().skip(1).copied().chain([end]))
        .map(|(first, next)| (*first, next))
        .collect()
}

/// The type a value is drawn for, or `None` for one left as the setup left it: functions,
/// fields and pointers (DIFF-12.2).
const fn drawable(def_type: DefType) -> Option<Etype> {
    match def_type {
        DefType::Known(etype @ (Etype::Float | Etype::Vector | Etype::Entity | Etype::String)) => {
            Some(etype)
        }
        DefType::Known(Etype::Void | Etype::Field | Etype::Function | Etype::Pointer)
        | DefType::Unknown(_) => None,
    }
}

/// SplitMix64, seeded with an FNV-1a hash of a function's name and the round (DIFF-12.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(name: &[u8], round: u32) -> Self {
        let mut seed: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in name.iter().chain(&round.to_le_bytes()) {
            seed = (seed ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
        Self(seed)
    }

    pub(crate) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number below `bound`; 0 when `bound` is 0.
    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        self.next().checked_rem(bound).unwrap_or(0)
    }

    /// An element of `items`; `None` when it is empty.
    pub(crate) fn pick<'t, T>(&mut self, items: &'t [T]) -> Option<&'t T> {
        let count = u64::try_from(items.len()).ok()?;
        items.get(usize::try_from(self.below(count)).ok()?)
    }

    /// A float from the pool of DIFF-12.2: a whole number from -1 to 10 (3 in 10), a product
    /// of two powers of two up to 2^23 (2 in 10), a 24-bit flag mask (1 in 10), a whole
    /// number below 1000 (2 in 10), a fraction in thousandths below 1 (1 in 10), or a negative
    /// whole number above -1000 (1 in 10).
    pub(crate) fn float(&mut self) -> f32 {
        let whole = |n: u64| f32::from(u16::try_from(n).unwrap_or(0));
        match self.below(10) {
            0..=2 => whole(self.below(12)) - 1.0,
            3 | 4 => whole(1_u64 << self.below(16)) * whole(1_u64 << self.below(9)),
            5 => {
                let mask = self.next() & 0x00FF_FFFF;
                whole(mask >> 8) * 256.0 + whole(mask & 0xFF)
            }
            6 | 7 => whole(self.below(1000)),
            8 => whole(self.below(1000)) / 1000.0,
            _ => -whole(self.below(1000)),
        }
    }
}
