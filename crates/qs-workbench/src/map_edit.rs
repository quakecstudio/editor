// SPDX-License-Identifier: GPL-3.0-or-later

//! The open map and its entity edits (P5-06, MAP-4, MAP-22): `map.open` reads a `.map`, or a
//! shipped `.bsp`'s entity lump when the map has no source; `map.select`, `map.place`,
//! `map.move`, `map.rotate`, `map.delete`, `map.set_key` and `map.remove_key` change it, each
//! with undo; `map.entity` gives an entity's keys typed by its class, with what a target key
//! may name; and `map.save` writes the `.map`, or for a BSP a `.ent` override beside it or the
//! BSP with its entity lump rewritten.
//!
//! Edits go through `qs-map`'s lossless document, so a saved `.map` differs from the one read
//! only where an edit changed it (MAP-7).

use std::path::{Path, PathBuf};

use qs_bsp::{Bsp, BspLoader};
use qs_commands::{BoxError, Command, Outcome, RegisterError, Registry, UndoStep};
use qs_map::{Entity, Item, MapFile};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::entity_classes::KeyInfo;
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `map.open`.
pub const MAP_OPEN: &str = "map.open";
/// The id of `map.select`.
pub const MAP_SELECT: &str = "map.select";
/// The id of `map.place`.
pub const MAP_PLACE: &str = "map.place";
/// The id of `map.move`.
pub const MAP_MOVE: &str = "map.move";
/// The id of `map.rotate`.
pub const MAP_ROTATE: &str = "map.rotate";
/// The id of `map.delete`.
pub const MAP_DELETE: &str = "map.delete";
/// The id of `map.set_key`.
pub const MAP_SET_KEY: &str = "map.set_key";
/// The id of `map.remove_key`.
pub const MAP_REMOVE_KEY: &str = "map.remove_key";
/// The id of `map.entity`.
pub const MAP_ENTITY: &str = "map.entity";
/// The id of `map.save`.
pub const MAP_SAVE: &str = "map.save";

/// Where the open map came from, which decides how it is saved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MapSource {
    /// A `.map` file, saved over itself.
    Map,
    /// A compiled BSP with no `.map` source (MAP-22): its bytes, for rewriting the lump.
    Bsp(Vec<u8>),
}

/// The map open in the map view.
#[derive(Clone, Debug, PartialEq)]
pub struct MapDocument {
    path: PathBuf,
    source: MapSource,
    file: MapFile,
    selected: Option<usize>,
    unsaved: bool,
}

impl MapDocument {
    /// The file it was read from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where it came from.
    #[must_use]
    pub const fn source(&self) -> &MapSource {
        &self.source
    }

    /// The map as edited.
    #[must_use]
    pub const fn file(&self) -> &MapFile {
        &self.file
    }

    /// The selected entity, by its place in the file from 0, the world first.
    #[must_use]
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Whether it was edited since it was opened or saved.
    #[must_use]
    pub const fn is_unsaved(&self) -> bool {
        self.unsaved
    }

    fn entity_mut(&mut self, entity: usize) -> Result<&mut Entity, WorkbenchError> {
        self.file
            .entities
            .get_mut(entity)
            .ok_or(WorkbenchError::NoEntity { entity })
    }
}

/// How `map.save` writes a map read from a BSP (MAP-22).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BspSave {
    /// A `.ent` file beside the BSP, which QuakeSpasm and its descendants read in place of
    /// the lump.
    #[default]
    Ent,
    /// The BSP itself, its entity lump replaced.
    Lump,
}

/// The parameters of `map.open`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MapOpenParams {
    /// A `.map` file, or a `.bsp` whose entities to edit.
    pub path: PathBuf,
}

/// What `map.open` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapOpened {
    /// The map.
    pub path: PathBuf,
    /// Its entities.
    pub entities: usize,
}

/// The parameters of `map.select`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectParams {
    /// The entity to select, or none to clear the selection.
    pub entity: Option<usize>,
}

/// The parameters of `map.place`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaceParams {
    /// The new entity's classname.
    pub classname: String,
    /// Where it stands.
    pub origin: [f64; 3],
    /// Its other keys and values, in order.
    #[serde(default)]
    pub keys: Vec<(String, String)>,
}

/// The parameters of `map.move`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MoveParams {
    /// The entity.
    pub entity: usize,
    /// How far, in map units.
    pub by: [f64; 3],
}

