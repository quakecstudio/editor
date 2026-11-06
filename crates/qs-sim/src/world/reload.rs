// SPDX-License-Identifier: GPL-3.0-or-later

//! Loading a progs again over the running map (WORLD-10, P3-03): a new machine for the new
//! progs, the edicts kept, and every global and field carried over by name and type, each value
//! translated into the new progs' numbering; what could not be carried is reported.

use std::collections::{HashMap, HashSet};

use qc_edict::EdictNumber;
use qc_progs::{Def, Progs};
use qc_types::{EntityRef, Etype, Slot};
use qc_vm::Machine;

use super::save::known;
use super::{MAXCLIENTS, World, WorldError};
use crate::rerelease::BuiltinTable;

/// What a reload carried over and what it could not (WORLD-10.5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Migration {
    /// The values carried over: globals and the fields of every edict in use.
    pub migrated: usize,
    /// What the new progs lacks, has of another type, or what a value named that it lacks.
    pub lost: Vec<Change>,
    /// What only the new progs has, which starts at its initial value or 0.
    pub added: Vec<Change>,
}

/// One global, field or function a reload reports (WORLD-10.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// What it is.
    pub kind: ChangeKind,
    /// Its name.
    pub name: String,
    /// Why it is reported.
    pub reason: Reason,
}

/// What a reported name is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// A global.
    Global,
    /// An entity field.
    Field,
    /// A function.
    Function,
}

impl ChangeKind {
    /// The name the bridge gives it: `global`, `field` or `function` (BRIDGE-4.9).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Field => "field",
            Self::Function => "function",
        }
    }
}

/// Why a name is reported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Only the old progs has it.
    Removed,
    /// Only the new progs has it.
    Added,
    /// Both have it, of these types: its values are not carried.
    TypeChanged {
        /// Its type in the old progs.
        from: Etype,
        /// Its type in the new progs.
        to: Etype,
    },
    /// A value at this place named it, and the new progs lacks it: the value is 0.
    Referenced(String),
}

impl core::fmt::Display for Reason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Removed => f.write_str("removed"),
            Self::Added => f.write_str("added"),
            Self::TypeChanged { from, to } => {
                write!(
                    f,
                    "type changed from {} to {}",
                    from.name().trim_start_matches("ev_"),
                    to.name().trim_start_matches("ev_")
                )
            }
            Self::Referenced(place) => write!(f, "removed, and {place} named it: now 0"),
        }
    }
}

/// A definition both progs name, by its name: its type and first slot in each.
struct Pair {
    name: String,
    old: (Etype, usize),
    new: (Etype, usize),
}

impl World {
    /// `new`, the progs loaded again over the running map that `old` set up (WORLD-10), with
    /// the builtin table `table` chose for it (RRE-4.2): the edicts kept, every global and
    /// field carried over by name and type, and what could not be carried reported. The
    /// debugger and the profile counts, which number the old progs' statements and functions,
    /// are dropped, and the carried values are not recorded as changes.
    ///
    /// # Errors
    ///
    /// [`WorldError::Progs`] when the new progs cannot back an edict store.
    pub fn reload(
        &mut self,
        old: &Progs<'_>,
        new: &Progs<'_>,
        table: BuiltinTable,
    ) -> Result<Migration, WorldError> {
        let mut machine =
            Machine::new(new, MAXCLIENTS).map_err(|e| WorldError::Progs(e.to_string()))?;
        let store = self
            .machine
            .edicts()
            .with_layout(*machine.edicts().layout());
        *machine.edicts_mut() = store;
        let mut migration = Migration::default();
        let mut reload = Reload {
            old,
            new,
            old_defs: Defs::of(old),
            new_defs: Defs::of(new),
            from: &self.machine,
            to: &mut machine,
            strings: string_starts(new),
            made: Vec::new(),
            lost: Vec::new(),
        };
        reload.functions(&mut migration);
        migration.migrated = reload
            .globals(&mut migration)
            .saturating_add(reload.fields(&mut migration));
        let Reload { made, lost, .. } = reload;
        migration.lost.extend(lost);
        // Each string the old progs held that the new one lacks is an engine string now.
        for (place, text) in made {
            let value = self.make_string(text);
            place.write(&mut machine, Slot::from_i32(value));
        }
        // What the migration wrote is no change of the game's.
        drop(machine.edicts_mut().take_events());
        self.machine = machine;
        self.builtins.table = table;
        self.builtins.profile.clear();
        self.set_debugger(None);
        Ok(migration)
    }
}

/// Where a translated value goes: a global slot, or a field slot of an edict.
#[derive(Clone, Copy, Debug)]
enum Place {
    Global(usize),
    Field(usize, usize),
}

