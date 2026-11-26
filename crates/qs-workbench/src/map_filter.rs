// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.entities_removed_by_skill` (P5-13, MAP-27): the entities a skill and a game mode remove,
//! found by spawning the open map in `qs-sim` against the project's server program, compiled in
//! memory, so the answer is the game's, the spawnflags test (EDICT-8.4) and the entities the QuakeC
//! removes itself included (BRIDGE-4.13). A map read from a BSP spawns its entity lump in that BSP;
//! a `.map` spawns the entity lump qbsp would write for it (MAP-2.6, MAP-6.2) in the map compiled
//! in memory as id's qbsp compiles it, so each brush entity has the model its `model` key names;
//! when the map does not compile, in the BSP beside it when there is one, else in an empty world.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
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

use crate::{HasWorkbench, Workbench, WorkbenchError};
use qs_map_edit::{BSP_EXTENSION, EntityIndex, MapSource};

/// The id of `map.entities_removed_by_skill`.
pub const MAP_FILTER: &str = "map.entities_removed_by_skill";

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

/// The parameters of `map.entities_removed_by_skill`.
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

/// What `map.entities_removed_by_skill` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MapFiltered {
    /// The entities the game does not keep at that skill and in that kind of game, the world
    /// never among them.
    pub removed: Vec<EntityIndex>,
}

/// The open map as the game spawns it (DIFF-3.5): the project's server program, compiled in
/// memory, the entity lump, and the BSP it spawns in.
pub(crate) struct MapSpawn {
    program: Vec<u8>,
    entities: Vec<u8>,
    /// The compiled map, `None` for a `.map` that does not compile and has no BSP beside it,
    /// which spawns in an empty world.
    compiled: Option<Vec<u8>>,
    /// The place in the document of each entity the lump holds, in the lump's order.
    places: Vec<usize>,
    path: PathBuf,
    root: Option<PathBuf>,
}

impl MapSpawn {
    /// The place in the document of each entity the lump holds, in the lump's order.
    pub(crate) fn places(&self) -> &[usize] {
        &self.places
    }

    /// Runs `with` on the map's world, spawned at `skill` in a game of `mode`, and the progs
    /// it runs.
    pub(crate) fn world<T>(
        &self,
        skill: Skill,
        mode: GameMode,
        with: impl FnOnce(&Progs<'_>, World) -> Result<T, WorkbenchError>,
    ) -> Result<T, WorkbenchError> {
        let progs = Progs::read(&self.program).map_err(|_| WorkbenchError::NotBuilt)?;
        let mut bsp = match &self.compiled {
            Some(bytes) => Bsp::read(bytes, BspLoader::default()).map_err(|error| {
                crate::MapEditError::Bsp {
                    path: self.path.clone(),
                    error,
                }
            })?,
            None => Bsp::empty(&self.entities),
        };
        bsp.entity_lump = &self.entities;
        let root = self.root.clone();
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
        let name = self
            .path
            .file_stem()
            .map_or_default(|stem| stem.to_string_lossy().into_owned());
        let world =
            World::load(&progs, &name, &bsp, files, options).map_err(|error| self.failed(error))?;
        with(&progs, world)
    }

    /// The error of a world of this map that did not spawn or run.
    pub(crate) fn failed(&self, error: qs_sim::WorldError) -> WorkbenchError {
        WorkbenchError::Spawn {
            path: self.path.clone(),
            error,
        }
    }
}

/// The world a `.map` was last compiled to for spawning, kept for the map's text so that
/// firing and filtering again do not compile it again (FIRE-5.4).
#[derive(Clone, Debug, Default)]
pub(crate) struct SpawnWorld {
    /// The map's text, and its BSP, `None` when it did not compile to one a loader reads.
    last: Option<(Vec<u8>, Option<Vec<u8>>)>,
}

impl SpawnWorld {
    /// The BSP id's qbsp compiles `text` to, compiled once for each text.
    fn of(&mut self, text: &[u8]) -> Option<Vec<u8>> {
        if let Some((kept, bsp)) = &self.last
            && kept == text
        {
            return bsp.clone();
        }
        let bsp = qs_map_bsp::Compiler::default()
            .compile(text, None)
            .ok()
            .map(|compiled| compiled.bsp)
            // A map with no brush that has volume compiles to a BSP no loader reads.
            .filter(|bsp| Bsp::read(bsp, BspLoader::default()).is_ok());
        self.last = Some((text.to_vec(), bsp.clone()));
        bsp
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// What the game spawns the open map from: the project built, and the map's entity lump.
    pub(crate) fn map_spawn(&mut self) -> Result<MapSpawn, WorkbenchError> {
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
                // The world the lump's `*n` models are in: the map compiled in memory as id's qbsp
                // compiles it, which gives each brush entity the model the lump names (MAP-6.2);
                // when it does not compile, the BSP beside it, else an empty world.
                let compiled = self.spawn_world.of(&text);
                let bsp =
                    compiled.or_else(|| std::fs::read(path.with_extension(BSP_EXTENSION)).ok());
                (lump, places, bsp)
            }
        };
        Ok(MapSpawn {
            program,
            entities,
            compiled,
            places,
            path,
            root,
        })
    }

    /// The entities of the open map the game removes at `skill` in a game of `mode`: the
    /// project built, and the map's entities spawned in `qs-sim` with those settings.
    fn map_removed(
        &mut self,
        skill: Skill,
        mode: GameMode,
    ) -> Result<Vec<EntityIndex>, WorkbenchError> {
        let spawn = self.map_spawn()?;
        let kept: BTreeSet<usize> = spawn.world(skill, mode, |progs, world| {
            Ok(spawn
                .places()
                .iter()
                .enumerate()
                .skip(1)
                .filter(|&(place, _)| world.map_entity(progs, MapPlace::new(place)).is_some())
                .map(|(_, &entity)| entity)
                .collect())
        })?;
        let count = self
            .map()
            .map_or(0, |document| document.file().entities.len());
        Ok((1..count)
            .filter(|entity| !kept.contains(entity))
            .map(EntityIndex::new)
            .collect())
    }
}

/// `map.entities_removed_by_skill`.
pub(crate) struct MapFilter;

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
