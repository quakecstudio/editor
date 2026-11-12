// SPDX-License-Identifier: GPL-3.0-or-later

//! Rooms: TrenchBroom's layers and groups, the names a mapper gives parts of a map (MAP-9),
//! described from TrenchBroom's `isLayer`, `isGroup` and `extractContainerInfo`, not ported.

use crate::{Entity, MapFile};

/// The classname TrenchBroom saves a layer or a group as (MAP-9.1).
const CONTAINER_CLASSNAME: &[u8] = b"func_group";

/// What a room is in TrenchBroom (MAP-9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomKind {
    /// A layer: `_tb_type` `_tb_layer`.
    Layer,
    /// A group: `_tb_type` `_tb_group`.
    Group,
}

impl RoomKind {
    /// The kind `entity` is, `None` for an entity that is neither (MAP-9.1).
    fn of(entity: &Entity) -> Option<Self> {
        if entity.classname() != Some(CONTAINER_CLASSNAME) {
            return None;
        }
        match entity.value(b"_tb_type")? {
            b"_tb_layer" => Some(Self::Layer),
            b"_tb_group" => Some(Self::Group),
            _ => None,
        }
    }

    /// The key a member names a room of this kind with (MAP-9.2).
    const fn member_key(self) -> &'static [u8] {
        match self {
            Self::Layer => b"_tb_layer",
            Self::Group => b"_tb_group",
        }
    }
}

/// A room of a map (MAP-9).
#[derive(Clone, Debug, PartialEq)]
pub struct Room {
    /// Its name, `_tb_name`.
    pub name: String,
    /// A layer or a group.
    pub kind: RoomKind,
    /// Its id, `_tb_id`, which its members name.
    id: u64,
    /// The box around its brushes, `[min, max]`, `None` with none (MAP-9.3).
    pub bounds: Option<[[f64; 3]; 2]>,
}

impl Room {
    /// Whether `point` lies in the room's bounds, edges included (MAP-9.4).
    #[must_use]
    pub fn holds(&self, point: [f64; 3]) -> bool {
        self.bounds.is_some_and(|[min, max]| {
            point
                .iter()
                .zip(min.iter().zip(&max))
                .all(|(p, (lo, hi))| lo <= p && p <= hi)
        })
    }
}

/// A whole number from 0, as TrenchBroom reads the id a member names (MAP-9.2): `None` for
/// anything else.
fn id(value: &[u8]) -> Option<u64> {
    core::str::from_utf8(value).ok()?.parse().ok()
}

/// The box `bounds` grown to hold `point`.
fn grow(bounds: Option<[[f64; 3]; 2]>, point: [f64; 3]) -> [[f64; 3]; 2] {
    let [mut min, mut max] = bounds.unwrap_or([point, point]);
    for ((lo, hi), p) in min.iter_mut().zip(max.iter_mut()).zip(point) {
        *lo = lo.min(p);
        *hi = hi.max(p);
    }
    [min, max]
}

impl Entity {
    /// The room this entity names, its kind and id (MAP-9.2): its `_tb_layer`, else its
    /// `_tb_group`; `None` when it names none.
    fn named_room(&self) -> Option<(RoomKind, u64)> {
        [RoomKind::Layer, RoomKind::Group]
            .into_iter()
            .find_map(|kind| Some((kind, id(self.value(kind.member_key())?)?)))
    }

    /// The room this entity is in, of `rooms` (from [`MapFile::rooms`]): its own, for a room's
    /// own entity; the one it names; else the first whose bounds hold its `origin` (MAP-9.4).
    #[must_use]
    pub fn room<'r>(&self, rooms: &'r [Room]) -> Option<&'r Room> {
        let own = RoomKind::of(self).zip(self.value(b"_tb_id").and_then(id));
        if let Some((kind, id)) = own.or_else(|| self.named_room()) {
            return rooms.iter().find(|room| room.kind == kind && room.id == id);
        }
        self.value(b"origin")?;
        let origin = self.origin();
        rooms.iter().find(|room| room.holds(origin))
    }
}

impl MapFile {
    /// The map's rooms, in the map's order (MAP-9.1, MAP-9.3).
    #[must_use]
    pub fn rooms(&self) -> Vec<Room> {
        let mut rooms: Vec<Room> = self
            .entities
            .iter()
            .filter_map(|entity| {
                let kind = RoomKind::of(entity)?;
                let name = entity
                    .value(b"_tb_name")
                    .filter(|n| !n.trim_ascii().is_empty())?;
                Some(Room {
                    name: String::from_utf8_lossy(name).into_owned(),
                    kind,
                    id: id(entity.value(b"_tb_id")?).filter(|&id| id > 0)?,
                    bounds: None,
                })
            })
            .collect();
        for entity in &self.entities {
            let room = match RoomKind::of(entity) {
                Some(kind) => entity.value(b"_tb_id").and_then(id).map(|id| (kind, id)),
                None => entity.named_room(),
            };
            let Some(room) =
                room.and_then(|(kind, id)| rooms.iter_mut().find(|r| r.kind == kind && r.id == id))
            else {
                continue;
            };
            for brush in entity.brushes() {
                for polygon in brush.polygons() {
                    for point in polygon {
                        room.bounds = Some(grow(room.bounds, point));
                    }
                }
            }
        }
        rooms
    }
}

#[cfg(test)]
mod tests;