/// The parameters of `map.rotate`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RotateParams {
    /// The entity, a point entity.
    pub entity: usize,
    /// Degrees to turn it about the vertical axis, counter-clockwise seen from above.
    pub degrees: f64,
}

/// The parameters of `map.delete` and `map.entity`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityParams {
    /// The entity.
    pub entity: usize,
}

/// The parameters of `map.set_key`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetKeyParams {
    /// The entity.
    pub entity: usize,
    /// The key.
    pub key: String,
    /// Its value.
    pub value: String,
}

/// The parameters of `map.remove_key`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveKeyParams {
    /// The entity.
    pub entity: usize,
    /// The key.
    pub key: String,
}

/// The parameters of `map.save`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SaveParams {
    /// For a map read from a BSP, a `.ent` override (the default) or the BSP rewritten.
    #[serde(default)]
    pub bsp: BspSave,
}

/// What `map.save` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapSaved {
    /// The file written.
    pub path: PathBuf,
}

/// What an edit answers: the entity it placed or changed, when there is one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapEdited {
    /// The entity, by its place in the file.
    pub entity: Option<usize>,
}

/// What `map.entity` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct EntityKeys {
    /// The classname, when it has one.
    pub classname: Option<String>,
    /// Its pairs, in file order.
    pub pairs: Vec<(String, String)>,
    /// The keys its class reads, typed, each with its default; empty for a classname that
    /// is no class.
    pub keys: Vec<KeyInfo>,
    /// The targetnames of the map, the values a `target` or `killtarget` may take.
    pub targetnames: Vec<String>,
    /// Whether it has brushes.
    pub brush: bool,
}

impl<L: crate::Loader> Workbench<L> {
    /// The map open in the map view.
    #[must_use]
    pub const fn map(&self) -> Option<&MapDocument> {
        self.map.as_ref()
    }

    /// Opens the `.map` at `path`, or a `.bsp`'s entity lump, in the map view.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoFile`] when it cannot be read, [`WorkbenchError::Bsp`] for a BSP
    /// that does not load and [`WorkbenchError::Map`] for entities that do not parse.
    pub fn open_map(&mut self, path: &Path) -> Result<&MapDocument, WorkbenchError> {
        let bytes = std::fs::read(path).map_err(|_| WorkbenchError::NoFile {
            path: path.to_path_buf(),
        })?;
        let is_bsp = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("bsp"));
        let (source, text) = if is_bsp {
            let bsp =
                Bsp::read(&bytes, BspLoader::default()).map_err(|error| WorkbenchError::Bsp {
                    path: path.to_path_buf(),
                    error,
                })?;
            let text = bsp.entities().to_vec();
            (MapSource::Bsp(bytes), text)
        } else {
            (MapSource::Map, bytes)
        };
        let file = MapFile::parse(&text).map_err(|error| WorkbenchError::Map {
            path: path.to_path_buf(),
            error,
        })?;
        Ok(self.map.insert(MapDocument {
            path: path.to_path_buf(),
            source,
            file,
            selected: None,
            unsaved: false,
        }))
    }

    fn map_mut(&mut self) -> Result<&mut MapDocument, WorkbenchError> {
        self.map.as_mut().ok_or(WorkbenchError::NoMap)
    }

    /// The keys of `entity` of the open map, typed by its class (MAP-4).
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoMap`] with no map open and [`WorkbenchError::NoEntity`] for an
    /// entity it does not have.
    pub fn map_entity(&mut self, entity: usize) -> Result<EntityKeys, WorkbenchError> {
        let document = self.map.as_ref().ok_or(WorkbenchError::NoMap)?;
        let found = document
            .file
            .entities
            .get(entity)
            .ok_or(WorkbenchError::NoEntity { entity })?;
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let classname = found.classname().map(text);
        let pairs = found
            .pairs()
            .map(|pair| (text(pair.key()), text(pair.value())))
            .collect();
        let brush = found.has_brushes();
        let mut targetnames: Vec<String> = document
            .file
            .entities
            .iter()
            .filter_map(|e| e.value(b"targetname"))
            .map(text)
            .filter(|name| !name.is_empty())
            .collect();
        targetnames.sort();
        targetnames.dedup();
        let keys = match (&classname, self.root().is_some()) {
            (Some(name), true) => self
                .entity_classes()?
                .iter()
                .find(|class| &class.name == name)
                .map(|class| crate::entity_classes::ClassInfo::from(class).keys)
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        Ok(EntityKeys {
            classname,
            pairs,
            keys,
            targetnames,
            brush,
        })
    }

    /// Writes the open map (MAP-22 for a BSP): a `.map` over itself; a BSP's entities as a
    /// `.ent` beside it, or into the BSP as its entity lump.
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoMap`] with no map open, [`WorkbenchError::Bsp`] when the BSP
    /// cannot take the lump and [`WorkbenchError::Save`] when the file cannot be written.
    pub fn save_map(&mut self, how: BspSave) -> Result<PathBuf, WorkbenchError> {
        let document = self.map_mut()?;
        let text = document.file.to_bytes();
        let (path, bytes) = match &document.source {
            MapSource::Map => (document.path.clone(), text),
            MapSource::Bsp(bsp) => {
                // A lump read from a BSP ends at its NUL (MAP-1.1); the bytes after it are
                // not entities.
                let entities = text.split(|&b| b == 0).next().unwrap_or_default().to_vec();
                match how {
                    BspSave::Ent => (document.path.with_extension("ent"), entities),
                    BspSave::Lump => {
                        let rewritten =
                            qs_bsp::replace_entities(bsp, &entities, BspLoader::default())
                                .map_err(|error| WorkbenchError::Bsp {
                                    path: document.path.clone(),
                                    error,
                                })?;
                        (document.path.clone(), rewritten)
                    }
                }
            }
        };
        std::fs::write(&path, &bytes).map_err(|error| WorkbenchError::Save {
            path: path.clone(),
            error,
        })?;
        if let (MapSource::Bsp(bsp), BspSave::Lump) = (&mut document.source, how) {
            *bsp = bytes;
        }
        document.unsaved = false;
        Ok(path)
    }

    /// Runs `edit` on the open map and returns its answer and the step that undoes it; an
    /// edit that fails leaves the map as it was.
    fn edit_map<T>(
        &mut self,
        edit: impl FnOnce(&mut MapDocument) -> Result<T, WorkbenchError>,
    ) -> Result<(T, MapStep), WorkbenchError> {
        let document = self.map_mut()?;
        let before = (document.file.clone(), document.selected);
        match edit(document) {
            Ok(answer) => {
                document.unsaved = true;
                let step = MapStep {
                    path: document.path.clone(),
                    other: before,
                };
                Ok((answer, step))
            }
            Err(error) => {
                (document.file, document.selected) = before;
                Err(error)
            }
        }
    }
}

