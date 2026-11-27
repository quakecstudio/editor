// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.brushes` and `map.summary` (L-40, MAP-16): the open map as an agent reads it to build
//! on and to review. `map.brushes` names each brush of an entity by its place, with the box
//! around it and its textures, so the brush a tool takes by its place need not be counted by
//! hand; `map.summary` is the map in short, to judge it by and to compare maps with.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use qs_commands::{Command, Outcome};
use qs_map::{Brush, MapFile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{BrushIndex, EntityIndex, MAP_BRUSHES, MAP_SUMMARY, MapRead};
use crate::issues::{bounds, solid_bounds};
use crate::textures::uses_in;
use crate::{HasMap, MapEditError};

/// A box, its least corner then its greatest.
pub type Bounds = [[f64; 3]; 2];

/// The box with corners `a` and `b`, whichever way round they were given on each axis.
fn ordered([a, b]: Bounds) -> Bounds {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [
        [ax.min(bx), ay.min(by), az.min(bz)],
        [ax.max(bx), ay.max(by), az.max(bz)],
    ]
}

/// Whether the boxes `a` and `b` touch or overlap.
fn touch([a_min, a_max]: Bounds, [b_min, b_max]: Bounds) -> bool {
    a_min.iter().zip(b_max).all(|(&min, max)| min <= max)
        && b_min.iter().zip(a_max).all(|(&min, max)| min <= max)
}

/// The textures of `brush`'s faces, each once, in face order.
fn textures_of(brush: &Brush) -> Vec<String> {
    let mut textures: Vec<String> = Vec::new();
    for face in &brush.faces {
        let name = String::from_utf8_lossy(face.texture()).into_owned();
        if !textures.contains(&name) {
            textures.push(name);
        }
    }
    textures
}

/// The parameters of `map.brushes`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushesParams {
    /// The entity whose brushes are listed; the world when not given.
    #[serde(default)]
    pub entity: Option<EntityIndex>,
    /// Only the brushes whose box touches this one, two opposite corners in either order;
    /// every brush when not given.
    #[serde(default)]
    pub within: Option<Bounds>,
}

/// A brush, as `map.brushes` answers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushInfo {
    /// Its place among its entity's brushes, which the `map.brush_*` commands take.
    pub brush: BrushIndex,
    /// The box around it, `[min, max]`; none for a brush with no volume.
    pub bounds: Option<Bounds>,
    /// The textures of its faces, each once.
    pub textures: Vec<String>,
}

/// What `map.brushes` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BrushesListed {
    /// The entity.
    pub entity: EntityIndex,
    /// How many brushes the entity has; a new brush takes this place.
    pub total: usize,
    /// Its brushes, in the map's order, those `within` leaves.
    pub brushes: Vec<BrushInfo>,
}

/// `map.brushes`.
pub(super) struct MapBrushes;

impl<C: HasMap + 'static> Command<C> for MapBrushes {
    const ID: &'static str = MAP_BRUSHES;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Lists the brushes of an entity of the open map, the world's \
        when none is named: each brush's place, which the map.brush_* tools take, the box \
        around it and its textures; all of them, or those touching a box. The brush tools \
        answer the places of the brushes they make, so this is for brushes whose places are \
        not known: a map someone else built, or after a tool that changed how many brushes \
        the entity has.";
    type Params = BrushesParams;
    type Output = BrushesListed;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushesParams,
    ) -> Result<Outcome<BrushesListed, C>, MapEditError> {
        let document = context.map_store().map().ok_or(MapEditError::NoMap)?;
        let entity = params.entity.unwrap_or(EntityIndex::WORLD);
        let of = document
            .file()
            .entities
            .get(entity.get())
            .ok_or(MapEditError::NoEntity { entity })?;
        let all: Vec<BrushInfo> = of
            .brushes()
            .enumerate()
            .map(|(index, brush)| BrushInfo {
                brush: BrushIndex::new(index),
                bounds: solid_bounds(brush),
                textures: textures_of(brush),
            })
            .collect();
        let total = all.len();
        let within = params.within.map(ordered);
        let brushes = all
            .into_iter()
            .filter(|brush| {
                within.is_none_or(|within| brush.bounds.is_some_and(|bounds| touch(bounds, within)))
            })
            .collect();
        Ok(Outcome::unchanged(BrushesListed {
            entity,
            total,
            brushes,
        }))
    }
}

