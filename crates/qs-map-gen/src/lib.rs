// SPDX-License-Identifier: GPL-3.0-or-later

//! The level maker: a random playable map from a seed (L-48).
//!
//! [`Request::generate`] plans a level as OBLIGE does, rooms on a grid joined into a path from
//! the start to the exit with a key and its door on the way, then builds it: each room's cell
//! starts as a block of rock, and every room and passage is taken out of the rock, so the
//! level is sealed by what was never removed. The rooms are the stock shapes of id's levels:
//! a pillared hall, an eight-sided chamber under a stepped dome, a hall around a pit crossed
//! by bridges, a room on two levels, and a courtyard under the sky.
//!
//! One seed gives one level: nothing here reads a clock or the machine.

mod kit;
mod layout;
mod mass;
mod mission;
mod passage;
mod populate;
mod rng;
mod rooms;
mod score;
#[cfg(test)]
mod tests;
mod theme;

use std::collections::BTreeSet;
use std::fmt;

use qs_map::{Brush, BrushError, Entity, Item, MapError, MapFile};

pub use kit::{
    Arch, Door, KIT_DOORWAY_HEIGHT, KIT_DOORWAY_WIDTH, KIT_GRID, KIT_MAX_CLIMB, KIT_MAX_HALF,
    KIT_MAX_HEIGHT, KIT_MAX_LENGTH, KIT_MAX_WALL, KIT_MIN_WALL, KIT_WALL, KitError, Opening,
    PassageKit, PassageParts, RoomKit, RoomParts, Textures, WallLights,
};
pub use layout::Side;
use layout::{Doorway, Has, Liquid, Plan, RoomPlan, Sky};
use mass::{Coat, Mass, Point};
pub use mission::{LockKind, Report};
use mission::{Mission, Role, Signal};
use passage::Passage;
use populate::{Depth, Placed};
use rng::Rng;
use rooms::{Part, Room};
use score::Score;
use theme::{Paints, WorldType};

/// How large a level is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Size {
    /// Five rooms.
    Small,
    /// Eight rooms.
    #[default]
    Medium,
    /// Twelve rooms.
    Large,
}

/// The shape of a room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomShape {
    /// A hall with pillars, its ceiling raised in the middle.
    Hall,
    /// An eight-sided chamber under a stepped dome.
    Chamber,
    /// A hall around a pit of liquid crossed by bridges.
    Pit,
    /// A room on two levels joined by stairs.
    Split,
    /// A courtyard under the sky.
    Court,
    /// Two aisles that cross under a lantern.
    Cross,
    /// A long room under a pitched vault.
    Vault,
    /// A room two storeys high with a balcony along one side, dropped from to its floor.
    Atrium,
}

impl RoomShape {
    /// Its name, as a summary gives it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hall => "hall",
            Self::Chamber => "chamber",
            Self::Pit => "pit",
            Self::Split => "split",
            Self::Court => "court",
            Self::Cross => "cross",
            Self::Vault => "vault",
            Self::Atrium => "atrium",
        }
    }
}

/// The entity classes the game defines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Classes {
    /// Not known: every class of id's game is taken to be there.
    #[default]
    Any,
    /// These, by classname.
    Named(BTreeSet<String>),
}

impl Classes {
    /// Whether the game defines `classname`.
    #[must_use]
    pub fn has(&self, classname: &str) -> bool {
        match self {
            Self::Any => true,
            Self::Named(names) => names.contains(classname),
        }
    }
}

/// Why a level could not be made.
#[derive(Debug)]
pub enum GenError {
    /// The game has no texture to paint a wall with.
    NoTextures,
    /// No path of rooms could be drawn.
    Layout,
    /// A brush could not be made.
    Brush(BrushError),
    /// An entity could not be written.
    Map(MapError),
    /// A room or a passage cannot be built as asked.
    Kit(KitError),
}

impl fmt::Display for GenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTextures => write!(f, "the game has no texture to paint a wall with"),
            Self::Layout => write!(f, "no path of rooms could be drawn"),
            Self::Brush(error) => write!(f, "a brush could not be made: {error}"),
            Self::Map(error) => write!(f, "an entity could not be written: {error}"),
            Self::Kit(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for GenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoTextures | Self::Layout => None,
            Self::Brush(error) => Some(error),
            Self::Map(error) => Some(error),
            Self::Kit(error) => Some(error),
        }
    }
}

impl From<BrushError> for GenError {
    fn from(error: BrushError) -> Self {
        Self::Brush(error)
    }
}