/// The open map's other state: what undo puts back, then what redo does.
struct MapStep {
    path: PathBuf,
    other: (MapFile, Option<usize>),
}

impl MapStep {
    /// Swaps the document's map and selection with the other state.
    fn swap(&mut self, document: &mut MapDocument) {
        core::mem::swap(&mut document.file, &mut self.other.0);
        core::mem::swap(&mut document.selected, &mut self.other.1);
        document.unsaved = true;
    }

    fn apply<C: HasWorkbench>(&mut self, context: &mut C) -> Result<(), BoxError> {
        let document = context
            .workbench()
            .map
            .as_mut()
            .filter(|document| document.path == self.path)
            .ok_or(WorkbenchError::NoMap)?;
        self.swap(document);
        Ok(())
    }
}

impl<C: HasWorkbench + 'static> UndoStep<C> for MapStep {
    fn undo(&mut self, context: &mut C) -> Result<(), BoxError> {
        self.apply(context)
    }

    fn redo(&mut self, context: &mut C) -> Result<(), BoxError> {
        self.apply(context)
    }
}

/// A number as a map writes it: whole numbers with no point.
fn number(value: f64) -> String {
    // Adding 0 makes -0 into 0.
    format!("{}", value + 0.0)
}

/// `origin` as a map writes it (MAP-2.7).
fn origin_text(origin: [f64; 3]) -> String {
    origin.map(number).join(" ")
}

/// A map edit's error from `qs-map`, at the open map.
fn map_error(document: &MapDocument, error: qs_map::MapError) -> WorkbenchError {
    WorkbenchError::Map {
        path: document.path.clone(),
        error,
    }
}

