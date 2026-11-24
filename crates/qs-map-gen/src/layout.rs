// SPDX-License-Identifier: GPL-3.0-or-later

//! The plan of a level: rooms on a grid of columns, each on a storey, joined into a path from
//! the start to the exit, with rooms off it. The path doubles back on itself in a hook: it
//! leaves a tall atrium on its floor, climbs a storey through two rooms and comes back onto a
//! balcony over the atrium, which the player sees before reaching it and drops from, one way,
//! back to its floor. Floors are in the plan from the start, as OBLIGE's author found a
//! generator for Quake needs (Doomworld topic 114383, 2020). What locks the way and where its
//! keys are is the mission's (`mission`).

use crate::kit::JUNCTION_HALF;
use crate::mission::{KeyColour, Role, Signal};
use crate::passage::Passage;
use crate::rng::Rng;
use crate::theme::{Hazard, Paints};
use crate::{GenError, RoomShape, Size};

/// The distance between the middles of two rooms side by side.
pub(crate) const CELL: f64 = 768.0;
/// Half of [`CELL`].
pub(crate) const HALF_CELL: f64 = CELL / 2.0;

/// The halves a room's width may be.
const HALVES: &[f64] = &[192.0, 224.0, 256.0];
/// The halves of a room that needs space: the chamber, the pit and the room on two levels.
const WIDE_HALVES: &[f64] = &[224.0, 256.0];
/// The halves of a great room, as wide as its cell holds with a wall of 32 to spare; two side
/// by side are 64 apart, so the passage between them is an arch in a thick wall.
const GREAT_HALVES: &[f64] = &[288.0, 320.0, 352.0];
/// A great room's height above an ordinary one's.
const GREAT_RISE: f64 = 64.0;
/// The chances in [`CHANCES`] that a room, and a courtyard, is a great one.
const GREAT_CHANCE: usize = 2;
const GREAT_COURT_CHANCE: usize = 6;
/// A junction's half, a corridor's turn, and its height.
const JUNCTION_HALVES: &[f64] = &[JUNCTION_HALF];
const JUNCTION_HEIGHT: f64 = 192.0;
/// The chances in [`CHANCES`] that a room of the path, not the start's nor the exit's, is a
/// junction where the corridor turns.
const JUNCTION_CHANCE: usize = 2;
/// What a room gives way by when a passage is too short for its stairs, and the most times.
const FIT_STEP: f64 = 32.0;
const FIT_TRIES: usize = 16;
/// The latest place on the path a hub may be, and how many courtyards a canyon runs through.
const HUB_LATEST: usize = 3;
const CANYON_RUN: usize = 3;
/// The shapes the exit's room may be.
const EXIT_SHAPES: &[RoomShape] = &[RoomShape::Chamber, RoomShape::Hall, RoomShape::Vault];
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
/// How far one storey's floor is above the one below. A passage between rooms on storeys side
/// by side climbs it with a room on two levels' [`RAISE`] besides, 192 in all, which a
/// passage of the least length, 256, holds with its landings (`Passage::least_length`); and a
/// drop of it is one way, since a jump rises about 45 (`docs/agent-reference/maps.md`).
pub(crate) const STOREY: f64 = 128.0;
/// The highest and the lowest storey.
const STOREY_REACH: i32 = 2;
/// The height of an atrium above its balcony.
const ATRIUM_UPPER: f64 = 192.0;
/// How much higher the raised part of a room on two levels is.
pub(crate) const RAISE: f64 = 64.0;

/// The rooms of a small, a medium and a large level.
const SMALL_ROOMS: usize = 5;
const MEDIUM_ROOMS: usize = 8;
const LARGE_ROOMS: usize = 12;
/// The times a path is drawn again when it walks into a corner or, in a level of more than
/// [`SMALL_ROOMS`], has no hook.
const PATH_TRIES: usize = 256;
/// The chances in [`CHANCES`] that a step of the path climbs or falls a storey.
const STOREY_CHANCE: usize = 3;
/// The most loops a level is given besides its hook.
const MOST_LOOPS: usize = 2;
/// The chances in [`CHANCES`] that a passage that climbs a storey has a lift.
const LIFT_CHANCE: usize = 5;
/// The places a hook takes after its atrium, three, and one more for the exit after it.
const HOOK_PLACES: usize = 4;
/// The chances in [`CHANCES`] that a passage is arched, and that it has a door.
const ARCH_CHANCE: usize = 4;
const DOOR_CHANCE: usize = 3;
/// What a chance is out of.
const CHANCES: usize = 10;

