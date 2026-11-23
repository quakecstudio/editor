// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.build_room` and `map.build_passage` (L-49): one room of a shape of id's levels, or
//! one passage between two doorways, built by `qs-map-gen`'s kit into the open map as one step
//! with undo. A room is sealed but for the doorways asked for; rooms and passages that meet at
//! a doorway seal each other.

use qs_commands::{Command, Outcome};
use qs_map::{Brush, Entity, Item};
use qs_map_gen::{
    Arch, DOOR, DOOR_SILVER_KEY, Door, KIT_WALL, PassageKit, RoomKit, RoomShape, Side, TORCH,
    TORCH_LIGHT, Textures, UP, WallLights,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    EntityIndex, HasMap, MapDocument, MapEditError, MapProject, WORLD_CLASSNAME, edit_map,
    origin_text,
};

/// The id of `map.build_room`.
pub const MAP_BUILD_ROOM: &str = "map.build_room";
/// The id of `map.build_passage`.
pub const MAP_BUILD_PASSAGE: &str = "map.build_passage";

/// The shape of a room to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoomKind {
    /// Four pillars in from its corners, the ceiling raised in the middle, pilasters with
    /// torches. At least 192 in each half-width and 192 high.
    Hall,
    /// Eight walls under a dome of two steps, a dais of two steps in the middle; as wide as
    /// long. At least 224 in each half-width and 192 high.
    Chamber,
    /// A walk around a pit of liquid, two bridges crossing in its middle. At least 224 in
    /// each half-width and 192 high.
    Pit,
    /// The part toward the side it is turned to raised by 64, stairs up to it in the middle.
    /// At least 224 in each half-width and 256 high.
    Split,
    /// High walls open to the sky, an obelisk in the middle. At least 192 in each half-width
    /// and 256 high.
    Court,
    /// Two aisles 256 wide through the middle, the one toward the side it is turned to the
    /// higher, a lantern over their crossing. At least 224 in each half-width and 192 high.
    Cross,
    /// A long room under a pitched ceiling, its ridge toward the side it is turned to. At
    /// least 192 in each half-width and 224 high.
    Vault,
    /// Two storeys high, a balcony 128 up along the side it is turned to, whose doorway is at
    /// the balcony and which a player drops from to the floor and does not climb back to. At
    /// least 224 in each half-width and 320 high.
    Atrium,
}

impl From<RoomKind> for RoomShape {
    fn from(kind: RoomKind) -> Self {
        match kind {
            RoomKind::Hall => Self::Hall,
            RoomKind::Chamber => Self::Chamber,
            RoomKind::Pit => Self::Pit,
            RoomKind::Split => Self::Split,
            RoomKind::Court => Self::Court,
            RoomKind::Cross => Self::Cross,
            RoomKind::Vault => Self::Vault,
            RoomKind::Atrium => Self::Atrium,
        }
    }
}

/// A side of a room.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RoomSide {
    /// Toward +x.
    #[default]
    East,
    /// Toward +y.
    North,
    /// Toward -x.
    West,
    /// Toward -y.
    South,
}

impl From<RoomSide> for Side {
    fn from(side: RoomSide) -> Self {
        match side {
            RoomSide::East => Self::East,
            RoomSide::North => Self::North,
            RoomSide::West => Self::West,
            RoomSide::South => Self::South,
        }
    }
}

impl From<Side> for RoomSide {
    fn from(side: Side) -> Self {
        match side {
            Side::East => Self::East,
            Side::North => Self::North,
            Side::West => Self::West,
            Side::South => Self::South,
        }
    }
}

/// How the top of a doorway or a passage is shaped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TopShape {
    /// Square.
    #[default]
    Square,
    /// Its upper corners cut at half a right angle.
    Arch,
}

impl From<TopShape> for Arch {
    fn from(top: TopShape) -> Self {
        match top {
            TopShape::Square => Self::Square,
            TopShape::Arch => Self::Cut,
        }
    }
}

/// What stands in a passage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PassageDoor {
    /// Nothing.
    #[default]
    None,
    /// A door that opens when walked up to.
    Door,
    /// A door that needs the silver key.
    Locked,
}

impl From<PassageDoor> for Door {
    fn from(door: PassageDoor) -> Self {
        match door {
            PassageDoor::None => Self::None,
            PassageDoor::Door => Self::Plain,
            PassageDoor::Locked => Self::Locked,
        }
    }
}