impl From<MapError> for GenError {
    fn from(error: MapError) -> Self {
        Self::Map(error)
    }
}

/// What to make.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// The seed: one seed, one level.
    pub seed: u64,
    /// How large.
    pub size: Size,
    /// The names of the textures the game has.
    pub textures: &'a [String],
    /// The entity classes the game defines.
    pub classes: &'a Classes,
    /// The level's own name, as `map` takes it: its exit leads back to it.
    pub name: &'a str,
}

/// One room of a level made.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomMade {
    /// Its shape.
    pub shape: RoomShape,
    /// Where the player's box stands in it, on its floor and clear of what is built there.
    pub stand: [f64; 3],
}

/// A level made.
#[derive(Clone, Debug)]
pub struct Level {
    /// The map.
    pub map: MapFile,
    /// The theme it is painted in.
    pub theme: &'static str,
    /// Its rooms: the first holds the start, and [`Level::exit`] says which holds the exit.
    pub rooms: Vec<RoomMade>,
    /// The place in [`Level::rooms`] of the room the exit is in.
    pub exit: usize,
    /// Whether a door on the way needs a key.
    pub locked: bool,
    /// How many monsters it holds.
    pub monsters: usize,
    /// How many items, the key among them.
    pub items: usize,
    /// How many brushes its world has.
    pub brushes: usize,
    /// What its mission came to: its floors, loops, locks, teasers and secrets.
    pub report: Report,
}

/// The key that opens a locked door: `item_key1` is the silver key (`items.qc`), and
/// `DOOR_SILVER_KEY = 16` (`doors.qc`).
const KEY: &str = "item_key1";
/// The `spawnflags` of a door that needs the silver key: `DOOR_SILVER_KEY = 16` (`doors.qc`).
pub const DOOR_SILVER_KEY: &str = "16";
/// How high above a liquid's surface the edge of a pool may be for a swimming player to climb
/// out (`client.qc`, `CheckWaterJump`), measured by the water-escape test of `qs-tests`.
pub const CLIMB_OUT: f64 = 28.0;
/// The lift (`plats.qc`).
pub const PLAT: &str = "func_plat";
/// The door (`doors.qc`).
pub const DOOR: &str = "func_door";
/// The torch on a wall (`misc.qc`).
pub const TORCH: &str = "light_torch_small_walltorch";
/// How bright a torch on a wall is, as its `light` key.
pub const TORCH_LIGHT: &str = "250";
/// The `angle` of a door that opens upward (`SetMovedir`, `subs.qc`).
pub const UP: &str = "-1";
/// Half the width of the exit's trigger, its height, and its posts' place, half-width and
/// height.
const EXIT_HALF: f64 = 32.0;
const EXIT_HEIGHT: f64 = 64.0;
const POST_AT: f64 = 48.0;
const POST_HALF: f64 = 8.0;
const POST_HEIGHT: f64 = 72.0;
/// How far above the floor the exit's light is, and how bright.
const EXIT_LIGHT_LIFT: f64 = 96.0;
const EXIT_LIGHT: f64 = 300.0;

/// A button: half its width and height, how far it stands out of its wall, and how far above
/// the floor its bottom is, at a standing player's chest.
const BUTTON_HALF: f64 = 16.0;
const BUTTON_DEPTH: f64 = 8.0;
const BUTTON_LIFT: f64 = 32.0;
/// The `wait` of a door or button that stays where it moved to (`doors.qc`, `buttons.qc`).
const STAY_OPEN: &str = "-1";
/// The `spawnflags` of a secret door that stays open once opened and moves down first
/// (`doors.qc`: `SECRET_OPEN_ONCE` 1, `SECRET_1ST_DOWN` 4).
const SECRET_FLAGS: &str = "5";
/// The `spawnflags` of a megahealth (`items.qc`: `H_MEGA` 2).
const MEGAHEALTH: &str = "2";
/// Half the width of a secret room's trigger.
const SECRET_HALF: f64 = 64.0;

/// How many plans are drawn from a seed for the best to be kept.
const CANDIDATES: usize = 8;

/// A level being put together.
struct Assembly<'a> {
    request: &'a Request<'a>,
    paints: Paints,
    mass: Mass,
    solids: Vec<Brush>,
    entities: Vec<Entity>,
}

/// An entity of `classname` with `pairs`.
fn entity(classname: &str, pairs: &[(&str, &str)]) -> Result<Entity, MapError> {
    let mut entity = Entity::new();
    entity.set(b"classname", classname.as_bytes())?;
    for (key, value) in pairs {
        entity.set(key.as_bytes(), value.as_bytes())?;
    }
    Ok(entity)
}