/// Moves `entity` by `by`: a point entity's origin; a brush entity's brushes, and its origin
/// when it has one (MAP-2.7, MAP-3.2).
fn move_entity(
    document: &mut MapDocument,
    entity: usize,
    by: [f64; 3],
) -> Result<(), WorkbenchError> {
    let found = document.entity_mut(entity)?;
    let shift = |p: [f64; 3]| [p[0] + by[0], p[1] + by[1], p[2] + by[2]];
    let mut moved = Ok(());
    for item in &mut found.items {
        if let Item::Brush(brush) = item {
            for face in &mut brush.faces {
                let points = face.points().map(shift);
                let texture = face.texture().to_vec();
                let texdef = *face.texdef();
                if let Err(error) = face.set(points, &texture, texdef) {
                    moved = Err(error);
                }
            }
        }
    }
    let has_origin = found.value(b"origin").is_some();
    if !found.has_brushes() || has_origin {
        let origin = origin_text(shift(found.origin()));
        if let Err(error) = found.set(b"origin", origin.as_bytes()) {
            moved = Err(error);
        }
    }
    moved.map_err(|error| map_error(document, error))
}

/// Turns point entity `entity` by `degrees` about the vertical: its `angles` yaw when it has
/// `angles`, else its `angle` (EDICT-8.2 reads `angle` as the yaw), kept from 0 to 360.
fn rotate_entity(
    document: &mut MapDocument,
    entity: usize,
    degrees: f64,
) -> Result<(), WorkbenchError> {
    let found = document.entity_mut(entity)?;
    if found.has_brushes() || entity == 0 {
        return Err(WorkbenchError::NotPointEntity { entity });
    }
    let read = |text: &[u8]| -> f64 { String::from_utf8_lossy(text).trim().parse().unwrap_or(0.0) };
    let turn = |yaw: f64| (yaw + degrees).rem_euclid(360.0);
    let result = if let Some(angles) = found.value(b"angles") {
        let mut parts: Vec<f64> = String::from_utf8_lossy(angles)
            .split_whitespace()
            .map(|part| part.parse().unwrap_or(0.0))
            .collect();
        parts.resize(3, 0.0);
        let yaw = parts.get(1).copied().unwrap_or(0.0);
        if let Some(slot) = parts.get_mut(1) {
            *slot = turn(yaw);
        }
        let text: Vec<String> = parts.into_iter().map(number).collect();
        found.set(b"angles", text.join(" ").as_bytes())
    } else {
        let yaw = found.value(b"angle").map_or(0.0, read);
        found.set(b"angle", number(turn(yaw)).as_bytes())
    };
    result.map_err(|error| map_error(document, error))
}

/// `map.open`.
struct MapOpen;

impl<C: HasWorkbench + 'static> Command<C> for MapOpen {
    const ID: &'static str = MAP_OPEN;
    const SUMMARY: &'static str = "Opens a .map in the map view, or a shipped .bsp's entities \
        when the map has no .map source; nothing is selected.";
    type Params = MapOpenParams;
    type Output = MapOpened;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: MapOpenParams,
    ) -> Result<Outcome<MapOpened, C>, WorkbenchError> {
        let document = context.workbench().open_map(&params.path)?;
        Ok(Outcome::outside_history(MapOpened {
            path: document.path.clone(),
            entities: document.file.entities.len(),
        }))
    }
}

/// `map.select`.
struct MapSelect;

impl<C: HasWorkbench + 'static> Command<C> for MapSelect {
    const ID: &'static str = MAP_SELECT;
    const SUMMARY: &'static str = "Selects an entity of the open map, or clears the selection.";
    type Params = SelectParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SelectParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let document = context.workbench().map_mut()?;
        if let Some(entity) = params.entity {
            document.entity_mut(entity)?;
        }
        document.selected = params.entity;
        Ok(Outcome::outside_history(MapEdited {
            entity: params.entity,
        }))
    }
}

/// `map.place`.
struct MapPlace;

impl<C: HasWorkbench + 'static> Command<C> for MapPlace {
    const ID: &'static str = MAP_PLACE;
    const SUMMARY: &'static str = "Places a point entity of a classname at an origin in the \
        open map, with the keys given, and selects it.";
    type Params = PlaceParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: PlaceParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let (entity, step) = context.workbench().edit_map(|document| {
            let mut entity = Entity::new();
            let origin = origin_text(params.origin);
            let mut pairs = vec![
                ("classname".to_owned(), params.classname.clone()),
                ("origin".to_owned(), origin),
            ];
            pairs.extend(params.keys.iter().cloned());
            for (key, value) in &pairs {
                entity
                    .set(key.as_bytes(), value.as_bytes())
                    .map_err(|error| map_error(document, error))?;
            }
            document.file.push_entity(entity);
            let index = document.file.entities.len().saturating_sub(1);
            document.selected = Some(index);
            Ok(index)
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(entity),
            },
            step,
        ))
    }
}

/// `map.move`.
struct MapMove;

