// SPDX-License-Identifier: GPL-3.0-or-later

//! Which edict each of the map's entities became, so the map editor's edits reach the running
//! game (MAP-12, BRIDGE-4.13). Not id's: the engine forgets an entity's place in the lump once
//! it spawns.
//!
//! An edict is the entity's while it stays in use with the `freetime` it had when the entity
//! took it, and holds the entity's classname: `ED_Alloc` keeps `freetime` when it reuses an
//! edict (EDICT-6.5), and `ED_Free` sets it to the clock (EDICT-7.1), so an edict freed since
//! no longer matches; in the map's first seconds, when the clock may not have moved, an edict
//! another entity took holds that one's classname.

use qc_edict::EdictNumber;
use qc_progs::Progs;

use super::World;
use crate::spawn::LoadedEntity;

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

/// One of the map's entities: its classname and the edict it took, if any.
#[derive(Clone, Debug, PartialEq)]
struct Entry {
    classname: String,
    taken: Option<Taken>,
}

/// The map's entities, in the editor's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct MapEntities(Vec<Entry>);

impl MapEntities {
    /// The list after the map's load: an edict that a later entity of the lump took is not the
    /// earlier entity's.
    pub(crate) fn loaded(world: &World, spawned: &[LoadedEntity]) -> Self {
        let mut list: Vec<Entry> = Vec::with_capacity(spawned.len());
        for entity in spawned {
            if let Some(edict) = entity.edict {
                for earlier in &mut list {
                    if earlier
                        .taken
                        .is_some_and(|taken| taken.edict == edict.index())
                    {
                        earlier.taken = None;
                    }
                }
            }
            list.push(Entry {
                classname: String::from_utf8_lossy(&entity.classname).into_owned(),
                taken: entity.edict.and_then(|edict| world.taken(edict.index())),
            });
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

    /// The classname of each of the map's entities, in the editor's order, which the editor
    /// checks its own list against (BRIDGE-4.13); empty after a saved game is restored.
    #[must_use]
    pub fn map_classnames(&self) -> Vec<String> {
        self.map_entities
            .0
            .iter()
            .map(|entry| entry.classname.clone())
            .collect()
    }

    /// The edict the map's entity at `place` is in the game, `None` when it is not: inhibited,
    /// classless, freed since, holding another classname, or past the list (BRIDGE-4.13).
    #[must_use]
    pub fn map_entity(&self, progs: &Progs<'_>, place: MapPlace) -> Option<usize> {
        let entry = self.map_entities.0.get(place.0)?;
        let taken = entry.taken?;
        self.taken(taken.edict)
            .filter(|now| now.freetime.to_bits() == taken.freetime.to_bits())
            .filter(|now| self.classname(progs, now.edict) == entry.classname)
            .map(|now| now.edict)
    }

    /// Puts a map entity of `classname` at `place` in the list, in edict `edict` or none: the
    /// entities from `place` on move one later. A place past the end is the end.
    pub fn insert_map_entity(
        &mut self,
        place: MapPlace,
        classname: &str,
        edict: Option<EdictNumber>,
    ) {
        let taken = edict.and_then(|edict| self.taken(edict.index()));
        let at = place.0.min(self.map_entities.0.len());
        self.map_entities.0.insert(
            at,
            Entry {
                classname: classname.to_owned(),
                taken,
            },
        );
    }

    /// Takes the map's entity at `place` out of the list, the entities after it moving one
    /// earlier; the edict it is in the game, if it is.
    pub fn take_map_entity(&mut self, progs: &Progs<'_>, place: MapPlace) -> Option<usize> {
        let edict = self.map_entity(progs, place);
        if place.0 < self.map_entities.0.len() {
            self.map_entities.0.remove(place.0);
        }
        edict
    }
}

#[cfg(test)]
mod tests;
