// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiling the open map (P6-11, P6-13, MAP-7, MAP-16): `map.find_leaks` compiles it in memory
//! with `qs-map-bsp` and reports whether it leaks, the leak's trail from the entity the outside
//! reached out to the void (QBSP-9.8), and the textures its WADs lack; `map.compile` does the
//! same and writes the BSP, portal and leak files beside the map, as id's qbsp names them.
//! Both read the map as it is in the editor, saved or not, without the layers left out of
//! exports (MAP-9), and its WADs from the project, the map's folder or the game, as the
//! texture browser finds them. The last report is kept with the map, for the map view.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use qs_bsp::{Bsp, BspLoader};
use qs_commands::{Command, NoParams, Outcome};
use qs_engine::{ClientError, ErrorCode};
use qs_map::qbsp::QbspWarning;
use qs_map_bsp::{Compiled, Compiler, Warning};
use qs_map_light::{Extras, Finish, Light, Sampling, Sidecars};
use qs_sim::DEFAULT_VIEWHEIGHT;
use qs_sim::collision::{CONTENTS_SOLID, HullSet};
use qs_wad::Wad;
use qs_wad::write::WadBuilder;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::preview::{GameStart, HasPreview, PreviewError};
use crate::{Workbench, WorkbenchError};
use qs_map::{Region, Surface, TexDef};
use qs_map_edit::textures::{Game, GameTextures, find_wad, light_palette};
use qs_map_edit::{EntityIndex, MapEditError, MapReport, MapSource};

/// The id of `map.find_leaks`.
pub const MAP_LEAKS: &str = "map.find_leaks";
/// The id of `map.compile`.
pub const MAP_COMPILE: &str = "map.compile";
/// The id of `map.play_from_here`.
pub const MAP_PLAY_FROM_HERE: &str = "map.play_from_here";
/// The id of `map.portals`.
pub const MAP_PORTALS: &str = "map.portals";

impl<L: crate::Loader> Workbench<L> {
    /// The open map compiled in memory, its WADs found in the project, beside the map or in
    /// the game at `game`; its report, kept with the map; and the map's path. With `region`
    /// only that part is compiled, sealed (MAP-16), and its report is not kept as the map's.
    pub(crate) fn compile_map(
        &mut self,
        game: Option<&Path>,
        region: Option<&CompileRegion>,
    ) -> Result<(Compiled, MapReport, PathBuf), WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        if let MapSource::Bsp(_) = document.source() {
            return Err(WorkbenchError::NoMapSource {
                path: document.path().to_path_buf(),
            });
        }
        let places = document.external_folders(self.root());
        let mut load = |name: &str| {
            places
                .iter()
                .find_map(|dir| std::fs::read(dir.join(name)).ok())
        };
        // The region's rooms are named by their places in the map as the editor holds it, before
        // its omitted layers are taken out.
        let region = region
            .map(|region| region.of(document.file()))
            .transpose()?;
        let mut file = document.file().without_omitted_layers();
        // The seal's `skip` is not the map's: a WAD with no `skip` is not reported for it.
        let seal_only = region.is_some() && !uses_texture(&file, SEAL_TEXTURE);
        if let Some(region) = &region {
            file = region
                .cut(&file, &CompileRegion::seal())
                .map_err(WorkbenchError::RegionSeal)?;
        }
        let text = file.to_bytes();
        let compiler = Compiler::default().with_dialect(qs_map_bsp::Dialect::Extended);
        let failed = |error| WorkbenchError::MapCompile { error };
        // The world's `_wad` or `wad`, split at `;` as the texture browser and ericw-tools read
        // it (MAP-6.3). The compiler takes one WAD: one found is passed as it is; several are
        // merged, the first texture of a name kept.
        let game = game.map(Game::at);
        let mut found: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        let mut missing_wads = Vec::new();
        for raw in file.wad_paths(qs_map::Compiler::EricwTools) {
            let name = String::from_utf8_lossy(&raw).into_owned();
            match find_wad(&name, self.root(), document.path().parent(), game.as_ref()) {
                Some((path, bytes)) => found.push((path.unwrap_or_else(|| name.into()), bytes)),
                None => missing_wads.push(name),
            }
        }
        // P6-43: a texture no WAD of the world has is looked for in the game's own levels,
        // whose BSPs hold the game's textures; they are read in place and passed on in memory.
        if let Some(game) = &game {
            let lacking = lacking_textures(&file, &found);
            let levels = (!lacking.is_empty()).then(|| GameTextures::read(game));
            if let Some(wad) = levels.and_then(|levels| levels.wad_of(lacking)) {
                found.push((PathBuf::from(GAME_LEVELS), wad));
            }
        }
        let wad = match found.len() {
            0 => None,
            1 => found.pop().map(|(_, bytes)| bytes),
            _ => Some(merged(&found)?),
        };
        let mut compiled = compiler
            .compile_with_externals(&text, wad.as_deref(), &mut load)
            .map_err(failed)?;
        if seal_only {
            compiled.warnings.retain(|warning| {
                !matches!(warning, Warning::Map(QbspWarning::TextureNotFound { name })
                    if name.as_slice() == SEAL_TEXTURE)
            });
        }
        let report = MapReport::of(&compiled, missing_wads);
        let path = document.path().to_path_buf();
        if region.is_none() {
            let edits = document.edits();
            self.maps.keep_report(report.clone(), edits)?;
        }
        Ok((compiled, report, path))
    }
}

