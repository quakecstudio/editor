// SPDX-License-Identifier: GPL-3.0-or-later

//! The map outline (P5-16, MAP-31): every layer, group and entity of the open map in a tree as
//! TrenchBroom's map inspector shows them: the default layer, holding the world and what no
//! layer names, then each layer; in each, its groups, then its entities by classname and
//! targetname, each with its brush count. `map.outline` filters it by text; `map.move_to` moves
//! an entity or a group into another layer or group, as dragging a row does.

use std::collections::{BTreeMap, BTreeSet};

use qs_commands::{Command, Outcome};
use qs_map::{ContainerId, Entity, MapFile, RoomKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{EntityIndex, MapDocument, MapEdited, edit_map};
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.outline`.
pub const MAP_OUTLINE: &str = "map.outline";
/// The id of `map.move_to`.
pub const MAP_MOVE_TO: &str = "map.move_to";

/// The key holding a layer's or group's name (MAP-9.1).
const NAME_KEY: &[u8] = b"_tb_name";
/// The key naming the group an entity is in (MAP-9.2).
const GROUP_KEY: &[u8] = b"_tb_group";
/// The key naming the layer an entity is in (MAP-9.2).
const LAYER_KEY: &[u8] = b"_tb_layer";
/// The key holding a group's link id (MAP-9.5).
const LINK_KEY: &[u8] = b"_tb_linked_group_id";
/// The key an entity is fired by.
const TARGETNAME_KEY: &[u8] = b"targetname";
/// The key holding an entity's class.
const CLASSNAME_KEY: &[u8] = b"classname";

/// The parameters of `map.outline`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutlineParams {
    /// Keep only what holds this text, ignoring case: a classname, a key, a value or a name;
    /// the layers and groups around a match stay.
    #[serde(default)]
    pub filter: Option<String>,
}

/// What a row of the outline is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    /// A layer, or the default layer, which is the world.
    Layer,
    /// A group.
    Group,
    /// A group linked to others (MAP-9.5).
    LinkedGroup,
    /// An entity.
    Entity,
}

/// One row of the outline and the rows under it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OutlineRow {
    /// The entity the row is: the world for the default layer.
    pub entity: EntityIndex,
    /// What it is.
    pub kind: RowKind,
    /// A layer's or group's name, an entity's classname.
    pub label: String,
    /// An entity's targetname.
    pub targetname: Option<String>,
    /// The brushes of the entity itself.
    pub brushes: usize,
    /// The rows under it: groups, then entities.
    pub rows: Vec<OutlineRow>,
}

impl OutlineRow {
    /// How many rows this row and those under it hold.
    #[must_use]
    pub fn count(&self) -> usize {
        self.rows
            .iter()
            .map(Self::count)
            .fold(1, usize::saturating_add)
    }
}

/// The parameters of `map.move_to`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveToParams {
    /// The entity or group to move.
    pub entity: EntityIndex,
    /// The layer or group to move it into; the world for the default layer.
    pub into: EntityIndex,
}

/// The text of `key` on `entity`.
fn text(entity: &Entity, key: &[u8]) -> Option<String> {
    entity
        .value(key)
        .map(|value| String::from_utf8_lossy(value).into_owned())
}

/// The map's containers by kind and id, and the place of each.
struct Containers {
    places: BTreeMap<(RoomKindKey, ContainerId), usize>,
}

/// [`RoomKind`] as a key that orders.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RoomKindKey {
    Layer,
    Group,
}

impl From<RoomKind> for RoomKindKey {
    fn from(kind: RoomKind) -> Self {
        match kind {
            RoomKind::Layer => Self::Layer,
            RoomKind::Group => Self::Group,
        }
    }
}

impl Containers {
    fn of(file: &MapFile) -> Self {
        let places = file
            .entities
            .iter()
            .enumerate()
            .filter_map(|(place, entity)| {
                entity
                    .container()
                    .map(|(kind, id)| ((kind.into(), id), place))
            })
            .collect();
        Self { places }
    }

    /// The container `entity` is directly in: its group when that group exists, else its layer
    /// when that layer exists (MAP-9.2), else the default layer, the world, at 0.
    fn parent(&self, entity: &Entity) -> usize {
        let group = entity
            .group()
            .and_then(|id| self.places.get(&(RoomKindKey::Group, id)));
        let layer = entity
            .layer()
            .and_then(|id| self.places.get(&(RoomKindKey::Layer, id)));
        group.or(layer).copied().unwrap_or(0)
    }
}

impl MapDocument {
    /// The outline of the open map, filtered by `filter` (MAP-31).
    #[must_use]
    pub fn outline(&self, filter: Option<&str>) -> Vec<OutlineRow> {
        let file = &self.file;
        let containers = Containers::of(file);
        let mut children: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for (place, entity) in file.entities.iter().enumerate().skip(1) {
            // A layer is a top row; it stands in no container.
            if matches!(entity.container(), Some((RoomKind::Layer, _))) {
                continue;
            }
            children
                .entry(containers.parent(entity))
                .or_default()
                .push(place);
        }
        let filter = filter
            .map(str::to_lowercase)
            .filter(|filter| !filter.is_empty());
        let mut seen = BTreeSet::new();
        let mut layers = vec![0];
        layers.extend(
            file.entities
                .iter()
                .enumerate()
                .filter(|(_, e)| matches!(e.container(), Some((RoomKind::Layer, _))))
                .map(|(place, _)| place),
        );
        layers
            .into_iter()
            .filter_map(|place| self.row(place, &children, filter.as_deref(), &mut seen))
            .collect()
    }

    /// The row of the entity at `place` and those under it, or none when `filter` keeps
    /// nothing of it; `seen` stops a loop of groups naming each other.
    fn row(
        &self,
        place: usize,
        children: &BTreeMap<usize, Vec<usize>>,
        filter: Option<&str>,
        seen: &mut BTreeSet<usize>,
    ) -> Option<OutlineRow> {
        if !seen.insert(place) {
            return None;
        }
        let entity = self.file.entities.get(place)?;
        let (kind, label) = match entity.container() {
            Some((RoomKind::Layer, _)) => (RowKind::Layer, text(entity, NAME_KEY)),
            Some((RoomKind::Group, _)) if entity.value(LINK_KEY).is_some() => {
                (RowKind::LinkedGroup, text(entity, NAME_KEY))
            }
            Some((RoomKind::Group, _)) => (RowKind::Group, text(entity, NAME_KEY)),
            None if place == 0 => (RowKind::Layer, None),
            None => (RowKind::Entity, text(entity, CLASSNAME_KEY)),
        };
        let mut under: Vec<OutlineRow> = children
            .get(&place)
            .into_iter()
            .flatten()
            .filter_map(|&child| self.row(child, children, filter, seen))
            .collect();
        under.sort_by(|a, b| {
            let rank = |row: &OutlineRow| row.kind == RowKind::Entity;
            rank(a)
                .cmp(&rank(b))
                .then_with(|| a.label.cmp(&b.label))
                .then_with(|| a.targetname.cmp(&b.targetname))
                .then_with(|| a.entity.cmp(&b.entity))
        });
        let row = OutlineRow {
            entity: EntityIndex::new(place),
            kind,
            label: label.unwrap_or_default(),
            targetname: text(entity, TARGETNAME_KEY),
            brushes: entity.brushes().count(),
            rows: under,
        };
        let keep = match filter {
            None => true,
            Some(filter) => !row.rows.is_empty() || matches(entity, filter),
        };
        keep.then_some(row)
    }

    /// Moves the entity or group at `entity` into the layer or group at `into`, the world for
    /// the default layer.
    fn move_to(&mut self, entity: EntityIndex, into: EntityIndex) -> Result<(), WorkbenchError> {
        if entity.is_world() || entity == into {
            return Err(WorkbenchError::NotContainer { entity: into });
        }
        self.entity_mut(entity)?;
        let target = self.entity_mut(into)?.container();
        let containers = Containers::of(&self.file);
        // A group cannot go into itself or a group inside it.
        let mut at = into.get();
        for _ in 0..self.file.entities.len() {
            if at == entity.get() {
                return Err(WorkbenchError::NotContainer { entity: into });
            }
            let Some(next) = self.file.entities.get(at) else {
                break;
            };
            let parent = containers.parent(next);
            if parent == at || parent == 0 {
                break;
            }
            at = parent;
        }
        let moved = self.entity_mut(entity)?;
        let set = match (target, into.is_world()) {
            (Some((RoomKind::Group, id)), _) => {
                moved.remove(LAYER_KEY);
                moved.set(GROUP_KEY, id.get().to_string().as_bytes())
            }
            (Some((RoomKind::Layer, id)), _) => {
                moved.remove(GROUP_KEY);
                moved.set(LAYER_KEY, id.get().to_string().as_bytes())
            }
            (None, true) => {
                moved.remove(GROUP_KEY);
                moved.remove(LAYER_KEY);
                Ok(())
            }
            (None, false) => return Err(WorkbenchError::NotContainer { entity: into }),
        };
        set.map_err(|error| self.map_error(error))
    }
}

/// Whether `entity`'s keys or values hold `filter`, already lower case.
fn matches(entity: &Entity, filter: &str) -> bool {
    entity.pairs().any(|pair| {
        [pair.key(), pair.value()].into_iter().any(|bytes| {
            String::from_utf8_lossy(bytes)
                .to_lowercase()
                .contains(filter)
        })
    })
}

/// `map.outline`.
pub(super) struct MapOutline;

impl<C: HasWorkbench + 'static> Command<C> for MapOutline {
    const ID: &'static str = MAP_OUTLINE;
    const SUMMARY: &'static str = "The open map's outline: the default layer and each layer, \
        their groups and linked groups, then their entities by classname and targetname, with \
        brush counts; with filter, only what holds the text and what is around it.";
    type Params = OutlineParams;
    type Output = Vec<OutlineRow>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: OutlineParams,
    ) -> Result<Outcome<Vec<OutlineRow>, C>, WorkbenchError> {
        let document = context.workbench().map().ok_or(WorkbenchError::NoMap)?;
        Ok(Outcome::unchanged(
            document.outline(params.filter.as_deref()),
        ))
    }
}

/// `map.move_to`.
pub(super) struct MapMoveTo;

impl<C: HasWorkbench + 'static> Command<C> for MapMoveTo {
    const ID: &'static str = MAP_MOVE_TO;
    const SUMMARY: &'static str = "Moves an entity or a group of the open map into a layer or \
        a group, or into the default layer given the world, as TrenchBroom writes it.";
    type Params = MoveToParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: MoveToParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            document.move_to(params.entity, params.into)
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

#[cfg(test)]
mod tests;
