// SPDX-License-Identifier: GPL-3.0-or-later

//! What the bridge's `inspect` reads (BRIDGE-4.1, BRIDGE-5): an edict's fields and the globals,
//! each a typed QuakeC value, between two server frames.

use qc_edict::EdictNumber;
use qc_progs::{Def, Progs};
use qc_types::{DefType, EntityRef, Etype, Slot};

use super::World;
use super::save::known;
use super::state::{RawState, State, read_globals, read_state, typed_value};

/// A QuakeC value with its type (BRIDGE-5.1).
#[derive(Clone, Debug, PartialEq)]
pub enum QcValue {
    /// A float.
    Float(f32),
    /// A vector.
    Vector([f32; 3]),
    /// A string's bytes; one that names no string is empty.
    String(Vec<u8>),
    /// An edict's number; a value naming no edict is 0, the world.
    Entity(usize),
    /// A field's name; one naming no field is empty.
    Field(String),
    /// A function's name; one naming no function is empty.
    Function(String),
    /// Nothing.
    Void,
}

/// A field's or a global's name and value.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedValue {
    /// Its name.
    pub name: String,
    /// Its value.
    pub value: QcValue,
}

impl World {
    /// Whether edict `n` is in use: below `num_edicts` and not free (BRIDGE-4.5).
    #[must_use]
    pub fn in_use(&self, n: usize) -> bool {
        let edicts = self.machine.edicts();
        n < edicts.num_edicts() && edicts.is_free(EdictNumber::new(n)) == Some(false)
    }

    /// `sv.num_edicts`: every edict below it has been used on this map (EDICT-1.4).
    #[must_use]
    pub fn num_edicts(&self) -> usize {
        self.machine.edicts().num_edicts()
    }

    /// Every field of edict `n` with its value, `None` when it is not in use: each field
    /// definition after the first, but a vector's `_x`, `_y` and `_z`, as `ED_Print` lists
    /// them.
    #[must_use]
    pub fn edict_fields(&self, progs: &Progs<'_>, n: usize) -> Option<Vec<NamedValue>> {
        if !self.in_use(n) {
            return None;
        }
        Some(
            progs
                .field_defs()
                .iter()
                .skip(1)
                .filter_map(|def| self.field_value(progs, n, def))
                .collect(),
        )
    }

    /// Field `def` of edict `n` with its value, `None` for a definition [`named`] skips.
    pub(super) fn field_value(&self, progs: &Progs<'_>, n: usize, def: Def) -> Option<NamedValue> {
        let (name, etype, first) = named(progs, def)?;
        let slots: Vec<Slot> = (0..etype.slot_count())
            .map(|i| self.slot_of(n, first.saturating_add(i)))
            .collect();
        Some(NamedValue {
            name,
            value: self.value(progs, etype, &slots),
        })
    }

    /// Every named global with its value, but a vector's `_x`, `_y` and `_z` and the
    /// functions' and fields' own definitions, whose values are their code and offsets.
    #[must_use]
    pub fn named_globals(&self, progs: &Progs<'_>) -> Vec<NamedValue> {
        read_globals(progs, self)
    }

    /// Edict `n`'s `classname`, empty when it has none.
    #[must_use]
    pub fn classname(&self, progs: &Progs<'_>, n: usize) -> String {
        let value = self.field(n, "classname");
        String::from_utf8_lossy(&self.string_value(progs, value).unwrap_or_default()).into_owned()
    }

    /// The value `slots` hold as `etype`.
    fn value(&self, progs: &Progs<'_>, etype: Etype, slots: &[Slot]) -> QcValue {
        typed_value(progs, self, etype, slots)
    }

    /// The whole state, every edict below `num_edicts` included, free or not (P3-25).
    #[must_use]
    pub fn state(&self, progs: &Progs<'_>) -> State {
        read_state(progs, self)
    }
}

impl RawState for World {
    fn global(&self, slot: usize) -> Slot {
        i32::try_from(slot)
            .ok()
            .and_then(|slot| self.machine.global(slot).ok())
            .unwrap_or(Slot::ZERO)
    }

    fn field(&self, edict: usize, slot: usize) -> Slot {
        self.slot_of(edict, slot)
    }

    fn num_edicts(&self) -> usize {
        self.machine.edicts().num_edicts()
    }

    fn entity(&self, value: Slot) -> usize {
        self.machine
            .edicts()
            .num_for_edict(EntityRef::from(value))
            .map_or(0, EdictNumber::index)
    }

    fn string(&self, progs: &Progs<'_>, value: Slot) -> Vec<u8> {
        self.string_value(progs, value).unwrap_or_default()
    }
}

/// A definition's name, type and first slot, `None` for one with no name or a vector's
/// component.
pub(super) fn named(progs: &Progs<'_>, def: Def) -> Option<(String, Etype, usize)> {
    let name = progs.string(def.s_name).filter(|n| !n.is_empty())?;
    let components = [&b"_x"[..], b"_y", b"_z"];
    if components.iter().any(|c| name.ends_with(c)) {
        return None;
    }
    let etype = match def.def_type() {
        DefType::Known(etype) => etype,
        DefType::Unknown(_) => known(def.def_type()),
    };
    let first = usize::try_from(def.ofs).ok()?;
    Some((String::from_utf8_lossy(name).into_owned(), etype, first))
}

#[cfg(test)]
mod tests;