/// A side of a room, by the way it faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
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

    /// The two sides a quarter turn from it.
    pub(crate) const fn across(self) -> [Self; 2] {
        match self {
            Self::East | Self::West => [Self::North, Self::South],
            Self::North | Self::South => [Self::East, Self::West],
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

/// How far from the grid's middle a room may be, in cells each way. The game sends a position
/// as the short `(int) (f * 8)` (NET-1.1), which holds less than 4096: four cells of 768 and
/// half a cell more is 3456, and a fifth would pass it.
const GRID_REACH: i32 = 4;

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

    /// Whether a room may be there: within [`GRID_REACH`] of the grid's middle.
    const fn in_reach(self) -> bool {
        self.x.abs() <= GRID_REACH && self.y.abs() <= GRID_REACH
    }

    /// Its middle, in map units.
    pub(crate) fn middle(self) -> [f64; 2] {
        [f64::from(self.x) * CELL, f64::from(self.y) * CELL]
    }
}

/// A room of the plan, by its place in [`Plan::rooms`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RoomId(pub(crate) usize);

impl RoomId {
    /// Its place in [`Plan::rooms`].
    pub(crate) const fn place(self) -> usize {
        self.0
    }
}

/// A room as planned.
#[derive(Clone, Debug)]
pub(crate) struct RoomPlan {
    /// Its middle on the map.
    pub(crate) middle: [f64; 2],
    /// Its shape.
    pub(crate) shape: RoomShape,
    /// Its floor; the lower one of a room on two levels and of an atrium.
    pub(crate) floor: f64,
    /// Half its width in x and in y.
    pub(crate) half: [f64; 2],
    /// Floor to ceiling.
    pub(crate) height: f64,
    /// The side the room is turned to: the raised part of a room on two levels is on it, a
    /// vault's ridge and a cross's higher aisle run toward it, and an atrium's balcony is
    /// along it.
    pub(crate) turned: Side,
    /// How many rooms from the start it is.
    pub(crate) depth: usize,
    /// What it is to the level's mission.
    pub(crate) role: Role,
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
            RoomShape::Atrium if side == self.turned => self.floor + STOREY,
            RoomShape::Split
            | RoomShape::Atrium
            | RoomShape::Hall
            | RoomShape::Chamber
            | RoomShape::Pit
            | RoomShape::Court
            | RoomShape::Cross
            | RoomShape::Vault
            | RoomShape::Junction
            | RoomShape::Lake => self.floor,
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
    /// A door that needs a key.
    Locked(KeyColour),
    /// A door opened by something elsewhere: a button, or the death of an arena's monsters.
    Remote(Signal),
    /// A secret door, at the passage's first end, that opens when shot.
    Secret,
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
    /// How it climbs between its floors.
    pub(crate) climb: Climb,
    /// How wide it is.
    pub(crate) span: Span,
}

/// How a passage climbs from one floor to another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Climb {
    /// By stairs.
    Stairs,
    /// By a lift (`func_plat`), for a climb of a storey or more.
    Lift,
}

/// A level as planned.
#[derive(Clone, Debug)]
pub(crate) struct Plan {
    /// The rooms: the path from the start to the exit, then the rooms off it.
    pub(crate) rooms: Vec<RoomPlan>,
    /// Each room's column on the grid, in the rooms' order.
    pub(crate) cells: Vec<Cell>,
    /// The passages: the path's first, in its order, then the others.
    pub(crate) links: Vec<Link>,
    /// How many rooms the path has: the last of them holds the exit.
    pub(crate) path: usize,
    /// Each hook: the atrium, and the room of the path whose passage reaches its balcony.
    pub(crate) hooks: Vec<(RoomId, RoomId)>,
    /// The level's form.
    pub(crate) form: LevelForm,
    /// The hub, in a level of that form.
    pub(crate) hub: Option<RoomId>,
    /// The canyon's courtyards, in a level of that form.
    pub(crate) canyon: Vec<RoomId>,
}

/// What the plan can paint: a pit needs a liquid and a courtyard a sky.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Has {
    /// Whether the game has a liquid.
    pub(crate) liquid: Liquid,
    /// Whether the game has a sky.
    pub(crate) sky: Sky,
}

/// Whether the game has a liquid texture, and whether a player who falls in it lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Liquid {
    /// Water, which a lake may hold.
    Water,
    /// Slime or lava, which a pit may hold, railed when it kills.
    Harmful,
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

