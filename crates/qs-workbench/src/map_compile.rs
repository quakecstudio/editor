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

use qs_commands::{Command, NoParams, Outcome};
use qs_map::qbsp::QbspWarning;
use qs_map_bsp::{Compiled, Compiler, Warning};
use qs_wad::Wad;
use qs_wad::write::WadBuilder;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::map_edit::MapSource;
use crate::map_textures::{Game, find_wad};
use crate::preview::HasPreview;
use crate::{Workbench, WorkbenchError};

/// The id of `map.leaks`.
pub const MAP_LEAKS: &str = "map.leaks";
/// The id of `map.compile`.
pub const MAP_COMPILE: &str = "map.compile";

/// Whether the outside of a map was filled (QBSP-9.2, QBSP-9.3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Seal {
    /// The outside was filled: no entity is in reach of the void.
    #[default]
    Sealed,
    /// The outside reaches an entity.
    Leaks,
    /// No entity stands in empty space, so nothing could be filled and a leak cannot be found.
    NoOccupant,
}

/// What compiling the open map found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapReport {
    /// Whether the outside was filled.
    pub seal: Seal,
    /// When the map leaks, the way from the entity the outside reached out to the void: its
    /// origin, then the middle of each portal crossed; empty when it does not (QBSP-9.8).
    pub leak: Vec<[f32; 3]>,
    /// The textures the map's faces name that its WADs lack, each once (MAP-6.8).
    pub missing_textures: Vec<String>,
    /// The WADs the world names that were not found, whose textures could not be read.
    pub missing_wads: Vec<String>,
    /// What qbsp would print and go on after, one line each.
    pub warnings: Vec<String>,
    /// The files written, for `map.compile`.
    pub written: Vec<PathBuf>,
}

impl MapReport {
    /// The report of `compiled`.
    fn of(compiled: &Compiled, missing_wads: Vec<String>) -> Self {
        let mut missing_textures = Vec::new();
        let mut seal = Seal::Sealed;
        for warning in &compiled.warnings {
            match warning {
                Warning::Map(QbspWarning::TextureNotFound { name }) => {
                    let name = String::from_utf8_lossy(name).into_owned();
                    if !missing_textures.contains(&name) {
                        missing_textures.push(name);
                    }
                }
                Warning::Leak => seal = Seal::Leaks,
                Warning::NoOccupant => seal = Seal::NoOccupant,
                Warning::NoBrushFaces
                | Warning::PortalClippedAway
                | Warning::BadFace(_)
                | Warning::MixedContents
                | Warning::NoValidBrushes
                | Warning::Bsp2(_)
                | Warning::Map(_) => {}
            }
        }
        Self {
            seal,
            leak: compiled.leak_trail.clone(),
            missing_textures,
            missing_wads,
            warnings: compiled.warnings.iter().map(ToString::to_string).collect(),
            written: Vec::new(),
        }
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// The open map compiled in memory, its WADs found in the project, beside the map or in
    /// the game at `game`; its report, kept with the map; and the map's path.
    fn compile_map(
        &mut self,
        game: Option<&Path>,
    ) -> Result<(Compiled, MapReport, PathBuf), WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        if let MapSource::Bsp(_) = document.source() {
            return Err(WorkbenchError::NoMapSource {
                path: document.path().to_path_buf(),
            });
        }
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
        let compiled = compiler.compile(&text, wad.as_deref()).map_err(failed)?;
        let report = MapReport::of(&compiled, missing_wads);
        let path = document.path().to_path_buf();
        let edits = document.edits();
        self.map_mut()?.set_report(report.clone(), edits);
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
            let Some(bytes) = bytes else {
                continue;
            };
            let file = path.with_extension(extension);
            std::fs::write(&file, bytes).map_err(|error| WorkbenchError::Save {
                path: file.clone(),
                error,
            })?;
            report.written.push(file);
        }
        Ok(Outcome::outside_history(report))
    }
}

#[cfg(test)]
mod tests;
