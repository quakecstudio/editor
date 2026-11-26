// SPDX-License-Identifier: GPL-3.0-or-later

//! Whether a compiled map can be finished (L-77, MAP-36): from the player start, the exit is
//! reached, and from every place and state a player reaches the exit can still be reached, the
//! property AG(EF(exit)) of Mawhorter and Smith's softlock detection (FDG 2021); and which of its
//! keys, weapons, items and secrets are never reached. It reads what the game reads: the BSP's
//! hulls, its entity lump and its brush models' bounds, which `setmodel` gives an entity.
//!
//! A player's moves are a choice ([`Moves`]). Both walk, up a step of at most `STEPSIZE` (18,
//! PHYS-8.3), and fall, which is one way; ride a lift (`func_plat`) up from its bottom, which
//! carries the player up at once, and walk off its edge at its top, where it stays up under the
//! player; are sent by a teleporter to its destination (`teleport_touch`), one way; swim in any
//! direction in a liquid, and climb out over a rim at most [`crate::CLIMB_OUT`] above its
//! surface (`CheckWaterJump`). The generator's moves stop there, so a level made to pass them
//! holds no trap a jump would hide; a player's also jump onto a ledge up to [`JUMP`] higher,
//! measured from id's `PlayerJump` in `qs-sim`. Lava kills before a player climbs out
//! (`client.qc`, `WaterMove`): a player who reaches it is dead, not trapped.
//!
//! What opens what is id's QuakeC (`doors.qc`, `buttons.qc`, `triggers.qc`, `items.qc`, at
//! id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73, `qcc/v101qc/`):
//!
//! - a door opens to a player with its key (`DOOR_SILVER_KEY` 16, `DOOR_GOLD_KEY` 8); one with
//!   a `targetname` once something fires it; one with `health` once a player sees it, to shoot
//!   it; any other when touched. Doors that touch are linked and open together (`LinkDoors`),
//!   unless `DOOR_DONT_LINK` (4). An open door is solid where it opened to, `lip` short of its
//!   whole size along its `angle` (`pos2`); one that stays open (`wait -1`, or a key door) stays
//!   there, and any other comes back, after `wait` or, for `DOOR_TOGGLE`, at its next firing, so
//!   a player passes where either place leaves room. One that starts open (`DOOR_START_OPEN`, 1) is drawn shut and spawns open, and
//!   firing it shuts it, as a bridge that rises.
//!   A key is taken to open every door of its kind: the check does not count keys.
//! - a secret door (`func_door_secret`) with a `targetname` opens once it is fired; one with
//!   none, or with `SECRET_YES_SHOOT` (16), once a player sees it.
//! - a button fires its targets when a player comes beside it, or sees it when it has `health`;
//!   a `trigger_once`, `trigger_multiple` or `trigger_secret` when a player walks into it,
//!   unless it is `notouch` (1), or sees it when it has `health`; an item or a weapon when it is
//!   picked up; a monster when it dies, which it is taken to do once a player sees it, or, for
//!   one with a `targetname`, which waits in a closet until fired, once it is fired.
//! - a relay, a counter, a trigger, a button or a door that is fired fires its own targets (a
//!   counter is taken to fire at its first firing: the check does not count).
//! - a `func_wall` and a `func_bossgate` are solid; a `func_episodegate` is not in a new game
//!   (`misc.qc`: `if (!(serverflags & self.spawnflags)) return`).
//!
//! - a train (`func_train`) spawns at its first corner (`func_train_find`) and waits there
//!   when it has a `targetname` until fired; a player on it, or a step below its top at a
//!   corner, rides it to each of its corners. Where it is between corners is not followed: it
//!   is taken to be out of the way once it moves.
//!
//! An entity of any other class, a mod's or id's `misc_teleporttrain` and `trigger_push`,
//! which move a player in ways the check does not follow, is listed as not judged (PRIN-8), and
//! so is a train, whose rides between corners the check does not follow.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};

use qs_bsp::Bsp;
use qs_map::{Entity, MapFile};
use qs_sim::collision::{CONTENTS_SOLID, Hull, HullSet, Trace, recursive_hull_check};
use qs_sim::spawn::{Deathmatch, inhibited};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The skill a map is played at, the `skill` setting (DIFF-3.2), which decides the entities the
/// game keeps (EDICT-8.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Skill {
    /// 0.
    Easy,
    /// 1, the game's own default.
    #[default]
    Normal,
    /// 2.
    Hard,
    /// 3.
    Nightmare,
}

/// The `skill` setting of nightmare, which the inhibition test reads as hard (EDICT-8.4).
const NIGHTMARE_LEVEL: i32 = 3;

impl Skill {
    /// The skill as the engine's inhibition test reads it, 0 to 3 (EDICT-8.4).
    #[must_use]
    pub const fn level(self) -> i32 {
        match self {
            Self::Easy => 0,
            Self::Normal => 1,
            Self::Hard => 2,
            Self::Nightmare => NIGHTMARE_LEVEL,
        }
    }

    /// The value of the `skill` setting.
    #[must_use]
    pub const fn setting(self) -> &'static str {
        match self {
            Self::Easy => "0",
            Self::Normal => "1",
            Self::Hard => "2",
            Self::Nightmare => "3",
        }
    }
}

/// How high a ledge a player gets onto with a jump, in map units: id's `PlayerJump` run in
/// `qs-sim` against ledges of each height (the jump test of `qs-tests`).
pub const JUMP: f32 = 43.0;

/// How wide a gap a running jump clears, in map units, edge to edge: id's `PlayerJump` run in
/// `qs-sim` by a player running at `sv_maxspeed` (the jump test of `qs-tests`).
pub const LEAP: f32 = 240.0;

/// The most places and states a check visits before it stops, which a map larger than any
/// id or LibreQuake map would pass.
pub const MOST_PLACES: usize = 4_000_000;

/// `CONTENTS_WATER` and `CONTENTS_LAVA` (BSP-10.2): the liquids are water, slime and lava, -3
/// to -5.
const WATER: i32 = -3;
const LAVA: i32 = -5;
const LIQUIDS: std::ops::RangeInclusive<i32> = LAVA..=WATER;
/// How far apart a walk's paces are, the height a pace climbs (`STEPSIZE`, PHYS-8.3), and how
/// finely heights are tried.
const PACE: f32 = 16.0;
const CLIMB: f32 = 18.0;
const FINE: f32 = 2.0;
/// The player's box below and above its origin (`VEC_HULL_MIN`, `VEC_HULL_MAX`), half of it
/// across, and its middle above the origin, where `SV_CheckWater` finds it waist deep
/// (PHYS-8.4).
const FEET: f32 = 24.0;
const HALF: f32 = 16.0;
const HEAD: f32 = 32.0;
const WAIST: f32 = 4.0;
/// How high the player's eye is above its origin (`view_ofs`, `PutClientInServer`).
const EYE: f32 = 22.0;
/// How far `PlaceItem` raises an item before dropping it, and how far `droptofloor` drops it
/// (`items.qc`, `self.origin_z = self.origin_z + 6`; `PF_droptofloor`, 256).
const ITEM_LIFT: f32 = 6.0;
const ITEM_DROP: f32 = 256.0;
/// How far `SV_LinkEdict` grows an item's box across, "to make items easier to pick up", and
/// how high an item's box is (`items.qc`, `setsize`).
const ITEM_REACH: f32 = 15.0;
const ITEM_HEIGHT: f32 = 56.0;
/// How far above a door's top an item may be and still stand on it.
const ITEM_REST: f32 = 32.0;
/// How far a player may be from a point and have reached it.
const NEAR: f32 = 48.0;
/// How much the engine spreads a brush model's bounds on every side when it loads them
/// (BSP-15.4).
const SPREAD: f32 = 1.0;
/// How much lower a `func_plat` with no `height` goes than it is tall (`plats.qc`:
/// `self.pos2_z = self.origin_z - self.size_z + 8`).
const PLAT_LIP: f32 = 8.0;
/// How far above its origin an `info_teleport_destination` puts a player (`triggers.qc`:
/// `self.origin = self.origin + '0 0 27'`).
const DESTINATION_LIFT: f32 = 27.0;
/// How many times a fall's last fine step is halved to find the floor: to a 64th of a unit.
const REST_HALVINGS: u32 = 7;
/// The height of a cell of the search, in units.
const CELL_HEIGHT: f32 = 8.0;
/// The world hull a player's box collides with, and the one drawn (BSP-15).
const PLAYER_HULL: usize = 1;
const DRAWN_HULL: usize = 0;

/// `spawnflags` of a door (`doors.qc`).
const DOOR_START_OPEN: u32 = 1;
const DOOR_DONT_LINK: u32 = 4;
const DOOR_GOLD_KEY: u32 = 8;
const DOOR_SILVER_KEY: u32 = 16;
const DOOR_TOGGLE: u32 = 32;
/// `spawnflags` of a secret door that is shot open even when it is fired (`doors.qc`).
const SECRET_YES_SHOOT: u32 = 16;
/// `spawnflags` of a trigger only something else fires (`triggers.qc`, `SPAWNFLAG_NOTOUCH`).
const NOTOUCH: u32 = 1;
/// The `angle` of an entity that moves up, and of one that moves down (`SetMovedir`).
const UP_ANGLE: f32 = -1.0;
const DOWN_ANGLE: f32 = -2.0;
/// How much of a door's size it keeps in its frame when open, when it has no `lip` (`doors.qc`).
const DOOR_LIP: f32 = 8.0;
/// Where a door is from where it is drawn before it opens: there.
const NOT_MOVED: [f32; 3] = [0.0, 0.0, 0.0];
/// The most corners a train's path is followed round.
const MOST_STOPS: usize = 64;
/// The most doors a check follows; one past them is taken as open and listed as not judged.
const MOST_DOORS: usize = 128;

