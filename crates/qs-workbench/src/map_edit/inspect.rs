// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.inspect` (P5-08, MAP-16): the open map as the agent reads it before it edits: its rooms,
//! TrenchBroom's named layers and groups with their bounds (MAP-9), and each entity's place,
//! classname, origin and room, so a prompt that names a room ("three Shamblers in the armory")
//! can be carried out with `map.place` and `map.set_key`.

use qs_commands::{Command, Outcome};
use qs_map::RoomKind;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{EntityIndex, MAP_INSPECT};
use crate::{HasWorkbench, WorkbenchError};

/// The parameters of `map.inspect`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectParams {
    /// Only the entities in the room of this name, in any case, or a room inside it; every
    /// entity with none. A name no room has is refused.
    #[serde(default)]
    pub room: Option<String>,
    /// Only the entities whose classname starts with this, as written; every entity with none.
    #[serde(default)]
    pub classname: Option<String>,
}

/// What a room is in TrenchBroom (MAP-9.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoomShape {
    /// A layer.
    Layer,
    /// A group.
    Group,
}

impl From<RoomKind> for RoomShape {
    fn from(kind: RoomKind) -> Self {
        match kind {
            RoomKind::Layer => Self::Layer,
            RoomKind::Group => Self::Group,
        }
    }
}

/// A room of the map, as `map.inspect` answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomInfo {
    /// Its name.
    pub name: String,
    /// A layer or a group.
    pub kind: RoomShape,
    /// The box around its brushes, `[min, max]`; none for a room with no brush.
    pub bounds: Option<[[f64; 3]; 2]>,
}

/// An entity of the map, as `map.inspect` answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EntityInfo {
    /// Its place in the map, which the other `map.*` commands take.
    pub entity: EntityIndex,
    /// Its classname, empty for none.
    pub classname: String,
    /// Its origin, when it has one.
    pub origin: Option<[f64; 3]>,
    /// Its targetname, when it has one.
    pub targetname: Option<String>,
    /// The innermost room it is in, when it is in one (MAP-9.4).
    pub room: Option<String>,
}

/// What `map.inspect` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapInspected {
    /// The map's file.
    pub path: std::path::PathBuf,
    /// Its rooms, in the map's order.
    pub rooms: Vec<RoomInfo>,
    /// Its entities, the world first, those `room` and `classname` leave.
    pub entities: Vec<EntityInfo>,
}

/// `map.inspect`.
pub(super) struct MapInspect;

impl<C: HasWorkbench + 'static> Command<C> for MapInspect {
    const ID: &'static str = MAP_INSPECT;
    const SUMMARY: &'static str = "The open map's rooms (TrenchBroom's named layers and groups, \
        with their bounds) and its entities: each one's place, classname, origin, targetname \
        and room, filtered by room or classname.";
    type Params = InspectParams;
    type Output = MapInspected;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: InspectParams,
    ) -> Result<Outcome<MapInspected, C>, WorkbenchError> {
        let document = context.workbench().map().ok_or(WorkbenchError::NoMap)?;
        let file = document.file();
        let rooms = file.rooms();
        if let Some(room) = &params.room
            && !rooms.named_any(room)
        {
            return Err(WorkbenchError::NoRoom { name: room.clone() });
        }
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let entities = file
            .entities
            .iter()
            .enumerate()
            .filter(|(_, entity)| {
                params.classname.as_ref().is_none_or(|prefix| {
                    entity
                        .classname()
                        .is_some_and(|name| name.starts_with(prefix.as_bytes()))
                })
            })
            .filter(|(_, entity)| {
                params
                    .room
                    .as_ref()
                    .is_none_or(|room| rooms.holds(entity, room))
            })
            .map(|(index, entity)| EntityInfo {
                entity: EntityIndex::new(index),
                classname: entity.classname().map(text).unwrap_or_default(),
                origin: entity.value(b"origin").map(|_| entity.origin()),
                targetname: entity.value(b"targetname").map(text),
                room: rooms.of(entity).first().map(|room| room.name.clone()),
            })
            .collect();
        let rooms = rooms
            .list()
            .iter()
            .map(|room| RoomInfo {
                name: room.name.clone(),
                kind: room.kind.into(),
                bounds: room.bounds,
            })
            .collect();
        Ok(Outcome::unchanged(MapInspected {
            path: document.path().to_path_buf(),
            rooms,
            entities,
        }))
    }
}

#[cfg(test)]
mod tests;
