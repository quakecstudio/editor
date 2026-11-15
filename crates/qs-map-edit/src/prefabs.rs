// SPDX-License-Identifier: GPL-3.0-or-later

//! Prefabs (P6-23, MAP-33): reusable pieces kept as `.map` files in the project's `prefabs`
//! folder, after TrenchBroom's. `map.prefabs` lists them, `map.place_prefab` copies one into
//! the open map at a point, with its textures fixed to it, and `map.save_prefab` writes the
//! selected brushes and entities as one, around the middle of their floor.

use std::path::{Path, PathBuf};

use qs_commands::{Command, NoParams, Outcome};
use qs_map::{Brush, Entity, Item, MapFile, TextureLock};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::brushes::snap;
use super::{BrushIndex, EntityIndex, MapDocument, MapEdited, edit_map, origin_text};
use crate::{HasMap, MapEditError, MapProject};

/// The id of `map.prefabs`.
pub const MAP_PREFABS: &str = "map.prefabs";
/// The id of `map.place_prefab`.
pub const MAP_PLACE_PREFAB: &str = "map.place_prefab";
/// The id of `map.save_prefab`.
pub const MAP_SAVE_PREFAB: &str = "map.save_prefab";

/// The project's folder its prefabs are kept in.
pub const PREFAB_FOLDER: &str = "prefabs";
/// A prefab's file extension.
const PREFAB_EXTENSION: &str = "map";
/// The key marking a layer or group, whose brushes are the world's (MAP-9).
const CONTAINER_TYPE: &[u8] = b"_tb_type";
/// The keys placing an entity in a layer or group of its map, by ids another map does not
/// share (MAP-9).
const CONTAINER_KEYS: [&[u8]; 2] = [b"_tb_layer", b"_tb_group"];

/// One prefab of the project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Prefab {
    /// Its name: the file's, without the extension.
    pub name: String,
    /// The file.
    pub path: PathBuf,
}

/// What `map.prefabs` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Prefabs {
    /// The project's prefabs, by name.
    pub prefabs: Vec<Prefab>,
}

/// The parameters of `map.place_prefab`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlacePrefabParams {
    /// The prefab's file, or its path in the project.
    pub path: PathBuf,
    /// Where its origin goes, snapped to the grid.
    pub origin: [f64; 3],
    /// The grid, in map units; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// The parameters of `map.save_prefab`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavePrefabParams {
    /// The prefab's name: letters, digits, `-` and `_`.
    pub name: String,
    /// The brushes, each by its entity: one of the world or of a layer or group is kept in the
    /// prefab's world; one of another entity brings that entity whole.
    #[serde(default)]
    pub brushes: Vec<(EntityIndex, BrushIndex)>,
    /// The entities to keep whole, a brush entity with all its brushes; a layer or group gives
    /// its brushes to the prefab's world.
    #[serde(default)]
    pub entities: Vec<EntityIndex>,
    /// The grid the prefab's origin is put on across, in map units; 16 when not given, 0 for
    /// none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// What `map.save_prefab` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PrefabSaved {
    /// The prefab written.
    pub prefab: Prefab,
}

/// The prefab named `name`, when the name is one a file of the folder can have.
fn prefab_file(root: &Path, name: &str) -> Result<PathBuf, MapEditError> {
    let fine = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !fine {
        return Err(MapEditError::PrefabName {
            name: name.to_owned(),
        });
    }
    Ok(root
        .join(PREFAB_FOLDER)
        .join(name)
        .with_extension(PREFAB_EXTENSION))
}

/// `entity` moved by `by`: its brushes, their textures kept on them, and its origin when it
/// has one.
fn moved(entity: &Entity, by: [f64; 3]) -> Result<Entity, MapEditError> {
    let mut out = entity.clone();
    for item in &mut out.items {
        if let Item::Brush(brush) = item {
            *brush = brush
                .translated(by, TextureLock::Locked)
                .map_err(|error| MapEditError::Brush { error })?;
        }
    }
    if entity.value(b"origin").is_some() {
        let [x, y, z] = entity.origin();
        let origin = origin_text([x + by[0], y + by[1], z + by[2]]);
        out.set(b"origin", origin.as_bytes())
            .map_err(|error| MapEditError::Brush {
                error: qs_map::BrushError::Map(error),
            })?;
    }
    Ok(out)
}

