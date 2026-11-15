// SPDX-License-Identifier: GPL-3.0-or-later

//! The open map's objects as a query sees them, and the search that selects what a query
//! finds. The world is entity 0; a layer is a layer's own entity, or the default layer that
//! holds what no layer names; a group is a group's own entity; every other entity is an
//! entity; the brushes of the world, a layer or a group belong to the world, those of a brush
//! entity to it. What is found is selected as TrenchBroom's search selects it: a layer by what
//! it holds, never the world, and nothing hidden or locked (the manual's "Searching").

use std::collections::{BTreeSet, HashMap};

use qs_map::{Brush, ContainerId, Entity, Face, RoomKind};

use super::Query;
use super::domain::{Field, Fields, ObjectType};
use super::value::{Pattern, Value};
use crate::issues::{bounds, bounds_of};
use crate::uv::{FaceIndex, FaceRef};
use crate::{BrushIndex, EntityIndex, MapDocument, MapFound};

/// The default layer's name, as TrenchBroom names it ("Every map contains a \"Default Layer\"",
/// its manual's "Layers").
const DEFAULT_LAYER: &str = "Default Layer";

/// The box of a point entity whose class gives none: 16 units across, as the map view draws
/// one.
const DEFAULT_BOX: [[f64; 3]; 2] = [[-8.0; 3], [8.0; 3]];

/// The keys that put an entity in a layer or a group, which TrenchBroom keeps as the map's
/// structure rather than as the entity's properties.
const MEMBERSHIP_KEYS: [&[u8]; 2] = [b"_tb_layer", b"_tb_group"];

/// The key naming a layer or a group.
const NAME_KEY: &[u8] = b"_tb_name";

/// TrenchBroom's Quake smart tags on brushes, each matching the classname of the brush's
/// entity (its `games/Quake/GameConfig.cfg`, `tags.brush`).
const BRUSH_TAGS: [(&str, &str); 3] = [
    ("Detail", "func_detail*"),
    ("Trigger", "trigger*"),
    ("Func", "func*"),
];

/// Its Quake smart tags on faces, each matching the face's texture (`tags.brushface`).
const FACE_TAGS: [(&str, &str); 4] = [
    ("Clip", "clip"),
    ("Skip", "skip"),
    ("Hint", "hint*"),
    ("Liquid", "\\**"),
];

/// Each point entity class's box, by classname, from the project's definitions.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PointSizes(HashMap<String, [[f64; 3]; 2]>);

impl PointSizes {
    /// The boxes given.
    pub(crate) const fn new(sizes: HashMap<String, [[f64; 3]; 2]>) -> Self {
        Self(sizes)
    }

    /// The box of a point entity of `classname` at `origin`.
    fn around(&self, classname: &str, origin: [f64; 3]) -> [[f64; 3]; 2] {
        let [min, max] = self.0.get(classname).copied().unwrap_or(DEFAULT_BOX);
        let mut placed = [min, max];
        for corner in &mut placed {
            for (c, o) in corner.iter_mut().zip(origin) {
                *c += o;
            }
        }
        placed
    }
}

/// A layer: one's own entity, or the default layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    Default,
    Own(EntityIndex),
}

/// An object a query is evaluated for; the world never is, since it is never selected.
#[derive(Clone, Copy)]
enum Object<'a> {
    Layer(Layer),
    Group(EntityIndex),
    Entity(EntityIndex),
    Brush(EntityIndex, &'a Brush),
    Face(FaceRef, &'a Face, Option<[[f64; 3]; 2]>),
}

/// The open map, read once for a search.
struct Model<'a> {
    document: &'a MapDocument,
    /// Each entity's layer, through its groups.
    layers: Vec<Option<ContainerId>>,
    /// Each group's own entity, by its id.
    groups: HashMap<ContainerId, EntityIndex>,
    /// Each layer's own entity, by its id.
    layer_entities: HashMap<ContainerId, EntityIndex>,
    /// Each entity's box: a point entity's, a brush entity's brushes', a layer's or group's
    /// whole content's.
    boxes: Vec<Option<[[f64; 3]; 2]>>,
    /// The default layer's box.
    default_box: Option<[[f64; 3]; 2]>,
}

