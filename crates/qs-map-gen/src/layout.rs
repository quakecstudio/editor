// SPDX-License-Identifier: GPL-3.0-or-later

//! The plan of a level: rooms on a grid, joined into a path from the start to the exit, with
//! rooms off it, a key in one of those and its door further along the path. The model is
//! OBLIGE's: a level is a path through rooms with a quest on it, drawn before anything is
//! built.

use crate::rng::Rng;
use crate::{GenError, RoomShape, Size};

/// The distance between the middles of two rooms side by side.
pub(crate) const CELL: f64 = 768.0;
/// Half of [`CELL`].
pub(crate) const HALF_CELL: f64 = CELL / 2.0;

/// The halves a room's width may be.
const HALVES: &[f64] = &[192.0, 224.0, 256.0];
/// The halves of a room that needs space: the chamber, the pit and the room on two levels.
const WIDE_HALVES: &[f64] = &[224.0, 256.0];
/// The half a room has should a list above be empty.
const SPARE_HALF: f64 = 224.0;
/// The heights a room may have, floor to ceiling.
const HEIGHTS: &[f64] = &[192.0, 224.0, 256.0];
/// The height of a room on two levels, from its lower floor.
const SPLIT_HEIGHT: f64 = 288.0;
/// The height of a courtyard's walls.
const COURT_HEIGHT: f64 = 352.0;
/// The height of a vault at its ridge.
const VAULT_HEIGHT: f64 = 288.0;
/// How far a floor is above or below the one before it.
const FLOOR_STEP: f64 = 64.0;
/// The highest and the lowest a floor goes.
const FLOOR_REACH: f64 = 128.0;
/// How much higher the raised part of a room on two levels is.
pub(crate) const RAISE: f64 = 64.0;

/// The rooms of a small, a medium and a large level.
const SMALL_ROOMS: usize = 5;
const MEDIUM_ROOMS: usize = 8;
const LARGE_ROOMS: usize = 12;
/// Of that many rooms, one is off the path.
const ROOMS_A_BRANCH: usize = 4;
/// The times a path is drawn again when it walks into a corner.
const PATH_TRIES: usize = 64;
/// The chances in [`CHANCES`] that a passage is arched, and that it has a door.
const ARCH_CHANCE: usize = 4;
const DOOR_CHANCE: usize = 3;
/// What a chance is out of.
const CHANCES: usize = 10;

/// A side of a room, by the way it faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    /// +x.
    East,
    /// +y.
    North,
    /// -x.
    West,
    /// -y.
    South,
}

impl Side {
    /// Every side.
    pub(crate) const ALL: [Self; 4] = [Self::East, Self::North, Self::West, Self::South];

    /// The side across from it.
    pub(crate) const fn opposite(self) -> Self {
        match self {
            Self::East => Self::West,
            Self::North => Self::South,
            Self::West => Self::East,
            Self::South => Self::North,
        }
    }

    /// The way it faces, in x and y.
    pub(crate) const fn facing(self) -> [f64; 2] {
        match self {
            Self::East => [1.0, 0.0],
            Self::North => [0.0, 1.0],
            Self::West => [-1.0, 0.0],
            Self::South => [0.0, -1.0],
        }
    }

    /// The yaw of something that faces its way, as an `angle` key takes it.
    pub(crate) const fn yaw(self) -> &'static str {
        match self {
            Self::East => "0",
            Self::North => "90",
            Self::West => "180",
            Self::South => "270",
        }
    }

    /// Whether it faces along x.
    const fn along_x(self) -> bool {
        match self {
            Self::East | Self::West => true,
            Self::North | Self::South => false,
        }
    }
}

/// A place on the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cell {
    x: i32,
    y: i32,
}

impl Cell {
    /// The grid's middle, where the first room is.
    const ORIGIN: Self = Self { x: 0, y: 0 };