/// The least and greatest of every corner of `brushes` and every point entity's origin of
/// `entities`, for one that has the key.
fn bounds<'a>(
    brushes: impl Iterator<Item = &'a Brush>,
    entities: &[&Entity],
) -> Option<[[f64; 3]; 2]> {
    let mut points: Vec<[f64; 3]> = brushes.flat_map(Brush::vertices).collect();
    for entity in entities {
        points.extend(entity.brushes().flat_map(Brush::vertices));
        if !entity.has_brushes() && entity.value(b"origin").is_some() {
            points.push(entity.origin());
        }
    }
    let mut found: Option<[[f64; 3]; 2]> = None;
    for point in points {
        let [lo, hi] = found.unwrap_or([point, point]);
        found = Some([
            [
                lo[0].min(point[0]),
                lo[1].min(point[1]),
                lo[2].min(point[2]),
            ],
            [
                hi[0].max(point[0]),
                hi[1].max(point[1]),
                hi[2].max(point[2]),
            ],
        ]);
    }
    found
}

/// Where a prefab or a copy is moved to as it is made.
#[derive(Clone, Copy, Debug)]
enum Anchor {
    /// The middle of its floor to the origin, across on the grid: a prefab, placed later at a
    /// point.
    FloorMiddle(Option<f64>),
    /// Nowhere: a copy keeps its place, as TrenchBroom's Copy writes the objects "as if they
    /// were saved to a map".
    InPlace,
}

/// No move at all: where a copy, or a paste at its original place, goes by.
const UNMOVED: [f64; 3] = [0.0, 0.0, 0.0];

/// The middle of the floor of the box `[lo, hi]`, across on `grid`.
fn floor_middle([lo, hi]: [[f64; 3]; 2], grid: Option<f64>) -> [f64; 3] {
    [
        snap((lo[0] + hi[0]) / 2.0, grid),
        snap((lo[1] + hi[1]) / 2.0, grid),
        lo[2],
    ]
}

impl MapDocument {
    /// Copies `prefab` into the map at `origin`: its world's brushes into the world, its other
    /// entities after the map's; the first entity added, or the world.
    fn place_prefab(
        &mut self,
        prefab: &MapFile,
        origin: [f64; 3],
    ) -> Result<EntityIndex, MapEditError> {
        let mut first = EntityIndex::new(0);
        for (place, entity) in prefab.entities.iter().enumerate() {
            let placed = moved(entity, origin)?;
            if place == 0 {
                let world = self.entity_mut(EntityIndex::new(0))?;
                world.items.extend(
                    placed
                        .items
                        .into_iter()
                        .filter(|i| matches!(i, Item::Brush(_))),
                );
            } else {
                self.file.push_entity(placed);
                if first.get() == 0 {
                    first = EntityIndex::new(self.file.entities.len().saturating_sub(1));
                }
            }
        }
        Ok(first)
    }

    /// The prefab of `brushes` and `entities`, moved by `anchor`: a prefab's so the middle of
    /// their floor is the origin, across on its grid and up exactly; a copy's not at all. A brush of the world, or of a layer or group,
    /// goes into the prefab's world; one of another entity brings that entity whole, as an
    /// entity named does. A layer or group named gives its brushes to the world, and an entity's
    /// keys placing it in this map's layers and groups are left out.
    fn prefab_of(
        &mut self,
        brushes: &[(EntityIndex, BrushIndex)],
        entities: &[EntityIndex],
        anchor: Anchor,
    ) -> Result<MapFile, MapEditError> {
        let mut world_brushes = Vec::new();
        let mut whole = std::collections::BTreeSet::new();
        for &(entity, brush) in brushes {
            if self.is_world_part(entity)? {
                world_brushes.push(self.brush(entity, brush)?);
            } else {
                whole.insert(entity);
            }
        }
        for &entity in entities {
            if self.is_world_part(entity)? {
                let found = self.entity_mut(entity)?;
                world_brushes.extend(found.brushes().cloned());
            } else {
                whole.insert(entity);
            }
        }
        let mut kept_entities = Vec::new();
        for entity in whole {
            let mut kept = self.entity_mut(entity)?.clone();
            for key in CONTAINER_KEYS {
                kept.remove(key);
            }
            kept_entities.push(kept);
        }
        let all: Vec<&Entity> = kept_entities.iter().collect();
        let floor = bounds(world_brushes.iter(), &all).ok_or(MapEditError::NoSelection)?;
        let by = match anchor {
            Anchor::FloorMiddle(grid) => floor_middle(floor, grid).map(|c| -c),
            Anchor::InPlace => UNMOVED,
        };
        let mut prefab =
            MapFile::parse(b"{\n\"classname\" \"worldspawn\"\n}\n").map_err(|error| {
                MapEditError::Brush {
                    error: qs_map::BrushError::Map(error),
                }
            })?;
        let mut world_entity = Entity::new();
        world_entity.items = world_brushes.into_iter().map(Item::Brush).collect();
        let world_moved = moved(&world_entity, by)?;
        if let Some(prefab_world) = prefab.entities.first_mut() {
            prefab_world.items.extend(world_moved.items);
        }
        for entity in &kept_entities {
            prefab.push_entity(moved(entity, by)?);
        }
        Ok(prefab)
    }