/// The part of the open map a compile takes (MAP-16), when it is not the whole map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompileRegion {
    /// The rooms these entities are, or are in, innermost: a layer's or group's own entity is
    /// that room, any other entity the room around it (MAP-9.4). The region is the box around
    /// their bounds (MAP-16.1).
    Rooms(Vec<EntityIndex>),
    /// The box from `min` to `max`.
    Box {
        /// Its least corner.
        min: [f64; 3],
        /// Its greatest corner.
        max: [f64; 3],
    },
}

impl CompileRegion {
    /// The region of `file` it names.
    ///
    /// # Errors
    ///
    /// [`MapEditError::NoEntity`] for an entity past the map's,
    /// [`WorkbenchError::NotInRoom`] for one in no room, and [`WorkbenchError::NoRegion`] for a
    /// region with no volume.
    fn of(&self, file: &qs_map::MapFile) -> Result<Region, WorkbenchError> {
        let region = match self {
            Self::Box { min, max } => Region::new(*min, *max),
            Self::Rooms(entities) => {
                let rooms = file.rooms();
                let mut around = Vec::with_capacity(entities.len());
                for &entity in entities {
                    let found = file
                        .entities
                        .get(entity.get())
                        .ok_or(WorkbenchError::MapEdit(MapEditError::NoEntity { entity }))?;
                    let room = rooms.of(found).first().copied();
                    around.push(room.ok_or(WorkbenchError::NotInRoom { entity })?);
                }
                Region::of_rooms(around)
            }
        };
        region.ok_or(WorkbenchError::NoRegion)
    }

    /// The surface the region is sealed with (MAP-16.4): `skip`, solid and never drawn, so the
    /// cut draws nothing and lights nothing, and the player sees the void where the region
    /// ends.
    fn seal() -> Surface {
        Surface {
            texture: SEAL_TEXTURE.to_vec(),
            texdef: TexDef::Standard {
                shift: [0.0; 2],
                rotate: 0.0,
                scale: [1.0; 2],
            },
        }
    }
}

/// The texture a region is sealed with (MAP-16.4).
const SEAL_TEXTURE: &[u8] = b"skip";
/// What a region compile's files are named after, beside the map: `<map>-region.bsp`, so the
/// whole map's compile is not written over (MAP-16.5).
const REGION_SUFFIX: &str = "-region";

/// What the WAD of the game's levels' textures is called in an error about it.
const GAME_LEVELS: &str = "the game's levels";

