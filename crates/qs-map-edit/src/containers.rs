// SPDX-License-Identifier: GPL-3.0-or-later

//! Making layers and groups (P7-08, MAP-30), as TrenchBroom's manual's "Groups", "Linked
//! Groups" and "Layers" describe them, written as it writes them (MAP-9): `map.group` puts a
//! selection in a new group, `map.ungroup` takes a group's contents out of it, `map.add_layer`
//! adds a layer, `map.link_duplicate` copies a group as one linked to it, and `map.separate`
//! unlinks a group from the groups linked to it.

use qs_commands::{Command, Outcome};
use qs_map::{ContainerId, Entity, EntityPlace, Item, LinkError, RoomKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::brushes::BrushIndex;
use super::{EntityIndex, MapDocument, MapEdited, edit_map};
use crate::{HasMap, MapEditError};

/// The id of the command that puts a selection in a new group.
pub const MAP_GROUP: &str = "map.group";

/// The id of the command that takes a group's contents out of it.
pub const MAP_UNGROUP: &str = "map.ungroup";

/// The id of the command that adds a layer.
pub const MAP_ADD_LAYER: &str = "map.add_layer";

/// The id of the command that copies a group as one linked to it.
pub const MAP_LINK_DUPLICATE: &str = "map.link_duplicate";

/// The id of the command that unlinks a group from the groups linked to it.
pub const MAP_SEPARATE: &str = "map.separate";

/// The id of the command that moves members of a linked group into a new linked group.
pub const MAP_EXTRACT_LINKED: &str = "map.extract_linked";

/// The parameters of `map.extract_linked`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractLinkedParams {
    /// The linked group's own entity.
    pub group: EntityIndex,
    /// The entities of it to extract.
    pub entities: Vec<EntityIndex>,
}

/// The id of the command that makes world brushes a brush entity.
pub const MAP_MAKE_BRUSH_ENTITY: &str = "map.make_brush_entity";

/// The id of the command that gives a brush entity's brushes back to the world.
pub const MAP_MAKE_STRUCTURAL: &str = "map.make_structural";

/// The parameters of `map.make_brush_entity`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MakeBrushEntityParams {
    /// The new entity's classname, such as `func_door`.
    pub classname: String,
    /// The world's brushes it takes.
    pub brushes: Vec<BrushIndex>,
}

/// The parameters of `map.make_structural`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MakeStructuralParams {
    /// The brush entity.
    pub entity: EntityIndex,
}

/// The parameters of `map.group`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupParams {
    /// The group's name.
    pub name: String,
    /// Whole entities to put in it.
    #[serde(default)]
    pub entities: Vec<EntityIndex>,
    /// Brushes of the world to move into it.
    #[serde(default)]
    pub brushes: Vec<BrushIndex>,
}

/// The parameters of `map.ungroup`, `map.link_duplicate` and `map.separate`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContainerParams {
    /// The group's own entity.
    pub group: EntityIndex,
}

/// The parameters of `map.add_layer`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddLayerParams {
    /// The layer's name.
    pub name: String,
}

impl MapDocument {
    /// The group or layer `entity` is in, as its keys name it (MAP-9.2).
    fn parent_of(
        &mut self,
        entity: EntityIndex,
    ) -> Result<Option<(RoomKind, ContainerId)>, MapEditError> {
        let found = self.entity_mut(entity)?;
        Ok(found
            .layer()
            .map(|id| (RoomKind::Layer, id))
            .or_else(|| found.group().map(|id| (RoomKind::Group, id))))
    }

    /// The entity whose brushes are `container`'s own: the layer's or group's entity, or the
    /// world's for none (MAP-9.1).
    fn holder_of(&self, container: Option<(RoomKind, ContainerId)>) -> EntityIndex {
        container
            .and_then(|container| {
                self.file
                    .entities
                    .iter()
                    .position(|entity| entity.container() == Some(container))
            })
            .map_or(EntityIndex::new(0), EntityIndex::new)
    }

    /// `group`'s id, refused when it is no group's own entity.
    fn group_id(&mut self, group: EntityIndex) -> Result<ContainerId, MapEditError> {
        match self.entity_mut(group)?.container() {
            Some((RoomKind::Group, id)) => Ok(id),
            Some((RoomKind::Layer, _)) | None => Err(MapEditError::NotGroup { entity: group }),
        }
    }