impl Place {
    fn write(self, machine: &mut Machine, value: Slot) {
        // A slot past the globals or an edict's fields is not written.
        match self {
            Self::Global(slot) => {
                if let Ok(slot) = i32::try_from(slot) {
                    let _written = machine.set_global(slot, value);
                }
            }
            Self::Field(edict, slot) => {
                let _written = machine
                    .edicts_mut()
                    .set_field(EdictNumber::new(edict), slot, value);
            }
        }
    }
}

/// One progs' definitions as a reload pairs them, each looked up once (WORLD-10.2).
struct Defs {
    /// The named globals but the functions' locals, a vector's components and any name given
    /// to more than one, in definition order, with their types and first slots.
    globals: Vec<(String, Etype, usize)>,
    /// The names given to more than one global.
    repeated: HashSet<String>,
    /// The named fields, but a vector's components, in definition order.
    fields: Vec<(String, Etype, usize)>,
    /// Each function's index by its name.
    functions: HashMap<Vec<u8>, i32>,
    /// Each field's first slot by its name.
    field_slots: HashMap<Vec<u8>, i32>,
}

impl Defs {
    fn of(progs: &Progs<'_>) -> Self {
        let mut locals: Vec<(usize, usize)> = progs
            .functions()
            .iter()
            .filter(|f| f.first_statement > 0)
            .filter_map(|f| {
                let start = usize::try_from(f.parm_start).ok()?;
                Some((start, start.saturating_add(usize::try_from(f.locals).ok()?)))
            })
            .collect();
        locals.sort_unstable();
        let local = |ofs: usize| {
            let after = locals.partition_point(|&(start, _)| start <= ofs);
            locals
                .get(..after)
                .unwrap_or_default()
                .iter()
                .rev()
                .any(|&(start, end)| ofs >= start && ofs < end)
        };
        let all = definitions(progs, progs.global_defs().iter());
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for (name, _, _) in &all {
            let count = counts.entry(name.as_str()).or_insert(0);
            *count = count.saturating_add(1);
        }
        let repeated: HashSet<String> = counts
            .iter()
            .filter(|&(_, &count)| count > 1)
            .map(|(&name, _)| name.to_owned())
            .collect();
        let globals = all
            .iter()
            .filter(|(name, _, first)| !repeated.contains(name) && !local(*first))
            .cloned()
            .collect();
        let fields = definitions(progs, progs.field_defs().iter().skip(1));
        let functions = (0_i32..)
            .zip(progs.functions().iter())
            .skip(1)
            .filter_map(|(i, f)| Some((progs.string(f.s_name)?.to_vec(), i)))
            .collect();
        let field_slots = fields
            .iter()
            .filter_map(|(name, _, first)| {
                Some((name.as_bytes().to_vec(), i32::try_from(*first).ok()?))
            })
            .collect();
        Self {
            globals,
            repeated,
            fields,
            functions,
            field_slots,
        }
    }
}

/// The named definitions of `defs`, each with its type and first slot, but a vector's
/// components: a name ending in `_x`, `_y` or `_z` whose stem is a vector's.
fn definitions(progs: &Progs<'_>, defs: impl Iterator<Item = Def>) -> Vec<(String, Etype, usize)> {
    let mut all = Vec::new();
    for def in defs {
        let Some(name) = progs.string(def.s_name).filter(|n| !n.is_empty()) else {
            continue;
        };
        let Ok(first) = usize::try_from(def.ofs) else {
            continue;
        };
        let etype = match def.def_type() {
            qc_types::DefType::Known(etype) => etype,
            unknown @ qc_types::DefType::Unknown(_) => known(unknown),
        };
        all.push((String::from_utf8_lossy(name).into_owned(), etype, first));
    }
    let vectors: HashSet<String> = all
        .iter()
        .filter(|(_, etype, _)| *etype == Etype::Vector)
        .map(|(name, _, _)| name.clone())
        .collect();
    all.retain(|(name, _, _)| {
        let component = ["_x", "_y", "_z"].iter().any(|c| {
            name.strip_suffix(c)
                .is_some_and(|stem| vectors.contains(stem))
        });
        !component
    });
    all
}

/// The reload of one world: the two progs, the old machine read and the new one written.
struct Reload<'a, 'p> {
    old: &'a Progs<'p>,
    new: &'a Progs<'p>,
    old_defs: Defs,
    new_defs: Defs,
    from: &'a Machine,
    to: &'a mut Machine,
    /// Where each string of the new progs' table starts, by its text.
    strings: HashMap<Vec<u8>, i32>,
    /// The strings to make as engine strings, where each is written.
    made: Vec<(Place, Vec<u8>)>,
    /// The references to a function or field the new progs lacks.
    lost: Vec<Change>,
}