/// The textures the faces of `file` name that no WAD of `wads` holds, each once in any case;
/// a WAD that does not read holds none, and is reported when the WADs are merged.
fn lacking_textures<'f>(file: &'f qs_map::MapFile, wads: &[(PathBuf, Vec<u8>)]) -> Vec<&'f [u8]> {
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    for (_, bytes) in wads {
        if let Ok(wad) = Wad::read(bytes) {
            seen.extend(
                wad.lumps()
                    .iter()
                    .map(|lump| lump.name.as_bytes().to_ascii_lowercase()),
            );
        }
    }
    let mut lacking = Vec::new();
    for entity in &file.entities {
        for brush in entity.brushes() {
            for face in &brush.faces {
                if seen.insert(face.texture().to_ascii_lowercase()) {
                    lacking.push(face.texture());
                }
            }
        }
    }
    lacking
}

/// Whether any face of `file` names `texture`, in any case.
fn uses_texture(file: &qs_map::MapFile, texture: &[u8]) -> bool {
    file.entities.iter().any(|entity| {
        entity.brushes().any(|brush| {
            brush
                .faces
                .iter()
                .any(|face| face.texture().eq_ignore_ascii_case(texture))
        })
    })
}

/// One WAD of the texture lumps of `wads`, each a path and its bytes, in order, the first of
/// a name kept.
fn merged(wads: &[(PathBuf, Vec<u8>)]) -> Result<Vec<u8>, WorkbenchError> {
    let mut builder = WadBuilder::new();
    let mut taken: HashSet<Vec<u8>> = HashSet::new();
    for (path, bytes) in wads {
        let unreadable = |error| WorkbenchError::Wad {
            path: path.clone(),
            error,
        };
        let unwritable = |error| WorkbenchError::WadWrite {
            path: path.clone(),
            error,
        };
        let wad = Wad::read(bytes).map_err(unreadable)?;
        for lump in wad.lumps() {
            if !taken.insert(lump.name.as_bytes().to_ascii_lowercase()) {
                continue;
            }
            let data = wad.data(lump).map_err(unreadable)?;
            builder
                .add(lump.name.as_bytes(), lump.lump_type, data.to_vec())
                .map_err(unwritable)?;
        }
    }
    builder
        .to_bytes()
        .map_err(|error| WorkbenchError::WadWrite {
            path: wads.first().map_or_default(|(path, _)| path.clone()),
            error,
        })
}

/// `map.find_leaks`.
pub(crate) struct MapLeaks;

impl<C: HasPreview + 'static> Command<C> for MapLeaks {
    const ID: &'static str = MAP_LEAKS;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Compiles the open map in memory and reports whether it leaks, \
        with the trail from the entity the outside reaches out to the void, and the textures \
        its WADs lack.";
    type Params = NoParams;
    type Output = MapReport;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<MapReport, C>, WorkbenchError> {
        let (workbench, preview) = context.workbench_and_preview();
        let game = preview.game_base_path();
        let (_, report, _) = workbench.compile_map(game.as_deref(), None)?;
        Ok(Outcome::outside_history(report))
    }
}

/// The parameters of `map.compile`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct MapCompileParams {
    /// Only this part of the map, sealed (MAP-16); the whole map when not given.
    #[serde(default)]
    pub region: Option<CompileRegion>,
}

/// `map.compile`.
pub(crate) struct MapCompile;