    /// The entities that name the group `id` as theirs (MAP-9.6).
    fn members_of(&self, id: ContainerId) -> Vec<EntityIndex> {
        self.file
            .entities
            .iter()
            .enumerate()
            .filter(|(_, entity)| entity.group() == Some(id) && entity.layer().is_none())
            .map(|(place, _)| EntityIndex::new(place))
            .collect()
    }
}

/// `map.group`.
pub(super) struct Group;

impl<C: HasMap + 'static> Command<C> for Group {
    const ID: &'static str = MAP_GROUP;
    const SUMMARY: &'static str = "Puts entities and brushes of the world in a new group, in \
        the layer or group the first is in, as TrenchBroom writes a group.";
    type Params = GroupParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: GroupParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let (entity, step) = edit_map(context, |document| {
            if params.entities.is_empty() && params.brushes.is_empty() {
                return Err(MapEditError::NoSelection);
            }
            let id = document.file.free_container_id();
            let parent = match params.entities.first() {
                Some(&first) => document.parent_of(first)?,
                None => None,
            };
            let mut group = Entity::container_of(RoomKind::Group, id, &params.name)
                .map_err(|error| document.map_error(error))?;
            group
                .put_in(parent)
                .map_err(|error| document.map_error(error))?;
            let world = EntityIndex::new(0);
            let mut brushes = params.brushes.clone();
            brushes.sort();
            brushes.dedup();
            for &brush in brushes.iter().rev() {
                let moved = document.brush(world, brush)?;
                document.replace_brush(world, brush, Vec::new())?;
                group.items.insert(0, Item::Brush(moved));
            }
            for &entity in &params.entities {
                // Neither the world nor a layer goes in a group (MAP-9.1).
                let layer = matches!(
                    document.entity_mut(entity)?.container(),
                    Some((RoomKind::Layer, _))
                );
                if entity.is_world() || layer {
                    return Err(MapEditError::NotPointEntity { entity });
                }
                document.editable(entity)?;
                let found = document.entity_mut(entity)?;
                let joined = found.put_in(Some((RoomKind::Group, id)));
                joined.map_err(|error| document.map_error(error))?;
            }
            document.file.push_entity(group);
            Ok(EntityIndex::new(
                document.file.entities.len().saturating_sub(1),
            ))
        })?;
        Ok(MapEdited::undoable(entity, step))
    }
}

/// `map.ungroup`.
pub(super) struct Ungroup;

impl<C: HasMap + 'static> Command<C> for Ungroup {
    const ID: &'static str = MAP_UNGROUP;
    const SUMMARY: &'static str = "Takes a group's entities and brushes out of it into the layer \
        or group it is in, or the world's, and removes the group.";
    type Params = ContainerParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: ContainerParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let ((), step) = edit_map(context, |document| {
            let id = document.group_id(params.group)?;
            document.editable(params.group)?;
            let parent = document.parent_of(params.group)?;
            let members = document.members_of(id);
            for &member in &members {
                document.editable(member)?;
            }
            for member in members {
                let found = document.entity_mut(member)?;
                let left = found.put_in(parent);
                left.map_err(|error| document.map_error(error))?;
            }
            let brushes: Vec<Item> = document
                .entity_mut(params.group)?
                .items
                .iter()
                .filter(|item| matches!(item, Item::Brush(_)))
                .cloned()
                .collect();
            let into = document.holder_of(parent);
            document.entity_mut(into)?.items.extend(brushes);
            document.file.entities.remove(params.group.get());
            document.view.deleted(params.group);
            document.selected = None;
            Ok(())
        })?;
        Ok(Outcome::undoable(MapEdited { entity: None }, step))
    }
}

/// `map.add_layer`.
pub(super) struct AddLayer;

impl<C: HasMap + 'static> Command<C> for AddLayer {
    const ID: &'static str = MAP_ADD_LAYER;
    const SUMMARY: &'static str = "Adds an empty layer to the open map, as TrenchBroom writes one.";
    type Params = AddLayerParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: AddLayerParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let (entity, step) = edit_map(context, |document| {
            let id = document.file.free_container_id();
            let layer = Entity::container_of(RoomKind::Layer, id, &params.name)
                .map_err(|error| document.map_error(error))?;
            document.file.push_entity(layer);
            Ok(EntityIndex::new(
                document.file.entities.len().saturating_sub(1),
            ))
        })?;
        Ok(MapEdited::undoable(entity, step))
    }
}

