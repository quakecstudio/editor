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

use core::fmt;
use std::path::{Path, PathBuf};

use qs_bsp::{Bsp, BspLoader, ent_file_names};
use qs_commands::{BoxError, Command, Outcome, RegisterError, Registry, UndoStep};
use qs_map::{Entity, Item, MapFile};

pub use disk::{
    FILES_CHANGED, FilesChanged, FilesChangedParams, Keep, MAP_COMPARE, MAP_RESOLVE, MapDifference,
    MapOnDisk, ResolveParams,
};
pub use filter::{FilterParams, GameMode, MAP_FILTER, MapFiltered, Skill};
pub use flags::{FlagChange, MAP_SET_FLAG, SetFlagParams};
pub use groups::{
    EntitiesParams, ExportParams, LayerInfo, LayerOption, MAP_EXPORT, MAP_HIDE, MAP_ISOLATE,
    MAP_LAYERS, MAP_LOCK, MAP_SET_LAYER, MAP_SHOW_ALL, MAP_UNLOCK_ALL, SetLayerParams, Switch,
};
pub use inspect::{EntityInfo, InspectParams, MapInspected, RoomInfo, RoomShape};
pub use links::{LinkParams, MAP_LINK};
pub use live::LiveChange;

mod disk;
mod filter;
mod flags;
mod groups;
mod inspect;
mod links;
mod live;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::entity_classes::{ClassInfo, FlagInfo, KeyInfo};
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// Degrees in a whole turn; a rotated yaw is kept below it.
const FULL_TURN: f64 = 360.0;
/// The angles id's QuakeC reads as straight up, `if (self.angles == '0 -1 0')
/// self.movedir = '0 0 1';` (subs.qc, `SetMovedir`), which turning leaves.
const UP: [f64; 3] = [0.0, -1.0, 0.0];
/// The angles `SetMovedir` reads as straight down, `'0 -2 0'`, which turning leaves.
const DOWN: [f64; 3] = [0.0, -2.0, 0.0];

/// The extension of a map's source file.
pub const MAP_EXTENSION: &str = "map";
/// The extension of a compiled map, whose entities `map.open` reads when it has no source.
pub const BSP_EXTENSION: &str = "bsp";

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
/// The id of `map.duplicate`.
pub const MAP_DUPLICATE: &str = "map.duplicate";
/// The id of `map.inspect`.
pub const MAP_INSPECT: &str = "map.inspect";
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

/// An entity of a map, by its place in the file from 0, the world first.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct EntityIndex(usize);

impl EntityIndex {
    /// The world, the first entity of every map.
    pub const WORLD: Self = Self(0);

    /// The entity at `index`.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its place in the file.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }

    /// Whether it is the world.
    #[must_use]
    pub const fn is_world(self) -> bool {
        self.0 == Self::WORLD.0
    }
}

impl fmt::Display for EntityIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A count of the changes to the open map and its selection, for a view to know when to read
/// it again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MapRevision(u64);