impl<C: HasPreview + 'static> Command<C> for MapCompile {
    const ID: &'static str = MAP_COMPILE;
    const SUMMARY: &'static str = "Compiles the open map, as it is in the editor, or only a \
        region of it, sealed, to a BSP beside it, with its portal and leak files, and reports \
        whether it leaks and the textures its WADs lack.";
    type Params = MapCompileParams;
    type Output = MapReport;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: MapCompileParams,
    ) -> Result<Outcome<MapReport, C>, WorkbenchError> {
        let (workbench, preview) = context.workbench_and_preview();
        let game = preview.game_base_path();
        let (compiled, mut report, path) =
            workbench.compile_map(game.as_deref(), params.region.as_ref())?;
        let path = if params.region.is_some() {
            let stem = path
                .file_stem()
                .map_or_default(|stem| stem.to_string_lossy().into_owned());
            path.with_file_name(format!("{stem}{REGION_SUFFIX}.map"))
        } else {
            path
        };
        let files = [
            ("bsp", Some(&compiled.bsp)),
            ("prt", compiled.portal_file.as_ref()),
            ("pts", compiled.leak_file.as_ref()),
        ];
        for (extension, bytes) in files {
            let file = path.with_extension(extension);
            let failed = |error| WorkbenchError::Save {
                path: file.clone(),
                error,
            };
            let Some(bytes) = bytes else {
                // A file of an earlier compile would be read as this one's (P6-20).
                match std::fs::remove_file(&file) {
                    Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                        return Err(failed(error));
                    }
                    Ok(()) | Err(_) => continue,
                }
            };
            std::fs::write(&file, bytes).map_err(failed)?;
            report.written.push(file);
        }
        Ok(Outcome::outside_history(report))
    }
}

/// The portals of the open map's last compile (P6-20, MAP-28).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapPortals {
    /// The portal file read.
    pub path: PathBuf,
    /// Each portal's points, in order.
    pub portals: Vec<Vec<[f32; 3]>>,
}

/// `map.portals`.
pub(crate) struct MapPortalsCommand;

impl<C: HasPreview + 'static> Command<C> for MapPortalsCommand {
    const ID: &'static str = MAP_PORTALS;
    const SUMMARY: &'static str = "Reads the portal file the open map's last compile wrote \
        beside it, each portal's points, to show where the map's leaves meet.";
    type Params = NoParams;
    type Output = MapPortals;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<MapPortals, C>, WorkbenchError> {
        let document = context.workbench().map().ok_or(WorkbenchError::NoMap)?;
        let path = document.path().with_extension("prt");
        let text = std::fs::read_to_string(&path)
            .map_err(|_| WorkbenchError::NoFile { path: path.clone() })?;
        let portals =
            qs_map_vis::portal_windings(&text).map_err(|error| WorkbenchError::PortalFile {
                path: path.clone(),
                error,
            })?;
        Ok(Outcome::unchanged(MapPortals { path, portals }))
    }
}

/// The parameters of `map.play_from_here`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayFromHereParams {
    /// Where the player's eye goes: the map view's camera. The player's origin is
    /// `view_ofs` below it, moved up or down to the nearest place its box fits. When not
    /// given the player starts where the map's own first `info_player_start` stands, as
    /// the game starts the map; a map with none is refused.
    #[serde(default)]
    pub eye: Option<[f64; 3]>,
    /// Which way the player faces, pitch, yaw and roll in degrees; when not given, the way
    /// the player start faces with no eye, and ahead along x with one.
    #[serde(default)]
    pub angles: Option<[f64; 3]>,
    /// Only this part of the map, sealed, so the room being worked on compiles at once
    /// (MAP-16); the whole map when not given.
    #[serde(default)]
    pub region: Option<CompileRegion>,
    /// Whether the game runs or is left paused where the player was put. Paused, the game
    /// starts paused, the player is put in place and one frame is run to show the view from
    /// there: `engine.screenshot` then shows the map with every monster where the map put
    /// it, not yet awake, which is the picture to review a room by; `preview.resume` plays
    /// on. Running when not given.
    #[serde(default)]
    pub start: GameStart,
}

/// How far, in whole units up or down, [`PlayFromHereParams::origin_in`] looks for room for
/// the player's box: as far as the box is tall above its origin and then some, so a camera
/// near a floor or a ceiling still finds the room beside it.
const PLAYER_ROOM_SEARCH: u8 = 64;

/// Hull 1, the clipping hull of the player's box, (-16, -16, -24) to (16, 16, 32) (BSP-11.3).
const PLAYER_HULL: usize = 1;

/// The classname of the entity a map starts its player at.
const PLAYER_START: &[u8] = b"info_player_start";