/// `map.link_duplicate`.
pub(super) struct LinkDuplicate;

impl<C: HasMap + 'static> Command<C> for LinkDuplicate {
    const ID: &'static str = MAP_LINK_DUPLICATE;
    const SUMMARY: &'static str = "Copies a group and its entities as a new group linked to it, \
        so an edit in one is made in the other (TrenchBroom's Create Linked Duplicate).";
    type Params = ContainerParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: ContainerParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let (entity, step) = edit_map(context, |document| {
            let id = document.group_id(params.group)?;
            // A linked group holding a group is not kept in step with its copies (MAP-9.5), so
            // one is not made.
            let members = document.members_of(id);
            for &member in &members {
                if matches!(
                    document.entity_mut(member)?.container(),
                    Some((RoomKind::Group, _))
                ) {
                    return Err(MapEditError::Link {
                        error: LinkError::Nested(EntityPlace::new(params.group.get())),
                    });
                }
            }
            let copy_id = document.file.free_container_id();
            let link = match document.entity_mut(params.group)?.link_id() {
                Some(link) => link,
                None => {
                    let link = format!("{LINK_PREFIX}{}", copy_id.get());
                    let source = document.entity_mut(params.group)?;
                    let linked = source.set_link_id(Some(&link));
                    linked.map_err(|error| document.map_error(error))?;
                    link
                }
            };
            let mut copy = document.entity_mut(params.group)?.clone();
            let made = copy
                .set(b"_tb_id", copy_id.get().to_string().as_bytes())
                .and_then(|()| copy.set_link_id(Some(&link)));
            made.map_err(|error| document.map_error(error))?;
            let members: Vec<Entity> = members
                .into_iter()
                .map(|member| document.entity_mut(member).map(|found| found.clone()))
                .collect::<Result<_, _>>()?;
            document.file.push_entity(copy);
            let at = EntityIndex::new(document.file.entities.len().saturating_sub(1));
            for mut member in members {
                let moved = member.put_in(Some((RoomKind::Group, copy_id)));
                moved.map_err(|error| document.map_error(error))?;
                document.file.push_entity(member);
            }
            Ok(at)
        })?;
        Ok(MapEdited::undoable(entity, step))
    }
}

/// `map.separate`.
pub(super) struct Separate;

impl<C: HasMap + 'static> Command<C> for Separate {
    const ID: &'static str = MAP_SEPARATE;
    const SUMMARY: &'static str = "Unlinks a group from the groups linked to it, which then \
        change on their own (TrenchBroom's Separate Linked Groups).";
    type Params = ContainerParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: ContainerParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let ((), step) = edit_map(context, |document| {
            document.group_id(params.group)?;
            let group = document.entity_mut(params.group)?;
            if group.link_id().is_none() {
                return Err(MapEditError::NotGroup {
                    entity: params.group,
                });
            }
            let unlinked = group.set_link_id(None);
            unlinked.map_err(|error| document.map_error(error))
        })?;
        Ok(MapEdited::undoable(params.group, step))
    }
}

/// `map.make_brush_entity`.
pub(super) struct MakeBrushEntity;

impl<C: HasMap + 'static> Command<C> for MakeBrushEntity {
    const ID: &'static str = MAP_MAKE_BRUSH_ENTITY;
    const SUMMARY: &'static str = "Makes brushes of the world a new brush entity of a class, \
        such as func_door (TrenchBroom's Create Brush Entity).";
    type Params = MakeBrushEntityParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: MakeBrushEntityParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let (entity, step) = edit_map(context, |document| {
            if params.brushes.is_empty() {
                return Err(MapEditError::NoSelection);
            }
            let mut made = Entity::new();
            made.set(b"classname", params.classname.as_bytes())
                .map_err(|error| document.map_error(error))?;
            let world = EntityIndex::new(0);
            let mut brushes = params.brushes.clone();
            brushes.sort();
            brushes.dedup();
            for &brush in brushes.iter().rev() {
                let moved = document.brush(world, brush)?;
                document.replace_brush(world, brush, Vec::new())?;
                made.items.insert(1, Item::Brush(moved));
            }
            document.file.push_entity(made);
            Ok(EntityIndex::new(
                document.file.entities.len().saturating_sub(1),
            ))
        })?;
        Ok(MapEdited::undoable(entity, step))
    }
}

