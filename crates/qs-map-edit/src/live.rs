// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's edits as the running game takes them (P5-07, MAP-12): each edit, undo
//! and redo of the open map changes one entity, and the change goes to the game by the
//! entity's place in the map (BRIDGE-4.13), so the game shows what the editor shows with no
//! recompile. A brush's faces are not entities' keys: moving a brush reaches the game at the
//! next compile.

use qs_map::Entity;

use super::EntityIndex;

/// One change to the map's entities, as the bridge sends it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveChange {
    /// An entity put at `place`, spawned as the map's load spawns one (BRIDGE-4.12).
    Spawn {
        /// Its place in the map.
        place: EntityIndex,
        /// Its classname.
        classname: String,
        /// Its other key-values, in order.
        pairs: Vec<(String, String)>,
    },
    /// The entity at `place` taken out.
    Remove {
        /// Its place in the map before.
        place: EntityIndex,
    },
    /// A brush entity put at `place`, whose brushes reach the game only at the next compile.
    NeedsCompile {
        /// Its place in the map.
        place: EntityIndex,
    },
    /// Key-values of the entity at `place` set; a key taken out is set to an empty value, which
    /// the spawn parse reads as 0, the empty string or the world (EDICT-8.2).
    Edit {
        /// Its place in the map.
        place: EntityIndex,
        /// The key-values that changed.
        pairs: Vec<(String, String)>,
    },
}

impl LiveChange {
    /// The changes that make the entities `before` into `after`: an entity put in or taken out
    /// where the two first differ, and an edit of each entity whose key-values changed.
    pub(crate) fn between(before: &[Entity], after: &[Entity]) -> Vec<Self> {
        let mut out = Vec::new();
        let first = before
            .iter()
            .zip(after)
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| before.len().min(after.len()));
        let (before_rest, after_rest) = if after.len() == before.len().saturating_add(1) {
            if let Some(entity) = after.get(first) {
                out.push(Self::spawn(EntityIndex::new(first), entity));
            }
            (before.get(first..), after.get(first.saturating_add(1)..))
        } else if before.len() == after.len().saturating_add(1) {
            out.push(Self::Remove {
                place: EntityIndex::new(first),
            });
            (before.get(first.saturating_add(1)..), after.get(first..))
        } else {
            (Some(before), Some(after))
        };
        let offset = after
            .len()
            .saturating_sub(after_rest.map_or(0, <[Entity]>::len));
        let (before_rest, after_rest) = (
            before_rest.unwrap_or_default(),
            after_rest.unwrap_or_default(),
        );
        for (index, (a, b)) in before_rest.iter().zip(after_rest).enumerate() {
            if a == b {
                continue;
            }
            let pairs = changed_pairs(a, b);
            if !pairs.is_empty() {
                out.push(Self::Edit {
                    place: EntityIndex::new(index.saturating_add(offset)),
                    pairs,
                });
            }
        }
        // Lists that differ by more than one entity, which no single edit makes: the extra
        // entities are put in or taken out at the end.
        let common = before_rest.len().min(after_rest.len());
        for (index, entity) in after_rest.iter().enumerate().skip(common) {
            out.push(Self::spawn(
                EntityIndex::new(index.saturating_add(offset)),
                entity,
            ));
        }
        for _ in common..before_rest.len() {
            out.push(Self::Remove {
                place: EntityIndex::new(common.saturating_add(offset)),
            });
        }
        out
    }

    /// The change as the map editor says it, `after` the entities it leaves (P5-17).
    pub(crate) fn describe(&self, after: &[Entity]) -> super::disk::MapDifference {
        let class = |place: EntityIndex| {
            after
                .get(place.get())
                .and_then(Entity::classname)
                .map_or_default(text)
        };
        let pairs = |pairs: &[(String, String)]| {
            pairs
                .iter()
                .map(|(key, value)| format!("{key} \"{value}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let (entity, text) = match self {
            Self::Spawn {
                place, classname, ..
            } => (*place, format!("added: {classname}")),
            Self::NeedsCompile { place } => (*place, format!("added: {}", class(*place))),
            Self::Remove { place } => (*place, "removed".to_owned()),
            Self::Edit {
                place,
                pairs: changed,
            } => (*place, format!("{}: {}", class(*place), pairs(changed))),
        };
        super::disk::MapDifference { entity, text }
    }

    /// The entity `entity` spawned at `place`, or put there for the next compile when it has
    /// brushes: a brush entity's model is the compiler's to make.
    fn spawn(place: EntityIndex, entity: &Entity) -> Self {
        if entity.has_brushes() {
            return Self::NeedsCompile { place };
        }
        let classname = entity.classname().map_or_default(text);
        let pairs = pairs(entity)
            .into_iter()
            .filter(|(key, _)| key != "classname")
            .collect();
        Self::Spawn {
            place,
            classname,
            pairs,
        }
    }
}

/// `entity`'s key-values as text, in order.
fn pairs(entity: &Entity) -> Vec<(String, String)> {
    entity
        .pairs()
        .map(|pair| (text(pair.key()), text(pair.value())))
        .collect()
}

/// The key-values of `after` whose values `before` does not have, each key once with its last
/// value, and each key of `before` that `after` lacks, empty.
fn changed_pairs(before: &Entity, after: &Entity) -> Vec<(String, String)> {
    let last =
        |entity: &Entity, key: &str| -> Option<String> { entity.value(key.as_bytes()).map(text) };
    let mut out: Vec<(String, String)> = Vec::new();
    for (key, _) in pairs(after) {
        let value = last(after, &key);
        if value != last(before, &key) && !out.iter().any(|(k, _)| *k == key) {
            out.push((key, value.unwrap_or_default()));
        }
    }
    for (key, _) in pairs(before) {
        if last(after, &key).is_none() && !out.iter().any(|(k, _)| *k == key) {
            out.push((key, String::new()));
        }
    }
    out
}

/// Bytes as text, a byte that is not UTF-8 replaced.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[cfg(test)]
mod tests;