/// The moves a player is taken to have.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Moves {
    /// A player's: walking, swimming, lifts, teleporters and a jump of [`JUMP`].
    #[default]
    Player,
    /// The level generator's: a player's with no jump, so that a ledge or a rail a player
    /// could jump over counts as a wall.
    Generator,
}

impl Moves {
    /// How high a jump climbs, if the moves have one.
    const fn jump(self) -> Option<f32> {
        match self {
            Self::Player => Some(JUMP),
            Self::Generator => None,
        }
    }
}

/// Why a map could not be checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayError {
    /// The BSP's entity lump does not parse.
    Lump(String),
    /// The BSP has no world model.
    NoWorld,
    /// The map has no `info_player_start`.
    NoStart,
    /// The search passed [`MOST_PLACES`].
    TooLarge,
}

impl core::fmt::Display for PlayError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Lump(error) => write!(f, "the entity lump does not read: {error}"),
            Self::NoWorld => f.write_str("the BSP has no world model"),
            Self::NoStart => f.write_str("the map has no info_player_start to start from"),
            Self::TooLarge => write!(
                f,
                "the map has more than {MOST_PLACES} places and states to search"
            ),
        }
    }
}

impl core::error::Error for PlayError {}

/// What the generator's check of a level found wrong.
#[derive(Clone, Debug, PartialEq)]
pub enum Trap {
    /// The exit is reached from nowhere the start reaches.
    NoExit,
    /// A place the start reaches from which the exit is not.
    Stuck([f32; 3]),
    /// Walking reaches lava.
    Lava([f32; 3]),
    /// The search passed [`MOST_PLACES`].
    TooLarge,
}

impl core::fmt::Display for Trap {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoExit => f.write_str("the exit is reached from nowhere the start reaches"),
            Self::Stuck([x, y, z]) => {
                write!(f, "a player who comes to {x} {y} {z} cannot reach the exit")
            }
            Self::Lava([x, y, z]) => write!(f, "walking reaches lava at {x} {y} {z}"),
            Self::TooLarge => write!(
                f,
                "the level has more than {MOST_PLACES} places and states to search"
            ),
        }
    }
}

/// A box, by its least and greatest corners.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Volume {
    /// The least corner.
    pub min: [f32; 3],
    /// The greatest corner.
    pub max: [f32; 3],
}

impl Volume {
    /// The bounds of brush model `model` of `bsp`, as `setmodel` gives an entity.
    fn of_model(bsp: &Bsp<'_>, model: usize) -> Option<Self> {
        bsp.models.get(model).map(|m| Self {
            min: m.mins,
            max: m.maxs,
        })
    }

    /// The point in the middle.
    fn middle(&self) -> [f32; 3] {
        let [x0, y0, z0] = self.min;
        let [x1, y1, z1] = self.max;
        [(x0 + x1) / 2.0, (y0 + y1) / 2.0, (z0 + z1) / 2.0]
    }

    /// The box around this one and `other`.
    fn around(self, other: Self) -> Self {
        let mut around = self;
        for (low, other) in around.min.iter_mut().zip(other.min) {
            *low = low.min(other);
        }
        for (high, other) in around.max.iter_mut().zip(other.max) {
            *high = high.max(other);
        }
        around
    }

    /// Whether the box overlaps `other`.
    fn overlaps(&self, other: &Self) -> bool {
        self.min
            .iter()
            .zip(&self.max)
            .zip(other.min.iter().zip(&other.max))
            .all(|((min, max), (other_min, other_max))| min <= other_max && max >= other_min)
    }

    /// Whether a player's box with its origin at `at`, grown by `margin` on every side,
    /// overlaps the box.
    fn touched(&self, at: [f32; 3], margin: f32) -> bool {
        let low = [at[0] - HALF, at[1] - HALF, at[2] - FEET];
        let high = [at[0] + HALF, at[1] + HALF, at[2] + HEAD];
        low.iter()
            .zip(high)
            .zip(self.min.iter().zip(self.max))
            .all(|((l, h), (min, max))| l - margin <= max && h + margin >= *min)
    }
}

/// The doors a player holds open, a bit for each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
struct Held(u128);

impl Held {
    /// Door `n`'s bit.
    fn door(n: usize) -> Self {
        Self(
            u32::try_from(n)
                .ok()
                .and_then(|n| 1u128.checked_shl(n))
                .unwrap_or(0),
        )
    }

    fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    fn has(self, door: Self) -> bool {
        self.0 & door.0 != 0
    }

    fn covers(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// How much of the places and states a search visits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Search {
    /// Every state at every place, as a trap needs: a player may come to a place before
    /// opening what leads out of it.
    Whole,
    /// A state at a place only when no state already there opens all it opens, a move to it
    /// taken as one to the state that covers it: what is reached at all, since opening more
    /// never closes a way (but for a door that starts open and is shut by firing it). A trap
    /// found this way is a place no state leads out of: one that holds only for a player who
    /// comes there before opening what leads out is not found.
    Covering,
}

/// Where a player sets a firing off.
#[derive(Clone, Copy, Debug)]
enum Where {
    /// Touching an item's box at either place, as an item is picked up: where it lies, and
    /// where it lies once the door it stands on has moved. The box reaches `below` under the
    /// item's origin.
    Item { at: [[f32; 3]; 2], below: f32 },
    /// Inside a volume, as a trigger is touched.
    Inside(Volume),
    /// Beside a solid volume, as a button is pressed: within a pace of it.
    Beside(Volume),
    /// Where a player sees a point, to shoot what is there.
    Seen([f32; 3]),
}

/// What a player opens by reaching a place.
#[derive(Clone, Copy, Debug)]
struct Firing {
    at: Where,
    opens: Held,
}

/// A door that something opens: its brush model and its bit.
#[derive(Clone, Copy, Debug)]
struct Door {
    model: usize,
    /// How far it is from where it is drawn before it opens: a train spawns at its first
    /// corner (`func_train_find`).
    from: [f32; 3],
    /// What opens it; `None` for one taken as open from the start.
    opener: Option<Held>,
    /// Where it is when open.
    open: Open,
    /// Whether it stays open.
    stays: Stays,
    /// Where it starts.
    starts: Starts,
}

/// Where a door starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Starts {
    /// Where it is drawn.
    Shut,
    /// Where it opens to (`DOOR_START_OPEN`): firing it moves it to where it is drawn.
    Open,
}

/// Whether a door stays where it opens to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stays {
    /// It does: `wait -1`, which a key door always has.
    Open,
    /// It comes back after `wait` seconds, or at its next firing for `DOOR_TOGGLE`
    /// (`door_hit_top`), so a player passes where either place leaves room.
    Returns,
}

/// Where a door is when open.
#[derive(Clone, Copy, Debug)]
enum Open {
    /// Moved by this much from where it is drawn, and as solid there (`func_door`).
    By([f32; 3]),
    /// Out of the way (`func_door_secret`, whose two moves the check does not follow).
    Gone,
}

impl Door {
    /// Whether the door, as it is with `held`, holds the point `at` of the player's hull.
    fn holds(&self, ground: &Ground<'_>, held: Held, at: [f32; 3]) -> bool {
        // A door nothing need fire opens when touched.
        let fired = self.opener.is_none_or(|opener| held.has(opener));
        let [fx, fy, fz] = self.from;
        let at = [at[0] - fx, at[1] - fy, at[2] - fz];
        let drawn = || ground.solid_in(PLAYER_HULL, self.model, at);
        let moved = || match self.open {
            Open::By([dx, dy, dz]) => ground.solid_in(
                PLAYER_HULL,
                self.model,
                [at[0] - dx, at[1] - dy, at[2] - dz],
            ),
            Open::Gone => false,
        };
        match (self.starts, fired, self.stays) {
            (Starts::Shut, false, _) | (Starts::Open, true, Stays::Open) => drawn(),
            (Starts::Shut, true, Stays::Open) | (Starts::Open, false, _) => moved(),
            (Starts::Shut | Starts::Open, true, Stays::Returns) => drawn() && moved(),
        }
    }
}

/// `SetMovedir` (`subs.qc`): the way an entity of `angles` moves, `'0 -1 0'` up and `'0 -2 0'`
/// down, else `makevectors (self.angles)`'s `v_forward`.
#[expect(
    clippy::float_cmp_const,
    reason = "SetMovedir compares the angles exactly: `if (self.angles == '0 -1 0')`"
)]
fn movedir([pitch, yaw, _]: [f32; 3]) -> [f32; 3] {
    if pitch == 0.0 && yaw == UP_ANGLE {
        return [0.0, 0.0, 1.0];
    }
    if pitch == 0.0 && yaw == DOWN_ANGLE {
        return [0.0, 0.0, -1.0];
    }
    let (pitch, yaw) = (pitch.to_radians(), yaw.to_radians());
    [
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    ]
}

/// A lift (`func_plat`): the box over which it moves, the height of its top when up, as it is
/// drawn, and when down, its `height` lower (`pos2_z = self.origin_z - self.height`).
#[derive(Clone, Copy, Debug)]
struct Lift {
    /// What sends it down from its top for the first time, when it waits to be fired.
    opener: Option<Held>,
    min: [f32; 2],
    max: [f32; 2],
    top: f32,
    bottom: f32,
}

