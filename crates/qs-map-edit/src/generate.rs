// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.generate` (L-48): a random playable level made by `qs-map-gen` from the textures of
//! the game's own levels and the entity classes the project defines, written into the project
//! as a new map and opened.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use qs_commands::{Command, Outcome};
use qs_map_gen::{Classes, Request, Size};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::textures::{Game, GameTextures};
use crate::{HasMap, MapEditError, MapProject, new_map_path, write_new_map};

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
    /// Its shape: `hall`, `chamber`, `pit`, `split`, `court`, `cross` or `vault`.
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
    /// Whether a door on the way needs the silver key, which is then in the level.
    pub locked: bool,
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
        shapes joined by passages, stairs and doors, from a start to an exit, with lights, \
        monsters, weapons and a key, textured from the game's own levels. One seed gives one \
        level.";
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
        let seed = seed.unwrap_or_else(drawn_seed);
        let path = path
            .unwrap_or_else(|| PathBuf::from(MAPS_FOLDER).join(format!("{NAME_START}{seed}.map")));
        let path = new_map_path(context, &path)?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let game = context.game_folders();
        let textures: Vec<String> = game.base.as_deref().map_or_default(|base| {
            GameTextures::read(&Game::at(base))
                .names()
                .map(str::to_owned)
                .collect()
        });
        let project = context.map_project();
        let defined: BTreeSet<String> = Classes::wanted()
            .filter(|classname| project.entity_class(classname).is_some())
            .map(str::to_owned)
            .collect();
        // A project whose code defines none of them is one the classes cannot be read from.
        let classes = if defined.is_empty() {
            Classes::Any
        } else {
            Classes::Named(defined)
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
            monsters: level.monsters,
            items: level.items,
            brushes: level.brushes,
            entities: opened.entities,
        }))
    }
}

/// A seed from the clock, for a level asked for with none: small enough to read and type.
fn drawn_seed() -> u64 {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    u64::from(since.subsec_nanos()) ^ since.as_secs()
}