    /// The cell beside it on `side`.
    const fn beside(self, side: Side) -> Self {
        match side {
            Side::East => Self {
                x: self.x.saturating_add(1),
                y: self.y,
            },
            Side::North => Self {
                x: self.x,
                y: self.y.saturating_add(1),
            },
            Side::West => Self {
                x: self.x.saturating_sub(1),
                y: self.y,
            },
            Side::South => Self {
                x: self.x,
                y: self.y.saturating_sub(1),
            },
        }
    }

    /// Its middle, in map units.
    pub(crate) fn middle(self) -> [f64; 2] {
        [f64::from(self.x) * CELL, f64::from(self.y) * CELL]
    }
}

/// A room of the plan, by its place in [`Plan::rooms`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RoomId(usize);

impl RoomId {
    /// Its place in [`Plan::rooms`].
    pub(crate) const fn place(self) -> usize {
        self.0
    }
}

/// A room as planned.
#[derive(Clone, Debug)]
pub(crate) struct RoomPlan {
    /// Where it is.
    pub(crate) cell: Cell,
    /// Its shape.
    pub(crate) shape: RoomShape,
    /// Its floor; the lower one of a room on two levels.
    pub(crate) floor: f64,
    /// Half its width in x and in y.
    pub(crate) half: [f64; 2],
    /// Floor to ceiling.
    pub(crate) height: f64,
    /// The side the room is turned to: the raised part of a room on two levels is on it, a
    /// vault's ridge and a cross's higher aisle run toward it.
    pub(crate) turned: Side,
    /// How many rooms from the start it is.
    pub(crate) depth: usize,
}

impl RoomPlan {
    /// How far its wall on `side` is from its middle.
    pub(crate) fn reach(&self, side: Side) -> f64 {
        let [x, y] = self.half;
        if side.along_x() { x } else { y }
    }

    /// The floor at its doorway on `side`.
    pub(crate) fn door_floor(&self, side: Side) -> f64 {
        match self.shape {
            RoomShape::Split if side == self.turned => self.floor + RAISE,
            RoomShape::Split
            | RoomShape::Hall
            | RoomShape::Chamber
            | RoomShape::Pit
            | RoomShape::Court
            | RoomShape::Cross
            | RoomShape::Vault => self.floor,
        }
    }
}

/// What stands in a passage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Doorway {
    /// Nothing: a square opening.
    Open,
    /// Nothing, under an arch.
    Arch,
    /// A door that opens when walked up to.
    Door,
    /// A door that needs the silver key.
    Locked,
}

/// Two rooms joined.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Link {
    /// The room it leaves.
    pub(crate) from: RoomId,
    /// The room it reaches.
    pub(crate) to: RoomId,
    /// The side of `from` it leaves by.
    pub(crate) side: Side,
    /// What stands in it.
    pub(crate) doorway: Doorway,
}

/// A level as planned.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
    /// The rooms: the path from the start to the exit, then the rooms off it.
    pub(crate) rooms: Vec<RoomPlan>,
    /// The passages.
    pub(crate) links: Vec<Link>,
    /// How many rooms the path has: the last of them holds the exit.
    pub(crate) path: usize,
    /// The room the key is in, when a door is locked.
    pub(crate) key: Option<RoomId>,
}

/// What the plan can paint: a pit needs a liquid and a courtyard a sky.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Has {
    /// Whether the game has a liquid.
    pub(crate) liquid: Liquid,
    /// Whether the game has a sky.
    pub(crate) sky: Sky,
}

/// Whether the game has a liquid texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Liquid {
    /// It has.
    There,
    /// It has none.
    Missing,
}

/// Whether the game has a sky texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sky {
    /// It has.
    There,
    /// It has none.
    Missing,
}

impl Size {
    /// How many rooms a level of this size has.
    const fn rooms(self) -> usize {
        match self {
            Self::Small => SMALL_ROOMS,
            Self::Medium => MEDIUM_ROOMS,
            Self::Large => LARGE_ROOMS,
        }
    }
}