/// The box holding both.
fn union(a: Option<[[f64; 3]; 2]>, b: Option<[[f64; 3]; 2]>) -> Option<[[f64; 3]; 2]> {
    bounds(a.into_iter().chain(b).flatten())
}

impl<'a> Model<'a> {
    fn new(document: &'a MapDocument, sizes: &PointSizes) -> Self {
        let entities = &document.file().entities;
        let of_kind = |kind: RoomKind| -> HashMap<ContainerId, EntityIndex> {
            entities
                .iter()
                .enumerate()
                .filter_map(|(place, e)| match e.container() {
                    Some((k, id)) if k == kind => Some((id, EntityIndex::new(place))),
                    _ => None,
                })
                .collect()
        };
        let own: Vec<Option<[[f64; 3]; 2]>> = entities
            .iter()
            .enumerate()
            .map(|(place, entity)| {
                let brushes = bounds(entity.brushes().filter_map(bounds_of).flatten());
                let point = place != 0 && entity.container().is_none() && !entity.has_brushes();
                if point {
                    let classname = Self::classname(entity).unwrap_or_default();
                    Some(sizes.around(&classname, entity.origin()))
                } else {
                    brushes
                }
            })
            .collect();
        let mut model = Self {
            document,
            layers: document.file().layer_of_each(),
            groups: of_kind(RoomKind::Group),
            layer_entities: of_kind(RoomKind::Layer),
            boxes: own.clone(),
            default_box: None,
        };
        // Each entity's box grows its groups', outward, and its layer's.
        for (place, entity_box) in own.iter().enumerate().skip(1) {
            let mut at = EntityIndex::new(place);
            for _ in 0..entities.len() {
                let Some(group) = model.group_of(at) else {
                    break;
                };
                if let Some(slot) = model.boxes.get_mut(group.get()) {
                    *slot = union(*slot, *entity_box);
                }
                at = group;
            }
            match model.layer_entity(EntityIndex::new(place)) {
                Some(layer) if layer.get() != place => {
                    if let Some(slot) = model.boxes.get_mut(layer.get()) {
                        *slot = union(*slot, *entity_box);
                    }
                }
                Some(_) => {}
                None => model.default_box = union(model.default_box, *entity_box),
            }
        }
        model.default_box = union(model.default_box, own.first().copied().flatten());
        model
    }