impl PlayFromHereParams {
    /// The eye and the facing to play `file` from: those given, or with no eye the map's
    /// first `info_player_start`, the eye `view_ofs` above its origin and the facing its
    /// own unless one is given (L-52).
    ///
    /// # Errors
    ///
    /// [`WorkbenchError::NoPlayerStart`] when no eye is given and the map has no start.
    pub fn view_in(
        &self,
        file: &qs_map::MapFile,
    ) -> Result<([f64; 3], Option<[f64; 3]>), WorkbenchError> {
        if let Some(eye) = self.eye {
            return Ok((eye, self.angles));
        }
        let start = file
            .entities
            .iter()
            .find(|entity| entity.classname() == Some(PLAYER_START))
            .ok_or(WorkbenchError::NoPlayerStart)?;
        let [x, y, z] = start.origin();
        let eye = [x, y, z + f64::from(DEFAULT_VIEWHEIGHT)];
        Ok((eye, self.angles.or_else(|| start.angles())))
    }

    /// Where the player's origin goes in `bsp` for the eye `eye`: `view_ofs` (id's
    /// `'0 0 22'`, NET-4.4) below the eye, at the nearest height within [`PLAYER_ROOM_SEARCH`] units where hull 1, the
    /// player's box, is not in solid; looked for first above, then below, a unit at a time.
    ///
    /// # Errors
    ///
    /// [`PreviewError::NoRoom`] when the box fits nowhere along that line, or the eye is not a
    /// point the game can hold.
    pub fn origin_in(eye: [f64; 3], bsp: &Bsp<'_>) -> Result<[f32; 3], PreviewError> {
        let no_room = || PreviewError::NoRoom(eye);
        let [x, y, eye] = eye.map(single);
        let z = eye - DEFAULT_VIEWHEIGHT;
        if ![x, y, z].iter().all(|c| c.is_finite()) {
            return Err(no_room());
        }
        let [h0, h1, h2, _] = bsp.models.first().map_or_default(|model| model.headnode);
        let hulls = HullSet::new(bsp);
        let hull = hulls.hull(PLAYER_HULL, [h0, h1, h2]);
        let fits = |p: [f32; 3]| {
            hull.point_contents(hull.first, p)
                .is_ok_and(|contents| contents != CONTENTS_SOLID)
        };
        (0..=PLAYER_ROOM_SEARCH)
            .flat_map(|step| [f32::from(step), -f32::from(step)])
            .map(|lift| [x, y, z + lift])
            .find(|&p| fits(p))
            .ok_or_else(no_room)
    }
}

/// The light below which a place is dark: the level id raises the player's own weapon to, so
/// that it can be seen (REND-4.11).
const DARK_BELOW: i32 = 24;

/// The classnames that start with this are lights.
const LIGHT_CLASS: &[u8] = b"light";

/// A place of a lit map and the light there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlaceLight {
    /// The classname of the entity that stands there.
    pub classname: String,
    /// Its origin.
    pub origin: [f32; 3],
    /// The light of the floor below it, as the game shades a model standing there
    /// (REND-4.10): 0 is black, about 100 a lit room, 255 the most a lightmap holds.
    pub light: i32,
}

/// The classnames that start with this are where a player enters the map.
const PLAYER_START_CLASS: &[u8] = b"info_player";
/// The first light style a switchable light is given (LIGHT-6): below it a style is one
/// of the fixed patterns, which are never off.
const FIRST_SWITCHABLE_STYLE: u32 = 32;
/// `START_OFF`, the spawnflag a light starts off with: id's `light` (`misc.qc` of QuakeC
/// 1.01) runs `lightstyle(self.style, "a")` for it.
const START_OFF: u32 = 1;