impl Plan {
    /// A plan of `size`, drawn by `rng`.
    ///
    /// # Errors
    ///
    /// [`GenError::Layout`] when no path could be drawn, which the tries make all but
    /// impossible.
    pub(crate) fn draw(size: Size, has: Has, rng: &mut Rng) -> Result<Self, GenError> {
        let rooms = size.rooms();
        let branches = rooms.checked_div(ROOMS_A_BRANCH).unwrap_or(0);
        let path = rooms.saturating_sub(branches);
        let cells = (0..PATH_TRIES)
            .find_map(|_| Self::walk(path, rng))
            .ok_or(GenError::Layout)?;
        let mut plan = Self {
            rooms: Vec::new(),
            links: Vec::new(),
            path,
            key: None,
        };
        let mut floor = 0.0;
        for (place, &cell) in cells.iter().enumerate() {
            let shape = if place.saturating_add(1) == path {
                // The exit is in the chamber, under its dome.
                RoomShape::Chamber
            } else {
                Self::shape(has, rng)
            };
            plan.rooms.push(Self::room(cell, shape, floor, place, rng));
            floor = Self::next_floor(floor, rng);
        }
        for (place, pair) in cells.windows(2).enumerate() {
            let [from, to] = pair else { continue };
            let Some(side) = Side::ALL.into_iter().find(|&s| from.beside(s) == *to) else {
                continue;
            };
            plan.links.push(Link {
                from: RoomId(place),
                to: RoomId(place.saturating_add(1)),
                side,
                doorway: Self::doorway(rng),
            });
        }
        plan.branch(branches, has, rng);
        Ok(plan)
    }

    /// A path of `length` cells from the grid's middle that never crosses itself; `None` when
    /// it walks into a corner.
    fn walk(length: usize, rng: &mut Rng) -> Option<Vec<Cell>> {
        let mut cells = vec![Cell::ORIGIN];
        while cells.len() < length {
            let last = *cells.last()?;
            let free: Vec<Cell> = Side::ALL
                .into_iter()
                .map(|side| last.beside(side))
                .filter(|cell| !cells.contains(cell))
                .collect();
            cells.push(*rng.pick(&free)?);
        }
        Some(cells)
    }

    /// A shape for a room that is not the exit's.
    fn shape(has: Has, rng: &mut Rng) -> RoomShape {
        let mut shapes = vec![
            RoomShape::Hall,
            RoomShape::Chamber,
            RoomShape::Split,
            RoomShape::Cross,
            RoomShape::Vault,
        ];
        match has.liquid {
            Liquid::There => shapes.push(RoomShape::Pit),
            Liquid::Missing => {}
        }
        match has.sky {
            Sky::There => shapes.push(RoomShape::Court),
            Sky::Missing => {}
        }
        rng.pick(&shapes).copied().unwrap_or(RoomShape::Hall)
    }

    /// A room of `shape` at `cell`, its size drawn by `rng`.
    fn room(cell: Cell, shape: RoomShape, floor: f64, depth: usize, rng: &mut Rng) -> RoomPlan {
        let halves = match shape {
            RoomShape::Hall | RoomShape::Court | RoomShape::Vault => HALVES,
            RoomShape::Chamber | RoomShape::Pit | RoomShape::Split | RoomShape::Cross => {
                WIDE_HALVES
            }
        };
        let mut half = || rng.pick(halves).copied().unwrap_or(SPARE_HALF);
        let half = match shape {
            // A chamber is as wide as it is long, so its eight sides are even.
            RoomShape::Chamber => {
                let both = half();
                [both, both]
            }
            RoomShape::Hall
            | RoomShape::Pit
            | RoomShape::Split
            | RoomShape::Court
            | RoomShape::Cross
            | RoomShape::Vault => [half(), half()],
        };
        let height = match shape {
            RoomShape::Split => SPLIT_HEIGHT,
            RoomShape::Court => COURT_HEIGHT,
            RoomShape::Vault => VAULT_HEIGHT,
            RoomShape::Hall | RoomShape::Chamber | RoomShape::Pit | RoomShape::Cross => {
                rng.pick(HEIGHTS).copied().unwrap_or(SPLIT_HEIGHT)
            }
        };
        let turned = rng.pick(&Side::ALL).copied().unwrap_or(Side::East);
        RoomPlan {
            cell,
            shape,
            floor,
            half,
            height,
            turned,
            depth,
        }
    }