impl Lift {
    /// Whether a player's middle at `at` is over the lift.
    fn holds(&self, at: [f32; 3]) -> bool {
        (self.min[0]..=self.max[0]).contains(&at[0]) && (self.min[1]..=self.max[1]).contains(&at[1])
    }

    /// Whether a player at `at` stands on the lift up.
    fn at_top(&self, at: [f32; 3]) -> bool {
        self.holds(at) && (at[2] - (self.top + FEET)).abs() < FINE
    }

    /// Whether a player at `at` stands on the lift down: on it, or on the floor of its shaft
    /// at most a step below its top, from where a player steps onto it.
    fn at_bottom(&self, at: [f32; 3]) -> bool {
        let on = self.bottom + FEET;
        self.holds(at) && (on - CLIMB..=on + FINE).contains(&at[2])
    }
}

/// Where a move ends, and the doors opened by what the player set off on the way.
#[derive(Clone, Copy, Debug)]
struct Landing {
    at: [f32; 3],
    crossed: Held,
}

impl Landing {
    /// A move to `at` that sets nothing off on the way.
    fn still(at: [f32; 3]) -> Self {
        Self {
            at,
            crossed: Held::default(),
        }
    }
}

/// A train (`func_train`): its brush model's bounds as drawn, each corner it stops at as how
/// far it is then from where it is drawn, and what starts it when it waits to be fired.
#[derive(Clone, Debug)]
struct Train {
    volume: Volume,
    stops: Vec<[f32; 3]>,
    opener: Option<Held>,
}

impl Train {
    /// The stop at which a player at `at` stands on the train, or on the floor at most a step
    /// below its top there, from where a player steps onto it.
    fn boarded(&self, at: [f32; 3], held: Held) -> Option<usize> {
        let moving = self.opener.is_none_or(|opener| held.has(opener));
        let stops = if moving { self.stops.len() } else { 1 };
        self.stops.iter().take(stops).position(|&[dx, dy, dz]| {
            let on = self.volume.max[2] - SPREAD + dz + FEET;
            (self.volume.min[0] + SPREAD + dx..=self.volume.max[0] - SPREAD + dx).contains(&at[0])
                && (self.volume.min[1] + SPREAD + dy..=self.volume.max[1] - SPREAD + dy)
                    .contains(&at[1])
                && (on - CLIMB..=on + FINE).contains(&at[2])
        })
    }
}

/// A teleporter: its volume and where it sends a player.
#[derive(Clone, Copy, Debug)]
struct Teleport {
    volume: Volume,
    to: [f32; 3],
}

/// What a sought entity is.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Sought {
    /// A key (`item_key1`, `item_key2`).
    Key,
    /// A weapon (`weapon_*`).
    Weapon,
    /// Any other item (`item_*`).
    Item,
    /// A secret (`trigger_secret`).
    Secret,
}

/// An entity a player should reach.
#[derive(Clone, Debug)]
struct Wanted {
    entity: usize,
    kind: Sought,
    classname: String,
    at: Where,
    point: [f32; 3],
    /// What sets it off when something fires it, as a secret with a `targetname` is.
    fired: Option<Held>,
}

/// A place in whole units, with its height in eighths: a cell of the search.
type Cell = [u32; 3];

/// A place, and what is held there.
type Node = (Cell, Held);

/// The cell `at` is in.
fn cell(at: [f32; 3]) -> Cell {
    // Adding 0 makes a negative zero positive, so that one cell has one key.
    [at[0], at[1], at[2] / CELL_HEIGHT].map(|c| (c.floor() + 0.0).to_bits())
}

/// The places and states a search has still to move from. A covering search moves first from
/// those that have opened the most, so that a place reached with less open later is covered
/// and not searched again; a whole search goes in the order the states were found.
#[derive(Default)]
struct Queue {
    /// The states, by their places in the search, by how many doors each has opened.
    by_opened: BTreeMap<u32, VecDeque<u32>>,
}

impl Queue {
    fn push(&mut self, id: u32, node: Node, search: Search) {
        let rank = match search {
            Search::Whole => 0,
            Search::Covering => node.1.0.count_ones(),
        };
        self.by_opened.entry(rank).or_default().push_back(id);
    }

    fn pop(&mut self) -> Option<u32> {
        let mut most = self.by_opened.last_entry()?;
        let node = most.get_mut().pop_front();
        if most.get().is_empty() {
            most.remove();
        }
        node
    }
}

/// What a search from a start found.
struct Explored {
    /// Each place and state reached, in the order found, and where in it.
    nodes: Vec<(Node, [f32; 3])>,
    /// Each move made: the state it reaches and the state it is from, by their places in
    /// `nodes`.
    moves: Vec<(u32, u32)>,
    /// Whether each state is in lava, where the player died.
    dead: Vec<bool>,
}

/// An entity of the lump as the check reads it.
struct Read<'e> {
    /// Its place in the lump.
    place: usize,
    entity: &'e Entity,
    classname: String,
    model: Option<usize>,
}

impl Read<'_> {
    fn text(&self, key: &[u8]) -> Option<String> {
        self.entity
            .value(key)
            .filter(|value| !value.is_empty())
            .map(|value| String::from_utf8_lossy(value).into_owned())
    }

    /// A whole number a key holds, 0 for none or one that is not a number.
    fn number(&self, key: &[u8]) -> u32 {
        self.text(key)
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0)
    }

    /// What firing the entity fires: its `target`, and its `killtarget` removed
    /// (`SUB_UseTargets`).
    fn fired(&self) -> Vec<Event> {
        let mut fired = Vec::new();
        fired.extend(self.text(b"target").map(Event::Name));
        fired.extend(self.text(b"killtarget").map(Event::Kill));
        fired
    }

    /// The number a key holds, if it holds one.
    fn float(&self, key: &[u8]) -> Option<f32> {
        self.text(key)
            .and_then(|text| text.trim().parse::<f32>().ok())
    }

    /// The entity's `angles`, or its `angle` as the yaw, as `ED_ParseEdict` reads it.
    fn angles(&self) -> [f32; 3] {
        if let Some(text) = self.text(b"angles") {
            let mut parts = text
                .split_whitespace()
                .map(|p| p.parse::<f32>().unwrap_or(0.0));
            let mut next = || parts.next().unwrap_or(0.0);
            return [next(), next(), next()];
        }
        [0.0, self.float(b"angle").unwrap_or(0.0), 0.0]
    }

    /// Whether a key holds a number other than 0, as QuakeC's `if (self.health)` reads it.
    fn set(&self, key: &[u8]) -> bool {
        self.text(key)
            .and_then(|text| text.trim().parse::<f32>().ok())
            .is_some_and(|value| value != 0.0)
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "the game holds positions in single precision"
    )]
    fn origin(&self) -> [f32; 3] {
        self.entity.origin().map(|at| at as f32)
    }
}

/// How the check takes an entity's class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// One it follows.
    Known,
    /// One whose effect on a player it does not follow.
    Unjudged,
}

impl Class {
    /// id's classes whose effect on a player the check follows, or which have none: decoration,
    /// sound, light, monsters, paths and the like.
    const KNOWN: [&'static str; 30] = [
        "worldspawn",
        "air_bubbles",
        "event_lightning",
        "func_bossgate",
        "func_button",
        "func_door",
        "func_door_secret",
        "func_episodegate",
        "func_illusionary",
        "func_plat",
        "func_wall",
        "misc_explobox",
        "misc_explobox2",
        "misc_fireball",
        "misc_noisemaker",
        "noclass",
        "path_corner",
        "trigger_changelevel",
        "trigger_counter",
        "trigger_hurt",
        "trigger_monsterjump",
        "trigger_multiple",
        "trigger_once",
        "trigger_onlyregistered",
        "trigger_relay",
        "trigger_secret",
        "trigger_setskill",
        "trigger_teleport",
        "viewthing",
        "info_teleport_destination",
    ];
    /// Prefixes of id's classes of the same kind.
    const KNOWN_PREFIXES: [&'static str; 7] = [
        "ambient_", "info_", "item_", "light", "monster_", "trap_", "weapon_",
    ];

    fn of(classname: &str) -> Self {
        if Self::KNOWN.contains(&classname)
            || Self::KNOWN_PREFIXES
                .iter()
                .any(|prefix| classname.starts_with(prefix))
        {
            Self::Known
        } else {
            Self::Unjudged
        }
    }
}

/// What is fired: a name, the entities of a name removed (`killtarget`, `SUB_UseTargets`), or
/// a door opened.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Event {
    Name(String),
    Kill(String),
    Door(usize),
}

/// What firing each name or opening each door fires in turn.
#[derive(Default)]
struct Chains(BTreeMap<Event, BTreeSet<Event>>);

impl Chains {
    fn link(&mut self, from: Event, to: Event) {
        self.0.entry(from).or_default().insert(to);
    }

    /// The doors `events` open, through every chain.
    fn opens(&self, events: impl IntoIterator<Item = Event>) -> Held {
        let mut seen = BTreeSet::new();
        let mut queue: Vec<Event> = events.into_iter().collect();
        let mut held = Held::default();
        while let Some(event) = queue.pop() {
            if !seen.insert(event.clone()) {
                continue;
            }
            if let Event::Door(n) = event {
                held = held.with(Held::door(n));
            }
            queue.extend(self.0.get(&event).into_iter().flatten().cloned());
        }
        held
    }
}

