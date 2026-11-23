// SPDX-License-Identifier: GPL-3.0-or-later

//! One room or one passage on its own (L-49): the shapes the level maker builds, for a map
//! that is being built by hand or by an agent. A room is its own block of rock with its space
//! taken out, sealed but for the doorways asked for; a passage joins two doorways, with stairs
//! where their floors differ. Rooms and passages that meet at a doorway seal each other.

use qs_map::{Brush, Surface, TexDef};

use crate::layout::{Doorway, RoomPlan, STOREY, Side};
use crate::mass::{Bounds, Mass};
use crate::mission::{KeyColour, Role};
use crate::passage::Passage;
use crate::rng::Rng;
use crate::rooms::RoomPart;
use crate::rooms::{Part, Room};
use crate::theme::{Paints, WorldType};
use crate::{GenError, RoomShape};

/// The grid a room's and a passage's numbers are on: every corner the kit makes is then on
/// whole units (MAP-3.3).
pub const KIT_GRID: f64 = 16.0;
/// The thinnest a shell's rock is around its space.
pub const KIT_MIN_WALL: f64 = 32.0;
/// The thickest a shell's rock is around its space.
pub const KIT_MAX_WALL: f64 = 256.0;
/// How thick a shell's rock is when none is asked for.
pub const KIT_WALL: f64 = 64.0;
/// The widest a room's half-width may be.
pub const KIT_MAX_HALF: f64 = 1024.0;
/// The highest a room may be, floor to ceiling.
pub const KIT_MAX_HEIGHT: f64 = 1024.0;
/// How wide a doorway is: a passage's width.
pub const KIT_DOORWAY_WIDTH: f64 = 112.0;
/// How high a doorway is: a passage's height above the floor at each of its ends.
pub const KIT_DOORWAY_HEIGHT: f64 = 128.0;
/// The longest a passage may be.
pub const KIT_MAX_LENGTH: f64 = 2048.0;
/// The most a passage may climb between its ends' floors.
pub const KIT_MAX_CLIMB: f64 = 512.0;

/// The least half-width and the least height of each shape: what stands in it needs them.
const SMALL_HALF: f64 = 192.0;
const WIDE_HALF: f64 = 224.0;
const LOW_HEIGHT: f64 = 192.0;
const VAULT_HEIGHT: f64 = 224.0;
const HIGH_HEIGHT: f64 = 256.0;
/// An atrium's balcony is a storey up, and a doorway's height above it.
const ATRIUM_HEIGHT: f64 = STOREY + LOW_HEIGHT;

impl RoomShape {
    /// Every shape.
    pub const ALL: [Self; 8] = [
        Self::Hall,
        Self::Chamber,
        Self::Pit,
        Self::Split,
        Self::Court,
        Self::Cross,
        Self::Vault,
        Self::Atrium,
    ];

    /// The least half-width a room of the shape has, in x and in y.
    #[must_use]
    pub const fn least_half(self) -> f64 {
        match self {
            Self::Hall | Self::Court | Self::Vault => SMALL_HALF,
            Self::Chamber | Self::Pit | Self::Split | Self::Cross | Self::Atrium => WIDE_HALF,
        }
    }

    /// The least height a room of the shape has, floor to ceiling.
    #[must_use]
    pub const fn least_height(self) -> f64 {
        match self {
            Self::Hall | Self::Chamber | Self::Pit | Self::Cross => LOW_HEIGHT,
            Self::Vault => VAULT_HEIGHT,
            Self::Split | Self::Court => HIGH_HEIGHT,
            Self::Atrium => ATRIUM_HEIGHT,
        }
    }
}

/// The textures a room or a passage is painted with, by name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Textures {
    /// The walls.
    pub wall: String,
    /// The floors.
    pub floor: String,
    /// The ceilings.
    pub ceiling: String,
    /// Pillars, steps, frames and platforms' sides.
    pub trim: String,
    /// Doors.
    pub door: String,
    /// What a pit holds; a pit needs one.
    pub liquid: Option<String>,
    /// The sky; a courtyard needs one.
    pub sky: Option<String>,
    /// What lights a room's walls.
    pub wall_lights: WallLights,
}

/// What lights a room's walls, where a theme has a say: a base is lit electrically.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WallLights {
    /// Torches, where the game has them.
    #[default]
    Torches,
    /// Lights with nothing to see.
    Lamps,
}