/// `map.make_structural`.
pub(super) struct MakeStructural;

impl<C: HasMap + 'static> Command<C> for MakeStructural {
    const ID: &'static str = MAP_MAKE_STRUCTURAL;
    const SUMMARY: &'static str = "Gives a brush entity's brushes back to the world, in the \
        layer or group the entity was in, and removes the entity (TrenchBroom's Make \
        Structural).";
    type Params = MakeStructuralParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: MakeStructuralParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let ((), step) = edit_map(context, |document| {
            let entity = params.entity;
            let found = document.entity_mut(entity)?;
            if entity.is_world() || !found.has_brushes() || found.container().is_some() {
                return Err(MapEditError::NotBrushEntity { entity });
            }
            document.editable(entity)?;
            let brushes: Vec<Item> = document
                .entity_mut(entity)?
                .items
                .iter()
                .filter(|item| matches!(item, Item::Brush(_)))
                .cloned()
                .collect();
            // The brushes stay in the layer or group the entity was in.
            let parent = document.parent_of(entity)?;
            let into = document.holder_of(parent);
            document.entity_mut(into)?.items.extend(brushes);
            document.file.entities.remove(entity.get());
            document.view.deleted(entity);
            document.selected = None;
            Ok(())
        })?;
        Ok(Outcome::undoable(MapEdited { entity: None }, step))
    }
}

/// `map.extract_linked`.
pub(super) struct ExtractLinked;

impl<C: HasMap + 'static> Command<C> for ExtractLinked {
    const ID: &'static str = MAP_EXTRACT_LINKED;
    const SUMMARY: &'static str = "Moves entities of a linked group, and the same entities of \
        every group linked to it, into new groups linked to each other (TrenchBroom's Extract \
        Linked Groups).";
    type Params = ExtractLinkedParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: ExtractLinkedParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let (entity, step) = edit_map(context, |document| {
            let id = document.group_id(params.group)?;
            let link =
                document
                    .entity_mut(params.group)?
                    .link_id()
                    .ok_or(MapEditError::NotGroup {
                        entity: params.group,
                    })?;
            let members = document.members_of(id);
            let places: Vec<usize> = params
                .entities
                .iter()
                .filter_map(|entity| members.iter().position(|m| m == entity))
                .collect();
            if places.is_empty() || places.len() == members.len() {
                return Err(MapEditError::NoSelection);
            }
            let linked: Vec<EntityIndex> = document
                .file
                .entities
                .iter()
                .enumerate()
                .filter(|(_, entity)| {
                    matches!(entity.container(), Some((RoomKind::Group, _)))
                        && entity.link_id().as_deref() == Some(link.as_str())
                })
                .map(|(place, _)| EntityIndex::new(place))
                .collect();
            let new_link = format!("{LINK_PREFIX}{}", document.file.free_container_id().get());
            let mut first = None;
            for group in linked {
                let group_id = document.group_id(group)?;
                let parent = document.parent_of(group)?;
                let name = document
                    .entity_mut(group)?
                    .text(b"_tb_name")
                    .unwrap_or_default();
                let new_id = document.file.free_container_id();
                let made = Entity::container_of(RoomKind::Group, new_id, &name)
                    .and_then(|mut made| {
                        made.put_in(parent)?;
                        made.set_link_id(Some(&new_link))?;
                        Ok(made)
                    })
                    .map_err(|error| document.map_error(error))?;
                let theirs = document.members_of(group_id);
                for &place in &places {
                    let Some(&member) = theirs.get(place) else {
                        continue;
                    };
                    let found = document.entity_mut(member)?;
                    let moved = found.put_in(Some((RoomKind::Group, new_id)));
                    moved.map_err(|error| document.map_error(error))?;
                }
                document.file.push_entity(made);
                first.get_or_insert_with(|| {
                    EntityIndex::new(document.file.entities.len().saturating_sub(1))
                });
            }
            first.ok_or(MapEditError::NoSelection)
        })?;
        Ok(MapEdited::undoable(entity, step))
    }
}

/// What a link id this editor makes starts with, after the new group's id.
const LINK_PREFIX: &str = "qs-link-";

#[cfg(test)]
mod tests;