/// `point` as an `origin` key writes it.
fn written([x, y, z]: Point) -> String {
    format!("{x} {y} {z}")
}

impl Assembly<'_> {
    /// Adds a point entity of `classname` at `origin` with `pairs`.
    fn place(
        &mut self,
        classname: &str,
        origin: Point,
        pairs: &[(&str, &str)],
    ) -> Result<(), MapError> {
        let mut made = entity(classname, pairs)?;
        made.set(b"origin", written(origin).as_bytes())?;
        self.entities.push(made);
        Ok(())
    }

    /// Adds a brush entity of `classname` with `pairs`.
    fn place_brush(
        &mut self,
        classname: &str,
        brush: Brush,
        pairs: &[(&str, &str)],
    ) -> Result<(), MapError> {
        let mut made = entity(classname, pairs)?;
        made.items.push(Item::Brush(brush));
        self.entities.push(made);
        Ok(())
    }

    /// Adds a light of `brightness` at `origin`.
    fn light(&mut self, (origin, brightness): (Point, f64)) -> Result<(), MapError> {
        self.place("light", origin, &[("light", &format!("{brightness}"))])
    }

    /// Adds a torch at `origin` where the game has one and the level is no base, whose light
    /// is electric; a light of a torch's brightness otherwise.
    fn torch(&mut self, origin: Point) -> Result<(), MapError> {
        let burns = match self.paints.world {
            WorldType::Medieval | WorldType::Metal => self.request.classes.has(TORCH),
            WorldType::Base => false,
        };
        let classname = if burns { TORCH } else { "light" };
        self.place(classname, origin, &[("light", TORCH_LIGHT)])
    }

    /// Adds each of `placed`.
    fn place_all(&mut self, placed: &[Placed]) -> Result<(), MapError> {
        for one in placed {
            self.place(one.classname, one.origin, &[])?;
        }
        Ok(())
    }

    /// A button in the middle of `room`'s wall on `side`, at a player's chest, that opens the
    /// door of `signal` and stays in (`buttons.qc`: `wait` -1 never returns). It moves into
    /// the wall, the way the wall faces out.
    fn button(&mut self, room: &RoomPlan, side: Side, signal: Signal) -> Result<(), GenError> {
        let [mx, my] = room.middle;
        let [fx, fy] = side.facing();
        let reach = room.reach(side);
        let (out, back) = (reach - BUTTON_DEPTH, reach);
        let wall = [mx + fx * out, my + fy * out];
        let deep = [mx + fx * back, my + fy * back];
        let across = [-fy * BUTTON_HALF, fx * BUTTON_HALF];
        let floor = room.door_floor(side);
        let min = [
            wall[0].min(deep[0]) - across[0].abs(),
            wall[1].min(deep[1]) - across[1].abs(),
            floor + BUTTON_LIFT,
        ];
        let max = [
            wall[0].max(deep[0]) + across[0].abs(),
            wall[1].max(deep[1]) + across[1].abs(),
            floor + BUTTON_LIFT + BUTTON_HALF + BUTTON_HALF,
        ];
        let brush = Coat::all(&self.paints.door).boxed(mass::Bounds { min, max })?;
        self.place_brush(
            mission::BUTTON,
            brush,
            &[
                ("angle", side.yaw()),
                ("target", &signal.name()),
                ("wait", STAY_OPEN),
            ],
        )?;
        Ok(())
    }

    /// An arena's monsters, each firing at its death (`monster_death_use`) a counter at
    /// `stand` that opens the door of `signal` once all of them have (`trigger_counter`).
    fn arena(&mut self, signal: Signal, stand: Point, monsters: &[Placed]) -> Result<(), GenError> {
        let counter = format!("{}_count", signal.name());
        for monster in monsters {
            self.place(monster.classname, monster.origin, &[("target", &counter)])?;
        }
        let count = monsters.len().to_string();
        self.place(
            mission::COUNTER,
            stand,
            &[
                ("targetname", &counter),
                ("target", &signal.name()),
                ("count", &count),
            ],
        )?;
        Ok(())
    }

    /// A secret room's reward at `stand`, a megahealth (`item_health` with `H_MEGA` 2,
    /// `items.qc`), in a `trigger_secret` that counts it found.
    fn secret(&mut self, [x, y, z]: Point) -> Result<(), GenError> {
        let floor = z - rooms::STAND;
        let trigger = Coat::all(self.paints.trigger()).boxed(mass::Bounds {
            min: [x - SECRET_HALF, y - SECRET_HALF, floor],
            max: [x + SECRET_HALF, y + SECRET_HALF, floor + EXIT_HEIGHT],
        })?;
        self.place_brush(mission::SECRET, trigger, &[])?;
        let half = populate::CORNERED_HALF;
        self.place(
            "item_health",
            [x - half, y - half, floor],
            &[("spawnflags", MEGAHEALTH)],
        )?;
        Ok(())
    }

    /// Builds and fills `plan`'s rooms, each as its role has it, counting what is placed in
    /// `level`. Returns the signals of arenas that had no monster, whose doors must open by
    /// touch.
    fn rooms(
        &mut self,
        plan: &Plan,
        level: &mut Level,
        rng: &mut Rng,
    ) -> Result<Vec<Signal>, GenError> {
        let exit = level.exit;
        let deepest = plan.rooms.iter().map(|room| room.depth).max().unwrap_or(0);
        let mut unopened = Vec::new();
        for (place, room_plan) in plan.rooms.iter().enumerate() {
            let part = if place == exit { Part::Exit } else { Part::Way };
            let room = Room::new(room_plan, part);
            let built = room.build(&mut self.mass, &self.paints)?;
            self.solids.extend(built.solids);
            for light in built.lights {
                self.light(light)?;
            }
            for torch in built.torches {
                self.torch(torch)?;
            }
            let stand = room.stand();
            level.rooms.push(RoomMade {
                shape: room_plan.shape,
                stand,
            });
            if place == 0 {
                let facing = plan.doors_of(place).first().copied().unwrap_or(Side::East);
                self.place("info_player_start", stand, &[("angle", facing.yaw())])?;
            }
            if place == exit {
                self.exit(stand)?;
            }
            let depth = Depth {
                rooms: room_plan.depth,
                deepest,
            };
            let filled = self.request.classes.fill(depth, &built.spots, rng);
            self.place_all(&filled.items)?;
            level.items = level.items.saturating_add(filled.items.len());
            level.monsters = level.monsters.saturating_add(filled.monsters.len());
            match room_plan.role {
                Role::Way => self.place_all(&filled.monsters)?,
                Role::Key(colour) => {
                    self.place_all(&filled.monsters)?;
                    self.place(colour.classname(), stand, &[])?;
                    level.items = level.items.saturating_add(1);
                }
                Role::Button(signal) => {
                    self.place_all(&filled.monsters)?;
                    let door = plan.doors_of(place).first().copied().unwrap_or(Side::East);
                    self.button(room_plan, door.opposite(), signal)?;
                }
                Role::Arena(signal) => {
                    if filled.monsters.is_empty() {
                        unopened.push(signal);
                    } else {
                        self.arena(signal, stand, &filled.monsters)?;
                    }
                }
                Role::Secret => {
                    self.place_all(&filled.monsters)?;
                    self.secret(stand)?;
                    level.items = level.items.saturating_add(1);
                }
            }
        }
        Ok(unopened)
    }

    /// Builds `plan`'s passages and their doors: a remote door whose signal is in
    /// `unopened` opens by touch.
    fn passages(&mut self, plan: &Plan, unopened: &[Signal]) -> Result<(), GenError> {
        for link in &plan.links {
            let Some(passage) = Passage::of(plan, link) else {
                continue;
            };
            let passed = passage.build(&mut self.mass, &self.paints)?;
            self.solids.extend(passed.solids);
            if let Some(light) = passed.light {
                self.light(light)?;
            }
            if let Some((plat, height)) = passed.lift {
                self.place_brush(PLAT, plat, &[("height", &format!("{height}"))])?;
            }
            let Some((door, doorway)) = passed.door else {
                continue;
            };
            match doorway {
                Doorway::Locked(colour) => self.place_brush(
                    DOOR,
                    door,
                    &[("angle", UP), ("spawnflags", colour.door_flags())],
                )?,
                Doorway::Remote(signal) if !unopened.contains(&signal) => {
                    self.place_brush(
                        DOOR,
                        door,
                        &[
                            ("angle", UP),
                            ("targetname", &signal.name()),
                            ("wait", STAY_OPEN),
                        ],
                    )?;
                }
                Doorway::Secret => self.place_brush(
                    mission::SECRET_DOOR,
                    door,
                    &[("angle", link.side.yaw()), ("spawnflags", SECRET_FLAGS)],
                )?,
                Doorway::Door | Doorway::Remote(_) | Doorway::Open | Doorway::Arch => {
                    self.place_brush(DOOR, door, &[("angle", UP)])?;
                }
            }
        }
        Ok(())
    }

    /// The exit at `stand`: a trigger back to the level's own start, between four posts
    /// under a light.
    fn exit(&mut self, [x, y, z]: Point) -> Result<(), GenError> {
        let floor = z - rooms::STAND;
        let trim = Coat::all(&self.paints.trim);
        for (px, py) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            let (cx, cy) = (x + px * POST_AT, y + py * POST_AT);
            self.solids.push(trim.boxed(mass::Bounds {
                min: [cx - POST_HALF, cy - POST_HALF, floor],
                max: [cx + POST_HALF, cy + POST_HALF, floor + POST_HEIGHT],
            })?);
        }
        let trigger = Coat::all(self.paints.trigger()).boxed(mass::Bounds {
            min: [x - EXIT_HALF, y - EXIT_HALF, floor],
            max: [x + EXIT_HALF, y + EXIT_HALF, floor + EXIT_HEIGHT],
        })?;
        self.place_brush(
            "trigger_changelevel",
            trigger,
            &[("map", self.request.name)],
        )?;
        self.light(([x, y, floor + EXIT_LIGHT_LIFT], EXIT_LIGHT))?;
        Ok(())
    }
}