impl Reload<'_, '_> {
    /// WORLD-10.4: the functions only one of the progs names, as removed and added.
    fn functions(&self, migration: &mut Migration) {
        let names = |progs: &Progs<'_>| -> Vec<String> {
            progs
                .functions()
                .iter()
                .skip(1)
                .filter_map(|f| progs.string(f.s_name))
                .filter(|n| !n.is_empty())
                .map(|n| String::from_utf8_lossy(n).into_owned())
                .collect()
        };
        let (old, new) = (names(self.old), names(self.new));
        let (old_set, new_set): (HashSet<&String>, HashSet<&String>) =
            (old.iter().collect(), new.iter().collect());
        for name in old.iter().filter(|n| !new_set.contains(n)) {
            migration
                .lost
                .push(change(ChangeKind::Function, name, Reason::Removed));
        }
        for name in new.iter().filter(|n| !old_set.contains(n)) {
            migration
                .added
                .push(change(ChangeKind::Function, name, Reason::Added));
        }
    }

    /// WORLD-10.2: each named global but a local carries its running value, translated, unless
    /// that is still its initial value in the old progs, when it takes its initial value in the
    /// new one; returns how many were carried. A function's and a field's own definitions are
    /// among them, reported as functions and fields rather than globals.
    fn globals(&mut self, migration: &mut Migration) -> usize {
        let repeated: HashSet<String> = self
            .old_defs
            .repeated
            .union(&self.new_defs.repeated)
            .cloned()
            .collect();
        let keep = |list: &[(String, Etype, usize)]| -> Vec<(String, Etype, usize)> {
            list.iter()
                .filter(|(name, _, _)| !repeated.contains(name))
                .cloned()
                .collect()
        };
        let (old, new) = (keep(&self.old_defs.globals), keep(&self.new_defs.globals));
        let reported = |etype: Etype| !matches!(etype, Etype::Function | Etype::Field);
        let mut carried = 0_usize;
        for pair in pairs(&old, &new, ChangeKind::Global, &reported, migration) {
            let (etype, from) = pair.old;
            let to = pair.new.1;
            let running: Vec<Slot> = (0..etype.slot_count())
                .map(|i| global(self.from, from.saturating_add(i)))
                .collect();
            let initial: Vec<Slot> = (0..etype.slot_count())
                .map(|i| initial(self.old, from.saturating_add(i)))
                .collect();
            if running == initial {
                continue;
            }
            let place = |i: usize| Place::Global(to.saturating_add(i));
            let at = format!("global {}", pair.name);
            self.carry(etype, &running, &place, &at);
            carried = carried.saturating_add(1);
        }
        carried
    }

    /// WORLD-10.3: each field both progs name with one type is carried for every edict below
    /// `num_edicts`, its values translated; returns how many values were carried.
    fn fields(&mut self, migration: &mut Migration) -> usize {
        let pairs = pairs(
            &self.old_defs.fields,
            &self.new_defs.fields,
            ChangeKind::Field,
            &|_| true,
            migration,
        );
        let mut carried = 0_usize;
        for n in 0..self.from.edicts().num_edicts() {
            let number = EdictNumber::new(n);
            if self.from.edicts().is_free(number) != Some(false) {
                continue;
            }
            for pair in &pairs {
                let (etype, from) = pair.old;
                let to = pair.new.1;
                let values: Vec<Slot> = (0..etype.slot_count())
                    .map(|i| {
                        self.from
                            .edicts()
                            .field(number, from.saturating_add(i))
                            .unwrap_or(Slot::ZERO)
                    })
                    .collect();
                if values.iter().all(|v| *v == Slot::ZERO) {
                    continue;
                }
                let place = |i: usize| Place::Field(n, to.saturating_add(i));
                let at = format!("edict {n} .{}", pair.name);
                self.carry(etype, &values, &place, &at);
                carried = carried.saturating_add(1);
            }
        }
        carried
    }

    /// WORLD-10.4: `values`, of type `etype`, written at `place` translated into the new
    /// progs: an entity by its edict, a string by its text, a field and a function by name;
    /// any other value as it is. A field or function the new progs lacks is a reference lost
    /// at `at`, and the value 0.
    fn carry(&mut self, etype: Etype, values: &[Slot], place: &dyn Fn(usize) -> Place, at: &str) {
        let first = values.first().copied().unwrap_or(Slot::ZERO);
        let one = match etype {
            Etype::Entity => {
                let edict = self
                    .from
                    .edicts()
                    .num_for_edict(EntityRef::from(first))
                    .map_or(0, EdictNumber::index);
                let value = i32::try_from(edict)
                    .ok()
                    .and_then(|n| self.to.edicts().edict_num(n).ok())
                    .map_or(Slot::ZERO, |e| Slot::from_i32(e.bits()));
                Some(value)
            }
            Etype::String if first.as_i32() >= 0 => {
                let text = self.old.string(first.as_i32()).unwrap_or_default().to_vec();
                if let Some(&ofs) = self.strings.get(&text) {
                    Some(Slot::from_i32(ofs))
                } else {
                    self.made.push((place(0), text));
                    return;
                }
            }
            Etype::Field => {
                let name = u32::try_from(first.as_i32())
                    .ok()
                    .and_then(|ofs| self.old.ed_field_at_ofs(ofs))
                    .and_then(|def| self.old.string(def.s_name).map(<[u8]>::to_vec));
                Some(self.reference(name, ChangeKind::Field, at))
            }
            Etype::Function => {
                let name = usize::try_from(first.as_i32())
                    .ok()
                    .filter(|&i| i > 0)
                    .and_then(|i| self.old.functions().get(i))
                    .and_then(|f| self.old.string(f.s_name).map(<[u8]>::to_vec));
                Some(self.reference(name, ChangeKind::Function, at))
            }
            Etype::Void | Etype::Float | Etype::Vector | Etype::String | Etype::Pointer => None,
        };
        match one {
            Some(value) => place(0).write(self.to, value),
            None => {
                for (i, value) in values.iter().enumerate() {
                    place(i).write(self.to, *value);
                }
            }
        }
    }

    /// The value naming the function or field `name` in the new progs; 0 for no name, and for
    /// a name the new progs lacks, which is a reference lost at `at`.
    fn reference(&mut self, name: Option<Vec<u8>>, kind: ChangeKind, at: &str) -> Slot {
        let Some(name) = name.filter(|n| !n.is_empty()) else {
            return Slot::ZERO;
        };
        let found = match kind {
            ChangeKind::Function => self.new_defs.functions.get(&name),
            ChangeKind::Field | ChangeKind::Global => self.new_defs.field_slots.get(&name),
        };
        if let Some(&value) = found {
            return Slot::from_i32(value);
        }
        let name = String::from_utf8_lossy(&name).into_owned();
        self.lost
            .push(change(kind, &name, Reason::Referenced(at.to_owned())));
        Slot::ZERO
    }
}

