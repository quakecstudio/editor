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

use super::brushes::{DEFAULT_GRID, snap};
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
}

/// The parameters of `map.save_prefab`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavePrefabParams {
    /// The prefab's name: letters, digits, `-` and `_`.
    pub name: String,
    /// The brushes of the world to keep in the prefab's world.
    #[serde(default)]
    pub brushes: Vec<BrushIndex>,
    /// The entities to keep whole, a brush entity with all its brushes.
    #[serde(default)]
    pub entities: Vec<EntityIndex>,
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
/// `entities`.
fn bounds<'a>(
    brushes: impl Iterator<Item = &'a Brush>,
    entities: &[&Entity],
) -> Option<[[f64; 3]; 2]> {
    let mut points: Vec<[f64; 3]> = brushes.flat_map(Brush::vertices).collect();
    for entity in entities {
        points.extend(entity.brushes().flat_map(Brush::vertices));
        if !entity.has_brushes() {
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

    /// The prefab of the world's `brushes` and the whole `entities`, moved so the middle of
    /// their floor, on the grid, is the origin.
    fn prefab_of(
        &mut self,
        brushes: &[BrushIndex],
        entities: &[EntityIndex],
    ) -> Result<MapFile, WorkbenchError> {
        let world = EntityIndex::new(0);
        let mut kept_brushes = Vec::new();
        for &brush in brushes {
            kept_brushes.push(self.brush(world, brush)?);
        }
        let mut kept_entities = Vec::new();
        for &entity in entities {
            if entity == world {
                continue;
            }
            kept_entities.push(self.entity_mut(entity)?.clone());
        }
        let all: Vec<&Entity> = kept_entities.iter().collect();
        let [lo, hi] = bounds(kept_brushes.iter(), &all).ok_or(WorkbenchError::NoSelection)?;
        let grid = Some(DEFAULT_GRID);
        let anchor = [
            snap((lo[0] + hi[0]) / 2.0, grid),
            snap((lo[1] + hi[1]) / 2.0, grid),
            snap(lo[2], grid),
        ];
        let by = anchor.map(|c| -c);
        let mut prefab =
            MapFile::parse(b"{\n\"classname\" \"worldspawn\"\n}\n").map_err(|error| {
                WorkbenchError::Brush {
                    error: qs_map::BrushError::Map(error),
                }
            })?;
        let mut world_entity = Entity::new();
        world_entity.items = kept_brushes.into_iter().map(Item::Brush).collect();
        let world_moved = moved(&world_entity, by)?;
        if let Some(prefab_world) = prefab.entities.first_mut() {
            prefab_world.items.extend(world_moved.items);
        }
        for entity in &kept_entities {
            prefab.push_entity(moved(entity, by)?);
        }
        Ok(prefab)
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
        let grid = Some(DEFAULT_GRID);
        let origin = params.origin.map(|c| snap(c, grid));
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
        let prefab = workbench
            .map_mut()?
            .prefab_of(&params.brushes, &params.entities)?;
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