impl Request<'_> {
    /// The best of [`CANDIDATES`] plans drawn by `rng`, each with its mission, by its
    /// [`Score`]; the first of the best when several score alike.
    ///
    /// # Errors
    ///
    /// [`GenError::Layout`] when no path could be drawn.
    fn best(&self, has: Has, rng: &mut Rng) -> Result<(Plan, Report), GenError> {
        let mut best: Option<(Score, Plan, Report)> = None;
        for _ in 0..CANDIDATES {
            let mut plan = Plan::draw(self.size, has, rng)?;
            let report = Mission::arrange(&mut plan, self.size, self.classes, rng);
            let score = Score::of(&plan, &report);
            if best.as_ref().is_none_or(|(kept, _, _)| score > *kept) {
                best = Some((score, plan, report));
            }
        }
        best.map(|(_, plan, report)| (plan, report))
            .ok_or(GenError::Layout)
    }

    /// Makes the level.
    ///
    /// # Errors
    ///
    /// [`GenError::NoTextures`] when the game has no plain texture; the others should not
    /// happen, and say what could not be made if one does.
    pub fn generate(&self) -> Result<Level, GenError> {
        let mut rng = Rng::new(self.seed);
        let paints = Paints::choose(self.textures, &mut rng)?;
        let has = Has {
            liquid: if paints.liquid.is_some() {
                Liquid::There
            } else {
                Liquid::Missing
            },
            sky: if paints.sky.is_some() {
                Sky::There
            } else {
                Sky::Missing
            },
        };
        let (mut plan, report) = self.best(has, &mut rng)?;
        if self.classes.has(PLAT) {
            plan.lifts(&mut rng);
        }
        if !self.classes.has(DOOR) {
            for link in &mut plan.links {
                if link.doorway == Doorway::Door {
                    link.doorway = Doorway::Open;
                }
            }
        }
        let mut assembly = Assembly {
            request: self,
            mass: Mass::of(&plan.cells, &paints.wall)?,
            paints,
            solids: Vec::new(),
            entities: Vec::new(),
        };
        let exit = plan.path.saturating_sub(1);
        let mut level = Level {
            map: MapFile::new(),
            theme: assembly.paints.name,
            rooms: Vec::new(),
            exit,
            locked: report
                .locks
                .iter()
                .any(|kind| matches!(kind, LockKind::SilverKey | LockKind::GoldKey)),
            monsters: 0,
            items: 0,
            brushes: 0,
            report,
        };
        let unopened = assembly.rooms(&plan, &mut level, &mut rng)?;
        assembly.passages(&plan, &unopened)?;
        let mut world = entity(
            "worldspawn",
            &[
                ("message", &format!("Random level {}", self.seed)),
                ("worldtype", assembly.paints.world.value()),
            ],
        )?;
        let rock = assembly.mass.into_brushes();
        level.brushes = rock.len().saturating_add(assembly.solids.len());
        world
            .items
            .extend(rock.into_iter().chain(assembly.solids).map(Item::Brush));
        level.map.push_entity(world);
        for made in assembly.entities {
            level.map.push_entity(made);
        }
        Ok(level)
    }
}