fn change(kind: ChangeKind, name: &str, reason: Reason) -> Change {
    Change {
        kind,
        name: name.to_owned(),
        reason,
    }
}

/// The definitions both lists name, paired by name; each only one names, and each of two
/// types, reported as `kind` when `reported` says its type is.
fn pairs(
    old: &[(String, Etype, usize)],
    new: &[(String, Etype, usize)],
    kind: ChangeKind,
    reported: &dyn Fn(Etype) -> bool,
    migration: &mut Migration,
) -> Vec<Pair> {
    let by_name: HashMap<&str, (Etype, usize)> = new
        .iter()
        .map(|(name, etype, first)| (name.as_str(), (*etype, *first)))
        .collect();
    let old_names: HashSet<&str> = old.iter().map(|(name, _, _)| name.as_str()).collect();
    let mut pairs = Vec::new();
    for (name, etype, first) in old {
        match by_name.get(name.as_str()) {
            None if reported(*etype) => migration.lost.push(change(kind, name, Reason::Removed)),
            None => {}
            Some(&(to, _)) if to != *etype => {
                migration
                    .lost
                    .push(change(kind, name, Reason::TypeChanged { from: *etype, to }))
            }
            Some(&(to, at)) => pairs.push(Pair {
                name: name.clone(),
                old: (*etype, *first),
                new: (to, at),
            }),
        }
    }
    for (name, etype, _) in new {
        if !old_names.contains(name.as_str()) && reported(*etype) {
            migration.added.push(change(kind, name, Reason::Added));
        }
    }
    pairs
}

fn global(machine: &Machine, slot: usize) -> Slot {
    i32::try_from(slot)
        .ok()
        .and_then(|slot| machine.global(slot).ok())
        .unwrap_or(Slot::ZERO)
}

fn initial(progs: &Progs<'_>, slot: usize) -> Slot {
    progs.globals().get(slot).unwrap_or(Slot::ZERO)
}

/// Where each string of `progs`' table starts, by its text: offset 0 and each offset after a
/// terminating 0.
fn string_starts(progs: &Progs<'_>) -> HashMap<Vec<u8>, i32> {
    let mut starts = HashMap::new();
    let mut at = 0_i32;
    while let Some(text) = progs.string(at) {
        starts.entry(text.to_vec()).or_insert(at);
        let len = i32::try_from(text.len()).unwrap_or(i32::MAX);
        let Some(next) = at.checked_add(len).and_then(|n| n.checked_add(1)) else {
            break;
        };
        at = next;
    }
    starts
}

#[cfg(test)]
mod tests;