/// Each `func_door`'s group of linked doors (`LinkDoors`): those that touch, unless one is
/// `DOOR_DONT_LINK`, are one door.
fn linked_doors(doors: &[(usize, Volume, u32)]) -> Vec<usize> {
    let mut group: Vec<usize> = (0..doors.len()).collect();
    let root = |group: &[usize], mut n: usize| {
        while let Some(&up) = group.get(n) {
            if up == n {
                break;
            }
            n = up;
        }
        n
    };
    for (a, (_, va, fa)) in doors.iter().enumerate() {
        for (b, (_, vb, fb)) in doors.iter().enumerate().skip(a.saturating_add(1)) {
            let touching = va.overlaps(vb);
            if touching && fa & DOOR_DONT_LINK == 0 && fb & DOOR_DONT_LINK == 0 {
                let (ra, rb) = (root(&group, a), root(&group, b));
                if let Some(slot) = group.get_mut(rb) {
                    *slot = ra;
                }
            }
        }
    }
    (0..doors.len()).map(|n| root(&group, n)).collect()
}

/// What a compiled map is to a moving player.
pub struct Ground<'a> {
    hulls: HullSet,
    bsp: &'a Bsp<'a>,
    moves: Moves,
    doors: Vec<Door>,
    /// The brush models that are always solid.
    walls: Vec<usize>,
    firings: Vec<Firing>,
    lifts: Vec<Lift>,
    trains: Vec<Train>,
    teleports: Vec<Teleport>,
    /// The world's bounds: past its sides and under its bottom is the void, as good as solid
    /// to a walker.
    bounds: Volume,
    /// The player start, if the map has one.
    start: Option<[f32; 3]>,
    /// Each `trigger_changelevel`'s volume and its entity.
    exits: Vec<(usize, Volume)>,
    wanted: Vec<Wanted>,
    /// The entities not judged, by place in the lump, with their classnames.
    unjudged: Vec<(usize, String)>,
}