impl Has {
    /// What `paints` can paint.
    pub(crate) fn of(paints: &Paints) -> Self {
        Self {
            liquid: match paints.liquid.as_ref().map(Hazard::of) {
                Some(Hazard::Harmless) => Liquid::Water,
                Some(Hazard::Survivable | Hazard::Deadly) => Liquid::Harmful,
                None => Liquid::Missing,
            },
            sky: if paints.sky.is_some() {
                Sky::There
            } else {
                Sky::Missing
            },
        }
    }
}

/// The form a level takes besides its hook, as Unexplored's cycle types give its dungeons
/// theirs (Boris the Brave, "Dungeon Generation in Unexplored", 2021).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LevelForm {
    /// The hook alone.
    #[default]
    Hook,
    /// A great room early on the path, the rooms that hold its keys and buttons off it.
    Hub,
    /// A great courtyard on the path open to the sky, a lake of water in its middle and a walk
    /// around it.
    Lake,
    /// A run of great courtyards on one floor, joined by openings as wide as they are and open
    /// to the sky: an outdoor canyon across several cells.
    Canyon,
}

impl LevelForm {
    /// A form drawn by `rng` from those `has` allows: a lake needs water and a sky, a canyon a
    /// sky.
    fn draw(has: Has, rng: &mut Rng) -> Self {
        let mut forms = vec![Self::Hook, Self::Hub];
        if has.sky == Sky::There {
            forms.push(Self::Canyon);
            if has.liquid == Liquid::Water {
                forms.push(Self::Lake);
            }
        }
        rng.pick(&forms).copied().unwrap_or(Self::Hook)
    }
}

/// How wide a passage is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Span {
    /// A doorway's width and height: a corridor.
    Corridor,
    /// As wide as the narrower of its rooms and open to the sky: a canyon's.
    Open,
}

/// How large a room is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scale {
    /// As the room's draw has it: now and then great.
    Drawn,
    /// Great: as wide as its cell holds.
    Great,
}

impl Size {
    /// How many rooms a level of this size has.
    pub(crate) const fn rooms(self) -> usize {
        match self {
            Self::Small => SMALL_ROOMS,
            Self::Medium => MEDIUM_ROOMS,
            Self::Large => LARGE_ROOMS,
        }
    }
}

/// A place of the path: its column and its storey.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    cell: Cell,
    storey: i32,
}

/// A path as walked: its places, and its hooks as places in it.
#[derive(Clone, Debug, Default)]
struct Walk {
    steps: Vec<Step>,
    /// Each hook's atrium and the place whose passage reaches the balcony.
    hooks: Vec<(usize, usize)>,
}

impl Walk {
    /// Whether `cell` is free: no place of the path is in its column.
    fn free(&self, cell: Cell) -> bool {
        cell.in_reach() && self.steps.iter().all(|step| step.cell != cell)
    }

    /// A storey a step from `storey`: mostly the same, now and then one up or down.
    fn next_storey(storey: i32, rng: &mut Rng) -> i32 {
        if !rng.chance(STOREY_CHANCE, CHANCES) {
            return storey;
        }
        let next = if rng.chance(1, 2) {
            storey.saturating_add(1)
        } else {
            storey.saturating_sub(1)
        };
        next.clamp(-STOREY_REACH, STOREY_REACH)
    }

    /// Hooks the path at its last place, the atrium, when the three columns the hook takes
    /// are free and the storey above is there: out by `out`, a quarter turn along `along`
    /// and back beside the atrium, the last two a storey up. `false` when it cannot.
    fn hook(&mut self, rng: &mut Rng) -> bool {
        let Some(&Step { cell, storey }) = self.steps.last() else {
            return false;
        };
        let up = storey.saturating_add(1);
        if up > STOREY_REACH {
            return false;
        }
        let mut sides = Side::ALL;
        rng.shuffle(&mut sides);
        for out in sides {
            let mut alongs = out.across();
            rng.shuffle(&mut alongs);
            for along in alongs {
                let first = cell.beside(out);
                let second = first.beside(along);
                let third = cell.beside(along);
                if !(self.free(first) && self.free(second) && self.free(third)) {
                    continue;
                }
                let atrium = self.steps.len().saturating_sub(1);
                self.steps.push(Step {
                    cell: first,
                    storey,
                });
                self.steps.push(Step {
                    cell: second,
                    storey: up,
                });
                self.steps.push(Step {
                    cell: third,
                    storey: up,
                });
                self.hooks
                    .push((atrium, self.steps.len().saturating_sub(1)));
                return true;
            }
        }
        false
    }