    /// Whether `entity` is the world or one of its layers or groups, whose brushes are the
    /// world's (MAP-9).
    fn is_world_part(&mut self, entity: EntityIndex) -> Result<bool, MapEditError> {
        Ok(entity.get() == 0 || self.entity_mut(entity)?.value(CONTAINER_TYPE).is_some())
    }
}

impl MapDocument {
    /// The folders an external map instance's file is looked for in, in order: beside the map,
    /// as qbsp's working folder holds it, then the project's `root` (MAP-14).
    #[must_use]
    pub fn external_folders(&self, root: Option<&Path>) -> Vec<PathBuf> {
        self.path()
            .parent()
            .into_iter()
            .chain(root)
            .map(Path::to_path_buf)
            .collect()
    }
}

/// `map.prefabs`.
pub(super) struct ListPrefabs;

impl<C: HasMap + 'static> Command<C> for ListPrefabs {
    const ID: &'static str = MAP_PREFABS;
    const SUMMARY: &'static str = "Lists the .map files in the project's prefabs folder.";
    type Params = NoParams;
    type Output = Prefabs;
    type Error = MapEditError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<Prefabs, C>, MapEditError> {
        let folder = context
            .map_project()
            .project_root()
            .ok_or(MapEditError::NoProject)?
            .join(PREFAB_FOLDER);
        let mut prefabs: Vec<Prefab> = std::fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(PREFAB_EXTENSION))
            })
            .filter_map(|path| {
                let name = path.file_stem()?.to_string_lossy().into_owned();
                Some(Prefab { name, path })
            })
            .collect();
        prefabs.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Outcome::outside_history(Prefabs { prefabs }))
    }
}

/// `map.place_prefab`.
pub(super) struct PlacePrefab;

impl<C: HasMap + 'static> Command<C> for PlacePrefab {
    const ID: &'static str = MAP_PLACE_PREFAB;
    const SUMMARY: &'static str = "Copies a prefab into the open map with its origin at a point \
        on the grid: its world's brushes into the world, its other entities after the map's.";
    type Params = PlacePrefabParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: PlacePrefabParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let path = match context.map_project().project_root() {
            Some(root) if params.path.is_relative() => root.join(&params.path),
            Some(_) | None => params.path.clone(),
        };
        let bytes =
            std::fs::read(&path).map_err(|_| MapEditError::NoFile { path: path.clone() })?;
        let prefab = MapFile::parse(&bytes).map_err(|error| MapEditError::Map {
            path: path.clone(),
            error,
        })?;
        let origin = params.origin.map(|c| snap(c, params.grid));
        let (entity, step) = edit_map(context, |document| document.place_prefab(&prefab, origin))?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(entity),
            },
            step,
        ))
    }
}

/// `map.save_prefab`.
pub(super) struct SavePrefab;