/// A whole number a map entity's key holds, 0 for none or one that is not a number.
fn key_number(entity: &qs_map::Entity, key: &[u8]) -> u32 {
    entity
        .value(key)
        .and_then(|value| std::str::from_utf8(value).ok())
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

impl PlaceLight {
    /// The light at `origin` of the lit map `bsp` as a game just started lights it: every
    /// style as the map was lit, but those of `off`, the switchable lights that start off.
    #[must_use]
    pub fn at(bsp: &Bsp<'_>, off: &[u8], origin: [f32; 3]) -> i32 {
        let scale = |style: u8| {
            if off.contains(&style) {
                0
            } else {
                qs_client::UNSET_STYLE_SCALE
            }
        };
        qs_client::light_point(bsp, &scale, origin)
    }

    /// The light styles of `lump`'s lights that start off: each light with a switchable
    /// style and the `START_OFF` spawnflag.
    fn styles_off(lump: &qs_map::MapFile) -> Vec<u8> {
        lump.entities
            .iter()
            .filter(|entity| {
                entity
                    .classname()
                    .is_some_and(|name| name.starts_with(LIGHT_CLASS))
                    && key_number(entity, b"spawnflags") & START_OFF != 0
            })
            .map(|entity| key_number(entity, b"style"))
            .filter(|&style| style >= FIRST_SWITCHABLE_STYLE)
            .filter_map(|style| u8::try_from(style).ok())
            .collect()
    }

    /// The light where the player was put, at `origin`, and the entities of the lit map `bsp`
    /// that stand in the dark: each entity of its entity lump, which is what the game spawns,
    /// that the game draws or a player enters at, where the light is below [`DARK_BELOW`].
    /// `drawn` says whether a classname's entities are drawn, which the project's QuakeC
    /// tells by the model its spawn function sets; a path corner, a trigger's target or a
    /// sound has nothing to see. An entity with a model of the map's is passed over: its
    /// origin is not where it stands.
    ///
    /// # Errors
    ///
    /// The lump's error when it does not parse, so that no list is not read as no dark.
    pub fn around(
        bsp: &Bsp<'_>,
        origin: [f32; 3],
        drawn: &dyn Fn(&str) -> bool,
    ) -> Result<(i32, Vec<Self>), qs_map::MapError> {
        let lump = qs_map::MapFile::parse(bsp.entity_lump)?;
        let off = Self::styles_off(&lump);
        let mut dark = Vec::new();
        for entity in lump.entities.iter().skip(1) {
            let Some(classname) = entity.classname() else {
                continue;
            };
            let name = String::from_utf8_lossy(classname);
            let seen = classname.starts_with(PLAYER_START_CLASS) || drawn(&name);
            if !seen || entity.value(b"origin").is_none() || entity.submodel().is_some() {
                continue;
            }
            let origin = entity.origin().map(single);
            let light = Self::at(bsp, &off, origin);
            if light < DARK_BELOW {
                dark.push(Self {
                    classname: name.into_owned(),
                    origin,
                    light,
                });
            }
        }
        Ok((Self::at(bsp, &off, origin), dark))
    }
}

/// What `map.play_from_here` played.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayedHere {
    /// The map's name, as the game's `map` command takes it.
    pub map: String,
    /// Where the game put the player: its origin, in the map's units. It is the eye less
    /// the view height when the player's box is free there, else the nearest height above
    /// or below where it is free of the world and of every entity (BRIDGE-4.19).
    pub origin: [f32; 3],
    /// What compiling it found.
    pub report: MapReport,
    /// What lighting it warned of, as `light` prints each.
    pub light_warnings: Vec<String>,
    /// The light where the player was put (REND-4.10): 0 is black, about 100 a lit room.
    /// Below 24 the place is too dark to see by, and a picture of it shows little. It is the
    /// fast light's, as the game starts with: the lights' own light with no bounce, and a
    /// switchable light that starts off as dark. The full light swapped in afterwards can
    /// only be brighter where the map sets bounce.
    pub light: i32,
    /// The entities that stand in the dark, a light below 24 by the same measure: the
    /// player starts, and the monsters, items and other things the game draws. A room is
    /// dark when what stands in it is; add a light there, or raise one's `light` key.
    pub dark: Vec<PlaceLight>,
}