    /// A path of `length` places from the grid's middle that never crosses itself, hooked
    /// where it can be; `None` when it walks into a corner.
    fn of(length: usize, rng: &mut Rng) -> Option<Self> {
        let mut walk = Self {
            steps: vec![Step {
                cell: Cell::ORIGIN,
                storey: 0,
            }],
            hooks: Vec::new(),
        };
        // Where along the path the hook is: anywhere it fits, the first place it can be from
        // there on; the start is no atrium, and the exit comes after the hook.
        let last_hook = length.saturating_sub(HOOK_PLACES);
        let hook_at = rng.below(last_hook.max(1)).max(1);
        while walk.steps.len() < length {
            let place = walk.steps.len().saturating_sub(1);
            let room_for_hook = place >= hook_at && place.saturating_add(HOOK_PLACES) < length;
            if room_for_hook && walk.hooks.is_empty() && walk.hook(rng) {
                continue;
            }
            let last = *walk.steps.last()?;
            let free: Vec<Cell> = Side::ALL
                .into_iter()
                .map(|side| last.cell.beside(side))
                .filter(|&cell| walk.free(cell))
                .collect();
            let cell = *rng.pick(&free)?;
            let storey = Self::next_storey(last.storey, rng);
            walk.steps.push(Step { cell, storey });
        }
        Some(walk)
    }
}

/// Where a level's form is on its path, by place.
struct Placed {
    /// The form it came to: a canyon with no run of places for it is a hook alone.
    form: LevelForm,
    lake: Option<usize>,
    hub: Option<usize>,
    canyon: Vec<usize>,
}

impl Placed {
    /// Where `form` goes on `walk`, a path of `path` places: never at the start, the exit or a
    /// hook's places.
    fn of(form: LevelForm, walk: &Walk, path: usize, rng: &mut Rng) -> Self {
        let hooked: Vec<usize> = walk
            .hooks
            .iter()
            .flat_map(|&(atrium, upper)| atrium..=upper)
            .collect();
        let free =
            |place: usize| place > 0 && place.saturating_add(1) < path && !hooked.contains(&place);
        let mut placed = Self {
            form,
            lake: None,
            hub: None,
            canyon: Vec::new(),
        };
        match form {
            LevelForm::Hook => {}
            LevelForm::Hub => {
                let early: Vec<usize> = (1..=HUB_LATEST).filter(|&place| free(place)).collect();
                placed.hub = rng.pick(&early).copied();
            }
            LevelForm::Lake => {
                let places: Vec<usize> = (0..path).filter(|&place| free(place)).collect();
                placed.lake = rng.pick(&places).copied();
            }
            LevelForm::Canyon => {
                let runs: Vec<usize> = (0..path)
                    .filter(|&first| {
                        let mut run = first..first.saturating_add(CANYON_RUN);
                        let storey = walk.steps.get(first).map(|step| step.storey);
                        run.all(|place| {
                            free(place) && walk.steps.get(place).map(|step| step.storey) == storey
                        })
                    })
                    .collect();
                if let Some(&first) = rng.pick(&runs) {
                    placed.canyon = (first..first.saturating_add(CANYON_RUN)).collect();
                }
            }
        }
        let missing = match form {
            LevelForm::Hook => false,
            LevelForm::Hub => placed.hub.is_none(),
            LevelForm::Lake => placed.lake.is_none(),
            LevelForm::Canyon => placed.canyon.is_empty(),
        };
        if missing {
            placed.form = LevelForm::Hook;
        }
        placed
    }
}

/// The floor of `storey`.
fn floor_of(storey: i32) -> f64 {
    f64::from(storey) * STOREY
}

