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
    /// The box around its brushes and those of the rooms in it, `[min, max]`, `None` with none
    /// (MAP-9.3).
    pub bounds: Option<[[f64; 3]; 2]>,
    /// The room it is in, by its place in [`Rooms::list`].
    parent: Option<usize>,
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

    /// The volume of its bounds, 0 with none.
    fn volume(&self) -> f64 {
        self.bounds.map_or(0.0, |[min, max]| {
            min.iter().zip(&max).map(|(lo, hi)| hi - lo).product()
        })
    }
}

/// A layer or group as TrenchBroom reads it, named or not, and the container it is in.
#[derive(Clone, Debug, PartialEq)]
struct Container {
    kind: RoomKind,
    id: u64,
    /// Its place in [`Rooms::list`], for one with a name.
    room: Option<usize>,
    /// The container its own entity names (MAP-9.2).
    parent: Option<(RoomKind, u64)>,
}

/// A map's rooms and the containers between them (MAP-9).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Rooms {
    list: Vec<Room>,
    containers: Vec<Container>,
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
    /// The container this entity names, its kind and id (MAP-9.2): its `_tb_layer`, else its
    /// `_tb_group`; `None` when it names none.
    fn named_container(&self) -> Option<(RoomKind, u64)> {
        [RoomKind::Layer, RoomKind::Group]
            .into_iter()
            .find_map(|kind| Some((kind, id(self.value(kind.member_key())?)?)))
    }

    /// The container that is this entity, for a layer's or group's own entity.
    fn own_container(&self) -> Option<(RoomKind, u64)> {
        RoomKind::of(self).zip(self.value(b"_tb_id").and_then(id))
    }
}

impl Rooms {
    /// The rooms, in the map's order.
    #[must_use]
    pub fn list(&self) -> &[Room] {
        &self.list
    }

    /// The nearest room at or above the container `start`, by its place in the list.
    fn named(&self, start: (RoomKind, u64)) -> Option<usize> {
        let mut at = start;
        // A cycle of containers ends once every one has been passed.
        for _ in 0..=self.containers.len() {
            let container = self.containers.iter().find(|c| (c.kind, c.id) == at)?;
            if let Some(room) = container.room {
                return Some(room);
            }
            at = container.parent?;
        }
        None
    }

    /// The room `entity`'s keys put it in: its own, for a room's own entity, else the nearest
    /// named one above the container it is or names (MAP-9.2, MAP-9.4).
    fn keyed(&self, entity: &Entity) -> Option<usize> {
        if let Some(own) = entity.own_container() {
            if let Some(room) = self.named(own) {
                return Some(room);
            }
            let container = self.containers.iter().find(|c| (c.kind, c.id) == own)?;
            return container.parent.and_then(|parent| self.named(parent));
        }
        entity.named_container().and_then(|named| self.named(named))
    }

    /// The rooms `entity` is in, the innermost first, then each room around it (MAP-9.4): the
    /// room its keys put it in, else the smallest whose bounds hold its `origin`.
    #[must_use]
    pub fn of(&self, entity: &Entity) -> Vec<&Room> {
        let innermost = self.keyed(entity).or_else(|| {
            entity.value(b"origin")?;
            let origin = entity.origin();
            self.list
                .iter()
                .enumerate()
                .filter(|(_, room)| room.holds(origin))
                .min_by(|(i, a), (j, b)| {
                    // The same box: the room inside the other first.
                    a.volume()
                        .total_cmp(&b.volume())
                        .then_with(|| self.depth(*j).cmp(&self.depth(*i)))
                })
                .map(|(index, _)| index)
        });
        let mut out = Vec::new();
        let mut at = innermost;
        while let Some(index) = at {
            let Some(room) = self.list.get(index) else {
                break;
            };
            if out.iter().any(|seen: &&Room| core::ptr::eq(*seen, room)) {
                break;
            }
            out.push(room);
            at = room.parent;
        }
        out
    }

    /// How many rooms are around the room at `index`.
    fn depth(&self, index: usize) -> usize {
        let mut depth = 0_usize;
        let mut at = self.list.get(index).and_then(|room| room.parent);
        while let Some(parent) = at {
            if depth > self.list.len() {
                break;
            }
            depth = depth.saturating_add(1);
            at = self.list.get(parent).and_then(|room| room.parent);
        }
        depth
    }

    /// Whether `entity` is in the room named `name`, in any case, or a room inside it.
    #[must_use]
    pub fn holds(&self, entity: &Entity, name: &str) -> bool {
        self.of(entity)
            .iter()
            .any(|room| room.name.eq_ignore_ascii_case(name))
    }

    /// Whether a room is named `name`, in any case.
    #[must_use]
    pub fn named_any(&self, name: &str) -> bool {
        self.list
            .iter()
            .any(|room| room.name.eq_ignore_ascii_case(name))
    }
}

impl MapFile {
    /// The map's rooms (MAP-9.1, MAP-9.3).
    #[must_use]
    pub fn rooms(&self) -> Rooms {
        let mut rooms = Rooms::default();
        for entity in &self.entities {
            let Some(kind) = RoomKind::of(entity) else {
                continue;
            };
            let Some(id) = entity.value(b"_tb_id").and_then(id).filter(|&id| id > 0) else {
                continue;
            };
            let name = entity
                .value(b"_tb_name")
                .filter(|n| !n.trim_ascii().is_empty());
            let room = name.map(|name| {
                rooms.list.push(Room {
                    name: String::from_utf8_lossy(name).into_owned(),
                    kind,
                    bounds: None,
                    parent: None,
                });
                rooms.list.len().saturating_sub(1)
            });
            rooms.containers.push(Container {
                kind,
                id,
                room,
                parent: entity.named_container(),
            });
        }
        for index in 0..rooms.containers.len() {
            let Some(container) = rooms.containers.get(index) else {
                continue;
            };
            let (room, parent) = (container.room, container.parent);
            let parent = parent.and_then(|parent| rooms.named(parent));
            if let Some(slot) = room.and_then(|room| rooms.list.get_mut(room)) {
                slot.parent = parent.filter(|&p| Some(p) != room);
            }
        }
        for entity in &self.entities {
            let Some(innermost) = rooms.keyed(entity) else {
                continue;
            };
            for brush in entity.brushes() {
                for polygon in brush.polygons() {
                    for point in polygon {
                        let mut at = Some(innermost);
                        for _ in 0..=rooms.list.len() {
                            let Some(room) = at.and_then(|index| rooms.list.get_mut(index)) else {
                                break;
                            };
                            room.bounds = Some(grow(room.bounds, point));
                            at = room.parent;
                        }
                    }
                }
            }
        }
        rooms
    }
}

#[cfg(test)]
mod tests;