impl Textures {
    /// The textures of a theme chosen by `seed` from the names in `names`, as a generated
    /// level's are: one seed, one choice.
    ///
    /// # Errors
    ///
    /// [`GenError::NoTextures`] when `names` holds no plain texture.
    pub fn themed(names: &[String], seed: u64) -> Result<Self, GenError> {
        let paints = Paints::choose(names, &mut Rng::new(seed))?;
        let name = |surface: &Surface| String::from_utf8_lossy(&surface.texture).into_owned();
        Ok(Self {
            wall: name(&paints.wall),
            floor: name(&paints.floor),
            ceiling: name(&paints.ceiling),
            trim: name(&paints.trim),
            door: name(&paints.door),
            liquid: paints.liquid.as_ref().map(name),
            sky: paints.sky.as_ref().map(name),
            wall_lights: match paints.world {
                WorldType::Medieval | WorldType::Metal => WallLights::Torches,
                WorldType::Base => WallLights::Lamps,
            },
        })
    }
}

impl From<&Textures> for Paints {
    fn from(textures: &Textures) -> Self {
        let surface = |name: &str| Surface {
            texture: name.as_bytes().to_vec(),
            texdef: TexDef::Standard {
                shift: [0.0, 0.0],
                rotate: 0.0,
                scale: [1.0, 1.0],
            },
        };
        Self::named(
            surface(&textures.wall),
            surface(&textures.floor),
            surface(&textures.ceiling),
            surface(&textures.trim),
            surface(&textures.door),
            textures.liquid.as_deref().map(surface),
            textures.sky.as_deref().map(surface),
        )
    }
}

/// How a passage's or a doorway's top is shaped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Arch {
    /// Square.
    #[default]
    Square,
    /// Its upper corners cut at half a right angle.
    Cut,
}

/// What stands in a passage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Door {
    /// Nothing.
    #[default]
    None,
    /// A door that opens when walked up to.
    Plain,
    /// A door that needs the silver key.
    Locked,
}

/// A room to build.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomKit {
    /// Its shape.
    pub shape: RoomShape,
    /// Its middle, in x and y.
    pub middle: [f64; 2],
    /// Its floor; the lower one of a room on two levels.
    pub floor: f64,
    /// Half its width in x and in y.
    pub half: [f64; 2],
    /// Floor to ceiling.
    pub height: f64,
    /// The side it is turned to: the raised part of a room on two levels is on it, a vault's
    /// ridge and a cross's higher aisle run toward it.
    pub turned: Side,
    /// The sides it has a doorway on.
    pub doors: Vec<Side>,
    /// How its doorways' tops are shaped.
    pub arch: Arch,
    /// How thick the rock around its space is.
    pub wall: f64,
    /// The parts it is dressed with on top of its shape; a part its shape does not take is
    /// passed over.
    pub parts: Vec<RoomPart>,
}

/// A doorway of a room built: where a passage to it starts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opening {
    /// The side of the room it is on.
    pub side: Side,
    /// The middle of the doorway on the shell's outer face, at the floor there.
    pub at: [f64; 3],
}

/// A room built.
#[derive(Clone, Debug)]
pub struct RoomParts {
    /// Its brushes, for the world.
    pub brushes: Vec<Brush>,
    /// Its doorways.
    pub openings: Vec<Opening>,
    /// Where a player's box stands in it, on its floor and clear of what is built there.
    pub stand: [f64; 3],
    /// Where other things may stand, each the origin of a box on the floor there.
    pub spots: Vec<[f64; 3]>,
    /// Where its lights hang, each with how bright it is.
    pub lights: Vec<([f64; 3], f64)>,
    /// Where torches burn on its walls.
    pub torches: Vec<[f64; 3]>,
}

/// Why a room or a passage cannot be built as asked.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KitError {
    /// A number is not a multiple of [`KIT_GRID`].
    OffGrid,
    /// The room is smaller than its shape needs, or larger than a room may be.
    Size {
        /// The shape.
        shape: RoomShape,
        /// The least half-width it needs.
        least_half: f64,
        /// The least height it needs.
        least_height: f64,
    },
    /// A chamber that is not as wide as it is long.
    NotSquare,
    /// The wall is thinner than [`KIT_MIN_WALL`] or thicker than [`KIT_MAX_WALL`].
    Wall,
    /// A side is given a doorway twice.
    DoorTwice,
    /// A pit with no liquid texture, or a courtyard with no sky texture.
    NeedsTexture(RoomShape),
    /// A passage whose ends are not in line along x or along y, or are one place.
    Askew,
    /// A passage longer than [`KIT_MAX_LENGTH`], or that climbs more than [`KIT_MAX_CLIMB`].
    TooFar,
    /// A passage too short for the stairs between its floors.
    TooSteep {
        /// The least length that holds them.
        least_length: f64,
    },
    /// Bays asked of a room whose wall is too thin to hold them.
    BayWall {
        /// The least wall that holds them.
        least_wall: f64,
    },
}

