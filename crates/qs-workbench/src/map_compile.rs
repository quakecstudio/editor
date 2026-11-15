// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiling the open map (P6-11, P6-13, MAP-7, MAP-16): `map.leaks` compiles it in memory
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
use qs_map_bsp::{Compiled, Compiler};
use qs_map_light::{Light, Sampling};
use qs_sim::DEFAULT_VIEWHEIGHT;
use qs_sim::collision::{CONTENTS_SOLID, HullSet};
use qs_wad::Wad;
use qs_wad::write::WadBuilder;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::preview::{HasPreview, PreviewError};
use crate::{Workbench, WorkbenchError};
use qs_map_edit::textures::{Game, find_wad};
use qs_map_edit::{MapEditError, MapReport, MapSource};

/// The id of `map.leaks`.
pub const MAP_LEAKS: &str = "map.leaks";
/// The id of `map.compile`.
pub const MAP_COMPILE: &str = "map.compile";
/// The id of `map.play_from_here`.
pub const MAP_PLAY_FROM_HERE: &str = "map.play_from_here";
/// The id of `map.portals`.
pub const MAP_PORTALS: &str = "map.portals";

impl<L: crate::Loader> Workbench<L> {
    /// The open map compiled in memory, its WADs found in the project, beside the map or in
    /// the game at `game`; its report, kept with the map; and the map's path.
    pub(crate) fn compile_map(
        &mut self,
        game: Option<&Path>,
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
        let file = document.file().without_omitted_layers();
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
        let wad = match found.len() {
            0 => None,
            1 => found.pop().map(|(_, bytes)| bytes),
            _ => Some(merged(&found)?),
        };
        let compiled = compiler
            .compile_with_externals(&text, wad.as_deref(), &mut load)
            .map_err(failed)?;
        let report = MapReport::of(&compiled, missing_wads);
        let path = document.path().to_path_buf();
        let edits = document.edits();
        self.maps.keep_report(report.clone(), edits)?;
        Ok((compiled, report, path))
    }
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
            path: wads
                .first()
                .map(|(path, _)| path.clone())
                .unwrap_or_default(),
            error,
        })
}

/// `map.leaks`.
pub(crate) struct MapLeaks;

impl<C: HasPreview + 'static> Command<C> for MapLeaks {
    const ID: &'static str = MAP_LEAKS;
    const SUMMARY: &'static str = "Compiles the open map in memory and reports whether it leaks, \
        with the trail from the entity the outside reaches out to the void, and the textures \
        its WADs lack.";
    type Params = NoParams;
    type Output = MapReport;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<MapReport, C>, WorkbenchError> {
        let (workbench, preview) = context.workbench_and_preview();
        let game = preview.game_base_path();
        let (_, report, _) = workbench.compile_map(game.as_deref())?;
        Ok(Outcome::outside_history(report))
    }
}

/// `map.compile`.
pub(crate) struct MapCompile;

impl<C: HasPreview + 'static> Command<C> for MapCompile {
    const ID: &'static str = MAP_COMPILE;
    const SUMMARY: &'static str = "Compiles the open map, as it is in the editor, to a BSP \
        beside it, with its portal and leak files, and reports whether it leaks and the \
        textures its WADs lack.";
    type Params = NoParams;
    type Output = MapReport;
    type Error = WorkbenchError;

    fn run(&self, context: &mut C, _: NoParams) -> Result<Outcome<MapReport, C>, WorkbenchError> {
        let (workbench, preview) = context.workbench_and_preview();
        let game = preview.game_base_path();
        let (compiled, mut report, path) = workbench.compile_map(game.as_deref())?;
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
    /// `view_ofs` below it, moved up or down to the nearest place its box fits.
    pub eye: [f64; 3],
    /// Which way the player faces, pitch, yaw and roll in degrees; ahead along x when not
    /// given.
    #[serde(default)]
    pub angles: Option<[f64; 3]>,
}

/// How far, in whole units up or down, [`PlayFromHereParams::origin_in`] looks for room for
/// the player's box: as far as the box is tall above its origin and then some, so a camera
/// near a floor or a ceiling still finds the room beside it.
const PLAYER_ROOM_SEARCH: u8 = 64;

/// Hull 1, the clipping hull of the player's box, (-16, -16, -24) to (16, 16, 32) (BSP-11.3).
const PLAYER_HULL: usize = 1;