    fn entity(&self, at: EntityIndex) -> Option<&'a Entity> {
        self.document.file().entities.get(at.get())
    }

    fn classname(entity: &Entity) -> Option<String> {
        entity
            .classname()
            .map(|c| String::from_utf8_lossy(c).into_owned())
    }

    /// The group `at` is directly in.
    fn group_of(&self, at: EntityIndex) -> Option<EntityIndex> {
        let id = self.entity(at)?.group()?;
        self.groups.get(&id).copied()
    }

    /// The layer's own entity holding `at`, through its groups; `at` itself for a layer.
    fn layer_entity(&self, at: EntityIndex) -> Option<EntityIndex> {
        if let Some((RoomKind::Layer, _)) = self.entity(at)?.container() {
            return Some(at);
        }
        let id = (*self.layers.get(at.get())?)?;
        self.layer_entities.get(&id).copied()
    }

    fn name_of(&self, at: EntityIndex) -> Value {
        self.entity(at)
            .and_then(|e| e.text(NAME_KEY))
            .map_or(Value::Undefined, Value::Text)
    }

    fn layer_name(&self, at: EntityIndex) -> Value {
        match self.layer_entity(at) {
            Some(layer) => self.name_of(layer),
            None => Value::Text(DEFAULT_LAYER.to_owned()),
        }
    }

    /// The name of the innermost group holding what entity `at` holds: itself when it is a
    /// group holding brushes, else the group it is in.
    fn group_name_holding(&self, at: EntityIndex) -> Value {
        match self.entity(at).and_then(Entity::container) {
            Some((RoomKind::Group, _)) => self.name_of(at),
            Some((RoomKind::Layer, _)) => Value::Undefined,
            None => self
                .group_of(at)
                .map_or(Value::Undefined, |group| self.name_of(group)),
        }
    }

    fn properties(entity: &Entity) -> Value {
        let mut pairs: Vec<(String, Value)> = Vec::new();
        for pair in entity.pairs() {
            if MEMBERSHIP_KEYS.contains(&pair.key()) {
                continue;
            }
            let key = String::from_utf8_lossy(pair.key()).into_owned();
            if pairs.iter().any(|(k, _)| *k == key) {
                continue;
            }
            let value = String::from_utf8_lossy(pair.value()).into_owned();
            pairs.push((key, Value::Text(value)));
        }
        Value::Map(pairs)
    }

    /// The entity a brush of entity `at` belongs to: a brush entity itself, else the world.
    fn owner(&self, at: EntityIndex) -> EntityIndex {
        match self.entity(at).and_then(Entity::container) {
            Some(_) => EntityIndex::new(0),
            None => at,
        }
    }

    fn entity_value(&self, at: EntityIndex) -> Value {
        let Some(entity) = self.entity(self.owner(at)) else {
            return Value::Undefined;
        };
        Value::Map(vec![
            (
                Field::Classname.name().to_owned(),
                Self::classname(entity).map_or(Value::Undefined, Value::Text),
            ),
            (
                Field::Properties.name().to_owned(),
                Self::properties(entity),
            ),
        ])
    }

    fn brush_tags(&self, at: EntityIndex) -> Value {
        let classname = self
            .entity(self.owner(at))
            .and_then(Self::classname)
            .unwrap_or_default();
        Value::Array(
            BRUSH_TAGS
                .iter()
                .filter(|(_, pattern)| Pattern::new(pattern).matches(&classname))
                .map(|(name, _)| Value::Text((*name).to_owned()))
                .collect(),
        )
    }

    fn visible(&self, at: EntityIndex) -> Value {
        Value::Bool(!self.document.is_hidden(at))
    }

    fn locked(&self, at: EntityIndex) -> Value {
        Value::Bool(self.document.is_locked(at))
    }
}

/// A box's value and its center's.
fn box_values(found: Option<[[f64; 3]; 2]>, field: Field) -> Value {
    let Some([min, max]) = found else {
        return Value::Undefined;
    };
    match field {
        Field::Center => {
            let mut center = min;
            for (c, hi) in center.iter_mut().zip(max) {
                *c = f64::midpoint(*c, hi);
            }
            Value::Vector(center)
        }
        _ => Value::Bounds([min, max]),
    }
}

/// An object and the map it is in: its fields.
struct Seen<'m, 'a> {
    model: &'m Model<'a>,
    object: Object<'a>,
}