impl core::fmt::Display for KitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OffGrid => write!(f, "every number must be a multiple of {KIT_GRID}"),
            Self::Size {
                shape,
                least_half,
                least_height,
            } => write!(
                f,
                "a {} is at least {least_half} in each half-width and {least_height} high, \
                 and at most {KIT_MAX_HALF} and {KIT_MAX_HEIGHT}",
                shape.name()
            ),
            Self::NotSquare => f.write_str("a chamber is as wide as it is long"),
            Self::Wall => write!(f, "a wall is from {KIT_MIN_WALL} to {KIT_MAX_WALL} thick"),
            Self::DoorTwice => f.write_str("a side has one doorway"),
            Self::NeedsTexture(shape) => write!(
                f,
                "a {} needs a {} texture",
                shape.name(),
                match shape {
                    RoomShape::Court => "sky",
                    RoomShape::Hall
                    | RoomShape::Chamber
                    | RoomShape::Pit
                    | RoomShape::Split
                    | RoomShape::Cross
                    | RoomShape::Vault
                    | RoomShape::Atrium => "liquid",
                }
            ),
            Self::Askew => {
                f.write_str("a passage's two ends are in line along x or along y, and apart")
            }
            Self::TooFar => write!(
                f,
                "a passage is at most {KIT_MAX_LENGTH} long and climbs at most {KIT_MAX_CLIMB}"
            ),
            Self::BayWall { least_wall } => {
                write!(f, "bays need a wall at least {least_wall} thick")
            }
            Self::TooSteep { least_length } => write!(
                f,
                "the passage is too short for its stairs: it needs a length of {least_length}"
            ),
        }
    }
}

impl core::error::Error for KitError {}

/// Whether every one of `numbers` is on the grid.
fn on_grid(numbers: &[f64]) -> bool {
    numbers.iter().all(|number| {
        let cells = number / KIT_GRID;
        number.is_finite() && (cells - cells.round()).abs() < f64::EPSILON
    })
}

impl RoomKit {
    /// Whether the room can be built as asked with `textures`.
    fn check(&self, textures: &Textures) -> Result<(), KitError> {
        let [mx, my] = self.middle;
        let [hx, hy] = self.half;
        if !on_grid(&[mx, my, hx, hy, self.floor, self.height, self.wall]) {
            return Err(KitError::OffGrid);
        }
        let (least_half, least_height) = (self.shape.least_half(), self.shape.least_height());
        let fits = self
            .half
            .iter()
            .all(|half| (least_half..=KIT_MAX_HALF).contains(half))
            && (least_height..=KIT_MAX_HEIGHT).contains(&self.height);
        if !fits {
            return Err(KitError::Size {
                shape: self.shape,
                least_half,
                least_height,
            });
        }
        if !(KIT_MIN_WALL..=KIT_MAX_WALL).contains(&self.wall) {
            return Err(KitError::Wall);
        }
        let least_wall = RoomPart::BAY_DEPTH + KIT_MIN_WALL;
        let bays = self.parts.contains(&RoomPart::Bays) && RoomPart::Bays.fits(self.shape);
        if bays && self.wall < least_wall {
            return Err(KitError::BayWall { least_wall });
        }
        let twice = self.doors.iter().enumerate().any(|(place, side)| {
            self.doors
                .iter()
                .skip(place.saturating_add(1))
                .any(|s| s == side)
        });
        if twice {
            return Err(KitError::DoorTwice);
        }
        match self.shape {
            RoomShape::Chamber if (hx - hy).abs() > f64::EPSILON => Err(KitError::NotSquare),
            RoomShape::Pit if textures.liquid.is_none() => {
                Err(KitError::NeedsTexture(RoomShape::Pit))
            }
            RoomShape::Court if textures.sky.is_none() => {
                Err(KitError::NeedsTexture(RoomShape::Court))
            }
            RoomShape::Hall
            | RoomShape::Chamber
            | RoomShape::Pit
            | RoomShape::Split
            | RoomShape::Court
            | RoomShape::Cross
            | RoomShape::Vault
            | RoomShape::Atrium => Ok(()),
        }
    }