/// How many of something a map has, by its name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Counted {
    /// The name: a classname, or a texture.
    pub name: String,
    /// How many.
    pub count: usize,
}

/// `counts` as a list, the most first and names in order among equals.
fn counted(counts: BTreeMap<String, usize>) -> Vec<Counted> {
    let mut list: Vec<Counted> = counts
        .into_iter()
        .map(|(name, count)| Counted { name, count })
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    list
}

/// What `map.summary` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapSummary {
    /// The map's file.
    pub path: PathBuf,
    /// The box around every brush, `[min, max]`; none for a map with no brush.
    pub bounds: Option<Bounds>,
    /// How many brushes the world has.
    pub world_brushes: usize,
    /// How many brushes its other entities have, doors and platforms among them.
    pub entity_brushes: usize,
    /// How many entities it has, the world among them.
    pub entities: usize,
    /// Its entities by classname, the most first: its monsters, lights, items and triggers.
    pub classes: Vec<Counted>,
    /// How many textures its faces wear, however many of them `textures` lists.
    pub texture_count: usize,
    /// Its textures by how many faces wear each, the most first: all of them, or the most
    /// used the request asked for.
    pub textures: Vec<Counted>,
    /// The world's keys and values, in order: its message, WADs, music and the like.
    pub world: Vec<(String, String)>,
}

/// The parameters of `map.summary`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SummaryParams {
    /// A `.map` file to summarise from the disk, which is not opened: the way to read many
    /// maps, as when comparing a set of them. The open map when not given.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Only this many of the textures, the most used first; all of them when not given. A
    /// full map names hundreds of textures, so a few is what to ask for when reading many
    /// maps. The classnames are never cut: they are the map's monsters, items and triggers.
    #[serde(default)]
    pub most_textures: Option<usize>,
}

impl MapSummary {
    /// The summary of the map `file` at `path`.
    fn of(path: &Path, file: &MapFile) -> Self {
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let mut classes: BTreeMap<String, usize> = BTreeMap::new();
        let mut corners = Vec::new();
        let (mut world_brushes, mut entity_brushes) = (0_usize, 0_usize);
        for (index, entity) in file.entities.iter().enumerate() {
            let class = entity.classname().map_or_default(text);
            let count = classes.entry(class).or_default();
            *count = count.saturating_add(1);
            for brush in entity.brushes() {
                let of = if index == EntityIndex::WORLD.get() {
                    &mut world_brushes
                } else {
                    &mut entity_brushes
                };
                *of = of.saturating_add(1);
                // A brush that is not closed has no box of its own to count.
                corners.extend(solid_bounds(brush).into_iter().flatten());
            }
        }
        let world = file
            .entities
            .first()
            .into_iter()
            .flat_map(|world| world.pairs())
            .map(|pair| (text(pair.key()), text(pair.value())))
            .collect();
        let textures = counted(uses_in(file));
        Self {
            path: path.to_path_buf(),
            bounds: bounds(corners),
            world_brushes,
            entity_brushes,
            entities: file.entities.len(),
            classes: counted(classes),
            texture_count: textures.len(),
            textures,
            world,
        }
    }
}

/// `map.summary`.
pub(super) struct MapSummarise;

impl<C: HasMap + 'static> Command<C> for MapSummarise {
    const ID: &'static str = MAP_SUMMARY;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "A map in short, to review it or compare it with others: the \
        box around its brushes, its brush and entity counts, its entities counted by classname \
        (monsters, lights, items, triggers), its textures counted by face, and its world's \
        keys. The open map, or with a path a .map file read from the disk without opening \
        it; with `most_textures`, only that many textures of the count it gives, which keeps \
        it short when many maps are read. map.list_entities lists each entity; this counts them \
        as the file has them, and map.balance counts what the game spawns on each skill.";
    type Params = SummaryParams;
    type Output = MapSummary;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: SummaryParams,
    ) -> Result<Outcome<MapSummary, C>, MapEditError> {
        let mut summary = if let Some(path) = params.path {
            MapSummary::of(&path, &MapRead::of(&path)?.file)
        } else {
            let document = context.map_store().map().ok_or(MapEditError::NoMap)?;
            MapSummary::of(document.path(), document.file())
        };
        if let Some(most) = params.most_textures {
            summary.textures.truncate(most);
        }
        Ok(Outcome::unchanged(summary))
    }
}

#[cfg(test)]
mod tests;