/// Whether a room's lights are put in with it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Lighting {
    /// Its lights and its wall torches are placed.
    #[default]
    Placed,
    /// None are: the room is dark until lights are placed.
    None,
}

/// The textures to build with. What is not named is a theme's, chosen from the textures of
/// the game's levels: one `theme` number, one choice, so rooms built with the same number
/// match.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildTextures {
    /// The number the theme is chosen by; 0 when not given.
    #[serde(default)]
    pub theme: u64,
    /// The walls' texture.
    #[serde(default)]
    pub wall: Option<String>,
    /// The floors' texture.
    #[serde(default)]
    pub floor: Option<String>,
    /// The ceilings' texture.
    #[serde(default)]
    pub ceiling: Option<String>,
    /// The texture of pillars, steps, frames and platforms' sides.
    #[serde(default)]
    pub trim: Option<String>,
    /// The doors' texture.
    #[serde(default)]
    pub door: Option<String>,
    /// The texture of what a pit holds, such as `*lava1`.
    #[serde(default)]
    pub liquid: Option<String>,
    /// The sky's texture, for a courtyard.
    #[serde(default)]
    pub sky: Option<String>,
}

impl BuildTextures {
    /// The textures to build with: those named, and for the rest a theme's from the game's
    /// levels. A game is looked for only when a wall, a floor, a ceiling or a trim is not
    /// named.
    fn resolve<C: HasMap>(self, context: &mut C) -> Result<Textures, MapEditError> {
        let Self {
            theme,
            wall,
            floor,
            ceiling,
            trim,
            door,
            liquid,
            sky,
        } = self;
        if let (Some(wall), Some(floor), Some(ceiling), Some(trim)) =
            (wall.clone(), floor.clone(), ceiling.clone(), trim.clone())
        {
            return Ok(Textures {
                door: door.unwrap_or_else(|| trim.clone()),
                wall,
                floor,
                ceiling,
                trim,
                liquid,
                sky,
                wall_lights: WallLights::Torches,
            });
        }
        let base = context.game_folders().base.ok_or(MapEditError::NoGame)?;
        let themed = Textures::themed(context.map_store().level_names(&base), theme)
            .map_err(|error| MapEditError::Generate { error })?;
        Ok(Textures {
            wall: wall.unwrap_or(themed.wall),
            floor: floor.unwrap_or(themed.floor),
            ceiling: ceiling.unwrap_or(themed.ceiling),
            trim: trim.unwrap_or(themed.trim),
            door: door.unwrap_or(themed.door),
            liquid: liquid.or(themed.liquid),
            sky: sky.or(themed.sky),
            wall_lights: themed.wall_lights,
        })
    }
}

/// The parameters of `map.build_room`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildRoomParams {
    /// The room's shape.
    pub shape: RoomKind,
    /// The room's middle, in x and y. Every number here is a multiple of 16.
    pub middle: [f64; 2],
    /// The room's floor; the lower one of a `split`.
    pub floor: f64,
    /// Half the room's width in x and in y.
    pub half: [f64; 2],
    /// Floor to ceiling.
    pub height: f64,
    /// The side the room is turned to, which a `split`, a `cross`, a `vault` and an `atrium`
    /// use; east when not given.
    #[serde(default)]
    pub turned: RoomSide,
    /// The sides to open a doorway on, each 112 wide and 128 high, in the middle of its side;
    /// none when not given, which leaves the room sealed.
    #[serde(default)]
    pub doors: Vec<RoomSide>,
    /// How the doorways' tops are shaped; square when not given.
    #[serde(default)]
    pub arch: TopShape,
    /// How thick the rock around the room's space is, 32 to 256; 64 when not given. The
    /// room takes this much more than its half-widths on every side, above and below.
    #[serde(default)]
    pub wall: Option<f64>,
    /// The textures; a theme's from the game's levels when not given.
    #[serde(default)]
    pub textures: BuildTextures,
    /// Whether its lights are placed with it; placed when not given.
    #[serde(default)]
    pub lights: Lighting,
}

/// A doorway of a room built.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DoorwayBuilt {
    /// The side of the room it is on.
    pub side: RoomSide,
    /// The middle of the doorway on the room's outer face, at the floor there: an end for
    /// `map.build_passage`.
    pub at: [f64; 3],
}

