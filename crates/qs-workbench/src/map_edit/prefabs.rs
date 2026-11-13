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
use crate::{HasWorkbench, WorkbenchError};

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
fn prefab_file(root: &Path, name: &str) -> Result<PathBuf, WorkbenchError> {
    let fine = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !fine {
        return Err(WorkbenchError::PrefabName {
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
fn moved(entity: &Entity, by: [f64; 3]) -> Result<Entity, WorkbenchError> {
    let mut out = entity.clone();
    for item in &mut out.items {
        if let Item::Brush(brush) = item {
            *brush = brush
                .translated(by, TextureLock::Locked)
                .map_err(|error| WorkbenchError::Brush { error })?;
        }
    }
    if entity.value(b"origin").is_some() {
        let [x, y, z] = entity.origin();
        let origin = origin_text([x + by[0], y + by[1], z + by[2]]);
        out.set(b"origin", origin.as_bytes())
            .map_err(|error| WorkbenchError::Brush {
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

impl MapDocument {
    /// Copies `prefab` into the map at `origin`: its world's brushes into the world, its other
    /// entities after the map's; the first entity added, or the world.
    fn place_prefab(
        &mut self,
        prefab: &MapFile,
        origin: [f64; 3],
    ) -> Result<EntityIndex, WorkbenchError> {
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

    /// The prefab of `brushes` and `entities`, moved so the middle of their floor is the
    /// origin, across on `grid` and up exactly. A brush of the world, or of a layer or group,
    /// goes into the prefab's world; one of another entity brings that entity whole, as an
    /// entity named does. A layer or group named gives its brushes to the world, and an entity's
    /// keys placing it in this map's layers and groups are left out.
    fn prefab_of(
        &mut self,
        brushes: &[(EntityIndex, BrushIndex)],
        entities: &[EntityIndex],
        grid: Option<f64>,
    ) -> Result<MapFile, WorkbenchError> {
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
        let [lo, hi] = bounds(world_brushes.iter(), &all).ok_or(WorkbenchError::NoSelection)?;
        let anchor = [
            snap((lo[0] + hi[0]) / 2.0, grid),
            snap((lo[1] + hi[1]) / 2.0, grid),
            lo[2],
        ];
        let by = anchor.map(|c| -c);
        let mut prefab =
            MapFile::parse(b"{\n\"classname\" \"worldspawn\"\n}\n").map_err(|error| {
                WorkbenchError::Brush {
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
    fn is_world_part(&mut self, entity: EntityIndex) -> Result<bool, WorkbenchError> {
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

impl<L: crate::Loader> crate::Workbench<L> {
    /// The project's folder, or [`WorkbenchError::NoProject`].
    fn project_root(&self) -> Result<PathBuf, WorkbenchError> {
        self.root()
            .map(Path::to_path_buf)
            .ok_or(WorkbenchError::NoProject)
    }
}

/// `map.prefabs`.
pub(super) struct ListPrefabs;

impl<C: HasWorkbench + 'static> Command<C> for ListPrefabs {
    const ID: &'static str = MAP_PREFABS;
    const SUMMARY: &'static str = "Lists the .map files in the project's prefabs folder.";
    type Params = NoParams;
    type Output = Prefabs;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<Prefabs, C>, WorkbenchError> {
        let folder = context.workbench().project_root()?.join(PREFAB_FOLDER);
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

impl<C: HasWorkbench + 'static> Command<C> for PlacePrefab {
    const ID: &'static str = MAP_PLACE_PREFAB;
    const SUMMARY: &'static str = "Copies a prefab into the open map with its origin at a point \
        on the grid: its world's brushes into the world, its other entities after the map's.";
    type Params = PlacePrefabParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: PlacePrefabParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let path = match context.workbench().root() {
            Some(root) if params.path.is_relative() => root.join(&params.path),
            Some(_) | None => params.path.clone(),
        };
        let bytes =
            std::fs::read(&path).map_err(|_| WorkbenchError::NoFile { path: path.clone() })?;
        let prefab = MapFile::parse(&bytes).map_err(|error| WorkbenchError::Map {
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

impl<C: HasWorkbench + 'static> Command<C> for SavePrefab {
    const ID: &'static str = MAP_SAVE_PREFAB;
    const SUMMARY: &'static str = "Writes brushes of the world and whole entities of the open map \
        as a prefab in the project's prefabs folder, around the middle of their floor.";
    type Params = SavePrefabParams;
    type Output = PrefabSaved;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SavePrefabParams,
    ) -> Result<Outcome<PrefabSaved, C>, WorkbenchError> {
        let workbench = context.workbench();
        let root = workbench.project_root()?;
        let path = prefab_file(&root, &params.name)?;
        let prefab =
            workbench
                .map_mut()?
                .prefab_of(&params.brushes, &params.entities, params.grid)?;
        let save = |error| WorkbenchError::Save {
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

#[cfg(test)]
mod tests;