impl Fields for Seen<'_, '_> {
    fn field(&self, field: Field) -> Value {
        let model = self.model;
        let text = |s: &str| Value::Text(s.to_owned());
        match (self.object, field) {
            (Object::Layer(_), Field::Type) => text(ObjectType::Layer.name()),
            (Object::Group(_), Field::Type) => text(ObjectType::Group.name()),
            (Object::Entity(_), Field::Type) => text(ObjectType::Entity.name()),
            (Object::Brush(..), Field::Type) => text(ObjectType::Brush.name()),
            (Object::Face(..), Field::Type) => text(ObjectType::Face.name()),
            (Object::Entity(at), Field::Classname | Field::Properties) => {
                let Some(entity) = model.entity(at) else {
                    return Value::Undefined;
                };
                if field == Field::Classname {
                    Model::classname(entity).map_or(Value::Undefined, Value::Text)
                } else {
                    Model::properties(entity)
                }
            }
            (Object::Entity(_), Field::Tags) => Value::Array(Vec::new()),
            (Object::Layer(Layer::Default), Field::Visible) => Value::Bool(true),
            (Object::Layer(Layer::Default), Field::Locked) => Value::Bool(false),
            (Object::Layer(Layer::Default), Field::Name | Field::LayerName) => text(DEFAULT_LAYER),
            (Object::Layer(Layer::Default), Field::Bounds | Field::Center) => {
                box_values(model.default_box, field)
            }
            (Object::Layer(Layer::Own(at)) | Object::Group(at), Field::Name) => model.name_of(at),
            (Object::Layer(Layer::Own(at)) | Object::Group(at) | Object::Entity(at), _) => {
                match field {
                    Field::Bounds | Field::Center => {
                        box_values(model.boxes.get(at.get()).copied().flatten(), field)
                    }
                    Field::LayerName => model.layer_name(at),
                    Field::GroupName => match self.object {
                        Object::Layer(_) => Value::Undefined,
                        _ => model
                            .group_of(at)
                            .map_or(Value::Undefined, |group| model.name_of(group)),
                    },
                    Field::Visible => model.visible(at),
                    Field::Locked => model.locked(at),
                    _ => Value::Undefined,
                }
            }
            (Object::Brush(at, brush), _) => match field {
                Field::Entity => model.entity_value(at),
                Field::Materials => {
                    let mut seen = BTreeSet::new();
                    Value::Array(
                        brush
                            .faces
                            .iter()
                            .map(|face| String::from_utf8_lossy(face.texture()).into_owned())
                            .filter(|name| seen.insert(name.clone()))
                            .map(Value::Text)
                            .collect(),
                    )
                }
                Field::Tags => model.brush_tags(at),
                Field::Bounds | Field::Center => box_values(bounds_of(brush), field),
                Field::LayerName => model.layer_name(at),
                Field::GroupName => model.group_name_holding(at),
                Field::Visible => model.visible(at),
                Field::Locked => model.locked(at),
                _ => Value::Undefined,
            },
            (Object::Face(at, face, polygon), _) => {
                let material = String::from_utf8_lossy(face.texture()).into_owned();
                match field {
                    Field::Entity => model.entity_value(at.entity),
                    Field::Material => Value::Text(material),
                    Field::Normal => face
                        .plane()
                        .map_or(Value::Undefined, |plane| Value::Vector(plane.normal)),
                    Field::Tags => Value::Array(
                        FACE_TAGS
                            .iter()
                            .filter(|(_, pattern)| Pattern::new(pattern).matches(&material))
                            .map(|(name, _)| Value::Text((*name).to_owned()))
                            .collect(),
                    ),
                    Field::Bounds | Field::Center => box_values(polygon, field),
                    Field::LayerName => model.layer_name(at.entity),
                    Field::GroupName => model.group_name_holding(at.entity),
                    Field::Visible => model.visible(at.entity),
                    Field::Locked => model.locked(at.entity),
                    _ => Value::Undefined,
                }
            }
            _ => Value::Undefined,
        }
    }
}

