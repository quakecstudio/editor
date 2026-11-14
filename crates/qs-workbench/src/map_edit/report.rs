// SPDX-License-Identifier: GPL-3.0-or-later

//! What compiling the open map found (P6-11): whether it leaks and where, and the textures
//! and WADs it lacks. The report is kept with the map, for the map view.

use std::path::PathBuf;

use qs_map::qbsp::QbspWarning;
use qs_map_bsp::{Compiled, Warning};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    /// The report of `compiled`, whose map named the WADs `missing_wads` that were not found.
    #[must_use]
    pub fn of(compiled: &Compiled, missing_wads: Vec<String>) -> Self {
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
