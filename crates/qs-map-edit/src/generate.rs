// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.generate` (L-48): a random playable level made by `qs-map-gen` from the textures of
//! the game's own levels and the entity classes the project defines, written into the project
//! as a new map and opened.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use qs_bsp::{Bsp, BspLoader};
use qs_commands::{Command, Outcome};
use qs_map_gen::playable::{Ground, Moves, PlayError, Skill, Trap};
use qs_map_gen::{Classes, Level, LevelForm, LockKind, Request, Size};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    HasMap, MapEditError, MapProject, RoomKind, WORLD_CLASSNAME, new_map_path, write_new_map,
};

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
    /// Its shape.
    pub shape: RoomKind,
    /// Where a player's box stands in it, on its floor: a place to look at it from.
    pub stand: [f64; 3],
}

/// A lock a generated level has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LockMade {
    /// A door the silver key opens.
    SilverKey,
    /// A door the gold key opens.
    GoldKey,
    /// A door a button elsewhere opens.
    Button,
    /// A door that opens when the monsters of the room before it are dead.
    Arena,
}

impl From<LockKind> for LockMade {
    fn from(kind: LockKind) -> Self {
        match kind {
            LockKind::SilverKey => Self::SilverKey,
            LockKind::GoldKey => Self::GoldKey,
            LockKind::Button => Self::Button,
            LockKind::Arena => Self::Arena,
        }
    }
}

/// The form a generated level takes besides its hook.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FormMade {
    /// The hook alone.
    Hook,
    /// A great room early on, the rooms of its keys and buttons off it.
    Hub,
    /// A courtyard with a lake of water and a walk around it.
    Lake,
    /// Courtyards open into each other under the sky.
    Canyon,
}

impl From<LevelForm> for FormMade {
    fn from(form: LevelForm) -> Self {
        match form {
            LevelForm::Hook => Self::Hook,
            LevelForm::Hub => Self::Hub,
            LevelForm::Lake => Self::Lake,
            LevelForm::Canyon => Self::Canyon,
        }
    }
}

/// What `map.generate` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapGenerated {
    /// The map made, which is now the open map.
    pub path: PathBuf,
    /// The seed it was made from: the one asked for, or the first after it whose level the
    /// playability check finds no trap in.
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
    /// Its locks, in the order they are met.
    pub locks: Vec<LockMade>,
    /// How many places are seen before they are reached: each atrium's balcony.
    pub teasers: usize,
    /// How many secret rooms it has, each behind a wall that opens when shot.
    pub secrets: usize,
    /// The form it takes besides its hook.
    pub form: FormMade,
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
        cannot leave: each level is compiled and checked as map.playability checks a map, with \
        no jump, and a seed whose level fails gives way to the seeds after it. One seed gives \
        one level.";
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
        let (seed, level) = checked_level(seed, |seed| {
            Request {
                seed,
                size: size.into(),
                textures: &textures,
                classes: &classes,
                name: &name,
            }
            .generate()
            .map_err(|error| MapEditError::Generate { error })
        })?;
        let opened = write_new_map(context, path, &level.map)?;
        Ok(Outcome::outside_history(MapGenerated {
            path: opened.path,
            seed,
            theme: level.theme.to_owned(),
            rooms: level
                .rooms
                .iter()
                .map(|room| RoomMade {
                    shape: room.shape.into(),
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
                .copied()
                .map(LockMade::from)
                .collect(),
            teasers: level.report.teasers,
            secrets: level.report.secrets,
            form: level.report.form.into(),
            monsters: level.monsters,
            items: level.items,
            brushes: level.brushes,
            entities: opened.entities,
        }))
    }
}

/// How many seeds, from the one asked for, are tried for a level the check finds no trap in.
const SEEDS_TRIED: u64 = 8;

/// The first level `make` makes from `seed` or a seed after it, at most [`SEEDS_TRIED`], in which
/// the playability check (L-77) with the generator's moves finds no trap between its first room
/// and its exit, compiled as `qs-map-bsp` compiles it; and its seed.
fn checked_level(
    seed: u64,
    mut make: impl FnMut(u64) -> Result<Level, MapEditError>,
) -> Result<(u64, Level), MapEditError> {
    let mut last = Trap::NoExit;
    for tried in 0..SEEDS_TRIED {
        let at = seed.wrapping_add(tried);
        let level = make(at)?;
        let compiled = qs_map_bsp::Compiler::default()
            .compile(&level.map.to_bytes(), None)
            .map_err(|error| MapEditError::LevelCompile { error })?;
        let bsp = Bsp::read(&compiled.bsp, BspLoader::default()).map_err(|error| {
            MapEditError::LevelUnchecked {
                error: PlayError::Lump(error.to_string()),
            }
        })?;
        let ground = Ground::new(&bsp, Moves::Generator, Skill::Normal)
            .map_err(|error| MapEditError::LevelUnchecked { error })?;
        let stand = |room: usize| {
            level
                .rooms
                .get(room)
                .map_or_default(|room| room.stand.map(single))
        };
        match ground.check(stand(0), stand(level.exit)) {
            Ok(()) => return Ok((at, level)),
            Err(trap) => last = trap,
        }
    }
    Err(MapEditError::Trapped { seed, trap: last })
}

/// A position of the map, as the game's single precision holds it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the game holds positions in single precision"
)]
const fn single(value: f64) -> f32 {
    value as f32
}

/// The seed of a level asked for with none at the time `since` the epoch: its seconds mixed
/// with the fraction of its second, so two asked for a moment apart differ, and small enough
/// to read and type.
fn seed_at(since: Duration) -> u64 {
    u64::from(since.subsec_nanos()) ^ since.as_secs()
}

#[cfg(test)]
mod tests;