impl<'a> Ground<'a> {
    /// The ground of `bsp`, from its entity lump as a single player game at `skill` spawns it,
    /// for a player with `moves`.
    ///
    /// # Errors
    ///
    /// [`PlayError::Lump`] when the lump does not parse and [`PlayError::NoWorld`] for a BSP
    /// with no models.
    #[expect(
        clippy::too_many_lines,
        reason = "one pass over the lump reads every class the check follows"
    )]
    pub fn new(bsp: &'a Bsp<'a>, moves: Moves, skill: Skill) -> Result<Self, PlayError> {
        let lump =
            MapFile::parse(bsp.entity_lump).map_err(|error| PlayError::Lump(error.to_string()))?;
        let world = Volume::of_model(bsp, 0).ok_or(PlayError::NoWorld)?;
        // The entities the engine keeps in a single player game at `skill`: it frees the rest
        // before their spawn functions run (EDICT-8.4); never the world.
        let read: Vec<Read<'_>> = lump
            .entities
            .iter()
            .enumerate()
            .filter(|&(place, entity)| {
                let flags = entity
                    .value(b"spawnflags")
                    .and_then(|text| std::str::from_utf8(text).ok()?.trim().parse::<f32>().ok())
                    .unwrap_or(0.0);
                place == 0 || !inhibited(flags, Deathmatch::Off, skill.level())
            })
            .map(|(place, entity)| Read {
                place,
                entity,
                classname: entity
                    .classname()
                    .map_or_default(|name| String::from_utf8_lossy(name).into_owned()),
                model: entity
                    .value(b"model")
                    .and_then(|model| std::str::from_utf8(model.strip_prefix(b"*")?).ok())
                    .and_then(|n| n.parse().ok()),
            })
            .collect();
        let mut ground = Self {
            hulls: HullSet::new(bsp),
            bsp,
            moves,
            doors: Vec::new(),
            walls: Vec::new(),
            firings: Vec::new(),
            lifts: Vec::new(),
            trains: Vec::new(),
            teleports: Vec::new(),
            bounds: world,
            start: None,
            exits: Vec::new(),
            wanted: Vec::new(),
            unjudged: Vec::new(),
        };
        // The doors, linked, each group a bit, and the names that fire them.
        let mut chains = Chains::default();
        let doors: Vec<(usize, Volume, u32)> = read
            .iter()
            .enumerate()
            .filter(|(_, r)| r.classname == "func_door")
            .filter_map(|(at, r)| {
                let volume = Volume::of_model(bsp, r.model?)?;
                Some((at, volume, r.number(b"spawnflags")))
            })
            .collect();
        let groups = linked_doors(&doors);
        let mut bits: BTreeMap<usize, usize> = BTreeMap::new();
        let mut keyed: Vec<(u32, usize)> = Vec::new();
        let mut shot: BTreeMap<usize, Volume> = BTreeMap::new();
        let mut closed: BTreeSet<usize> = BTreeSet::new();
        for (&(at, volume, flags), &group) in doors.iter().zip(&groups) {
            let Some(r) = read.get(at) else { continue };
            let key = flags & (DOOR_GOLD_KEY | DOOR_SILVER_KEY);
            let targetname = r.text(b"targetname");
            let health = r.set(b"health");
            if key == 0 && targetname.is_none() && !health {
                continue;
            }
            closed.insert(group);
            let next = bits.len();
            let bit = *bits.entry(group).or_insert(next);
            if key != 0 {
                keyed.push((key, bit));
            }
            if let Some(name) = targetname {
                chains.link(Event::Kill(name.clone()), Event::Door(bit));
                chains.link(Event::Name(name), Event::Door(bit));
            }
            if health {
                shot.entry(bit)
                    .and_modify(|v| *v = v.around(volume))
                    .or_insert(volume);
            }
        }
        for (&(at, volume, flags), &group) in doors.iter().zip(&groups) {
            let Some(r) = read.get(at) else { continue };
            // `pos2 = pos1 + movedir*(fabs(movedir*size) - lip)`.
            let way = movedir(r.angles());
            let size = [0, 1, 2].map(|axis| {
                volume.max.get(axis).unwrap_or(&0.0) - volume.min.get(axis).unwrap_or(&0.0)
            });
            let along = (way[0] * size[0] + way[1] * size[1] + way[2] * size[2]).abs();
            let lip = r
                .float(b"lip")
                .filter(|lip| *lip != 0.0)
                .unwrap_or(DOOR_LIP);
            let open = Open::By(way.map(|w| w * (along - lip)));
            // `if (!self.wait) self.wait = 3`, and `if (self.items) self.wait = -1`.
            // A toggle door is moved back by its next firing, so it is either place.
            let stays = if flags & DOOR_TOGGLE == 0
                && (flags & (DOOR_GOLD_KEY | DOOR_SILVER_KEY) != 0
                    || r.float(b"wait").is_some_and(|wait| wait < 0.0))
            {
                Stays::Open
            } else {
                Stays::Returns
            };
            let bit = bits.get(&group).copied();
            let opener = match bit {
                Some(bit) if closed.contains(&group) => {
                    if bit >= MOST_DOORS {
                        ground.unjudged.push((r.place, r.classname.clone()));
                        None
                    } else {
                        Some(Held::door(bit))
                    }
                }
                Some(_) | None => None,
            };
            if let Some(model) = r.model {
                let starts = if flags & DOOR_START_OPEN == 0 {
                    Starts::Shut
                } else {
                    Starts::Open
                };
                ground.doors.push(Door {
                    model,
                    from: NOT_MOVED,
                    opener,
                    open,
                    stays,
                    starts,
                });
            }
            let Some(bit) = bit.filter(|_| opener.is_some()) else {
                continue;
            };
            for event in r.fired() {
                chains.link(Event::Door(bit), event);
            }
        }
        let mut next_bit = bits.len();
        for r in &read {
            if r.classname != "func_door_secret" {
                continue;
            }
            let Some(model) = r.model else { continue };
            let bit = next_bit;
            next_bit = next_bit.saturating_add(1);
            if bit >= MOST_DOORS {
                ground.unjudged.push((r.place, r.classname.clone()));
                continue;
            }
            let targetname = r.text(b"targetname");
            if let Some(name) = &targetname {
                chains.link(Event::Kill(name.clone()), Event::Door(bit));
                chains.link(Event::Name(name.clone()), Event::Door(bit));
            }
            if (targetname.is_none() || r.number(b"spawnflags") & SECRET_YES_SHOOT != 0)
                && let Some(volume) = Volume::of_model(bsp, model)
            {
                shot.insert(bit, volume);
            }
            for event in r.fired() {
                chains.link(Event::Door(bit), event);
            }
            ground.doors.push(Door {
                model,
                from: NOT_MOVED,
                opener: Some(Held::door(bit)),
                open: Open::Gone,
                stays: Stays::Open,
                starts: Starts::Shut,
            });
        }
        // What fires what in turn.
        const RELAYS: [&str; 6] = [
            "trigger_relay",
            "trigger_counter",
            "trigger_once",
            "trigger_multiple",
            "trigger_secret",
            "func_button",
        ];
        for r in &read {
            // A monster with a `targetname` comes to the player once fired, out of a closet or
            // teleported in, and is taken to be killed, so its death fires its targets.
            if (RELAYS.contains(&r.classname.as_str()) || r.classname.starts_with("monster_"))
                && let Some(name) = r.text(b"targetname")
            {
                for event in r.fired() {
                    chains.link(Event::Name(name.clone()), event);
                }
            }
        }
        // A wall something removes (`killtarget`) is a door that opens out of the way.
        let killed: BTreeSet<String> = read.iter().filter_map(|r| r.text(b"killtarget")).collect();
        let mut removable: BTreeSet<usize> = BTreeSet::new();
        for r in &read {
            let (Some(model), Some(name)) = (r.model, r.text(b"targetname")) else {
                continue;
            };
            if !matches!(r.classname.as_str(), "func_wall" | "func_bossgate")
                || !killed.contains(&name)
                || next_bit >= MOST_DOORS
            {
                continue;
            }
            chains.link(Event::Kill(name), Event::Door(next_bit));
            ground.doors.push(Door {
                model,
                from: NOT_MOVED,
                opener: Some(Held::door(next_bit)),
                open: Open::Gone,
                stays: Stays::Open,
                starts: Starts::Shut,
            });
            removable.insert(model);
            next_bit = next_bit.saturating_add(1);
        }
        // A plat with a `targetname` waits at its top until fired (`plats.qc`: `self.state =
        // STATE_UP`), and a train spawns at its first corner, where it stays until fired
        // when it has one (`func_train_find`), and is out of the way once it moves.
        // Each corner: where it is, and the corner after it.
        let corners: BTreeMap<String, ([f32; 3], Option<String>)> = read
            .iter()
            .filter(|r| r.classname == "path_corner")
            .filter_map(|r| Some((r.text(b"targetname")?, (r.origin(), r.text(b"target")))))
            .collect();
        let mut plat_bits: BTreeMap<usize, Held> = BTreeMap::new();
        // A secret something fires is found when it fires (`multi_use`), as one a player
        // does not touch is (`notouch`).
        let mut secret_bits: BTreeMap<usize, Held> = BTreeMap::new();
        for r in &read {
            if r.classname == "trigger_secret"
                && let Some(name) = r.text(b"targetname").filter(|_| next_bit < MOST_DOORS)
            {
                chains.link(Event::Name(name), Event::Door(next_bit));
                secret_bits.insert(r.place, Held::door(next_bit));
                next_bit = next_bit.saturating_add(1);
            }
        }
        for r in &read {
            let Some(model) = r.model else { continue };
            let fired = r.text(b"targetname").filter(|_| next_bit < MOST_DOORS);
            match r.classname.as_str() {
                "func_plat" => {
                    if let Some(name) = fired {
                        chains.link(Event::Name(name), Event::Door(next_bit));
                        plat_bits.insert(model, Held::door(next_bit));
                        next_bit = next_bit.saturating_add(1);
                    }
                }
                "func_train" => {
                    // Its corners in the order it goes round them (`train_next`), each as how
                    // far it is from where it is drawn: `targ.origin - self.mins`.
                    let Some(volume) = Volume::of_model(bsp, model) else {
                        continue;
                    };
                    let mut stops: Vec<[f32; 3]> = Vec::new();
                    let mut name = r.text(b"target");
                    while let Some((corner, next)) = name.as_ref().and_then(|n| corners.get(n)) {
                        let stop = [0, 1, 2].map(|axis| {
                            corner.get(axis).unwrap_or(&0.0) - volume.min.get(axis).unwrap_or(&0.0)
                        });
                        if stops.contains(&stop) || stops.len() >= MOST_STOPS {
                            break;
                        }
                        stops.push(stop);
                        name.clone_from(next);
                    }
                    let Some(&first) = stops.first() else {
                        continue;
                    };
                    let opener = fired.map(|name| {
                        chains.link(Event::Name(name), Event::Door(next_bit));
                        let bit = Held::door(next_bit);
                        next_bit = next_bit.saturating_add(1);
                        bit
                    });
                    ground.doors.push(Door {
                        model,
                        from: first,
                        opener,
                        open: Open::Gone,
                        stays: Stays::Open,
                        starts: Starts::Shut,
                    });
                    ground.trains.push(Train {
                        volume,
                        stops,
                        opener,
                    });
                }
                _ => {}
            }
        }
        let fires = |r: &Read<'_>| chains.opens(r.fired());
        // The teleporters' destinations.
        let destinations: BTreeMap<String, [f32; 3]> = read
            .iter()
            .filter(|r| r.classname == "info_teleport_destination")
            .filter_map(|r| {
                let [x, y, z] = r.origin();
                Some((r.text(b"targetname")?, [x, y, z + DESTINATION_LIFT]))
            })
            .collect();
        for r in &read {
            let place = r.place;
            let volume = r.model.and_then(|model| Volume::of_model(bsp, model));
            let class = r.classname.as_str();
            match (class, volume) {
                ("info_player_start", _) => {
                    ground.start = ground.start.or_else(|| Some(r.origin()));
                }
                ("trigger_changelevel", Some(volume)) => ground.exits.push((place, volume)),
                ("func_wall" | "func_bossgate", Some(_)) => {
                    ground
                        .walls
                        .extend(r.model.filter(|model| !removable.contains(model)));
                }
                ("func_plat", Some(volume)) => {
                    let top = volume.max[2] - SPREAD;
                    let height = r
                        .text(b"height")
                        .and_then(|text| text.trim().parse::<f32>().ok())
                        .filter(|height| *height != 0.0)
                        .unwrap_or(volume.max[2] - volume.min[2] - PLAT_LIP);
                    ground.lifts.push(Lift {
                        opener: r.model.and_then(|model| plat_bits.get(&model).copied()),
                        min: [volume.min[0] + SPREAD, volume.min[1] + SPREAD],
                        max: [volume.max[0] - SPREAD, volume.max[1] - SPREAD],
                        top,
                        bottom: top - height,
                    });
                }
                ("trigger_teleport", Some(volume)) => {
                    match r.text(b"target").and_then(|name| destinations.get(&name)) {
                        Some(&to) => ground.teleports.push(Teleport { volume, to }),
                        None => ground.unjudged.push((place, r.classname.clone())),
                    }
                }
                ("func_button", Some(volume)) => {
                    let at = if r.set(b"health") {
                        Where::Seen(volume.middle())
                    } else {
                        Where::Beside(volume)
                    };
                    ground.firings.push(Firing {
                        at,
                        opens: fires(r),
                    });
                }
                (
                    "trigger_once"
                    | "trigger_multiple"
                    | "trigger_secret"
                    | "trigger_onlyregistered",
                    Some(volume),
                ) => {
                    let at = if r.set(b"health") {
                        Some(Where::Seen(volume.middle()))
                    } else if r.number(b"spawnflags") & NOTOUCH == 0 {
                        Some(Where::Inside(volume))
                    } else {
                        None
                    };
                    if let Some(at) = at {
                        ground.firings.push(Firing {
                            at,
                            opens: fires(r),
                        });
                    }
                    if class == "trigger_secret" {
                        ground.wanted.push(Wanted {
                            entity: place,
                            kind: Sought::Secret,
                            classname: r.classname.clone(),
                            at: Where::Inside(volume),
                            point: volume.middle(),
                            fired: secret_bits.get(&place).copied(),
                        });
                    }
                }
                _ if class.starts_with("item_") || class.starts_with("weapon_") => {
                    let kind = match class {
                        "item_key1" | "item_key2" => Sought::Key,
                        _ if class.starts_with("weapon_") => Sought::Weapon,
                        _ => Sought::Item,
                    };
                    let key = match class {
                        "item_key1" => DOOR_SILVER_KEY,
                        "item_key2" => DOOR_GOLD_KEY,
                        _ => 0,
                    };
                    let opened = keyed
                        .iter()
                        .filter(|(needs, _)| needs & key != 0)
                        .map(|&(_, bit)| Event::Door(bit));
                    let opens = chains.opens(opened.chain(r.fired()));
                    // A key, a sigil or an artifact's box reaches 24 below its origin, as the
                    // player's does; any other item's sits on its origin (`items.qc`).
                    let below = if class.starts_with("item_key")
                        || class.starts_with("item_artifact")
                        || class == "item_sigil"
                    {
                        FEET
                    } else {
                        0.0
                    };
                    // The game removes an item with no floor in reach.
                    let Some(point) = ground.dropped(r.origin(), below) else {
                        continue;
                    };
                    let at = Where::Item {
                        at: [point, ground.carried(point)],
                        below,
                    };
                    ground.firings.push(Firing { at, opens });
                    ground.wanted.push(Wanted {
                        entity: place,
                        kind,
                        classname: r.classname.clone(),
                        at,
                        point,
                        fired: None,
                    });
                }
                _ if class.starts_with("monster_") && r.text(b"target").is_some() => {
                    ground.firings.push(Firing {
                        at: Where::Seen(r.origin()),
                        opens: fires(r),
                    });
                }
                _ => {}
            }
            if Class::of(class) == Class::Unjudged {
                ground.unjudged.push((place, r.classname.clone()));
            }
        }
        for (bit, volume) in shot {
            ground.firings.push(Firing {
                at: Where::Seen(volume.middle()),
                opens: chains.opens([Event::Door(bit)]),
            });
        }
        ground.unjudged.sort_unstable();
        ground.unjudged.dedup();
        Ok(ground)
    }

    /// Where an item spawned at `origin`, whose box reaches `below` under it, comes to rest:
    /// `PlaceItem` raises it [`ITEM_LIFT`] and `droptofloor` drops it at most [`ITEM_DROP`];
    /// `None` when it finds no floor, and the game removes it.
    fn dropped(&self, [x, y, z]: [f32; 3], below: f32) -> Option<[f32; 3]> {
        // The player's box reaches FEET below its origin: an item's box bottom is where a
        // player standing there would have its feet.
        let top = z + ITEM_LIFT - below + FEET;
        let [_, _, rest] = self.rest(Held::default(), [x, y, top]);
        (top - rest <= ITEM_DROP).then_some([x, y, rest - FEET + below])
    }

    /// Where an item at `point` lies once the door it stands on, if any, has moved: an item
    /// rides down with its floor (`MOVETYPE_TOSS`).
    fn carried(&self, point: [f32; 3]) -> [f32; 3] {
        for door in &self.doors {
            let (Open::By(by), Some(volume)) = (door.open, Volume::of_model(self.bsp, door.model))
            else {
                continue;
            };
            // Where the door starts, and how far firing moves it: a door that starts open is
            // moved back to where it is drawn.
            let (start, moves) = match door.starts {
                Starts::Shut => (door.from, by),
                Starts::Open => (
                    [0, 1, 2].map(|axis| {
                        door.from.get(axis).unwrap_or(&0.0) + by.get(axis).unwrap_or(&0.0)
                    }),
                    by.map(|b| -b),
                ),
            };
            let [sx, sy, sz] = start;
            let [x, y, z] = point;
            let under = (volume.min[0] + sx..=volume.max[0] + sx).contains(&x)
                && (volume.min[1] + sy..=volume.max[1] + sy).contains(&y)
                && (volume.max[2] + sz - SPREAD..=volume.max[2] + sz + ITEM_REST).contains(&z);
            if under {
                return [x + moves[0], y + moves[1], z + moves[2]];
            }
        }
        point
    }

    /// Hull `index` of brush model `model`, and its first node.
    fn hull(&self, index: usize, model: usize) -> Option<(Hull<'_>, i32)> {
        let heads = self.bsp.models.get(model)?.headnode;
        let heads = [heads[0], heads[1], heads[2]];
        Some((self.hulls.hull(index, heads), *heads.get(index)?))
    }

    /// Whether `at` is in the solid of hull `index` of `model`; a hull that cannot be read is
    /// taken as solid.
    fn solid_in(&self, index: usize, model: usize, at: [f32; 3]) -> bool {
        // A brush entity's player hull is its bounds grown by the player's box: a point outside
        // that is out of it, and the hull need not be walked.
        if model != 0
            && index == PLAYER_HULL
            && let Some(Volume { min, max }) = Volume::of_model(self.bsp, model)
        {
            let [x, y, z] = at;
            if x < min[0] - HALF
                || x > max[0] + HALF
                || y < min[1] - HALF
                || y > max[1] + HALF
                || z < min[2] - HEAD
                || z > max[2] + FEET
            {
                return false;
            }
        }
        self.hull(index, model).is_none_or(|(hull, head)| {
            !hull
                .point_contents(head, at)
                .is_ok_and(|contents| contents != CONTENTS_SOLID)
        })
    }

    /// Whether the player's box at `at` is in the world's solid, a wall, or a door `held` does
    /// not open.
    fn blocked(&self, held: Held, at: [f32; 3]) -> bool {
        // Past the sides or under the bottom: nothing moves up but a swimmer, so the top
        // needs no bound, and a map open to the sky stands under none.
        let Volume { min, max } = self.bounds;
        if at[0] < min[0] || at[0] > max[0] || at[1] < min[1] || at[1] > max[1] || at[2] < min[2] {
            return true;
        }
        self.solid_in(PLAYER_HULL, 0, at)
            || self
                .walls
                .iter()
                .any(|&model| self.solid_in(PLAYER_HULL, model, at))
            || self.doors.iter().any(|door| door.holds(self, held, at))
    }

    /// The contents of the point `at`, as the world's drawn hull has them.
    fn contents(&self, at: [f32; 3]) -> i32 {
        self.hull(DRAWN_HULL, 0)
            .and_then(|(hull, head)| hull.point_contents(head, at).ok())
            .unwrap_or(CONTENTS_SOLID)
    }

    /// Whether the player's middle at `at` is in a liquid: waist deep (PHYS-8.4).
    fn swimming(&self, at: [f32; 3]) -> bool {
        LIQUIDS.contains(&self.contents([at[0], at[1], at[2] + WAIST]))
    }

    /// Whether any of the player's box at `at` is in lava, from the feet up.
    fn in_lava(&self, at: [f32; 3]) -> bool {
        let mut z = at[2] - FEET + 1.0;
        while z <= at[2] + HEAD {
            if self.contents([at[0], at[1], z]) == LAVA {
                return true;
            }
            z += PACE;
        }
        false
    }

    /// Whether a player whose origin is at `at` sees `point`: nothing of the world's drawn hull
    /// between its eye and it.
    fn sees(&self, at: [f32; 3], point: [f32; 3]) -> bool {
        let Some((hull, head)) = self.hull(DRAWN_HULL, 0) else {
            return false;
        };
        let eye = [at[0], at[1], at[2] + EYE];
        let mut trace = Trace::new(point);
        recursive_hull_check(&hull, head, (eye, point), &mut trace).is_ok()
            && !trace.startsolid
            && trace.fraction >= 1.0
    }

    /// Where the box at `[x, y, z]` comes to rest falling from there: on a floor, or at the
    /// first height in a liquid it falls into, since a swimmer is then held up.
    fn rest(&self, held: Held, [x, y, mut z]: [f32; 3]) -> [f32; 3] {
        // A pace at a time, then finely: a floor in the player's hull is at least its height
        // thick, so a pace does not step over one.
        while !self.blocked(held, [x, y, z - PACE]) && !self.swimming([x, y, z - PACE]) {
            z -= PACE;
        }
        while !self.blocked(held, [x, y, z - FINE]) && !self.swimming([x, y, z]) {
            z -= FINE;
        }
        // The floor is within a fine step below: halved down to near it, so that how high a
        // player stands does not hang on where its fall began.
        if !self.swimming([x, y, z]) {
            let mut low = z - FINE;
            for _ in 0..REST_HALVINGS {
                let middle = (low + z) / 2.0;
                if self.blocked(held, [x, y, middle]) {
                    low = middle;
                } else {
                    z = middle;
                }
            }
        }
        [x, y, z]
    }

    /// A fall from `top`: where it comes to rest, and the triggers the player falls through on
    /// the way; a teleporter on the way, the highest, sends the player on (`teleport_touch`).
    fn fall(&self, held: Held, top: [f32; 3]) -> Landing {
        let [x, y, z] = self.rest(held, top);
        let column = Volume {
            min: [x - HALF, y - HALF, z - FEET],
            max: [x + HALF, y + HALF, top[2] + HEAD],
        };
        let crossed = self
            .firings
            .iter()
            .filter(|firing| matches!(firing.at, Where::Inside(volume) if volume.overlaps(&column)))
            .fold(Held::default(), |held, firing| held.with(firing.opens));
        let teleport = self
            .teleports
            .iter()
            .filter(|teleport| teleport.volume.overlaps(&column))
            .max_by(|a, b| {
                a.volume.max[2]
                    .partial_cmp(&b.volume.max[2])
                    .unwrap_or(core::cmp::Ordering::Equal)
            });
        let at = match teleport {
            Some(teleport) if top[2] > z => self.rest(held, teleport.to),
            Some(_) | None => [x, y, z],
        };
        Landing { at, crossed }
    }

    /// Where a player at `from` stands after a pace along `step` that climbs at most `climb`;
    /// `None` when a wall is in the way.
    fn paced(&self, held: Held, from: [f32; 3], step: [f32; 2], climb: f32) -> Option<Landing> {
        let (x, y) = (from[0] + step[0], from[1] + step[1]);
        let mut z = from[2] + climb;
        // The head must clear what the feet climb.
        if self.blocked(held, [from[0], from[1], z]) {
            z = from[2];
        }
        // The highest room at most `climb` up, a pace at a time: any height in it falls to the
        // same floor.
        while self.blocked(held, [x, y, z]) {
            if z <= from[2] {
                return None;
            }
            z = (z - PACE).max(from[2]);
        }
        Some(self.fall(held, [x, y, z]))
    }

    /// Where a running jump from `at` along `step`, a pace or a diagonal one, lands across a gap: up to [`JUMP`] high, on
    /// ground across a gap whose sides, as the player's box meets them, are at most [`LEAP`]
    /// less the box's width apart, and not more than [`LEAP`] below; none for the generator's
    /// moves, which take no jump.
    fn leaps(&self, held: Held, at: [f32; 3], [dx, dy]: [f32; 2]) -> Vec<Landing> {
        let mut landed = Vec::new();
        let Some(jump) = self.moves.jump() else {
            return landed;
        };
        let mut top = at[2] + jump;
        while self.blocked(held, [at[0], at[1], top]) {
            top -= FINE;
            if top <= at[2] {
                return landed;
            }
        }
        let lowest = at[2] - LEAP;
        // Paces of `step` while their length is within the gap a leap clears.
        let pace = dx.hypot(dy);
        let mut over = [at[0], at[1], top];
        let mut across = pace;
        while across <= LEAP - HALF - HALF {
            over = [over[0] + dx, over[1] + dy, top];
            if self.blocked(held, over) {
                break;
            }
            if self.floor_above(held, over, lowest) {
                landed.push(self.fall(held, over));
            }
            across += pace;
        }
        landed
    }

    /// Whether the box falling from `top` comes to rest above `lowest`.
    fn floor_above(&self, held: Held, [x, y, mut z]: [f32; 3], lowest: f32) -> bool {
        while !self.blocked(held, [x, y, z - PACE]) && !self.swimming([x, y, z - PACE]) {
            z -= PACE;
            if z < lowest {
                return false;
            }
        }
        true
    }

    /// The surface of the liquid a swimmer at `at` is in: the first height above its middle
    /// that is out of it.
    fn surface(&self, at: [f32; 3]) -> f32 {
        let mut z = at[2] + WAIST;
        while LIQUIDS.contains(&self.contents([at[0], at[1], z])) {
            z += FINE;
        }
        z
    }

    /// The lift whose top `at` stands on, if any.
    fn riding(&self, at: [f32; 3]) -> Option<&Lift> {
        self.lifts.iter().find(|lift| lift.at_top(at))
    }

    /// The places a player at `at` holding `held` moves to in one move.
    fn moves(&self, held: Held, at: [f32; 3]) -> Vec<Landing> {
        // A teleporter sends a player whose box touches it at once (`teleport_touch`).
        if let Some(teleport) = self.teleports.iter().find(|t| t.volume.touched(at, 0.0)) {
            return vec![self.fall(held, teleport.to)];
        }
        let mut next = Vec::new();
        let flat = [[PACE, 0.0], [-PACE, 0.0], [0.0, PACE], [0.0, -PACE]];
        if self.swimming(at) {
            // A swimmer climbs out onto ground whose top is at most CLIMB_OUT above the
            // surface: a box standing there has its origin FEET above that.
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the game holds heights in single precision"
            )]
            let reach = self.surface(at) + crate::CLIMB_OUT as f32 + FEET - at[2];
            for step in flat {
                next.extend(self.paced(held, at, step, reach.max(0.0)));
                // A stroke at the swimmer's own depth, as along a flooded tunnel.
                let level = [at[0] + step[0], at[1] + step[1], at[2]];
                if !self.blocked(held, level) {
                    next.push(Landing::still(level));
                }
            }
            for dz in [PACE, -PACE] {
                let to = [at[0], at[1], at[2] + dz];
                if !self.blocked(held, to) {
                    next.push(Landing::still(to));
                }
            }
        } else if let Some(lift) = self.riding(at) {
            // On a lift at its top: across it at its height, or off it as from a floor. It
            // stays up while the player stands on it (`plat_center_touch`: "delay going down"),
            // so there is no way down but off its edge; its shaft is reached from beside it,
            // once it has gone down without the player (`plat_hit_top`).
            for [dx, dy] in flat {
                let to = [at[0] + dx, at[1] + dy, at[2]];
                if lift.holds(to) {
                    next.push(Landing::still(to));
                } else {
                    next.extend(self.paced(held, at, [dx, dy], CLIMB));
                }
            }
        } else if let Some(lift) = self
            .lifts
            .iter()
            .find(|lift| lift.at_bottom(at) && lift.opener.is_none_or(|opener| held.has(opener)))
        {
            // On a lift at its bottom: it takes the player up at once (`plat_center_touch`),
            // so the player is taken to be carried up, and not to walk off it while it rises.
            next.push(Landing::still([at[0], at[1], lift.top + FEET]));
        } else {
            // The paces that land beside the player at about its height.
            let mut level_steps: Vec<[f32; 2]> = Vec::new();
            for step in flat {
                let walked = self.paced(held, at, step, CLIMB);
                // At an edge, or a wall, a running jump may clear a gap: anywhere a pace does
                // not land beside the player at about its height, as one into a teleporter.
                let level = |landing: &Landing| {
                    let [x, y, z] = landing.at;
                    (x - (at[0] + step[0])).abs() <= FINE
                        && (y - (at[1] + step[1])).abs() <= FINE
                        && z >= at[2] - CLIMB
                };
                let is_level = walked.as_ref().is_some_and(level);
                if is_level {
                    level_steps.push(step);
                }
                if !is_level {
                    next.extend(self.leaps(held, at, step));
                }
                next.extend(walked);
                if let Some(jump) = self.moves.jump() {
                    next.extend(self.paced(held, at, step, jump));
                }
            }
            // On a train, it carries the player round its corners (`train_next`).
            for train in &self.trains {
                let Some(on) = train.boarded(at, held) else {
                    continue;
                };
                let Some(&[ox, oy, oz]) = train.stops.get(on) else {
                    continue;
                };
                let top = train.volume.max[2] - SPREAD + oz + FEET;
                for &[sx, sy, sz] in &train.stops {
                    let to = [at[0] + sx - ox, at[1] + sy - oy, top + sz - oz];
                    if !self.blocked(held, to) {
                        next.push(Landing::still(to));
                    }
                }
            }
            // Across a corner, as a running player cuts it: a diagonal leap wherever either of
            // the paces it is between leaves level ground.
            for dx in [PACE, -PACE] {
                for dy in [PACE, -PACE] {
                    let corner =
                        level_steps.contains(&[dx, 0.0]) && level_steps.contains(&[0.0, dy]);
                    if !corner {
                        next.extend(self.leaps(held, at, [dx, dy]));
                    }
                }
            }
        }
        next
    }

    /// What a player at `at` opens there, whatever it holds.
    fn opened_at(&self, at: [f32; 3]) -> Held {
        self.firings
            .iter()
            .filter(|firing| self.reaches(at, firing.at))
            .fold(Held::default(), |held, firing| held.with(firing.opens))
    }

    /// Whether a player at `at` reaches `place`.
    fn reaches(&self, at: [f32; 3], place: Where) -> bool {
        match place {
            Where::Item { at: points, below } => {
                points.iter().any(|&point| item_touched(at, point, below))
            }
            Where::Inside(volume) => volume.touched(at, 0.0),
            Where::Beside(volume) => volume.touched(at, PACE),
            Where::Seen(point) => self.sees(at, point),
        }
    }

    /// Every place and state a player at `start` reaches, each with those it is reached from
    /// in one move. Lava stops the search where it is reached: a player there is dead.
    fn explore(&self, start: [f32; 3], search: Search) -> Option<Explored> {
        let mut held_at: HashMap<Cell, Vec<Held>> = HashMap::new();
        let mut opened: HashMap<Cell, Held> = HashMap::new();
        let mut opened_at =
            |at: [f32; 3]| *opened.entry(cell(at)).or_insert_with(|| self.opened_at(at));
        let Landing { at: first, crossed } = self.fall(Held::default(), start);
        let start: Node = (cell(first), opened_at(first).with(crossed));
        let mut ids: HashMap<Node, u32> = HashMap::from([(start, 0)]);
        let mut explored = Explored {
            nodes: vec![(start, first)],
            moves: Vec::new(),
            dead: vec![false],
        };
        let mut queue = Queue::default();
        queue.push(0, start, search);
        while let Some(id) = queue.pop() {
            let Some(&((_, held), at)) = explored.nodes.get(usize::try_from(id).ok()?) else {
                continue;
            };
            // A player in lava is dying: not trapped there. The generator's moves end there; a
            // player's may dip in and out, as for an item at the bottom.
            if self.in_lava(at) {
                if let Some(dead) = usize::try_from(id)
                    .ok()
                    .and_then(|n| explored.dead.get_mut(n))
                {
                    *dead = true;
                }
                if self.moves == Moves::Generator {
                    continue;
                }
            }
            for Landing { at: to, crossed } in self.moves(held, at) {
                let mut next: Node = (cell(to), held.with(opened_at(to)).with(crossed));
                if search == Search::Covering {
                    let there = held_at.entry(next.0).or_default();
                    // A state the place already has one covering is taken as that one.
                    if let Some(&covering) = there.iter().find(|h| h.covers(next.1)) {
                        next.1 = covering;
                    } else {
                        there.push(next.1);
                    }
                }
                let next_id = match ids.entry(next) {
                    std::collections::hash_map::Entry::Occupied(slot) => *slot.get(),
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        if explored.nodes.len() >= MOST_PLACES {
                            return None;
                        }
                        let new = u32::try_from(explored.nodes.len()).ok()?;
                        slot.insert(new);
                        explored.nodes.push((next, to));
                        explored.dead.push(false);
                        queue.push(new, next, search);
                        new
                    }
                };
                explored.moves.push((next_id, id));
            }
        }
        Some(explored)
    }

    /// Every place a player at `start` reaches, in any state; `None` past [`MOST_PLACES`].
    #[must_use]
    pub fn reached(&self, start: [f32; 3]) -> Option<Vec<[f32; 3]>> {
        self.explore(start, Search::Covering)
            .map(|explored| explored.nodes.into_iter().map(|(_, at)| at).collect())
    }

    /// Checks, as the level generator needs, that from every place and state reached from
    /// `start` the exit, near `exit`, can still be reached, and that walking reaches no lava.
    ///
    /// # Errors
    ///
    /// The [`Trap`] found.
    pub fn check(&self, start: [f32; 3], exit: [f32; 3]) -> Result<(), Trap> {
        let explored = self.explore(start, Search::Whole).ok_or(Trap::TooLarge)?;
        if let Some(at) = explored
            .nodes
            .iter()
            .zip(&explored.dead)
            .filter(|(_, dead)| **dead)
            .map(|(&(_, at), _)| at)
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(core::cmp::Ordering::Equal))
        {
            return Err(Trap::Lava(at));
        }
        let goal = |at: [f32; 3]| near(at, exit);
        match explored.regions(&goal).first() {
            None if !explored.nodes.iter().any(|&(_, at)| goal(at)) => Err(Trap::NoExit),
            None => Ok(()),
            Some(region) => Err(Trap::Stuck(region.at)),
        }
    }

    /// Whether the map can be finished, and what of it is never reached, each entity named by
    /// `place` from its place in the BSP's entity lump, as the map it was compiled from numbers
    /// it; one `place` names none of is left out.
    ///
    /// # Errors
    ///
    /// [`PlayError::NoStart`] for a map with no player start and [`PlayError::TooLarge`] past
    /// [`MOST_PLACES`].
    pub fn playability<E>(
        &self,
        place: impl Fn(usize) -> Option<E>,
    ) -> Result<Playability<E>, PlayError> {
        let start = self.start.ok_or(PlayError::NoStart)?;
        let explored = self
            .explore(start, Search::Covering)
            .ok_or(PlayError::TooLarge)?;
        let reached: Vec<[f32; 3]> = explored.nodes.iter().map(|&(_, at)| at).collect();
        let held_ever = explored
            .nodes
            .iter()
            .fold(Held::default(), |held, &((_, h), _)| held.with(h));
        let exits = &self.exits;
        let goal = |at: [f32; 3]| exits.iter().any(|(_, volume)| volume.touched(at, 0.0));
        let exit = if exits.is_empty() {
            ExitReach::NoExit
        } else if reached.iter().any(|&at| goal(at)) {
            ExitReach::Reached
        } else {
            ExitReach::NotReached
        };
        let traps = match exit {
            ExitReach::Reached => explored.regions(&goal),
            ExitReach::NotReached | ExitReach::NoExit => Vec::new(),
        };
        let unreached = self
            .wanted
            .iter()
            .filter(|wanted| !wanted.fired.is_some_and(|bit| held_ever.has(bit)))
            .filter(|wanted| !reached.iter().any(|&at| self.reaches(at, wanted.at)))
            .filter_map(|wanted| {
                Some(Unreached {
                    entity: place(wanted.entity)?,
                    kind: wanted.kind,
                    classname: wanted.classname.clone(),
                    at: wanted.point,
                })
            })
            .collect();
        Ok(Playability {
            moves: self.moves,
            exit,
            exits: self
                .exits
                .iter()
                .filter_map(|(entity, _)| place(*entity))
                .collect(),
            unreached,
            traps,
            not_judged: self
                .unjudged
                .iter()
                .filter_map(|(entity, classname)| {
                    Some(NotJudged {
                        entity: place(*entity)?,
                        classname: classname.clone(),
                    })
                })
                .collect(),
            places: explored.nodes.len(),
        })
    }
}