impl MapRevision {
    /// The revision after this one.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

/// The map open in the map view.
#[derive(Clone, Debug, PartialEq)]
pub struct MapDocument {
    path: PathBuf,
    source: MapSource,
    file: MapFile,
    selected: Option<EntityIndex>,
    unsaved: bool,
    revision: MapRevision,
    edits: MapRevision,
    /// The last value [`MapDocument::edits`] was given, which only grows, so an undo step
    /// knows its own map.
    stamped: MapRevision,
    /// The file the entities were last read from or written to: the `.map`, the `.ent` beside
    /// a BSP, or the BSP (P5-17).
    origin: PathBuf,
    /// Its bytes as last read or written.
    disk: DiskStamp,
    /// The map on disk, when another program changed it while this one had unsaved edits.
    conflict: Option<MapFile>,
    /// The entities hidden and locked in the editor (P5-14).
    view: groups::ViewState,
}

/// What a file held, as a hash of its bytes, to tell when another program changed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DiskStamp(u64);

impl DiskStamp {
    /// The stamp of `bytes`.
    pub(crate) fn of(bytes: &[u8]) -> Self {
        use core::hash::{Hash as _, Hasher as _};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        Self(hasher.finish())
    }
}

/// A map as read from disk.
struct MapRead {
    source: MapSource,
    file: MapFile,
    /// The file its entities came from.
    origin: PathBuf,
    /// That file's bytes.
    disk: DiskStamp,
}

impl MapRead {
    /// The map at `path`: a `.map`, or a BSP's entities, from the `.ent` beside it the engine
    /// reads in place of the lump (BSP-19.1), else the lump.
    fn of(path: &Path) -> Result<Self, WorkbenchError> {
        let bytes = std::fs::read(path).map_err(|_| WorkbenchError::NoFile {
            path: path.to_path_buf(),
        })?;
        let is_bsp = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(BSP_EXTENSION));
        let (source, text, origin, disk) = if is_bsp {
            let bsp =
                Bsp::read(&bytes, BspLoader::default()).map_err(|error| WorkbenchError::Bsp {
                    path: path.to_path_buf(),
                    error,
                })?;
            let ent = ent_files(path, bsp.entity_lump)
                .into_iter()
                .find_map(|ent| std::fs::read(&ent).ok().map(|text| (ent, text)));
            let (text, origin, disk) = match ent {
                Some((ent, text)) => {
                    let disk = DiskStamp::of(&text);
                    (text, ent, disk)
                }
                None => (
                    bsp.entities().to_vec(),
                    path.to_path_buf(),
                    DiskStamp::of(&bytes),
                ),
            };
            (MapSource::Bsp(bytes), text, origin, disk)
        } else {
            let disk = DiskStamp::of(&bytes);
            (MapSource::Map, bytes, path.to_path_buf(), disk)
        };
        let file = MapFile::parse(&text).map_err(|error| WorkbenchError::Map {
            path: origin.clone(),
            error,
        })?;
        Ok(Self {
            source,
            file,
            origin,
            disk,
        })
    }
}

impl MapDocument {
    /// The files another program may change under it: its own and, for a BSP, the `.ent` its
    /// entities came from (P5-17).
    #[must_use]
    pub fn watched(&self) -> Vec<PathBuf> {
        let mut out = vec![self.path.clone()];
        if self.origin != self.path {
            out.push(self.origin.clone());
        }
        out
    }

    /// Its revision, which changes whenever the map or the selection does.
    #[must_use]
    pub const fn revision(&self) -> MapRevision {
        self.revision
    }

    /// Its revision of the map alone, which a selection leaves: a view of the map's contents
    /// reads it again only when this changes; an undo gives it back its earlier value.
    #[must_use]
    pub const fn edits(&self) -> MapRevision {
        self.edits
    }

    /// Marks the selection changed.
    const fn touch(&mut self) {
        self.revision = self.revision.next();
    }

    /// Marks the map changed to a state it has not had.
    const fn touch_map(&mut self) {
        self.stamped = self.stamped.next();
        self.edits = self.stamped;
        self.touch();
    }

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

    /// The selected entity.
    #[must_use]
    pub const fn selected(&self) -> Option<EntityIndex> {
        self.selected
    }

    /// Whether it was edited since it was opened or saved.
    #[must_use]
    pub const fn is_unsaved(&self) -> bool {
        self.unsaved
    }

    /// The `.ent` the engine reads in place of a BSP's entity lump, the first of its names
    /// beside the BSP (BSP-19.1); `None` for a `.map`.
    fn ent_override(&self) -> Option<PathBuf> {
        let MapSource::Bsp(bytes) = &self.source else {
            return None;
        };
        let bsp = Bsp::read(bytes, BspLoader::default()).ok()?;
        ent_files(&self.path, bsp.entity_lump)
            .into_iter()
            .find(|path| path.is_file())
    }

    fn entity_mut(&mut self, entity: EntityIndex) -> Result<&mut Entity, WorkbenchError> {
        self.file
            .entities
            .get_mut(entity.get())
            .ok_or(WorkbenchError::NoEntity { entity })
    }

    /// A map edit's error from `qs-map`, at the open map.
    fn map_error(&self, error: qs_map::MapError) -> WorkbenchError {
        WorkbenchError::Map {
            path: self.path.clone(),
            error,
        }
    }

    /// Moves `entity` by `by`: a point entity's origin; a brush entity's brushes, and its origin
    /// when it has one (MAP-2.7, MAP-3.2).
    fn move_entity(&mut self, entity: EntityIndex, by: [f64; 3]) -> Result<(), WorkbenchError> {
        let found = self.entity_mut(entity)?;
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
        moved.map_err(|error| self.map_error(error))
    }

