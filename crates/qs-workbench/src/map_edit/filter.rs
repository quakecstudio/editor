// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.filter` (P5-13, MAP-27): the entities a skill and a game mode remove, found by spawning
//! the open map in `qs-sim` against the project's server program, compiled in memory, so the
//! answer is the game's, the spawnflags test (EDICT-8.4) and the entities the QuakeC removes
//! itself included (BRIDGE-4.13). A map read from a BSP spawns its entity lump in that BSP; a
//! `.map` spawns the entity lump qbsp would write for it (MAP-2.6, MAP-6.2) in the BSP beside it
//! when there is one, else in an empty world.

use std::collections::BTreeSet;
use std::path::Path;
use std::rc::Rc;

use qc_codegen::compile_module;
use qc_progs::Progs;
use qc_project::ModuleKind;
use qs_bsp::{Bsp, BspLoader};
use qs_commands::{Command, Outcome};
use qs_map::qbsp::load_map_file;
use qs_sim::{LoadOptions, MapPlace, World};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{BSP_EXTENSION, EntityIndex, MapSource};
use crate::{HasWorkbench, Workbench, WorkbenchError};

/// The id of `map.filter`.
pub const MAP_FILTER: &str = "map.filter";

/// The skill a map is spawned at, the `skill` setting (DIFF-3.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Skill {
    /// 0.
    Easy,
    /// 1, the game's own default.
    #[default]
    Normal,
    /// 2.
    Hard,
    /// 3.
    Nightmare,
}

impl Skill {
    /// The value of the `skill` setting.
    const fn setting(self) -> &'static str {
        match self {
            Self::Easy => "0",
            Self::Normal => "1",
            Self::Hard => "2",
            Self::Nightmare => "3",
        }
    }
}

/// The kind of game a map is spawned for: the `coop` and `deathmatch` settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GameMode {
    /// Single player.
    #[default]
    Single,
    /// Cooperative: `coop 1`.
    Coop,
    /// Deathmatch: `deathmatch 1`.
    Deathmatch,
}

impl GameMode {
    /// The settings it sets.
    fn settings(self) -> Vec<(String, String)> {
        let set = |name: &str| vec![(name.to_owned(), "1".to_owned())];
        match self {
            Self::Single => Vec::new(),
            Self::Coop => set("coop"),
            Self::Deathmatch => set("deathmatch"),
        }
    }
}

/// The parameters of `map.filter`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterParams {
    /// The skill.
    #[serde(default)]
    pub skill: Skill,
    /// The kind of game.
    #[serde(default)]
    pub mode: GameMode,
}

/// What `map.filter` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapFiltered {
    /// The entities the game does not keep at that skill and in that kind of game, the world
    /// never among them.
    pub removed: Vec<EntityIndex>,
}

impl<L: crate::Loader> Workbench<L> {
    /// The entities of the open map the game removes at `skill` in a game of `mode`: the
    /// project built, and the map's entities spawned in `qs-sim` with those settings.
    fn map_removed(
        &mut self,
        skill: Skill,
        mode: GameMode,
    ) -> Result<Vec<EntityIndex>, WorkbenchError> {
        if self.root().is_none() {
            return Err(WorkbenchError::NoProject);
        }
        let mut program = None;
        for module in self.host.project_modules() {
            if module.kind != ModuleKind::Ssqc {
                continue;
            }
            let names = module.names;
            let compiled = compile_module(module.db, module.input, |id| {
                names.get(id.index()).cloned().unwrap_or_default()
            });
            program = compiled
                .ok()
                .and_then(|compiled| qc_progs::write(&compiled.progs).ok());
            break;
        }
        let program = program.ok_or(WorkbenchError::NotBuilt)?;
        let progs = Progs::read(&program).map_err(|_| WorkbenchError::NotBuilt)?;
        let root = self.root().map(Path::to_path_buf);
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        let path = document.path().to_path_buf();
        // The places in the document of the entities the lump holds, in its order.
        let (entities, places, compiled) = match document.source() {
            MapSource::Bsp(bytes) => {
                let count = document.file().entities.len();
                let places: Vec<usize> = (0..count).collect();
                (document.file().to_bytes(), places, Some(bytes.clone()))
            }
            MapSource::Map => {
                let text = document.file().to_bytes();
                let mut map = load_map_file(&text).map_err(|error| WorkbenchError::MapLump {
                    path: path.clone(),
                    error,
                })?;
                map.number_models();
                let lump = map.entity_lump().map_err(|error| WorkbenchError::MapLump {
                    path: path.clone(),
                    error,
                })?;
                // qbsp writes only the entities with pairs (MAP-2.6).
                let places = document
                    .file()
                    .entities
                    .iter()
                    .enumerate()
                    .filter(|(_, entity)| entity.pairs().next().is_some())
                    .map(|(place, _)| place)
                    .collect();
                let bsp = std::fs::read(path.with_extension(BSP_EXTENSION)).ok();
                (lump, places, bsp)
            }
        };
        let mut bsp = match &compiled {
            Some(bytes) => {
                Bsp::read(bytes, BspLoader::default()).map_err(|error| WorkbenchError::Bsp {
                    path: path.clone(),
                    error,
                })?
            }
            None => Bsp::empty(&entities),
        };
        bsp.entity_lump = &entities;
        let files: qs_sim::ModelFiles = Rc::new(move |name: &str| {
            root.as_ref()
                .and_then(|root| std::fs::read(root.join(name)).ok())
        });
        let mut settings = vec![("skill".to_owned(), skill.setting().to_owned())];
        settings.extend(mode.settings());
        let options = LoadOptions {
            settings,
            ..LoadOptions::default()
        };
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let world = World::load(&progs, &name, &bsp, files, options)
            .map_err(|error| WorkbenchError::Spawn { path, error })?;
        let kept: BTreeSet<usize> = places
            .iter()
            .enumerate()
            .skip(1)
            .filter(|&(place, _)| world.map_entity(&progs, MapPlace::new(place)).is_some())
            .map(|(_, &entity)| entity)
            .collect();
        let count = self
            .map()
            .map_or(0, |document| document.file().entities.len());
        Ok((1..count)
            .filter(|entity| !kept.contains(entity))
            .map(EntityIndex::new)
            .collect())
    }
}

/// `map.filter`.
pub(super) struct MapFilter;

impl<C: HasWorkbench + 'static> Command<C> for MapFilter {
    const ID: &'static str = MAP_FILTER;
    const SUMMARY: &'static str = "The entities of the open map the game removes at a skill and \
        in a kind of game (single, coop or deathmatch), found by spawning the map with the \
        project's program: those the spawnflags rule out and those the QuakeC removes.";
    type Params = FilterParams;
    type Output = MapFiltered;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: FilterParams,
    ) -> Result<Outcome<MapFiltered, C>, WorkbenchError> {
        let removed = context.workbench().map_removed(params.skill, params.mode)?;
        Ok(Outcome::outside_history(MapFiltered { removed }))
    }
}

#[cfg(test)]
mod tests;