impl Explored {
    /// The regions of places reached, alive, from which no place `goal` holds is reached, each
    /// a set of such places joined by moves; none when the start reaches no goal.
    fn regions(&self, goal: &dyn Fn([f32; 3]) -> bool) -> Vec<TrapRegion> {
        let count = self.nodes.len();
        let mut leads: Vec<bool> = self.nodes.iter().map(|&(_, at)| goal(at)).collect();
        if !leads.contains(&true) {
            return Vec::new();
        }
        // The moves by the state they reach, to walk them back from the goal.
        let mut by_target: Vec<(u32, u32)> = self.moves.clone();
        by_target.sort_unstable();
        let index = |n: u32| usize::try_from(n).unwrap_or(usize::MAX);
        let from_of = |to: u32| {
            let first = by_target.partition_point(|&(t, _)| t < to);
            by_target
                .get(first..)
                .unwrap_or_default()
                .iter()
                .take_while(move |&&(t, _)| t == to)
                .map(|&(_, from)| from)
        };
        let mut queue: VecDeque<u32> = (0..count)
            .filter(|&n| leads.get(n).copied().unwrap_or(false))
            .filter_map(|n| u32::try_from(n).ok())
            .collect();
        while let Some(node) = queue.pop_front() {
            for from in from_of(node) {
                if let Some(slot) = leads.get_mut(index(from))
                    && !*slot
                {
                    *slot = true;
                    queue.push_back(from);
                }
            }
        }
        let stuck = |n: usize| {
            !leads.get(n).copied().unwrap_or(true) && !self.dead.get(n).copied().unwrap_or(true)
        };
        let mut parent: Vec<usize> = (0..count).collect();
        let root = |parent: &[usize], mut n: usize| {
            while let Some(&up) = parent.get(n) {
                if up == n {
                    break;
                }
                n = up;
            }
            n
        };
        let mut entries: HashSet<usize> = HashSet::new();
        for &(to, from) in &self.moves {
            let (t, f) = (index(to), index(from));
            if !stuck(t) {
                continue;
            }
            if stuck(f) {
                let (rt, rf) = (root(&parent, t), root(&parent, f));
                if let Some(slot) = parent.get_mut(rf) {
                    *slot = rt;
                }
            } else {
                entries.insert(t);
            }
        }
        let stuck: Vec<usize> = (0..count).filter(|&n| stuck(n)).collect();
        // Each region, and whether its point is a place it is entered at.
        let mut regions: BTreeMap<usize, (TrapRegion, bool)> = BTreeMap::new();
        for &i in &stuck {
            let Some(&(_, at)) = self.nodes.get(i) else {
                continue;
            };
            let (region, entered) = regions.entry(root(&parent, i)).or_insert((
                TrapRegion {
                    at,
                    min: at,
                    max: at,
                },
                false,
            ));
            let grown = Volume {
                min: region.min,
                max: region.max,
            }
            .around(Volume { min: at, max: at });
            (region.min, region.max) = (grown.min, grown.max);
            // The place the region is entered at, the lowest first, is the point inside it.
            if entries.contains(&i) && (!*entered || at < region.at) {
                region.at = at;
                *entered = true;
            }
        }
        let mut regions: Vec<TrapRegion> = regions.into_values().map(|(r, _)| r).collect();
        regions.sort_by(|a, b| {
            a.at.partial_cmp(&b.at)
                .unwrap_or(core::cmp::Ordering::Equal)
        });
        // The same place reached with other doors open is one trap.
        regions.dedup_by(|a, b| cell(a.at) == cell(b.at));
        regions
    }
}