impl Plan {
    /// A plan of `size`, drawn by `rng`: the path, its hooks, and its rooms. What locks it is
    /// the mission's (`mission::Mission::arrange`).
    ///
    /// # Errors
    ///
    /// [`GenError::Layout`] when no path could be drawn, which the tries make all but
    /// impossible.
    pub(crate) fn draw(size: Size, has: Has, rng: &mut Rng) -> Result<Self, GenError> {
        let path = size.rooms();
        let wants_hook = path > SMALL_ROOMS;
        let walk = (0..PATH_TRIES)
            .filter_map(|_| Walk::of(path, rng))
            .find(|walk| !wants_hook || !walk.hooks.is_empty())
            .ok_or(GenError::Layout)?;
        let form = LevelForm::draw(has, rng);
        let placed = Placed::of(form, &walk, path, rng);
        let mut plan = Self {
            rooms: Vec::new(),
            cells: walk.steps.iter().map(|step| step.cell).collect(),
            links: Vec::new(),
            path,
            hooks: Vec::new(),
            form: placed.form,
            hub: placed.hub.map(RoomId),
            canyon: placed.canyon.iter().copied().map(RoomId).collect(),
        };
        for (place, step) in walk.steps.iter().enumerate() {
            let atrium = walk.hooks.iter().find(|(at, _)| *at == place);
            let (shape, turned, scale) = if place.saturating_add(1) == path {
                (
                    rng.pick(EXIT_SHAPES).copied().unwrap_or(RoomShape::Chamber),
                    None,
                    Scale::Drawn,
                )
            } else if let Some(&(_, upper)) = atrium {
                let balcony = walk.steps.get(upper).and_then(|up| {
                    Side::ALL
                        .into_iter()
                        .find(|&s| step.cell.beside(s) == up.cell)
                });
                (RoomShape::Atrium, balcony, Scale::Drawn)
            } else if placed.lake == Some(place) {
                (RoomShape::Lake, None, Scale::Great)
            } else if placed.canyon.contains(&place) {
                (RoomShape::Court, None, Scale::Great)
            } else if placed.hub == Some(place) {
                let shape = if has.sky == Sky::There && rng.chance(1, 2) {
                    RoomShape::Court
                } else {
                    RoomShape::Hall
                };
                (shape, None, Scale::Great)
            } else if place > 0 && rng.chance(JUNCTION_CHANCE, CHANCES) {
                (RoomShape::Junction, None, Scale::Drawn)
            } else {
                (Self::shape(has, rng), None, Scale::Drawn)
            };
            let mut room = Self::room(step.cell, shape, floor_of(step.storey), place, scale, rng);
            if let Some(side) = turned {
                room.turned = side;
            }
            plan.rooms.push(room);
        }
        for (place, pair) in walk.steps.windows(2).enumerate() {
            let [from, to] = pair else { continue };
            let Some(side) = Side::ALL
                .into_iter()
                .find(|&s| from.cell.beside(s) == to.cell)
            else {
                continue;
            };
            plan.links.push(Link {
                from: RoomId(place),
                to: RoomId(place.saturating_add(1)),
                side,
                doorway: Self::doorway(rng),
                climb: Climb::Stairs,
                span: Span::Corridor,
            });
        }
        for &(atrium, upper) in &walk.hooks {
            let Some(room) = plan.rooms.get(atrium) else {
                continue;
            };
            plan.links.push(Link {
                from: RoomId(upper),
                to: RoomId(atrium),
                side: room.turned.opposite(),
                doorway: Doorway::Arch,
                climb: Climb::Stairs,
                span: Span::Corridor,
            });
            plan.hooks.push((RoomId(atrium), RoomId(upper)));
        }
        Ok(plan)
    }

    /// A shape for a room that is not the exit's nor an atrium.
    fn shape(has: Has, rng: &mut Rng) -> RoomShape {
        let mut shapes = vec![
            RoomShape::Hall,
            RoomShape::Chamber,
            RoomShape::Split,
            RoomShape::Cross,
            RoomShape::Vault,
        ];
        match has.liquid {
            Liquid::Water | Liquid::Harmful => shapes.push(RoomShape::Pit),
            Liquid::Missing => {}
        }
        match has.sky {
            Sky::There => shapes.push(RoomShape::Court),
            Sky::Missing => {}
        }
        rng.pick(&shapes).copied().unwrap_or(RoomShape::Hall)
    }