/// What `map.build_room` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RoomBuilt {
    /// How many brushes it added to the world.
    pub brushes: usize,
    /// How many brushes the world has now.
    pub total: usize,
    /// Its doorways.
    pub doorways: Vec<DoorwayBuilt>,
    /// Where a player's box stands in it, on its floor and clear of what is built there: the
    /// origin for an `info_player_start`.
    pub stand: [f64; 3],
    /// Where other things may stand: origins for monsters and items.
    pub spots: Vec<[f64; 3]>,
    /// How many lights and torches it placed.
    pub lights: usize,
}

/// The parameters of `map.build_passage`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildPassageParams {
    /// One end: a doorway's `at`, as `map.build_room` answers it.
    pub from: [f64; 3],
    /// The other end: a doorway that faces the first along x or along y.
    pub to: [f64; 3],
    /// How the passage's top is shaped; square when not given, and square with a door,
    /// which is a box.
    #[serde(default)]
    pub arch: TopShape,
    /// What stands in it; nothing when not given.
    #[serde(default)]
    pub door: PassageDoor,
    /// How thick the rock around the passage is, 32 to 256; 64 when not given.
    #[serde(default)]
    pub wall: Option<f64>,
    /// The textures; a theme's from the game's levels when not given.
    #[serde(default)]
    pub textures: BuildTextures,
}

/// What `map.build_passage` answers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PassageBuilt {
    /// How many brushes it added to the world.
    pub brushes: usize,
    /// How many brushes the world has now.
    pub total: usize,
    /// How many steps it climbs between its ends' floors.
    pub steps: usize,
    /// Its door's entity, when it has one.
    pub door: Option<EntityIndex>,
}

impl MapDocument {
    /// Adds `brushes` to the world; answers how many the world then has.
    fn add_to_world(&mut self, brushes: Vec<Brush>) -> Result<usize, MapEditError> {
        let world = self.file.entities.first_mut().ok_or(MapEditError::NoMap)?;
        world.items.extend(brushes.into_iter().map(Item::Brush));
        Ok(world.brushes().count())
    }

    /// Adds an entity of `classname` with `pairs`, and with `brush` when it has one; answers
    /// its place.
    fn add_entity(
        &mut self,
        classname: &str,
        pairs: &[(&str, &str)],
        brush: Option<Brush>,
    ) -> Result<EntityIndex, MapEditError> {
        let mut entity = Entity::new();
        for (key, value) in core::iter::once(&("classname", classname)).chain(pairs) {
            entity
                .set(key.as_bytes(), value.as_bytes())
                .map_err(|error| self.map_error(error))?;
        }
        entity.items.extend(brush.map(Item::Brush));
        self.file.push_entity(entity);
        Ok(EntityIndex::new(self.file.entities.len().saturating_sub(1)))
    }
}

/// `map.build_room`.
pub(crate) struct MapBuildRoom;