impl PlayFromHereParams {
    /// Where the player's origin goes in `bsp`: `view_ofs` (id's `'0 0 22'`, NET-4.4) below
    /// the eye, at the nearest height within [`PLAYER_ROOM_SEARCH`] units where hull 1, the
    /// player's box, is not in solid; looked for first above, then below, a unit at a time.
    ///
    /// # Errors
    ///
    /// [`PreviewError::NoRoom`] when the box fits nowhere along that line, or the eye is not a
    /// point the game can hold.
    pub fn origin_in(&self, bsp: &Bsp<'_>) -> Result<[f32; 3], PreviewError> {
        let no_room = || PreviewError::NoRoom(self.eye);
        let [x, y, eye] = self.eye.map(single);
        let z = eye - DEFAULT_VIEWHEIGHT;
        if ![x, y, z].iter().all(|c| c.is_finite()) {
            return Err(no_room());
        }
        let [h0, h1, h2, _] = bsp
            .models
            .first()
            .map(|model| model.headnode)
            .unwrap_or_default();
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

/// What `map.play_from_here` played.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayedHere {
    /// The map's name, as the game's `map` command takes it.
    pub map: String,
    /// Where the player was put: its origin, in the map's units.
    pub origin: [f32; 3],
    /// What compiling it found.
    pub report: MapReport,
    /// What lighting it warned of, as `light` prints each.
    pub light_warnings: Vec<String>,
}

/// A float of a map, as the game's single precision holds it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the game holds positions and angles in single precision"
)]
const fn single(value: f64) -> f32 {
    value as f32
}

/// `map.play_from_here`.
pub(crate) struct MapPlayFromHere;

impl<C: HasPreview + 'static> Command<C> for MapPlayFromHere {
    const ID: &'static str = MAP_PLAY_FROM_HERE;
    const SUMMARY: &'static str = "Compiles the open map fast, with no vis and fast light, into \
        the project's build, starts the game on it and puts the player's eye at a point, facing \
        a way: the map view's camera (MAP-13).";
    type Params = PlayFromHereParams;
    type Output = PlayedHere;
    type Error = PreviewError;

    fn run(
        &self,
        context: &mut C,
        params: PlayFromHereParams,
    ) -> Result<Outcome<PlayedHere, C>, PreviewError> {
        let (workbench, preview) = context.workbench_and_preview();
        let game = preview.game_base_path();
        let (compiled, report, path) = workbench
            .compile_map(game.as_deref())
            .map_err(PreviewError::Build)?;
        let lit = Light::new(Sampling::Fast)
            .run(&compiled.bsp)
            .map_err(|error| PreviewError::Build(WorkbenchError::MapLight { error }))?;
        let bsp = Bsp::read(&lit.bsp, BspLoader::default()).map_err(|error| {
            PreviewError::Build(WorkbenchError::MapEdit(MapEditError::Bsp {
                path: path.clone(),
                error,
            }))
        })?;
        let built = workbench.built_program()?;
        let (project, _, mod_name) = &built;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let maps = project.join("build").join(mod_name).join("maps");
        std::fs::create_dir_all(&maps).map_err(|error| PreviewError::Write(maps.clone(), error))?;
        let files = [("bsp", Some(&lit.bsp)), ("lit", lit.lit.as_ref())];
        for (extension, bytes) in files {
            // A name with a dot of its own keeps it: `e1m1.v2` is `e1m1.v2.bsp`.
            let file = maps.join(format!("{name}.{extension}"));
            match bytes {
                Some(bytes) => std::fs::write(&file, bytes)
                    .map_err(|error| PreviewError::Write(file.clone(), error))?,
                // A `.lit` from an earlier play would colour this one wrongly (BSP-18.2).
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
        let origin = params.origin_in(&bsp)?;
        preview.play_built(built, Some(name.clone()))?;
        let client = preview.client().ok_or(PreviewError::NotRunning)?;
        let angles = params.angles.unwrap_or_default().map(single);
        client.spawn_player_at(origin, angles)?;
        // P7-02: full vis and full light of the same BSP, swapped in when done; a map that leaks
        // has no portals to see through, and plays with the fast compile.
        match compiled.portal_file {
            Some(portals) => preview.start_full_build(
                name.clone(),
                maps.join(format!("{name}.bsp")),
                (compiled.bsp, portals),
            ),
            None => preview.note(
                "The map leaks: it plays with the fast compile, and its full compile waits for \
                 it to be sealed",
            ),
        }
        let light_warnings: Vec<String> = lit.warnings.iter().map(ToString::to_string).collect();
        if let Some(first) = light_warnings.first() {
            preview.set_status(format!(
                "Playing {name}: {} light warnings, the first {first}",
                light_warnings.len()
            ));
        }
        Ok(Outcome::outside_history(PlayedHere {
            map: name,
            origin,
            report,
            light_warnings,
        }))
    }
}

#[cfg(test)]
pub(crate) mod tests;