/// Whether a player at `at` touches the box of an item at `point`, which reaches `below` under
/// it: the item's box is 32 across and 56 high (`items.qc`), grown by [`ITEM_REACH`] across
/// and a unit up and down (`SV_LinkEdict`, for `FL_ITEM`).
fn item_touched(at: [f32; 3], point: [f32; 3], below: f32) -> bool {
    let across = HALF + ITEM_REACH + HALF;
    let bottom = point[2] - below - SPREAD;
    let top = point[2] - below + ITEM_HEIGHT + SPREAD;
    (at[0] - point[0]).abs() <= across
        && (at[1] - point[1]).abs() <= across
        && at[2] - FEET <= top
        && at[2] + HEAD >= bottom
}

/// Whether `at` is within [`NEAR`] of `place` on each axis.
fn near(at: [f32; 3], place: [f32; 3]) -> bool {
    at.iter().zip(place).all(|(a, p)| (a - p).abs() <= NEAR)
}

/// Whether the start reaches an exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExitReach {
    /// A `trigger_changelevel` is reached.
    Reached,
    /// None is.
    NotReached,
    /// The map has none, so no place is a trap.
    NoExit,
}

/// A key, weapon, item or secret never reached from the start, its entity an `E`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Unreached<E> {
    /// The entity.
    pub entity: E,
    /// What it is.
    pub kind: Sought,
    /// Its classname.
    pub classname: String,
    /// Where it is: an item's origin, a secret's middle.
    pub at: [f32; 3],
}