impl<C: HasWorkbench + 'static> Command<C> for MapMove {
    const ID: &'static str = MAP_MOVE;
    const SUMMARY: &'static str = "Moves an entity of the open map: a point entity's origin, \
        a brush entity's brushes.";
    type Params = MoveParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: MoveParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = context
            .workbench()
            .edit_map(|document| move_entity(document, params.entity, params.by))?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.rotate`.
struct MapRotate;

impl<C: HasWorkbench + 'static> Command<C> for MapRotate {
    const ID: &'static str = MAP_ROTATE;
    const SUMMARY: &'static str = "Turns a point entity of the open map about the vertical: \
        its angle, or its angles' yaw.";
    type Params = RotateParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: RotateParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = context
            .workbench()
            .edit_map(|document| rotate_entity(document, params.entity, params.degrees))?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.delete`.
struct MapDelete;

impl<C: HasWorkbench + 'static> Command<C> for MapDelete {
    const ID: &'static str = MAP_DELETE;
    const SUMMARY: &'static str = "Deletes an entity of the open map; the world stays.";
    type Params = EntityParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: EntityParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let entity = params.entity;
        let ((), step) = context.workbench().edit_map(|document| {
            document.entity_mut(entity)?;
            if entity == 0 {
                return Err(WorkbenchError::NotPointEntity { entity });
            }
            document.file.entities.remove(entity);
            document.selected = match document.selected {
                Some(s) if s == entity => None,
                Some(s) if s > entity => Some(s.saturating_sub(1)),
                other => other,
            };
            Ok(())
        })?;
        Ok(Outcome::undoable(MapEdited { entity: None }, step))
    }
}

/// `map.set_key`.
struct MapSetKey;

impl<C: HasWorkbench + 'static> Command<C> for MapSetKey {
    const ID: &'static str = MAP_SET_KEY;
    const SUMMARY: &'static str = "Sets a key of an entity of the open map: its last \
        occurrence keeps its place and formatting, a new key goes after the others.";
    type Params = SetKeyParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SetKeyParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = context.workbench().edit_map(|document| {
            document
                .entity_mut(params.entity)?
                .set(params.key.as_bytes(), params.value.as_bytes())
                .map_err(|error| map_error(document, error))
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.remove_key`.
struct MapRemoveKey;

impl<C: HasWorkbench + 'static> Command<C> for MapRemoveKey {
    const ID: &'static str = MAP_REMOVE_KEY;
    const SUMMARY: &'static str = "Removes every pair with a key from an entity of the open map.";
    type Params = RemoveKeyParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: RemoveKeyParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = context.workbench().edit_map(|document| {
            document
                .entity_mut(params.entity)?
                .remove(params.key.as_bytes());
            Ok(())
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.entity`.
struct MapEntity;

impl<C: HasWorkbench + 'static> Command<C> for MapEntity {
    const ID: &'static str = MAP_ENTITY;
    const SUMMARY: &'static str = "An entity of the open map: its pairs, the keys its class \
        reads with their types and defaults, and the map's targetnames for its target keys.";
    type Params = EntityParams;
    type Output = EntityKeys;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: EntityParams,
    ) -> Result<Outcome<EntityKeys, C>, WorkbenchError> {
        Ok(Outcome::unchanged(
            context.workbench().map_entity(params.entity)?,
        ))
    }
}

/// `map.save`.
struct MapSave;

impl<C: HasWorkbench + 'static> Command<C> for MapSave {
    const ID: &'static str = MAP_SAVE;
    const SUMMARY: &'static str = "Saves the open map: a .map over itself; a BSP's entities as \
        a .ent beside it (bsp: ent, the default) or into the BSP's entity lump (bsp: lump).";
    type Params = SaveParams;
    type Output = MapSaved;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SaveParams,
    ) -> Result<Outcome<MapSaved, C>, WorkbenchError> {
        let path = context.workbench().save_map(params.bsp)?;
        Ok(Outcome::outside_history(MapSaved { path }))
    }
}

/// Registers the `map.*` editing commands.
pub(crate) fn register_map_edit<C: HasWorkbench + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(MapOpen)?;
    registry.register(MapSelect)?;
    registry.register(MapPlace)?;
    registry.register(MapMove)?;
    registry.register(MapRotate)?;
    registry.register(MapDelete)?;
    registry.register(MapSetKey)?;
    registry.register(MapRemoveKey)?;
    registry.register(MapEntity)?;
    registry.register(MapSave)
}

#[cfg(test)]
mod tests;
