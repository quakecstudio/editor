// SPDX-License-Identifier: GPL-3.0-or-later

//! A server's whole state as typed values, read the same way from this world and from any other
//! source of id's raw slots, such as the engine oracle's trace (P3-25): every named global and
//! every field of every edict below `num_edicts`, each by its definition's type (DIFF-16.2).

use crate::offsets::{FieldOffset, GlobalOffset};
use qc_edict::EdictNumber;
use qc_progs::Progs;
use qc_types::PROGS_VECTOR_SLOTS;
use qc_types::{Etype, Slot};

use super::inspect::{NamedValue, QcValue, named};

/// What a state is read from: id's raw global and field slots, and the two readings that depend
/// on where they live, an entity value's edict and a string value's text.
pub trait RawState {
    /// Global slot `slot`'s bits.
    fn global(&self, slot: GlobalOffset) -> Slot;
    /// Field slot `slot` of edict `edict`.
    fn field(&self, edict: EdictNumber, slot: FieldOffset) -> Slot;
    /// The edicts below `sv.num_edicts`.
    fn num_edicts(&self) -> usize;
    /// The edict an entity value names, 0 (the world) when it names none.
    fn entity(&self, value: Slot) -> usize;
    /// A string value's text, empty when it names none.
    fn string(&self, progs: &Progs<'_>, value: Slot) -> Vec<u8>;
}

/// A server's state: its named globals and each edict's fields, in definition order.
#[derive(Clone, Debug, PartialEq)]
pub struct State {
    /// Every named global, but a vector's components and the functions' and fields' own
    /// definitions.
    pub globals: Vec<NamedValue>,
    /// For each edict below `num_edicts`, every named field, but a vector's components.
    pub edicts: Vec<Vec<NamedValue>>,
}

/// The state `raw` holds, read through `progs`' definitions.
#[must_use]
pub fn read_state(progs: &Progs<'_>, raw: &impl RawState) -> State {
    let globals = read_globals(progs, raw);
    let edicts = (0..raw.num_edicts())
        .map(|n| read_fields(progs, raw, n))
        .collect();
    State { globals, edicts }
}

/// Every named global `raw` holds, as [`State::globals`] lists them.
pub(super) fn read_globals(progs: &Progs<'_>, raw: &impl RawState) -> Vec<NamedValue> {
    progs
        .global_defs()
        .iter()
        .filter_map(|def| {
            let (name, etype, first) = named(progs, def)?;
            if matches!(etype, Etype::Function | Etype::Field) {
                return None;
            }
            let slots: Vec<Slot> = (0..etype.slot_count())
                .map(|i| raw.global(GlobalOffset::new(first.saturating_add(i))))
                .collect();
            Some(NamedValue {
                name,
                value: typed_value(progs, raw, etype, &slots),
            })
        })
        .collect()
}

/// Every named field of edict `n` in `raw`, as [`State::edicts`] lists them.
fn read_fields(progs: &Progs<'_>, raw: &impl RawState, n: usize) -> Vec<NamedValue> {
    progs
        .field_defs()
        .iter()
        .skip(1)
        .filter_map(|def| {
            let (name, etype, first) = named(progs, def)?;
            let slots: Vec<Slot> = (0..etype.slot_count())
                .map(|i| {
                    raw.field(
                        EdictNumber::new(n),
                        FieldOffset::new(first.saturating_add(i)),
                    )
                })
                .collect();
            Some(NamedValue {
                name,
                value: typed_value(progs, raw, etype, &slots),
            })
        })
        .collect()
}

/// The value `slots` hold as `etype`, read through `raw` for entities and strings.
pub(super) fn typed_value(
    progs: &Progs<'_>,
    raw: &impl RawState,
    etype: Etype,
    slots: &[Slot],
) -> QcValue {
    let first = slots.first().copied().unwrap_or(Slot::ZERO);
    let name = |offset: i32| {
        progs
            .string(offset)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default()
    };
    match etype {
        Etype::Float => QcValue::Float(first.as_f32()),
        Etype::Vector => {
            let mut vector = [0.0; PROGS_VECTOR_SLOTS];
            for (out, slot) in vector.iter_mut().zip(slots) {
                *out = slot.as_f32();
            }
            QcValue::Vector(vector)
        }
        Etype::String => QcValue::String(raw.string(progs, first)),
        Etype::Entity => QcValue::Entity(raw.entity(first)),
        Etype::Field => QcValue::Field(
            u32::try_from(first.as_i32())
                .ok()
                .and_then(|ofs| progs.ed_field_at_ofs(ofs))
                .map(|def| name(def.s_name))
                .unwrap_or_default(),
        ),
        Etype::Function => QcValue::Function(
            usize::try_from(first.as_i32())
                .ok()
                .and_then(|index| progs.functions().get(index))
                .map(|function| name(function.s_name))
                .unwrap_or_default(),
        ),
        Etype::Void | Etype::Pointer => QcValue::Void,
    }
}

#[cfg(test)]
mod tests;