/// Places a player reaches from which the exit no longer is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TrapRegion {
    /// A player's origin inside it: where a player first comes into it.
    pub at: [f32; 3],
    /// The least corner of the origins in it.
    pub min: [f32; 3],
    /// The greatest corner of the origins in it.
    pub max: [f32; 3],
}

/// An entity whose effect on a player the check does not follow, an `E`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NotJudged<E> {
    /// The entity.
    pub entity: E,
    /// Its classname.
    pub classname: String,
}

/// Whether a map can be finished, each entity named by an `E`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Playability<E> {
    /// The moves the player was taken to have.
    pub moves: Moves,
    /// Whether the start reaches an exit.
    pub exit: ExitReach,
    /// The map's exits, its `trigger_changelevel` entities.
    pub exits: Vec<E>,
    /// Each key, weapon, item and secret never reached.
    pub unreached: Vec<Unreached<E>>,
    /// Each place a player reaches from which the exit no longer is.
    pub traps: Vec<TrapRegion>,
    /// Each entity the check could not judge.
    pub not_judged: Vec<NotJudged<E>>,
    /// How many places and states the search visited.
    pub places: usize,
}

impl<E> Unreached<E> {
    /// What is wrong, in words.
    #[must_use]
    pub fn message(&self) -> String {
        let what = match self.kind {
            Sought::Key => "this key",
            Sought::Weapon => "this weapon",
            Sought::Item => "this item",
            Sought::Secret => "this secret",
        };
        format!(
            "{what} ({}) is never reached from the player start",
            self.classname
        )
    }
}

impl TrapRegion {
    /// What is wrong, in words.
    #[must_use]
    pub fn message(&self) -> String {
        let [x, y, z] = self.at;
        format!(
            "a player who comes to {x} {y} {z} can no longer reach the exit: nothing there leads \
             back out"
        )
    }
}

#[cfg(test)]
mod tests;
