// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.playability` (L-77, MAP-36): whether the open map can be finished, checked by
//! `qs_map_gen::playable` on the BSP `map.compile` compiles it to, or the BSP it was opened from:
//! the exit reached from the player start, each key, weapon, item and secret never reached,
//! each place a player cannot leave, and each entity the check could not judge, named by its
//! place in the open map. The findings join `map.issues` until the map changes.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use qs_bsp::{Bsp, BspLoader};
use qs_commands::{Command, NoParams, Outcome, RegisterError, Registry};
use qs_map::{Entity, MapFile};
use qs_map_edit::{EntityIndex, EntityProblem, MapSource};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use qs_map_gen::playable::{
    ExitReach, Ground, JUMP, LEAP, Moves, NotJudged, PlayError, Playability, Skill, Sought,
    TrapRegion, Unreached,
};

use crate::map_compile::COMPILE_DIALECT;
use crate::preview::HasPreview;
use crate::{Workbench, WorkbenchError};

/// The id of `map.playability`.
pub const MAP_PLAYABILITY: &str = "map.playability";
/// The id of `map.playability_found`.
pub const MAP_PLAYABILITY_FOUND: &str = "map.playability_found";

/// The issue of a map whose exit is not reached.
const NO_EXIT: &str = "no exit (trigger_changelevel) is reached from the player start: the \
    map cannot be finished";

/// What `map.playability` last found, for `map.issues` and `map.playability_found`, while the
/// map is the same.
#[derive(Clone, Debug, Default)]
pub(crate) struct PlayFound {
    /// The map's text when they were found.
    map: Vec<u8>,
    problems: Vec<EntityProblem>,
    found: Option<Playability<EntityIndex>>,
}

impl PlayFound {
    /// What `found` makes of `map`, whose text is `text`, as issues of it (`map.issues`): an
    /// exit not reached, each key, weapon, item and secret never reached, and each trap, at its
    /// point.
    fn of(found: &Playability<EntityIndex>, map: &MapFile, text: Vec<u8>) -> Self {
        let line = |entity: EntityIndex| map.entities.get(entity.get()).map_or(0, Entity::line);
        let mut problems = Vec::new();
        if found.exit == ExitReach::NotReached {
            let entity = found.exits.first().copied();
            problems.push(EntityProblem {
                entity,
                line: entity.map_or(0, line),
                message: NO_EXIT.to_owned(),
                fix: None,
                point: None,
            });
        }
        for unreached in &found.unreached {
            problems.push(EntityProblem {
                entity: Some(unreached.entity),
                line: line(unreached.entity),
                message: unreached.message(),
                fix: None,
                point: None,
            });
        }
        for trap in &found.traps {
            problems.push(EntityProblem {
                entity: None,
                line: 0,
                message: trap.message(),
                fix: None,
                point: Some(trap.at.map(f64::from)),
            });
        }
        Self {
            map: text,
            problems,
            found: Some(found.clone()),
        }
    }

    /// What was found, while the map's text is still `map`.
    fn found(&self, map: &[u8]) -> Option<Playability<EntityIndex>> {
        self.found.clone().filter(|_| self.map == map)
    }

    /// The problems, while the map's text is still `map`.
    pub(crate) fn problems(&self, map: &[u8]) -> Vec<EntityProblem> {
        if self.map == map {
            self.problems.clone()
        } else {
            Vec::new()
        }
    }
}

/// The parameters of `map.playability`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlayabilityParams {
    /// The moves the player is taken to have: a player's, with a jump, when not given.
    #[serde(default)]
    pub moves: Moves,
    /// The skill the map is played at, which decides the entities the game keeps (EDICT-8.4).
    #[serde(default)]
    pub skill: Skill,
    /// When the answer is given: at once, the default, or later, the search run on its own
    /// thread and its answer taken with `map.playability_found`.
    #[serde(default)]
    pub answer: Answer,
}

/// When `map.playability` gives its answer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    /// The search runs, and its answer is given.
    #[default]
    Now,
    /// The map is compiled, the search started on its own thread, and nothing is answered:
    /// `map.playability_found` gives the answer once the search is done, so that a window
    /// stays live while a large map is searched.
    Later,
}

