// SPDX-License-Identifier: GPL-3.0-or-later

//! Which edict each of the map's entities became, so the map editor's edits reach the running
//! game (MAP-12, BRIDGE-4.13). Not id's: the engine forgets an entity's place in the lump once
//! it spawns.
//!
//! An edict is the entity's while it stays in use with the `freetime` it had when the entity
//! took it: `ED_Alloc` keeps `freetime` when it reuses an edict (EDICT-6.5), and `ED_Free`
//! sets it to the clock (EDICT-7.1), so an edict freed since, and perhaps reused by another
//! entity, no longer matches.

use qc_edict::EdictNumber;

use super::World;

/// An entity's place in the map editor's list of the map's entities, from 0, the world
/// (BRIDGE-4.13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MapPlace(usize);

impl MapPlace {
    /// The entity at `place`.
    #[must_use]
    pub const fn new(place: usize) -> Self {
        Self(place)
    }
}

/// An edict and the `freetime` it had when the entity took it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Taken {
    edict: usize,
    freetime: f32,
}

/// The edict of each of the map's entities, in the editor's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MapEntities(Vec<Option<Taken>>);

impl MapEntities {
    /// The list after the map's load, from the edict each entity's spawn ran on: an edict its
    /// spawn function freed, or a later entity took, is not the earlier entity's.
    pub(crate) fn loaded(world: &World, spawned: &[Option<usize>]) -> Self {
        let mut list: Vec<Option<Taken>> = Vec::with_capacity(spawned.len());
        for &edict in spawned {
            if let Some(edict) = edict {
                for earlier in &mut list {
                    if earlier.is_some_and(|taken| taken.edict == edict) {
                        *earlier = None;
                    }
                }
            }
            list.push(edict.and_then(|edict| world.taken(edict)));
        }
        Self(list)
    }
}

impl World {
    /// Edict `edict` as an entity takes it now: `None` when it is not in use.
    fn taken(&self, edict: usize) -> Option<Taken> {
        if !self.in_use(edict) {
            return None;
        }
        let freetime = self.machine.edicts().freetime(EdictNumber::new(edict))?;
        Some(Taken { edict, freetime })
    }

    /// The edict the map's entity at `place` is in the game, `None` when it is not: inhibited,
    /// classless, freed since, or past the list (BRIDGE-4.13).
    #[must_use]
    pub fn map_entity(&self, place: MapPlace) -> Option<usize> {
        let taken = (*self.map_entities.0.get(place.0)?)?;
        self.taken(taken.edict)
            .filter(|now| now.freetime.to_bits() == taken.freetime.to_bits())
            .map(|now| now.edict)
    }

    /// Puts a map entity at `place` in the list, in edict `edict` or none: the entities from
    /// `place` on move one later. A place past the end is the end.
    pub fn insert_map_entity(&mut self, place: MapPlace, edict: Option<EdictNumber>) {
        let taken = edict.and_then(|edict| self.taken(edict.index()));
        let at = place.0.min(self.map_entities.0.len());
        self.map_entities.0.insert(at, taken);
    }

    /// Takes the map's entity at `place` out of the list, the entities after it moving one
    /// earlier; the edict it is in the game, if it is.
    pub fn take_map_entity(&mut self, place: MapPlace) -> Option<usize> {
        let edict = self.map_entity(place);
        if place.0 < self.map_entities.0.len() {
            self.map_entities.0.remove(place.0);
        }
        edict
    }
}

#[cfg(test)]
mod tests;