    /// A room of `shape` at `cell`, its size drawn by `rng`: now and then a great room, as
    /// wide as its cell holds, more often for a courtyard, which is then an open place, so
    /// that great rooms stand beside ordinary ones, open beside cramped (Romero's contrast);
    /// a junction is small.
    fn room(
        cell: Cell,
        shape: RoomShape,
        floor: f64,
        depth: usize,
        scale: Scale,
        rng: &mut Rng,
    ) -> RoomPlan {
        let great_chance = match shape {
            RoomShape::Court => GREAT_COURT_CHANCE,
            RoomShape::Hall
            | RoomShape::Chamber
            | RoomShape::Pit
            | RoomShape::Split
            | RoomShape::Cross
            | RoomShape::Vault
            | RoomShape::Atrium => GREAT_CHANCE,
            RoomShape::Junction => 0,
            RoomShape::Lake => CHANCES,
        };
        let great = scale == Scale::Great || rng.chance(great_chance, CHANCES);
        let halves = match shape {
            RoomShape::Junction => JUNCTION_HALVES,
            _ if great => GREAT_HALVES,
            RoomShape::Hall | RoomShape::Court | RoomShape::Vault => HALVES,
            RoomShape::Chamber
            | RoomShape::Pit
            | RoomShape::Split
            | RoomShape::Cross
            | RoomShape::Atrium
            | RoomShape::Lake => WIDE_HALVES,
        };
        let mut half = || rng.pick(halves).copied().unwrap_or(SPARE_HALF);
        let half = match shape {
            // A chamber is as wide as it is long, so its eight sides are even.
            RoomShape::Chamber | RoomShape::Junction => {
                let both = half();
                [both, both]
            }
            RoomShape::Hall
            | RoomShape::Pit
            | RoomShape::Split
            | RoomShape::Court
            | RoomShape::Cross
            | RoomShape::Vault
            | RoomShape::Atrium
            | RoomShape::Lake => [half(), half()],
        };
        let raised = if great { GREAT_RISE } else { 0.0 };
        let height = raised
            + match shape {
                RoomShape::Split => SPLIT_HEIGHT,
                RoomShape::Court | RoomShape::Lake => COURT_HEIGHT,
                RoomShape::Vault => VAULT_HEIGHT,
                RoomShape::Atrium => STOREY + ATRIUM_UPPER,
                RoomShape::Junction => JUNCTION_HEIGHT,
                RoomShape::Hall | RoomShape::Chamber | RoomShape::Pit | RoomShape::Cross => {
                    rng.pick(HEIGHTS).copied().unwrap_or(SPLIT_HEIGHT)
                }
            };
        let turned = rng.pick(&Side::ALL).copied().unwrap_or(Side::East);
        RoomPlan {
            middle: cell.middle(),
            shape,
            floor,
            half,
            height,
            turned,
            depth,
            role: Role::Way,
        }
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

    /// Whether a room is in `cell`'s column.
    fn taken(&self, cell: Cell) -> bool {
        self.cells.contains(&cell)
    }

    /// Adds a room of `role` off one of `parents`, on its storey, in a free column beside
    /// it, joined by a passage with `doorway`; `None` when none of them has a free column
    /// beside it. The shape is one of `shapes`.
    pub(crate) fn sprout(
        &mut self,
        parents: &[RoomId],
        shapes: &[RoomShape],
        role: Role,
        doorway: Doorway,
        rng: &mut Rng,
    ) -> Option<RoomId> {
        let mut order = parents.to_vec();
        rng.shuffle(&mut order);
        let (parent, side, cell) = order.into_iter().find_map(|parent| {
            let at = *self.cells.get(parent.0)?;
            let mut sides = Side::ALL;
            rng.shuffle(&mut sides);
            let side = sides.into_iter().find(|&side| {
                let beside = at.beside(side);
                beside.in_reach() && !self.taken(beside) && self.side_free(parent, side)
            })?;
            Some((parent, side, at.beside(side)))
        })?;
        let from = self.rooms.get(parent.0)?;
        let floor = from.door_floor(side);
        let depth = from.depth.saturating_add(1);
        let shape = rng.pick(shapes).copied().unwrap_or(RoomShape::Hall);
        let id = RoomId(self.rooms.len());
        let mut room = Self::room(cell, shape, floor, depth, Scale::Drawn, rng);
        room.role = role;
        self.cells.push(cell);
        self.rooms.push(room);
        self.links.push(Link {
            from: parent,
            to: id,
            side,
            doorway,
            climb: Climb::Stairs,
            span: Span::Corridor,
        });
        Some(id)
    }

    /// Whether no passage leaves `room` by `side`, and it is no atrium's balcony, whose
    /// passage is the hook's alone.
    fn side_free(&self, room: RoomId, side: Side) -> bool {
        let balcony = self
            .rooms
            .get(room.0)
            .is_some_and(|plan| plan.shape == RoomShape::Atrium && plan.turned == side);
        !balcony && !self.doors_of(room.0).contains(&side)
    }

    /// Joins up to [`MOST_LOOPS`] pairs of rooms in columns side by side that no passage
    /// joins yet, a storey apart at most, for which `allowed` says yes: each passage closes a
    /// loop. The exit's room is left as it is: the level ends there.
    pub(crate) fn close_loops(&mut self, allowed: impl Fn(RoomId, RoomId) -> bool, rng: &mut Rng) {
        let exit = RoomId(self.path.saturating_sub(1));
        let mut pairs = Vec::new();
        for (a, &cell) in self.cells.iter().enumerate() {
            for side in [Side::East, Side::North] {
                let Some(b) = self
                    .cells
                    .iter()
                    .position(|&other| other == cell.beside(side))
                else {
                    continue;
                };
                pairs.push((RoomId(a), RoomId(b), side));
            }
        }
        rng.shuffle(&mut pairs);
        let mut added = 0;
        for (a, b, side) in pairs {
            if added >= MOST_LOOPS {
                break;
            }
            let joined = self
                .links
                .iter()
                .any(|link| (link.from, link.to) == (a, b) || (link.from, link.to) == (b, a));
            let (Some(ra), Some(rb)) = (self.rooms.get(a.0), self.rooms.get(b.0)) else {
                continue;
            };
            let climb = (ra.door_floor(side) - rb.door_floor(side.opposite())).abs();
            let usable = !joined
                && a != exit
                && b != exit
                && climb <= STOREY
                && self.side_free(a, side)
                && self.side_free(b, side.opposite())
                && allowed(a, b);
            if usable {
                self.links.push(Link {
                    from: a,
                    to: b,
                    side,
                    doorway: Doorway::Arch,
                    climb: Climb::Stairs,
                    span: Span::Corridor,
                });
                added = added.saturating_add(1);
            }
        }
    }

    /// Gives a lift, with the chance [`LIFT_CHANCE`] in [`CHANCES`], to each passage that
    /// climbs a storey or more between its rooms' doorways.
    pub(crate) fn lifts(&mut self, rng: &mut Rng) {
        let rooms = &self.rooms;
        for link in &mut self.links {
            let (Some(from), Some(to)) = (rooms.get(link.from.0), rooms.get(link.to.0)) else {
                continue;
            };
            let climb = (from.door_floor(link.side) - to.door_floor(link.side.opposite())).abs();
            if climb >= STOREY && rng.chance(LIFT_CHANCE, CHANCES) {
                link.climb = Climb::Lift;
            }
        }
    }

    /// Shrinks rooms until every passage holds what it climbs, stairs and their landings
    /// (`Passage::least_length`): two great rooms side by side are close, which a passage on
    /// one floor takes as an arch in a thick wall and one between floors cannot. The larger of
    /// the two rooms gives way, a step of [`FIT_STEP`] at a time, down to its shape's least,
    /// and then the other.
    pub(crate) fn fit_passages(&mut self) {
        for index in 0..self.links.len() {
            for _ in 0..FIT_TRIES {
                let Some(&link) = self.links.get(index) else {
                    break;
                };
                if self.shortfall(&link).is_none_or(|short| short <= 0.0) {
                    break;
                }
                let (Some(from), Some(to)) =
                    (self.rooms.get(link.from.0), self.rooms.get(link.to.0))
                else {
                    break;
                };
                let (larger, smaller) = if from.reach(link.side) >= to.reach(link.side.opposite()) {
                    (link.from, link.to)
                } else {
                    (link.to, link.from)
                };
                if !self.shrink(larger, link.side) && !self.shrink(smaller, link.side) {
                    break;
                }
            }
        }
    }

    /// How much shorter `link`'s passage is than the stairs and landings of its climb need
    /// (`Passage::least_length`), 0 or less when it holds them; `None` for a link to a room
    /// the plan does not have.
    pub(crate) fn shortfall(&self, link: &Link) -> Option<f64> {
        let (from, to) = (self.rooms.get(link.from.0)?, self.rooms.get(link.to.0)?);
        let [fx, fy] = link.side.facing();
        let [ax, ay] = from.middle;
        let [bx, by] = to.middle;
        let between = (bx - ax) * fx + (by - ay) * fy;
        let length = between - from.reach(link.side) - to.reach(link.side.opposite());
        let climb = (from.door_floor(link.side) - to.door_floor(link.side.opposite())).abs();
        Some(Passage::least_length(climb) - length)
    }

    /// Shrinks the room `id` by [`FIT_STEP`] along `side`'s axis, and across it too for a
    /// shape that is square; whether it was larger than its shape's least.
    fn shrink(&mut self, id: RoomId, side: Side) -> bool {
        let Some(room) = self.rooms.get_mut(id.0) else {
            return false;
        };
        let least = room.shape.least_half();
        let axis = usize::from(!side.along_x());
        let Some(&half) = room.half.get(axis) else {
            return false;
        };
        if half - FIT_STEP < least {
            return false;
        }
        match room.shape {
            RoomShape::Chamber | RoomShape::Junction => {
                room.half = room.half.map(|h| (h - FIT_STEP).max(least));
            }
            RoomShape::Hall
            | RoomShape::Pit
            | RoomShape::Split
            | RoomShape::Court
            | RoomShape::Cross
            | RoomShape::Vault
            | RoomShape::Atrium
            | RoomShape::Lake => {
                if let Some(h) = room.half.get_mut(axis) {
                    *h -= FIT_STEP;
                }
            }
        }
        true
    }

    /// Opens the passages between the canyon's courtyards: as wide as the narrower and up to
    /// its sky, so the run reads as one place outdoors. A passage that holds a door or a lock
    /// is a corridor, and a canyon none of whose passages opens, all of them locked, is no
    /// canyon: the level takes the hook form, its courtyards great courtyards still.
    pub(crate) fn open_canyon(&mut self) {
        let canyon = self.canyon.clone();
        let rooms = &self.rooms;
        let mut opened = false;
        for link in &mut self.links {
            let both = canyon.contains(&link.from) && canyon.contains(&link.to);
            let plain = matches!(link.doorway, Doorway::Open | Doorway::Arch);
            let level = match (rooms.get(link.from.0), rooms.get(link.to.0)) {
                (Some(a), Some(b)) => (a.floor - b.floor).abs() < f64::EPSILON,
                _ => false,
            };
            if both && plain && level {
                link.span = Span::Open;
                link.doorway = Doorway::Open;
                opened = true;
            }
        }
        if self.form == LevelForm::Canyon && !opened {
            self.form = LevelForm::Hook;
            self.canyon.clear();
        }
    }

    /// The room `id` names.
    pub(crate) fn room_of(&self, id: RoomId) -> Option<&RoomPlan> {
        self.rooms.get(id.0)
    }

    /// The middle of the doorway a player comes into the room at `place` by: the passage that
    /// reaches it, or for the start the first it leaves by.
    pub(crate) fn entry(&self, place: usize) -> [f64; 2] {
        let side = self
            .links
            .iter()
            .find(|link| link.to.place() == place)
            .map(|link| link.side.opposite())
            .or_else(|| self.doors_of(place).first().copied())
            .unwrap_or(Side::East);
        let Some(room) = self.rooms.get(place) else {
            return [0.0, 0.0];
        };
        let [fx, fy] = side.facing();
        let [mx, my] = room.middle;
        let reach = room.reach(side);
        [mx + fx * reach, my + fy * reach]
    }

    /// The sides of the room at `place` whose doorways take no frame: a secret door's, which
    /// a frame would show, a lift passage's, which is open to its full height, and a canyon's
    /// opening, which is as wide as its courtyards.
    pub(crate) fn unframed(&self, place: usize) -> Vec<Side> {
        self.links
            .iter()
            .filter_map(|link| {
                let side = if link.from.place() == place {
                    link.side
                } else if link.to.place() == place {
                    link.side.opposite()
                } else {
                    return None;
                };
                let open = link.doorway == Doorway::Secret
                    || link.climb == Climb::Lift
                    || link.span == Span::Open;
                open.then_some(side)
            })
            .collect()
    }

    /// The sides of the room at `place` open to the room beside as wide as the narrower of the
    /// two, a canyon's.
    pub(crate) fn open_sides(&self, place: usize) -> Vec<Side> {
        self.links
            .iter()
            .filter(|link| link.span == Span::Open)
            .filter_map(|link| {
                if link.from.place() == place {
                    Some(link.side)
                } else if link.to.place() == place {
                    Some(link.side.opposite())
                } else {
                    None
                }
            })
            .collect()
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

    /// How many loops the passages make: one for each passage more than a tree of the rooms
    /// has.
    pub(crate) fn loops(&self) -> usize {
        self.links
            .len()
            .saturating_add(1)
            .saturating_sub(self.rooms.len())
    }

    /// How many floors the level is on: each storey a room's floor or an atrium's balcony is
    /// on.
    pub(crate) fn floors(&self) -> usize {
        let mut heights: Vec<f64> = Vec::new();
        for room in &self.rooms {
            heights.push(room.floor);
            if room.shape == RoomShape::Atrium {
                heights.push(room.floor + STOREY);
            }
        }
        heights.sort_by(f64::total_cmp);
        heights.dedup_by(|a, b| (*a - *b).abs() < STOREY - RAISE / 2.0);
        heights.len()
    }
}