impl<C: HasMap + 'static> Command<C> for MapBuildRoom {
    const ID: &'static str = MAP_BUILD_ROOM;
    const SUMMARY: &'static str = "Builds one room of a shape of id's levels into the open map, \
        sealed but for the doorways asked for, with its lights: a pillared hall, an eight-sided \
        domed chamber, a hall around a pit crossed by bridges, a room on two levels, a \
        courtyard, a cross of two aisles, a vaulted room, or an atrium with a balcony. Answers its \
        doorways, for map.build_passage, and where things may stand.";
    type Params = BuildRoomParams;
    type Output = RoomBuilt;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BuildRoomParams,
    ) -> Result<Outcome<RoomBuilt, C>, MapEditError> {
        // Before a game's levels are read for textures: there must be a map to build in.
        context.map_store().map().ok_or(MapEditError::NoMap)?;
        let textures = params.textures.resolve(context)?;
        let room = RoomKit {
            shape: params.shape.into(),
            middle: params.middle,
            floor: params.floor,
            half: params.half,
            height: params.height,
            turned: params.turned.into(),
            doors: params.doors.iter().map(|&side| side.into()).collect(),
            arch: params.arch.into(),
            wall: params.wall.unwrap_or(KIT_WALL),
        }
        .build(&textures)
        .map_err(|error| MapEditError::Generate { error })?;
        // A torch where the theme burns them and the project's code has one, a light of its
        // brightness where not.
        let burns = match textures.wall_lights {
            WallLights::Torches => context.map_project().entity_class(TORCH).is_some(),
            WallLights::Lamps => false,
        };
        let torch = if burns { TORCH } else { "light" };
        let (built, step) = edit_map(context, |document| {
            let added = room.brushes.len();
            let total = document.add_to_world(room.brushes)?;
            let mut lights = 0_usize;
            match params.lights {
                Lighting::None => {}
                Lighting::Placed => {
                    for (at, brightness) in &room.lights {
                        let pairs = [
                            ("origin", origin_text(*at)),
                            ("light", format!("{brightness}")),
                        ];
                        let pairs = pairs.each_ref().map(|(key, value)| (*key, value.as_str()));
                        document.add_entity("light", &pairs, None)?;
                    }
                    for at in &room.torches {
                        let origin = origin_text(*at);
                        let pairs = [("origin", origin.as_str()), ("light", TORCH_LIGHT)];
                        document.add_entity(torch, &pairs, None)?;
                    }
                    lights = room.lights.len().saturating_add(room.torches.len());
                }
            }
            Ok(RoomBuilt {
                brushes: added,
                total,
                doorways: room
                    .openings
                    .iter()
                    .map(|opening| DoorwayBuilt {
                        side: opening.side.into(),
                        at: opening.at,
                    })
                    .collect(),
                stand: room.stand,
                spots: room.spots,
                lights,
            })
        })?;
        Ok(Outcome::undoable(built, step))
    }
}

/// `map.build_passage`.
pub(crate) struct MapBuildPassage;

impl<C: HasMap + 'static> Command<C> for MapBuildPassage {
    const ID: &'static str = MAP_BUILD_PASSAGE;
    const SUMMARY: &'static str = "Builds a passage between two doorways of the open map that \
        face each other along x or along y, 112 wide and 128 high, with stairs where their \
        floors differ, a light, and a door when one is asked for.";
    type Params = BuildPassageParams;
    type Output = PassageBuilt;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BuildPassageParams,
    ) -> Result<Outcome<PassageBuilt, C>, MapEditError> {
        context.map_store().map().ok_or(MapEditError::NoMap)?;
        // A door the project's code has no class for would be removed by the game: where
        // the code's classes can be read, as its world's can, the door must be among them.
        let project = context.map_project();
        let doorless =
            project.entity_class(WORLD_CLASSNAME).is_some() && project.entity_class(DOOR).is_none();
        match params.door {
            PassageDoor::Door | PassageDoor::Locked if doorless => {
                return Err(MapEditError::NoClass {
                    classname: DOOR.to_owned(),
                });
            }
            PassageDoor::Door | PassageDoor::Locked | PassageDoor::None => {}
        }
        let textures = params.textures.resolve(context)?;
        let passage = PassageKit {
            from: params.from,
            to: params.to,
            arch: params.arch.into(),
            door: params.door.into(),
            wall: params.wall.unwrap_or(KIT_WALL),
        }
        .build(&textures)
        .map_err(|error| MapEditError::Generate { error })?;
        let (built, step) = edit_map(context, |document| {
            let added = passage.brushes.len();
            let total = document.add_to_world(passage.brushes)?;
            if let Some((at, brightness)) = passage.light {
                let pairs = [
                    ("origin", origin_text(at)),
                    ("light", format!("{brightness}")),
                ];
                let pairs = pairs.each_ref().map(|(key, value)| (*key, value.as_str()));
                document.add_entity("light", &pairs, None)?;
            }
            let door = match (passage.door, params.door) {
                (Some(brush), PassageDoor::Locked) => Some(document.add_entity(
                    DOOR,
                    &[("angle", UP), ("spawnflags", DOOR_SILVER_KEY)],
                    Some(brush),
                )?),
                (Some(brush), PassageDoor::Door | PassageDoor::None) => {
                    Some(document.add_entity(DOOR, &[("angle", UP)], Some(brush))?)
                }
                (None, _) => None,
            };
            Ok(PassageBuilt {
                brushes: added,
                total,
                steps: passage.steps,
                door,
            })
        })?;
        Ok(Outcome::undoable(built, step))
    }
}

#[cfg(test)]
mod tests;