impl MapDocument {
    /// What `query` finds in the map, selected as TrenchBroom's search selects it.
    pub(crate) fn query(&self, query: &Query, sizes: &PointSizes) -> MapFound {
        let model = Model::new(self, sizes);
        let mut found = MapFound::default();
        let entities = &self.file().entities;
        let selectable = |at: EntityIndex| !self.is_hidden(at) && !self.is_locked(at);
        for (ty, expr) in query.domain() {
            let holds = |object: Object<'_>| {
                expr.holds(&Seen {
                    model: &model,
                    object,
                })
            };
            match ty {
                ObjectType::World => {}
                ObjectType::Layer => {
                    let mut layers = vec![Layer::Default];
                    layers.extend(
                        entities
                            .iter()
                            .enumerate()
                            .filter(|(_, e)| matches!(e.container(), Some((RoomKind::Layer, _))))
                            .map(|(place, _)| Layer::Own(EntityIndex::new(place))),
                    );
                    for layer in layers {
                        if holds(Object::Layer(layer)) {
                            self.select_layer(&model, layer, &mut found);
                        }
                    }
                }
                ObjectType::Group | ObjectType::Entity => {
                    for (place, entity) in entities.iter().enumerate().skip(1) {
                        let at = EntityIndex::new(place);
                        let object = match entity.container() {
                            Some((RoomKind::Group, _)) => Object::Group(at),
                            Some((RoomKind::Layer, _)) => continue,
                            None => Object::Entity(at),
                        };
                        let of_type = matches!(
                            (ty, object),
                            (ObjectType::Group, Object::Group(_))
                                | (ObjectType::Entity, Object::Entity(_))
                        );
                        if of_type && selectable(at) && holds(object) {
                            found.entities.push(at);
                        }
                    }
                }
                ObjectType::Brush | ObjectType::Face => {
                    for (place, entity) in entities.iter().enumerate() {
                        let at = EntityIndex::new(place);
                        if !selectable(at) {
                            continue;
                        }
                        for (b, brush) in entity.brushes().enumerate() {
                            let index = BrushIndex::new(b);
                            if ty == ObjectType::Brush {
                                if holds(Object::Brush(at, brush)) {
                                    found.brushes.push((at, index));
                                }
                                continue;
                            }
                            Self::faces(&holds, at, index, brush, &mut found.faces);
                        }
                    }
                }
            }
        }
        found.entities.sort_unstable();
        found.entities.dedup();
        found.brushes.sort_unstable();
        found.brushes.dedup();
        found
    }

    /// The faces of `brush` that `holds`.
    fn faces<'a>(
        holds: &dyn Fn(Object<'a>) -> bool,
        at: EntityIndex,
        index: BrushIndex,
        brush: &'a Brush,
        into: &mut Vec<FaceRef>,
    ) {
        // Each face's polygon is its own only when every face has a plane.
        let polygons = brush.polygons();
        let aligned = brush.faces.iter().all(|face| face.plane().is_some())
            && polygons.len() == brush.faces.len();
        for (f, face) in brush.faces.iter().enumerate() {
            let polygon = aligned
                .then(|| polygons.get(f).and_then(|p| bounds(p.iter().copied())))
                .flatten();
            let face_ref = FaceRef {
                entity: at,
                brush: index,
                face: FaceIndex::new(f),
            };
            if holds(Object::Face(face_ref, face, polygon)) {
                into.push(face_ref);
            }
        }
    }

    /// Selects what `layer` holds: its entities and groups not in another group, and its own
    /// brushes, or the world's for the default layer.
    fn select_layer(&self, model: &Model<'_>, layer: Layer, found: &mut MapFound) {
        let own = match layer {
            Layer::Default => EntityIndex::new(0),
            Layer::Own(at) => at,
        };
        let selectable = |at: EntityIndex| !self.is_hidden(at) && !self.is_locked(at);
        for (place, entity) in self.file().entities.iter().enumerate().skip(1) {
            let at = EntityIndex::new(place);
            let top = entity.group().is_none()
                && !matches!(entity.container(), Some((RoomKind::Layer, _)));
            let inside = match layer {
                Layer::Default => model.layer_entity(at).is_none(),
                Layer::Own(own) => model.layer_entity(at) == Some(own),
            };
            if top && inside && selectable(at) {
                found.entities.push(at);
            }
        }
        if selectable(own)
            && let Some(entity) = model.entity(own)
        {
            found
                .brushes
                .extend((0..entity.brushes().count()).map(|b| (own, BrushIndex::new(b))));
        }
    }
}
