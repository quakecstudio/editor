// SPDX-License-Identifier: GPL-3.0-or-later

//! What a level is filled with: monsters that get harder the further a room is from the
//! start, a weapon now and then, and ammunition, health and armour between. The classnames
//! are id's (`qcc/v101qc`: each monster's file and `items.qc`); only those the game defines
//! are placed.

use crate::mass::Point;
use crate::rng::Rng;
use crate::{Classes, DOOR, KEY, TORCH, mission};

/// The monsters of the first third of a level, of its middle, and of its end.
const EASY: &[&str] = &["monster_army", "monster_dog", "monster_knight"];
const MIDDLE: &[&str] = &["monster_enforcer", "monster_ogre", "monster_wizard"];
const HARD: &[&str] = &["monster_hell_knight", "monster_demon1", "monster_shambler"];
/// The parts a level's depth is cut in for its monsters.
const TIERS: usize = 3;

/// The weapons of a level, in the order they are found.
const WEAPONS: &[&str] = &[
    "weapon_supershotgun",
    "weapon_nailgun",
    "weapon_grenadelauncher",
    "weapon_supernailgun",
    "weapon_rocketlauncher",
];
/// A weapon is in every room this many rooms apart.
const ROOMS_A_WEAPON: usize = 2;

/// What lies about a level. `item_health` and the ammunition are boxes from their origin to
/// `(32 32 32)` (`items.qc`); the armour is one around its origin.
const SUPPLIES: &[Supply] = &[
    Supply::cornered("item_health"),
    Supply::cornered("item_shells"),
    Supply::cornered("item_health"),
    Supply::cornered("item_spikes"),
    Supply::centred("item_armor1"),
    Supply::cornered("item_rockets"),
];
/// Half the width of an item whose origin is its corner.
pub(crate) const CORNERED_HALF: f64 = 16.0;

/// The most monsters a room holds, and how many more rooms from the start add one.
const MOST_MONSTERS: usize = 5;
const ROOMS_A_MONSTER: usize = 2;

/// Where an item's origin is on its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// Its lowest corner.
    Corner,
    /// The middle of its foot.
    Centre,
}

/// Something that lies about.
#[derive(Clone, Copy, Debug)]
struct Supply {
    classname: &'static str,
    origin: Origin,
}

impl Supply {
    /// One whose origin is its corner.
    const fn cornered(classname: &'static str) -> Self {
        Self {
            classname,
            origin: Origin::Corner,
        }
    }

    /// One whose origin is the middle of its foot.
    const fn centred(classname: &'static str) -> Self {
        Self {
            classname,
            origin: Origin::Centre,
        }
    }

    /// The origin that puts its middle over `spot`.
    fn origin_over(self, [x, y, z]: Point) -> Point {
        match self.origin {
            Origin::Corner => [x - CORNERED_HALF, y - CORNERED_HALF, z],
            Origin::Centre => [x, y, z],
        }
    }
}

/// One thing placed.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Placed {
    /// Its classname.
    pub(crate) classname: &'static str,
    /// Its origin.
    pub(crate) origin: Point,
}

/// What a room is given.
#[derive(Clone, Debug, Default)]
pub(crate) struct Filled {
    /// Its monsters.
    pub(crate) monsters: Vec<Placed>,
    /// Its items.
    pub(crate) items: Vec<Placed>,
}

/// Where a room is in its level.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Depth {
    /// How many rooms from the start it is; the start is 0.
    pub(crate) rooms: usize,
    /// The most any room of the level is.
    pub(crate) deepest: usize,
}

impl Classes {
    /// Every classname a level may place, for a caller to find which the game defines.
    pub fn wanted() -> impl Iterator<Item = &'static str> {
        [EASY, MIDDLE, HARD, WEAPONS]
            .into_iter()
            .flatten()
            .copied()
            .chain(SUPPLIES.iter().map(|supply| supply.classname))
            .chain([KEY, TORCH, DOOR])
            .chain([
                mission::GOLD_KEY,
                mission::BUTTON,
                mission::COUNTER,
                mission::SECRET_DOOR,
                mission::SECRET,
            ])
    }

    /// Those of `wanted` the game defines.
    fn of(&self, wanted: &[&'static str]) -> Vec<&'static str> {
        wanted
            .iter()
            .copied()
            .filter(|name| self.has(name))
            .collect()
    }

    /// The monsters for a room at `depth`: the easy ones first, the hard ones last, and
    /// whatever the game defines when it has none of that part's.
    fn monsters(&self, depth: Depth) -> Vec<&'static str> {
        let part = depth
            .rooms
            .saturating_mul(TIERS)
            .checked_div(depth.deepest.saturating_add(1))
            .unwrap_or(0);
        let mut wanted = match part {
            0 => EASY.to_vec(),
            1 => [EASY, MIDDLE].concat(),
            _ => [MIDDLE, HARD].concat(),
        };
        if self.of(&wanted).is_empty() {
            wanted = [EASY, MIDDLE, HARD].concat();
        }
        self.of(&wanted)
    }

    /// Fills a room at `depth` whose free places are `spots`: an item in each room, a weapon
    /// in some, and monsters in every room but the first.
    pub(crate) fn fill(&self, depth: Depth, spots: &[Point], rng: &mut Rng) -> Filled {
        let mut spots = spots.to_vec();
        rng.shuffle(&mut spots);
        let mut filled = Filled::default();
        let supplies: Vec<Supply> = SUPPLIES
            .iter()
            .copied()
            .filter(|supply| self.has(supply.classname))
            .collect();
        if let (Some(supply), Some(spot)) = (rng.pick(&supplies), spots.pop()) {
            filled.items.push(Placed {
                classname: supply.classname,
                origin: supply.origin_over(spot),
            });
        }
        let weapons = self.of(WEAPONS);
        let due = depth.rooms.checked_rem(ROOMS_A_WEAPON) == Some(1);
        let which = depth.rooms.checked_div(ROOMS_A_WEAPON).unwrap_or(0);
        if due
            && let Some(&classname) = weapons.get(which)
            && let Some(origin) = spots.pop()
        {
            filled.items.push(Placed { classname, origin });
        }
        if depth.rooms == 0 {
            return filled;
        }
        let monsters = self.monsters(depth);
        let wanted = depth
            .rooms
            .checked_div(ROOMS_A_MONSTER)
            .unwrap_or(0)
            .saturating_add(1)
            .saturating_add(rng.below(2))
            .min(MOST_MONSTERS);
        for _ in 0..wanted {
            let (Some(&classname), Some(origin)) = (rng.pick(&monsters), spots.pop()) else {
                break;
            };
            filled.monsters.push(Placed { classname, origin });
        }
        filled
    }
}