/// What a search needs, all of it its own, to run on any thread: the compiled map, the place
/// in the open map of each entity of its lump, the moves and the skill.
struct PlaySearch {
    bsp: Vec<u8>,
    places: Vec<usize>,
    path: PathBuf,
    moves: Moves,
    skill: Skill,
}

impl PlaySearch {
    /// The search's answer, each entity named by its place in the open map.
    fn answer(&self) -> Result<Playability<EntityIndex>, WorkbenchError> {
        let bsp = Bsp::read(&self.bsp, BspLoader::default()).map_err(|error| {
            crate::MapEditError::Bsp {
                path: self.path.clone(),
                error,
            }
        })?;
        Ground::new(&bsp, self.moves, self.skill)
            .and_then(|ground| {
                ground.playability(|n| self.places.get(n).copied().map(EntityIndex::new))
            })
            .map_err(|error| WorkbenchError::Playability { error })
    }
}

/// A search running on its own thread, for the map whose text is `map`.
#[derive(Debug)]
pub(crate) struct PlayRun {
    map: Vec<u8>,
    done: Receiver<Result<Playability<EntityIndex>, WorkbenchError>>,
}

/// The whole open map as `compile_map` last compiled it: its text and its BSP, which
/// `map.playability` reads again while the map is the same rather than compile it twice.
#[derive(Clone, Debug, Default)]
pub(crate) struct LastCompile {
    map: Vec<u8>,
    bsp: Vec<u8>,
}

impl LastCompile {
    /// What `compile_map` compiled of the map whose text is `map`.
    pub(crate) const fn new(map: Vec<u8>, bsp: Vec<u8>) -> Self {
        Self { map, bsp }
    }
}

impl<L: crate::Loader> Workbench<L> {
    /// What a search of the open map needs: its `.map` compiled as `map.compile` compiles it,
    /// with its WADs found in the game at `game`, or the compile kept while the map is the same;
    /// or the BSP it was opened from. Its text too.
    fn play_search(
        &mut self,
        game: Option<&Path>,
        params: &PlayabilityParams,
    ) -> Result<(PlaySearch, Vec<u8>), WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        let text = document.file().to_bytes();
        let path = document.path().to_path_buf();
        let (bsp, places) = match document.source() {
            MapSource::Bsp(bytes) => (bytes.clone(), (0..document.file().entities.len()).collect()),
            MapSource::Map => {
                let file = document.file();
                // The entities of the lump are those a compile writes of the map exported,
                // its instances (`misc_external_map`) made what they name, as `compile_map`
                // makes them: an instance of a `func_detail` joins the world, as one does.
                let exported = file.exported_places();
                let folders = document.external_folders(self.root());
                let mut load = |name: &str| {
                    folders
                        .iter()
                        .find_map(|dir| std::fs::read(dir.join(name)).ok())
                };
                let made = file
                    .without_omitted_layers()
                    .with_external_maps(&mut load)
                    .map_err(|error| WorkbenchError::MapCompile {
                        error: qs_map_bsp::CompileError::ExternalMap(error),
                    })?;
                let lump = qs_map_bsp::Compiler::default()
                    .with_dialect(COMPILE_DIALECT)
                    .lump_places(&made);
                let places: Vec<usize> = lump
                    .iter()
                    .filter_map(|&n| exported.get(n).copied())
                    .collect();
                let bsp = match &self.last_compile {
                    Some(last) if last.map == text => last.bsp.clone(),
                    Some(_) | None => self.compile_map(game, None)?.0.bsp,
                };
                (bsp, places)
            }
        };
        let search = PlaySearch {
            bsp,
            places,
            path,
            moves: params.moves,
            skill: params.skill,
        };
        Ok((search, text))
    }

    /// Keeps `found`, the answer for the map whose text was `text`, for `map.issues` and
    /// `map.playability_found`, while the map is still that.
    fn keep_playability(&mut self, found: &Playability<EntityIndex>, text: Vec<u8>) {
        if let Some(document) = self.map()
            && document.file().to_bytes() == text
        {
            self.playable = PlayFound::of(found, document.file(), text);
        }
    }

    /// Whether the open map can be finished (L-77): answered at once, or with
    /// [`Answer::Later`] the search started on its own thread and `None` answered.
    fn map_playability(
        &mut self,
        game: Option<&Path>,
        params: &PlayabilityParams,
    ) -> Result<Option<Playability<EntityIndex>>, WorkbenchError> {
        let (search, text) = self.play_search(game, params)?;
        match params.answer {
            Answer::Now => {
                let found = search.answer()?;
                self.keep_playability(&found, text);
                Ok(Some(found))
            }
            Answer::Later => {
                let (send, done) = mpsc::channel();
                std::thread::spawn(move || {
                    // A run the workbench dropped, for a later one, is waited on by no one.
                    drop(send.send(search.answer()));
                });
                self.play_run = Some(PlayRun { map: text, done });
                Ok(None)
            }
        }
    }

    /// The answer of the search started last, once it is done and the map is still the one it
    /// searched, kept as [`Answer::Now`] keeps one; else what was kept, while the map is the
    /// same; else `None`.
    ///
    /// # Errors
    ///
    /// What the search failed with.
    fn playability_found(&mut self) -> Result<Option<Playability<EntityIndex>>, WorkbenchError> {
        if let Some(run) = self.play_run.take() {
            match run.done.try_recv() {
                Ok(answer) => {
                    let found = answer?;
                    self.keep_playability(&found, run.map);
                }
                Err(TryRecvError::Empty) => {
                    self.play_run = Some(run);
                    return Ok(None);
                }
                Err(TryRecvError::Disconnected) => {}
            }
        }
        let text = self.map().map(|document| document.file().to_bytes());
        Ok(text.and_then(|text| self.playable.found(&text)))
    }
}