    /// Turns point entity `entity` by `degrees` about the vertical: its `angles` yaw when it has
    /// `angles`, else its `angle` (EDICT-8.2 reads `angle` as the yaw), kept from 0 to 360.
    fn rotate_entity(&mut self, entity: EntityIndex, degrees: f64) -> Result<(), WorkbenchError> {
        let found = self.entity_mut(entity)?;
        if found.is_brush_entity() || entity.is_world() {
            return Err(WorkbenchError::NotPointEntity { entity });
        }
        let [pitch, yaw, roll] = found.angles().unwrap_or_default();
        if [UP, DOWN].contains(&[pitch, yaw, roll]) {
            return Ok(());
        }
        let yaw = number((yaw + degrees).rem_euclid(FULL_TURN));
        let result = if found.value(b"angles").is_some() {
            let text = [number(pitch), yaw, number(roll)].join(" ");
            found.set(b"angles", text.as_bytes())
        } else {
            found.set(b"angle", yaw.as_bytes())
        };
        result.map_err(|error| self.map_error(error))
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
    pub entity: Option<EntityIndex>,
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
    pub entity: EntityIndex,
    /// How far, in map units.
    pub by: [f64; 3],
}

/// The parameters of `map.rotate`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RotateParams {
    /// The entity, a point entity.
    pub entity: EntityIndex,
    /// Degrees to turn it about the vertical axis, counter-clockwise seen from above.
    pub degrees: f64,
}

/// The parameters of `map.delete` and `map.entity`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntityParams {
    /// The entity.
    pub entity: EntityIndex,
}

/// The parameters of `map.set_key`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetKeyParams {
    /// The entity.
    pub entity: EntityIndex,
    /// The key.
    pub key: String,
    /// Its value.
    pub value: String,
    /// Other entities to set it on too, in the same step.
    #[serde(default)]
    pub also: Vec<EntityIndex>,
}

