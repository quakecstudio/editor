// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.generate` (L-48): a random playable level made by `qs-map-gen` from the textures of
//! the game's own levels and the entity classes the project defines, written into the project
//! as a new map and opened.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use qs_commands::{Command, Outcome};
use qs_map_gen::{Classes, Request, Size};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{HasMap, MapEditError, MapProject, WORLD_CLASSNAME, new_map_path, write_new_map};

/// The id of `map.generate`.
pub const MAP_GENERATE: &str = "map.generate";

/// The folder a level is made in when no path is given, and what its file's name starts
/// with, before its seed.
const MAPS_FOLDER: &str = "maps";
const NAME_START: &str = "random";

/// How large a generated level is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LevelSize {
    /// Five rooms.
    Small,
    /// Eight rooms.
    #[default]
    Medium,
    /// Twelve rooms.
    Large,
}

impl From<LevelSize> for Size {
    fn from(size: LevelSize) -> Self {
        match size {
            LevelSize::Small => Self::Small,
            LevelSize::Medium => Self::Medium,
            LevelSize::Large => Self::Large,
        }
    }
}

/// The parameters of `map.generate`.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateParams {
    /// The seed: one seed and size give one level. One is drawn when none is given.
    #[serde(default)]
    pub seed: Option<u64>,
    /// How large the level is; medium when not given.
    #[serde(default)]
    pub size: LevelSize,
    /// The `.map` file to make, in the open project; `maps/random<seed>.map` when not given.
    /// One that is there already is refused.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

/// One room of a generated level.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomMade {
    /// Its shape: `hall`, `chamber`, `pit`, `split`, `court`, `cross`, `vault` or `atrium`.
    pub shape: String,
    /// Where a player's box stands in it, on its floor: a place to look at it from.
    pub stand: [f64; 3],
}

/// What `map.generate` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapGenerated {
    /// The map made, which is now the open map.
    pub path: PathBuf,
    /// The seed it was made from.
    pub seed: u64,
    /// The theme its textures are of.
    pub theme: String,
    /// Its rooms; the player starts in the first.
    pub rooms: Vec<RoomMade>,
    /// The place in `rooms` of the room the exit is in.
    pub exit: usize,
    /// Whether a door on the way needs a key, which is then in the level.
    pub locked: bool,
    /// How many floors it is on.
    pub floors: usize,
    /// How many loops its passages make.
    pub loops: usize,
    /// Its locks, in the order they are met: `silver_key`, `gold_key`, `button` (a door a
    /// button elsewhere opens) or `arena` (a door that opens when the room's monsters are
    /// dead).
    pub locks: Vec<String>,
    /// How many places are seen before they are reached: each atrium's balcony.
    pub teasers: usize,
    /// How many secret rooms it has, each behind a wall that opens when shot.
    pub secrets: usize,
    /// Its monsters.
    pub monsters: usize,
    /// Its items.
    pub items: usize,
    /// Its world's brushes.
    pub brushes: usize,
    /// Its entities.
    pub entities: usize,
}

/// `map.generate`.
pub(crate) struct MapGenerate;

impl<C: HasMap + 'static> Command<C> for MapGenerate {
    const ID: &'static str = MAP_GENERATE;
    const SUMMARY: &'static str = "Makes a random playable level and opens it: rooms of several \
        shapes on several floors joined by passages, stairs and doors, from a start to an exit, \
        with loops, a balcony dropped from, locks (keys, a button, an arena), a secret room, \
        lights, monsters and weapons, textured from the game's own levels, and no trap a player \
        cannot leave. One seed gives one level.";
    type Params = GenerateParams;
    type Output = MapGenerated;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        GenerateParams { seed, size, path }: GenerateParams,
    ) -> Result<Outcome<MapGenerated, C>, MapEditError> {
        if context.map_project().project_root().is_none() {
            return Err(MapEditError::NoProject);
        }
        // The clock is read here and nowhere else: the seed it gives is `seed_at`'s.
        let seed = seed.unwrap_or_else(|| {
            seed_at(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default(),
            )
        });
        let path = path
            .unwrap_or_else(|| PathBuf::from(MAPS_FOLDER).join(format!("{NAME_START}{seed}.map")));
        let path = new_map_path(context, &path)?;
        let name = path
            .file_stem()
            .map_or_default(|stem| stem.to_string_lossy().into_owned());
        let base = context.game_folders().base.ok_or(MapEditError::NoGame)?;
        let textures = context.map_store().level_names(&base).to_vec();
        let project = context.map_project();
        // Every game's code defines the world: a project that does not is one whose classes
        // cannot be read, such as the command line's, and id's are taken to be there. One
        // that does gets only what it defines, which may be none of them.
        let classes = if project.entity_class(WORLD_CLASSNAME).is_some() {
            Classes::Named(
                Classes::wanted()
                    .filter(|classname| project.entity_class(classname).is_some())
                    .map(str::to_owned)
                    .collect::<BTreeSet<String>>(),
            )
        } else {
            Classes::Any
        };
        let level = Request {
            seed,
            size: size.into(),
            textures: &textures,
            classes: &classes,
            name: &name,
        }
        .generate()
        .map_err(|error| MapEditError::Generate { error })?;
        let opened = write_new_map(context, path, &level.map)?;
        Ok(Outcome::outside_history(MapGenerated {
            path: opened.path,
            seed,
            theme: level.theme.to_owned(),
            rooms: level
                .rooms
                .iter()
                .map(|room| RoomMade {
                    shape: room.shape.name().to_owned(),
                    stand: room.stand,
                })
                .collect(),
            exit: level.exit,
            locked: level.locked,
            floors: level.report.floors,
            loops: level.report.loops,
            locks: level
                .report
                .locks
                .iter()
                .map(|kind| kind.name().to_owned())
                .collect(),
            teasers: level.report.teasers,
            secrets: level.report.secrets,
            monsters: level.monsters,
            items: level.items,
            brushes: level.brushes,
            entities: opened.entities,
        }))
    }
}

/// The seed of a level asked for with none at the time `since` the epoch: its seconds mixed
/// with the fraction of its second, so two asked for a moment apart differ, and small enough
/// to read and type.
fn seed_at(since: Duration) -> u64 {
    u64::from(since.subsec_nanos()) ^ since.as_secs()
}

#[cfg(test)]
mod tests;