/// `map.playability`.
struct MapPlayability;

impl<C: HasPreview + 'static> Command<C> for MapPlayability {
    const ID: &'static str = MAP_PLAYABILITY;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Checks that the open map can be finished, as a player moves \
        through it compiled: walking, falling, swimming, riding lifts and teleporters, opening \
        doors with keys, buttons and triggers, and jumping (or with moves \"generator\", no \
        jump). Answers whether an exit is reached from the player start, each key, weapon, item \
        and secret never reached, each place from which the exit can no longer be reached, with \
        a point inside it, and each entity whose class the check does not follow. Its findings \
        join map.issues until the map changes. With answer \"later\" it answers null at once and \
        searches on its own thread; map.playability_found gives the answer.";
    type Params = PlayabilityParams;
    type Output = Option<Playability<EntityIndex>>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: PlayabilityParams,
    ) -> Result<Outcome<Option<Playability<EntityIndex>>, C>, WorkbenchError> {
        let (workbench, preview) = context.workbench_and_preview();
        let game = preview.game_base_path();
        let found = workbench.map_playability(game.as_deref(), &params)?;
        Ok(Outcome::outside_history(found))
    }
}

/// `map.playability_found`.
struct MapPlayabilityFound;

impl<C: HasPreview + 'static> Command<C> for MapPlayabilityFound {
    const ID: &'static str = MAP_PLAYABILITY_FOUND;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "Answers what map.playability found of the open map as it is \
        now: the answer of a search started with answer \"later\" once it is done, or the last \
        answer while the map has not changed since; null while a search runs, and when there is \
        none.";
    type Params = NoParams;
    type Output = Option<Playability<EntityIndex>>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        _: NoParams,
    ) -> Result<Outcome<Option<Playability<EntityIndex>>, C>, WorkbenchError> {
        let (workbench, _) = context.workbench_and_preview();
        Ok(Outcome::outside_history(workbench.playability_found()?))
    }
}

/// Registers `map.playability` and `map.playability_found`.
///
/// # Errors
///
/// [`RegisterError`] when one is already registered.
pub(crate) fn register_map_playable<C: HasPreview + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(MapPlayability)?;
    registry.register(MapPlayabilityFound)
}

#[cfg(test)]
mod tests;