    /// The floor of the room after one at `floor`: a step up, a step down or level.
    fn next_floor(floor: f64, rng: &mut Rng) -> f64 {
        let steps = [-FLOOR_STEP, 0.0, 0.0, FLOOR_STEP];
        let next = floor + rng.pick(&steps).copied().unwrap_or(0.0);
        next.clamp(-FLOOR_REACH, FLOOR_REACH)
    }

    /// What stands in a passage of the path.
    fn doorway(rng: &mut Rng) -> Doorway {
        if rng.chance(ARCH_CHANCE, CHANCES) {
            Doorway::Arch
        } else if rng.chance(DOOR_CHANCE, CHANCES) {
            Doorway::Door
        } else {
            Doorway::Open
        }
    }

    /// Adds up to `count` rooms off the path, the key in the first of them and its door on
    /// the path after the room that one is off.
    fn branch(&mut self, count: usize, has: Has, rng: &mut Rng) {
        for _ in 0..count {
            // Not off the exit's room: the level ends there.
            let mut parents: Vec<usize> = (0..self.path.saturating_sub(1)).collect();
            rng.shuffle(&mut parents);
            let taken: Vec<Cell> = self.rooms.iter().map(|room| room.cell).collect();
            let found = parents.into_iter().find_map(|parent| {
                let room = self.rooms.get(parent)?;
                let mut sides = Side::ALL;
                rng.shuffle(&mut sides);
                let side = sides
                    .into_iter()
                    .find(|&side| !taken.contains(&room.cell.beside(side)))?;
                Some((parent, side, room.cell.beside(side), room.floor, room.depth))
            });
            let Some((parent, side, cell, floor, depth)) = found else {
                return;
            };
            let id = RoomId(self.rooms.len());
            let shape = Self::shape(has, rng);
            let floor = Self::next_floor(floor, rng);
            self.rooms
                .push(Self::room(cell, shape, floor, depth.saturating_add(1), rng));
            self.links.push(Link {
                from: RoomId(parent),
                to: id,
                side,
                doorway: Doorway::Arch,
            });
            if self.key.is_none() {
                self.lock(parent, id, rng);
            }
        }
    }

    /// Puts the key in `room`, which is off the path's room `parent`, and locks a passage of
    /// the path after that room.
    fn lock(&mut self, parent: usize, room: RoomId, rng: &mut Rng) {
        let after: Vec<usize> = (parent..self.path.saturating_sub(1)).collect();
        let Some(&locked) = rng.pick(&after) else {
            return;
        };
        // The path's passages are the first links, in its order.
        if let Some(link) = self.links.get_mut(locked) {
            link.doorway = Doorway::Locked;
            self.key = Some(room);
        }
    }

    /// The room `id` names.
    pub(crate) fn room_of(&self, id: RoomId) -> Option<&RoomPlan> {
        self.rooms.get(id.0)
    }

    /// The sides of the room at `place` that a passage leaves by.
    pub(crate) fn doors_of(&self, place: usize) -> Vec<Side> {
        self.links
            .iter()
            .filter_map(|link| {
                if link.from.0 == place {
                    Some(link.side)
                } else if link.to.0 == place {
                    Some(link.side.opposite())
                } else {
                    None
                }
            })
            .collect()
    }
}