    /// Builds the room, painted with `textures`.
    ///
    /// # Errors
    ///
    /// [`GenError::Kit`] for a room that cannot be built as asked; [`GenError::Brush`] should
    /// not happen, and says what could not be made if it does.
    pub fn build(&self, textures: &Textures) -> Result<RoomParts, GenError> {
        self.check(textures).map_err(GenError::Kit)?;
        let paints = Paints::from(textures);
        let plan = RoomPlan {
            middle: self.middle,
            shape: self.shape,
            floor: self.floor,
            half: self.half,
            height: self.height,
            turned: self.turned,
            depth: 0,
            role: Role::Way,
        };
        let room = Room::new(&plan, Part::Way).dressed(&self.parts, &self.doors);
        let [mx, my] = self.middle;
        let [hx, hy] = self.half;
        let [below, above] = room.beyond();
        let mut mass = Mass::of_blocks(
            &[Bounds {
                min: [
                    mx - hx - self.wall,
                    my - hy - self.wall,
                    self.floor - below - self.wall,
                ],
                max: [
                    mx + hx + self.wall,
                    my + hy + self.wall,
                    self.floor + self.height + above + self.wall,
                ],
            }],
            &paints.wall,
        )?;
        let built = room.build(&mut mass, &paints)?;
        let mut openings = Vec::new();
        let doorway = match self.arch {
            Arch::Square => Doorway::Open,
            Arch::Cut => Doorway::Arch,
        };
        for &side in &self.doors {
            let [fx, fy] = side.facing();
            let reach = plan.reach(side);
            let floor = plan.door_floor(side);
            let start = [mx + fx * reach, my + fy * reach];
            Passage::new(start, side, self.wall, [floor, floor], doorway)
                .build(&mut mass, &paints)?;
            let out = reach + self.wall;
            openings.push(Opening {
                side,
                at: [mx + fx * out, my + fy * out, floor],
            });
        }
        let mut brushes = mass.into_brushes();
        brushes.extend(built.solids);
        Ok(RoomParts {
            brushes,
            openings,
            stand: room.stand(),
            spots: built.spots,
            lights: built.lights,
            torches: built.torches,
        })
    }
}

/// A passage to build, between two doorways.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassageKit {
    /// One end: the middle of a doorway, at its floor.
    pub from: [f64; 3],
    /// The other end.
    pub to: [f64; 3],
    /// How its top is shaped. A passage with a door is square whatever this says: a door is
    /// a box.
    pub arch: Arch,
    /// What stands in it.
    pub door: Door,
    /// How thick the rock around its space is.
    pub wall: f64,
}

/// A passage built.
#[derive(Clone, Debug)]
pub struct PassageParts {
    /// Its brushes, for the world.
    pub brushes: Vec<Brush>,
    /// Its door's brush, for a `func_door`, when it has one.
    pub door: Option<Brush>,
    /// Where its light hangs, and how bright it is.
    pub light: Option<([f64; 3], f64)>,
    /// How many steps it climbs.
    pub steps: usize,
}

impl PassageKit {
    /// The side the passage leaves its first end by, and its length; `None` when its ends are
    /// not in line along an axis, or are one place.
    fn run(&self) -> Option<(Side, f64)> {
        let ([ax, ay, _], [bx, by, _]) = (self.from, self.to);
        let (dx, dy) = (bx - ax, by - ay);
        let level = |d: f64| d.abs() < f64::EPSILON;
        match (level(dx), level(dy)) {
            (false, true) if dx > 0.0 => Some((Side::East, dx)),
            (false, true) => Some((Side::West, -dx)),
            (true, false) if dy > 0.0 => Some((Side::North, dy)),
            (true, false) => Some((Side::South, -dy)),
            (true, true) | (false, false) => None,
        }
    }

    /// Builds the passage, painted with `textures`.
    ///
    /// # Errors
    ///
    /// [`GenError::Kit`] for a passage that cannot be built as asked; [`GenError::Brush`]
    /// should not happen, and says what could not be made if it does.
    pub fn build(&self, textures: &Textures) -> Result<PassageParts, GenError> {
        let ([ax, ay, az], [bx, by, bz]) = (self.from, self.to);
        if !on_grid(&[ax, ay, az, bx, by, bz, self.wall]) {
            return Err(GenError::Kit(KitError::OffGrid));
        }
        if !(KIT_MIN_WALL..=KIT_MAX_WALL).contains(&self.wall) {
            return Err(GenError::Kit(KitError::Wall));
        }
        let (side, length) = self.run().ok_or(GenError::Kit(KitError::Askew))?;
        let climb = (bz - az).abs();
        if length > KIT_MAX_LENGTH || climb > KIT_MAX_CLIMB {
            return Err(GenError::Kit(KitError::TooFar));
        }
        let least_length = Passage::least_length(climb);
        if length < least_length {
            return Err(GenError::Kit(KitError::TooSteep { least_length }));
        }
        let doorway = match (self.door, self.arch) {
            (Door::Plain, _) => Doorway::Door,
            (Door::Locked, _) => Doorway::Locked(KeyColour::Silver),
            (Door::None, Arch::Cut) => Doorway::Arch,
            (Door::None, Arch::Square) => Doorway::Open,
        };
        let paints = Paints::from(textures);
        let passage = Passage::new([ax, ay], side, length, [az, bz], doorway);
        let mut mass = Mass::of_blocks(&[passage.shell(self.wall)], &paints.wall)?;
        let passed = passage.build(&mut mass, &paints)?;
        let mut brushes = mass.into_brushes();
        // Each solid of a passage is a step.
        let steps = passed.solids.len();
        brushes.extend(passed.solids);
        Ok(PassageParts {
            brushes,
            door: passed.door.map(|(door, _)| door),
            light: passed.light,
            steps,
        })
    }
}