impl<C: HasMap + 'static> Command<C> for SavePrefab {
    const ID: &'static str = MAP_SAVE_PREFAB;
    const SUMMARY: &'static str = "Writes brushes of the world and whole entities of the open map \
        as a prefab in the project's prefabs folder, around the middle of their floor.";
    type Params = SavePrefabParams;
    type Output = PrefabSaved;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: SavePrefabParams,
    ) -> Result<Outcome<PrefabSaved, C>, MapEditError> {
        let root = context
            .map_project()
            .project_root()
            .ok_or(MapEditError::NoProject)?;
        let path = prefab_file(&root, &params.name)?;
        let prefab = context.map_store().map_mut()?.prefab_of(
            &params.brushes,
            &params.entities,
            Anchor::FloorMiddle(params.grid),
        )?;
        let save = |error| MapEditError::Save {
            path: path.clone(),
            error,
        };
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(save)?;
        }
        std::fs::write(&path, prefab.to_bytes()).map_err(save)?;
        Ok(Outcome::outside_history(PrefabSaved {
            prefab: Prefab {
                name: params.name,
                path,
            },
        }))
    }
}

/// The id of the command that writes a selection as map text, for the clipboard (P7-08).
pub const MAP_COPY: &str = "map.copy";

/// The id of the command that adds map text to the map, as pasted (P7-08).
pub const MAP_PASTE: &str = "map.paste";

/// The parameters of `map.copy`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CopyParams {
    /// The brushes, each by its entity: one of the world or of a layer or group is copied into
    /// the world; one of another entity brings that entity whole.
    #[serde(default)]
    pub brushes: Vec<(EntityIndex, BrushIndex)>,
    /// The entities to copy whole.
    #[serde(default)]
    pub entities: Vec<EntityIndex>,
}

/// What `map.copy` wrote: the selection as a `.map`'s text, where it was.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Copied {
    /// The text.
    pub text: String,
}

/// The parameters of `map.paste`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PasteParams {
    /// A `.map`'s text: its world's brushes go into the world, its other entities after the
    /// map's.
    pub text: String,
    /// Where the middle of its floor goes, snapped across to the grid; where the text has it
    /// when absent, as TrenchBroom's Paste at Original Position.
    #[serde(default)]
    pub at: Option<[f64; 3]>,
    /// The grid, in map units; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// `map.copy`.
pub(super) struct Copy;

impl<C: HasMap + 'static> Command<C> for Copy {
    const ID: &'static str = MAP_COPY;
    const SUMMARY: &'static str = "Writes brushes of the world and whole entities of the open map         as a .map's text, where they are, for the clipboard; the map is unchanged.";
    type Params = CopyParams;
    type Output = Copied;
    type Error = MapEditError;

    fn run(&self, context: &mut C, params: CopyParams) -> Result<Outcome<Copied, C>, MapEditError> {
        let copy = context.map_store().map_mut()?.prefab_of(
            &params.brushes,
            &params.entities,
            Anchor::InPlace,
        )?;
        let text = String::from_utf8_lossy(&copy.to_bytes()).into_owned();
        Ok(Outcome::outside_history(Copied { text }))
    }
}

/// `map.paste`.
pub(super) struct Paste;

impl<C: HasMap + 'static> Command<C> for Paste {
    const ID: &'static str = MAP_PASTE;
    const SUMMARY: &'static str = "Adds a .map's text to the open map, its world's brushes to the         world and its other entities after the map's: where the text has them, or with the         middle of their floor at a point on the grid.";
    type Params = PasteParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: PasteParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let pasted = MapFile::parse(params.text.as_bytes())
            .map_err(|error| MapEditError::NotMapText { error })?;
        let all: Vec<&Entity> = pasted.entities.iter().skip(1).collect();
        let world = pasted
            .entities
            .first()
            .into_iter()
            .flat_map(Entity::brushes);
        let found = bounds(world, &all).ok_or(MapEditError::NoSelection)?;
        let by = match params.at {
            Some(at) => {
                let from = floor_middle(found, Some(0.0));
                let to = [snap(at[0], params.grid), snap(at[1], params.grid), at[2]];
                [to[0] - from[0], to[1] - from[1], to[2] - from[2]]
            }
            None => UNMOVED,
        };
        let (entity, step) = edit_map(context, |document| document.place_prefab(&pasted, by))?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(entity),
            },
            step,
        ))
    }
}

#[cfg(test)]
mod tests;
