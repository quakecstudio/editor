// SPDX-License-Identifier: GPL-3.0-or-later

//! Compiling the open map (P6-11, P6-13, MAP-7, MAP-16): `map.leaks` compiles it in memory
//! with `qs-map-bsp` and reports whether it leaks, the leak's trail from the entity the outside
//! reached out to the void (QBSP-9.8), and the textures its WADs lack; `map.compile` does the
//! same and writes the BSP, portal and leak files beside the map, as id's qbsp names them.
//! Both read the map as it is in the editor, saved or not, and its WADs from the project, the
//! map's folder or the game, as the texture browser finds them.

use std::path::{Path, PathBuf};

use qs_commands::{Command, NoParams, Outcome};
use qs_map::qbsp::QbspWarning;
use qs_map_bsp::{Compiled, Compiler, Warning};
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

/// What compiling the open map found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapReport {
    /// When the map leaks, the way from the entity the outside reached out to the void: its
    /// origin, then the middle of each portal crossed; empty when it does not (QBSP-9.8).
    pub leak: Vec<[f32; 3]>,
    /// The textures the map's faces name that its WADs lack, each once (MAP-6.8).
    pub missing_textures: Vec<String>,
    /// What qbsp would print and go on after, one line each.
    pub warnings: Vec<String>,
    /// The files written, for `map.compile`.
    pub written: Vec<PathBuf>,
}

impl MapReport {
    /// The report of `compiled`.
    fn of(compiled: &Compiled) -> Self {
        let mut missing_textures = Vec::new();
        for warning in &compiled.warnings {
            if let Warning::Map(QbspWarning::TextureNotFound { name }) = warning {
                let name = String::from_utf8_lossy(name).into_owned();
                if !missing_textures.contains(&name) {
                    missing_textures.push(name);
                }
            }
        }
        Self {
            leak: compiled.leak_trail.clone(),
            missing_textures,
            warnings: compiled
                .warnings
                .iter()
                .map(|warning| match warning {
                    Warning::Map(printed) => printed.to_string(),
                    other => format!("{other:?}"),
                })
                .collect(),
            written: Vec::new(),
        }
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// The open map compiled in memory, its WADs found in the project, beside the map or in
    /// the game at `game`.
    fn compile_map(&self, game: Option<&Path>) -> Result<(Compiled, PathBuf), WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        if let MapSource::Bsp(_) = document.source() {
            return Err(WorkbenchError::NoMapSource {
                path: document.path().to_path_buf(),
            });
        }
        let text = document.file().to_bytes();
        let compiler = Compiler::default().with_dialect(qs_map_bsp::Dialect::Extended);
        let failed = |error| WorkbenchError::MapCompile { error };
        let wad = compiler
            .wad_path(&text)
            .map_err(failed)?
            .and_then(|name| {
                let game = game.map(Game::at);
                let name = String::from_utf8_lossy(&name).into_owned();
                find_wad(&name, self.root(), document.path().parent(), game.as_ref())
            })
            .map(|(_, bytes)| bytes);
        let compiled = compiler.compile(&text, wad.as_deref()).map_err(failed)?;
        Ok((compiled, document.path().to_path_buf()))
    }
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
        let (compiled, _) = workbench.compile_map(game.as_deref())?;
        Ok(Outcome::outside_history(MapReport::of(&compiled)))
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
        let (compiled, path) = workbench.compile_map(game.as_deref())?;
        let mut report = MapReport::of(&compiled);
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
