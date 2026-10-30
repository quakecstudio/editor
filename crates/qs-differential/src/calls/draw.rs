// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The context each scripted call is made in (`docs/spec/differential.md` DIFF-12.2): `self`,
//! `other`, the parameters, and values for some fields and variables, drawn from the first
//! progs' world by a generator seeded with the function's name and the round, then applied to
//! each progs by name.
//!
//! Implements DIFF-12.2.

use std::collections::{BTreeMap, BTreeSet};

use qc_progs::{Function, Progs};
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

/// The constants a value is drawn from (DIFF-12.2): float constants by their bits, and texts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Pools {
    pub(super) numbers: Vec<u32>,
    pub(super) texts: Vec<Vec<u8>>,
}

impl Pools {
    /// The constants among the global slots `slots`: each slot with a value other than 0 in the
    /// progs, read through the type of the first definition at it, a float or a string.
    pub(super) fn of(
        progs: &Progs<'_>,
        types: &BTreeMap<u32, DefType>,
        slots: impl Iterator<Item = i32>,
    ) -> Self {
        let mut numbers = BTreeSet::new();
        let mut texts = BTreeSet::new();
        for slot in slots {
            let (Ok(ofs), Ok(index)) = (u32::try_from(slot), usize::try_from(slot)) else {
                continue;
            };
            let Some(value) = progs.globals().get(index).filter(|value| value.bits() != 0) else {
                continue;
            };
            match types.get(&ofs) {
                Some(DefType::Known(Etype::Float)) => {
                    numbers.insert(value.bits());
                }
                Some(DefType::Known(Etype::String)) => {
                    if let Some(text) = progs.string(value.as_i32()) {
                        texts.insert(text.to_vec());
                    }
                }
                Some(_) | None => {}
            }
        }
        Self {
            numbers: numbers.into_iter().collect(),
            texts: texts.into_iter().collect(),
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
    variables: Vec<(Vec<u8>, DefType)>,
    /// The named fields of a type a value is drawn for.
    fields: Vec<(Vec<u8>, DefType)>,
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
        for def in progs.global_defs().iter() {
            types.entry(def.ofs).or_insert(def.def_type());
        }
        let owners = owners(world, progs);
        let slots = progs
            .global_defs()
            .iter()
            .filter_map(|def| i32::try_from(def.ofs).ok());
        let mut all = Pools::of(progs, &types, slots);
        all.texts.extend(TEXTS.iter().map(|text| text.to_vec()));
        Self {
            progs,
            all,
            types,
            bodies: bodies(progs),
            edicts: world.in_use(),
            owners,
            clients: u64::from(world.host.scenario.clients),
            variables,
            fields,
        }
    }

    /// The constants the statements of `function` name as operands (DIFF-12.2).
    pub(super) fn pools(&self, function: Function) -> Pools {
        let first = function.first_statement;
        let end = self.bodies.get(&first).copied().unwrap_or(first);
        let statements = self.progs.statements();
        let slots = (first..end)
            .filter_map(|n| statements.get(usize::try_from(n).ok()?))
            .flat_map(|s| [s.a, s.b, s.c]);
        Pools::of(self.progs, &self.types, slots)
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
        for edict in edicts {
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
            parms.push(self.value(rng, own, def_type).unwrap_or(Draw::Float(0.0)));
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
pub(super) struct Rng(u64);

impl Rng {
    pub(super) fn new(name: &[u8], round: u32) -> Self {
        let mut seed: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in name.iter().chain(&round.to_le_bytes()) {
            seed = (seed ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
        Self(seed)
    }

    pub(super) fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number below `bound`; 0 when `bound` is 0.
    pub(super) fn below(&mut self, bound: u64) -> u64 {
        self.next().checked_rem(bound).unwrap_or(0)
    }

    /// An element of `items`; `None` when it is empty.
    pub(super) fn pick<'t, T>(&mut self, items: &'t [T]) -> Option<&'t T> {
        let count = u64::try_from(items.len()).ok()?;
        items.get(usize::try_from(self.below(count)).ok()?)
    }

    /// A float from the pool of DIFF-12.2: a whole number from -1 to 10 (3 in 10), a product
    /// of two powers of two up to 2^23 (2 in 10), a 24-bit flag mask (1 in 10), a whole
    /// number below 1000 (2 in 10), a fraction in thousandths below 1 (1 in 10), or a negative
    /// whole number above -1000 (1 in 10).
    pub(super) fn float(&mut self) -> f32 {
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