/// A float of a map, as the game's single precision holds it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the game holds positions and angles in single precision"
)]
const fn single(value: f64) -> f32 {
    value as f32
}

/// `count` of a thing named `one`, or `many` when it is not one of them.
fn counted(count: usize, one: &str, many: &str) -> String {
    let name = if count == 1 { one } else { many };
    format!("{count} {name}")
}

/// The status line of a map played with something to say: its light warnings and what stands
/// in the dark, after whether the game runs or waits; none when there is neither.
fn played_status(
    name: &str,
    start: GameStart,
    warnings: &[String],
    dark: &[PlaceLight],
) -> Option<String> {
    let mut found = Vec::new();
    if let Some(first) = warnings.first() {
        let count = counted(warnings.len(), "light warning", "light warnings");
        found.push(format!("{count}, the first {first}"));
    }
    if let Some(first) = dark.first() {
        let count = counted(dark.len(), "entity stands", "entities stand");
        found.push(format!(
            "{count} in the dark, the first a {}",
            first.classname
        ));
    }
    if found.is_empty() {
        return None;
    }
    let verb = match start {
        GameStart::Running => "Playing",
        GameStart::Paused => "Paused in",
    };
    Some(format!("{verb} {name}: {}", found.join("; ")))
}

/// `map.play_from_here`.
pub(crate) struct MapPlayFromHere;

