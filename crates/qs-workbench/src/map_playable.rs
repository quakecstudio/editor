// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.playability` (L-77, MAP-36): whether the open map can be finished, checked by
//! `qs_map_gen::playable` on the BSP `map.compile` compiles it to, or the BSP it was opened from:
//! the exit reached from the player start, each key, weapon, item and secret never reached,
//! each place a player cannot leave, and each entity the check could not judge, named by its
//! place in the open map. The findings join `map.issues` until the map changes.

use std::path::Path;

use qs_bsp::{Bsp, BspLoader};
use qs_commands::{Command, Outcome, RegisterError, Registry};
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

/// The issue of a map whose exit is not reached.
const NO_EXIT: &str = "no exit (trigger_changelevel) is reached from the player start: the \
    map cannot be finished";

/// What `map.playability` last found, for `map.issues`, while the map is the same.
#[derive(Clone, Debug, Default)]
pub(crate) struct PlayFound {
    /// The map's text when they were found.
    map: Vec<u8>,
    problems: Vec<EntityProblem>,
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
        }
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
}

impl<L: crate::Loader> Workbench<L> {
    /// Whether the open map can be finished (L-77), its `.map` compiled as `map.compile` does,
    /// with its WADs found in the game at `game`; the findings kept for `map.issues`.
    fn map_playability(
        &mut self,
        game: Option<&Path>,
        params: &PlayabilityParams,
    ) -> Result<Playability<EntityIndex>, WorkbenchError> {
        let document = self.map().ok_or(WorkbenchError::NoMap)?;
        let text = document.file().to_bytes();
        let (bytes, places) = match document.source() {
            MapSource::Bsp(bytes) => (bytes.clone(), (0..document.file().entities.len()).collect()),
            MapSource::Map => {
                let file = document.file();
                // The entities of the lump are those a compile writes of the map exported.
                let exported = file.exported_places();
                let lump = qs_map_bsp::Compiler::default()
                    .with_dialect(COMPILE_DIALECT)
                    .lump_places(&file.without_omitted_layers());
                let places: Vec<usize> = lump
                    .iter()
                    .filter_map(|&n| exported.get(n).copied())
                    .collect();
                let (compiled, _, _) = self.compile_map(game, None)?;
                (compiled.bsp, places)
            }
        };
        let path = self.map().map_or_default(|d| d.path().to_path_buf());
        let bsp = Bsp::read(&bytes, BspLoader::default())
            .map_err(|error| crate::MapEditError::Bsp { path, error })?;
        let found = Ground::new(&bsp, params.moves, params.skill)
            .and_then(|ground| ground.playability(|n| places.get(n).copied().map(EntityIndex::new)))
            .map_err(|error| WorkbenchError::Playability { error })?;
        if let Some(document) = self.map() {
            self.playable = PlayFound::of(&found, document.file(), text);
        }
        Ok(found)
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
        join map.issues until the map changes.";
    type Params = PlayabilityParams;
    type Output = Playability<EntityIndex>;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: PlayabilityParams,
    ) -> Result<Outcome<Playability<EntityIndex>, C>, WorkbenchError> {
        let (workbench, preview) = context.workbench_and_preview();
        let game = preview.game_base_path();
        let found = workbench.map_playability(game.as_deref(), &params)?;
        Ok(Outcome::outside_history(found))
    }
}

/// Registers `map.playability`.
///
/// # Errors
///
/// [`RegisterError`] when it is already registered.
pub(crate) fn register_map_playable<C: HasPreview + 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(MapPlayability)
}

#[cfg(test)]
mod tests;