/// The parameters of `map.remove_key`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoveKeyParams {
    /// The entity.
    pub entity: EntityIndex,
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
    pub entity: Option<EntityIndex>,
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
    /// The spawnflags its class tests or its comment names, by value, each named where the
    /// code or the comment names it.
    pub spawnflags: Vec<FlagInfo>,
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
        let MapRead {
            source,
            file,
            origin,
            disk,
        } = MapRead::of(path)?;
        let stamped = self
            .map
            .as_ref()
            .map_or(MapRevision::default(), |old| old.stamped.next());
        Ok(self.map.insert(MapDocument {
            path: path.to_path_buf(),
            source,
            file,
            selected: None,
            view: groups::ViewState::default(),
            unsaved: false,
            revision: self
                .map
                .as_ref()
                .map_or(MapRevision::default(), |old| old.revision.next()),
            edits: stamped,
            stamped,
            origin,
            disk,
            conflict: None,
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
    pub fn map_entity(&mut self, entity: EntityIndex) -> Result<EntityKeys, WorkbenchError> {
        let document = self.map.as_ref().ok_or(WorkbenchError::NoMap)?;
        let found = document
            .file
            .entities
            .get(entity.get())
            .ok_or(WorkbenchError::NoEntity { entity })?;
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let classname = found.classname().map(text);
        let pairs = found
            .pairs()
            .map(|pair| (text(pair.key()), text(pair.value())))
            .collect();
        let brush = found.is_brush_entity();
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
        let class = match (&classname, self.root().is_some()) {
            (Some(name), true) => self
                .entity_classes()?
                .iter()
                .find(|class| &class.name == name)
                .map(ClassInfo::from),
            _ => None,
        };
        let (keys, spawnflags) = class
            .map(|class| (class.keys, class.spawnflags))
            .unwrap_or_default();
        Ok(EntityKeys {
            classname,
            pairs,
            keys,
            spawnflags,
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
    /// cannot take the lump, [`WorkbenchError::EntOverride`] for the lump when a `.ent` the
    /// engine reads in its place is beside the BSP, [`WorkbenchError::ChangedOnDisk`] when
    /// another program changed the map's file since it was read or saved (P5-17), and
    /// [`WorkbenchError::Save`] when the file cannot be written.
    pub fn save_map(&mut self, how: BspSave) -> Result<PathBuf, WorkbenchError> {
        let document = self.map_mut()?;
        let text = document.file.to_bytes();
        let ent = document.ent_override();
        let (path, bytes) = match &document.source {
            MapSource::Map => (document.path.clone(), text),
            MapSource::Bsp(bsp) => {
                // A lump read from a BSP ends at its NUL (MAP-1.1); the bytes after it are
                // not entities.
                let entities = text.split(|&b| b == 0).next().unwrap_or_default().to_vec();
                match how {
                    BspSave::Ent => (
                        ent.unwrap_or_else(|| document.path.with_extension("ent")),
                        entities,
                    ),
                    BspSave::Lump => {
                        if let Some(path) = ent {
                            return Err(WorkbenchError::EntOverride { path });
                        }
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
        // Saving never overwrites what another program wrote since (P5-17): the file the
        // entities came from, or the BSP read, must hold what was read, and any other file must
        // not be there.
        let known = if path == document.origin {
            Some(document.disk)
        } else if let MapSource::Bsp(read) = &document.source
            && path == document.path
        {
            Some(DiskStamp::of(read))
        } else {
            None
        };
        if let Ok(now) = std::fs::read(&path)
            && known != Some(DiskStamp::of(&now))
        {
            return Err(WorkbenchError::ChangedOnDisk { path });
        }
        std::fs::write(&path, &bytes).map_err(|error| WorkbenchError::Save {
            path: path.clone(),
            error,
        })?;
        document.disk = DiskStamp::of(&bytes);
        document.origin.clone_from(&path);
        document.conflict = None;
        if let (MapSource::Bsp(bsp), BspSave::Lump) = (&mut document.source, how) {
            *bsp = bytes;
        }
        document.unsaved = false;
        Ok(path)
    }
}

/// The `.ent` files the engine looks for, in order, beside the BSP at `path` whose entity
/// lump is `lump` (BSP-19.1).
fn ent_files(path: &Path, lump: &[u8]) -> [PathBuf; 2] {
    ent_file_names(&path.to_string_lossy(), lump).map(PathBuf::from)
}

/// Runs `edit` on the open map of `context` and returns its answer and the step that undoes
/// it, sending the change to the game when one runs on the map (P5-07); an edit that fails
/// leaves the map as it was.
fn edit_map<C: HasWorkbench, T>(
    context: &mut C,
    edit: impl FnOnce(&mut MapDocument) -> Result<T, WorkbenchError>,
) -> Result<(T, MapStep), WorkbenchError> {
    let document = context.workbench().map_mut()?;
    let before = (
        document.file.clone(),
        document.selected,
        document.view.clone(),
    );
    let edits = document.edits;
    match edit(document).and_then(|answer| document.after_edit(&before.0).map(|()| answer)) {
        Ok(answer) => {
            document.unsaved = true;
            document.touch_map();
            let step = MapStep {
                before: edits,
                after: document.edits,
                other: before,
            };
            send_changes(context, &step.other.0);
            Ok((answer, step))
        }
        Err(error) => {
            (document.file, document.selected, document.view) = before;
            Err(error)
        }
    }
}

/// Sends what changed from the entities of `before` to the open map's to the game of
/// `context`, when it runs on that map (P5-07, MAP-12).
fn send_changes<C: HasWorkbench>(context: &mut C, before: &MapFile) {
    let Some(path) = context.workbench().map.as_ref().map(|d| d.path.clone()) else {
        return;
    };
    if !context
        .preview()
        .is_some_and(|preview| preview.plays_in_step(&path))
    {
        return;
    }
    let Some(document) = context.workbench().map.as_ref() else {
        return;
    };
    let changes = LiveChange::between(&before.entities, &document.file.entities);
    if let Some(preview) = context.preview()
        && !changes.is_empty()
    {
        preview.send_map_changes(&path, &before.entities, &changes);
    }
}

/// The open map's other state: what undo puts back, then what redo does.
struct MapStep {
    /// The map's [`MapDocument::edits`] before the edit, which undo gives back.
    before: MapRevision,
    /// Its edits after, which redo gives back. A map opened since has had neither.
    after: MapRevision,
    other: (MapFile, Option<EntityIndex>, groups::ViewState),
}

impl MapStep {
    /// Swaps the map and selection of the open map at `from` edits with the other state,
    /// leaving it at `to`.
    fn apply<C: HasWorkbench>(
        &mut self,
        context: &mut C,
        from: MapRevision,
        to: MapRevision,
    ) -> Result<(), BoxError> {
        let document = context
            .workbench()
            .map
            .as_mut()
            .filter(|document| document.edits == from)
            .ok_or(WorkbenchError::NoMap)?;
        core::mem::swap(&mut document.file, &mut self.other.0);
        core::mem::swap(&mut document.selected, &mut self.other.1);
        core::mem::swap(&mut document.view, &mut self.other.2);
        document.unsaved = true;
        document.edits = to;
        document.touch();
        send_changes(context, &self.other.0);
        Ok(())
    }
}

impl<C: HasWorkbench + 'static> UndoStep<C> for MapStep {
    fn undo(&mut self, context: &mut C) -> Result<(), BoxError> {
        self.apply(context, self.after, self.before)
    }

    fn redo(&mut self, context: &mut C) -> Result<(), BoxError> {
        self.apply(context, self.before, self.after)
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
        document.touch();
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
        let (entity, step) = edit_map(context, |document| {
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
                    .map_err(|error| document.map_error(error))?;
            }
            document.file.push_entity(entity);
            let index = EntityIndex::new(document.file.entities.len().saturating_sub(1));
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
        let ((), step) = edit_map(context, |document| {
            document.move_with_group(params.entity, params.by)
        })?;
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
        let ((), step) = edit_map(context, |document| {
            document.rotate_entity(params.entity, params.degrees)
        })?;
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
        let ((), step) = edit_map(context, |document| {
            document.entity_mut(entity)?;
            if entity.is_world() {
                return Err(WorkbenchError::NotPointEntity { entity });
            }
            document.editable(entity)?;
            document.file.entities.remove(entity.get());
            document.view.deleted(entity);
            document.selected = match document.selected {
                Some(s) if s == entity => None,
                Some(s) if s > entity => Some(EntityIndex::new(s.get().saturating_sub(1))),
                other => other,
            };
            Ok(())
        })?;
        Ok(Outcome::undoable(MapEdited { entity: None }, step))
    }
}

/// `map.duplicate`.
struct MapDuplicate;

impl<C: HasWorkbench + 'static> Command<C> for MapDuplicate {
    const ID: &'static str = MAP_DUPLICATE;
    const SUMMARY: &'static str = "Duplicates an entity of the open map, its keys and brushes, \
        in place at the end of the map, and selects the copy; the world is not duplicated. A \
        targetname ending in a number takes the next free number, and the keys the code fires \
        entities by move on by as many.";
    type Params = EntityParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: EntityParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let entity = params.entity;
        let targeting = context.workbench().targeting_keys();
        let (copy, step) = edit_map(context, |document| {
            if entity.is_world() {
                return Err(WorkbenchError::NotPointEntity { entity });
            }
            let mut copy = Entity::new();
            copy.items.clone_from(&document.entity_mut(entity)?.items);
            document.number_onward(&mut copy, &targeting)?;
            document.file.push_entity(copy);
            let index = EntityIndex::new(document.file.entities.len().saturating_sub(1));
            document.selected = Some(index);
            Ok(index)
        })?;
        Ok(Outcome::undoable(MapEdited { entity: Some(copy) }, step))
    }
}

/// `map.set_key`.
struct MapSetKey;

impl<C: HasWorkbench + 'static> Command<C> for MapSetKey {
    const ID: &'static str = MAP_SET_KEY;
    const SUMMARY: &'static str = "Sets a key of an entity of the open map, and of the \
        entities in also with it in one step: its last occurrence keeps its place and \
        formatting, a new key goes after the others.";
    type Params = SetKeyParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: SetKeyParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            for entity in core::iter::once(params.entity).chain(params.also.iter().copied()) {
                let set = document
                    .entity_mut(entity)?
                    .set(params.key.as_bytes(), params.value.as_bytes());
                set.map_err(|error| document.map_error(error))?;
            }
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
        let ((), step) = edit_map(context, |document| {
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
    registry.register(MapDuplicate)?;
    registry.register(MapSetKey)?;
    registry.register(MapRemoveKey)?;
    registry.register(MapEntity)?;
    registry.register(inspect::MapInspect)?;
    registry.register(disk::FilesChangedCommand)?;
    registry.register(disk::MapResolve)?;
    registry.register(disk::MapCompare)?;
    registry.register(filter::MapFilter)?;
    registry.register(links::MapLink)?;
    registry.register(flags::MapSetFlag)?;
    registry.register(groups::MapView::<groups::Hide>::new())?;
    registry.register(groups::MapView::<groups::Isolate>::new())?;
    registry.register(groups::MapView::<groups::Lock>::new())?;
    registry.register(groups::ShowAll)?;
    registry.register(groups::UnlockAll)?;
    registry.register(groups::MapSetLayer)?;
    registry.register(groups::MapLayers)?;
    registry.register(groups::MapExport)?;
    registry.register(MapSave)
}

#[cfg(test)]
mod tests;