impl<C: HasPreview + 'static> Command<C> for MapPlayFromHere {
    const ID: &'static str = MAP_PLAY_FROM_HERE;
    const SUMMARY: &'static str = "Compiles the open map fast, with no vis and fast light, into \
        the project's build, starts the game on it and puts the player's eye at a point, facing \
        a way: the map view's camera (MAP-13), or with no eye the map's own player start. It \
        answers where the player was put, the light \
        there and the entities that stand in the dark; with start paused the game waits there \
        with its monsters not yet awake, for a screenshot of the map as it was built.";
    type Params = PlayFromHereParams;
    type Output = PlayedHere;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        params: PlayFromHereParams,
    ) -> Result<Outcome<PlayedHere, C>, PreviewError> {
        let (workbench, preview) = context.workbench_and_preview();
        let document = workbench
            .map()
            .ok_or(PreviewError::Build(WorkbenchError::NoMap))?;
        let (eye, angles) = params
            .view_in(document.file())
            .map_err(PreviewError::Build)?;
        // A region the camera is outside would put the player in the seal: refused before
        // anything is compiled or written.
        if let Some(region) = &params.region {
            let region = region.of(document.file()).map_err(PreviewError::Build)?;
            if !region.holds(eye) {
                return Err(PreviewError::Build(WorkbenchError::EyeOutsideRegion));
            }
        }
        let game = preview.game_base_path();
        let (compiled, report, path) = workbench
            .compile_map(game.as_deref(), params.region.as_ref())
            .map_err(PreviewError::Build)?;
        // The quick light, without dirt and bounce (MAP-13, LIGHT-8.8), with what the compile
        // tells light of each face and leaf (QBSP-15.54, QBSP-15.55).
        let sidecars = Sidecars::new(compiled.surfaces, compiled.leaves);
        // The game's palette colours what glass and a projected texture give (LIGHT-11.6).
        let palette = light_palette(workbench.root(), game.as_deref());
        let extras = Extras {
            palette: palette.as_deref(),
            ..sidecars.extras()
        };
        let lit = Light::new(Sampling::Fast)
            .with_finish(Finish::Quick)
            .run_with(&compiled.bsp, extras)
            .map_err(|error| PreviewError::Build(WorkbenchError::MapLight { error }))?;
        let bsp = Bsp::read(&lit.bsp, BspLoader::default()).map_err(|error| {
            PreviewError::Build(WorkbenchError::MapEdit(MapEditError::Bsp {
                path: path.clone(),
                error,
            }))
        })?;
        let built = workbench.built_program()?;
        let (project, mod_name) = (&built.project, &built.mod_name);
        let name = path
            .file_stem()
            .map_or_default(|stem| stem.to_string_lossy().into_owned());
        let maps = project.join("build").join(mod_name).join("maps");
        std::fs::create_dir_all(&maps).map_err(|error| PreviewError::Write(maps.clone(), error))?;
        let files = [
            ("bsp", Some(&lit.bsp)),
            ("lit", lit.lit.as_ref()),
            ("ent", None),
        ];
        for (extension, bytes) in files {
            // A name with a dot of its own keeps it: `e1m1.v2` is `e1m1.v2.bsp`.
            let file = maps.join(format!("{name}.{extension}"));
            match bytes {
                Some(bytes) => std::fs::write(&file, bytes)
                    .map_err(|error| PreviewError::Write(file.clone(), error))?,
                // A `.lit` from an earlier play would colour this one wrongly (BSP-18.2), and
                // an `.ent` would people it with another compile's entities.
                None => {
                    if file.exists() {
                        std::fs::remove_file(&file)
                            .map_err(|error| PreviewError::Write(file.clone(), error))?;
                    }
                }
            }
        }
        // The map is written first, as a compile writes it; a camera with no room for the
        // player then plays nothing.
        let origin = PlayFromHereParams::origin_in(eye, &bsp)?;
        // What is drawn is what the QuakeC gives a model; read before the game starts, so
        // that nothing plays when it cannot be told.
        let drawn: HashSet<String> = workbench
            .entity_classes()
            .map_err(PreviewError::Build)?
            .into_iter()
            .filter(|class| class.model.is_some())
            .map(|class| class.name)
            .collect();
        let entity_lump = |error| {
            PreviewError::Build(WorkbenchError::MapEdit(MapEditError::Map {
                path: path.clone(),
                error,
            }))
        };
        // The player's own light is read again where the game puts it.
        let (_, dark) =
            PlaceLight::around(&bsp, origin, &|name| drawn.contains(name)).map_err(entity_lump)?;
        preview.play_built(built, Some(name.clone()), params.start)?;
        let client = preview.client().ok_or(PreviewError::NotRunning)?;
        let angles = angles.unwrap_or_default().map(single);
        // BRIDGE-4.19: the game puts the player where its box is free of the map's entities
        // too, which the BSP's hull does not hold, and says where.
        let origin = match client.place_player_near(origin, angles) {
            Ok(placed) => placed,
            // An entity fills the place and every height near it: as with a camera in a
            // wall, nothing plays.
            Err(ClientError::Rpc {
                code: ErrorCode::InvalidParams,
                ..
            }) => {
                preview.stop();
                return Err(PreviewError::NoRoom(eye));
            }
            Err(error) => return Err(error.into()),
        };
        if params.start == GameStart::Paused {
            // One frame, so the client's view is the player's new place; no monster has
            // moved in it. A game that cannot be stepped is not left running half started.
            if let Err(error) = preview.step() {
                preview.stop();
                return Err(error);
            }
        }
        // P7-02: full vis and full light of the same BSP, swapped in when done; a map that leaks
        // has no portals to see through, and plays with the fast compile.
        match compiled.portal_file {
            Some(portals) => preview.start_full_build(
                name.clone(),
                maps.join(format!("{name}.bsp")),
                (compiled.bsp, portals, sidecars, palette),
            ),
            None => preview.note(
                "The map leaks: it plays with the fast compile, and its full compile waits for \
                 it to be sealed",
            ),
        }
        let light_warnings: Vec<String> = lit.warnings.iter().map(ToString::to_string).collect();
        let (light, _) = PlaceLight::around(&bsp, origin, &|_| false).map_err(entity_lump)?;
        if let Some(found) = played_status(&name, params.start, &light_warnings, &dark) {
            preview.set_status(found);
        }
        Ok(Outcome::outside_history(PlayedHere {
            map: name,
            origin,
            report,
            light_warnings,
            light,
            dark,
        }))
    }
}

#[cfg(test)]
pub(crate) mod tests;
