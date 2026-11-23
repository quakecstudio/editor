// SPDX-License-Identifier: GPL-3.0-or-later

//! The no-trap check of a compiled level (L-74): from every place and state a player reaches
//! from the start, the exit can still be reached, the property AG(EF(exit)) of Mawhorter and
//! Smith's softlock detection (FDG 2021). The player's moves are those a player always has
//! and no more, so a level that passes cannot hold a trap:
//!
//! - walking, up a step of at most `STEPSIZE` (18, PHYS-8.3), and falling, which is one way;
//! - swimming in any direction in a liquid, and climbing out over a rim at most
//!   [`qs_map_gen::CLIMB_OUT`] above its surface (`CheckWaterJump`, measured by
//!   `water_escape`);
//! - a door opens to a player with its key (`DOOR_SILVER_KEY` 16, `DOOR_GOLD_KEY` 8,
//!   `doors.qc`), and any other door when touched; a key is picked up where it lies.
//!
//! No jump is taken, so a ledge or a rail the player could jump over counts as a wall. Lava
//! kills before a player climbs out (`client.qc`, `WaterMove`), so it is a sink: walking must
//! never lead into it, which a rail along it ensures.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "test code: a failed unwrap or index is a failed test, and map units fit"
)]

use std::collections::{HashMap, VecDeque};

use qs_bsp::Bsp;
use qs_map::{Entity, MapFile};
use qs_sim::collision::{Hull, HullSet};

/// `CONTENTS_SOLID`, `CONTENTS_WATER` and `CONTENTS_LAVA` (BSP-10.2): the liquids are water,
/// slime and lava, -3 to -5.
const SOLID: i32 = -2;
const WATER: i32 = -3;
const LAVA: i32 = -5;
const LIQUIDS: std::ops::RangeInclusive<i32> = LAVA..=WATER;
/// How far apart a walk's paces are, the height a pace climbs (`STEPSIZE`, PHYS-8.3), and how
/// finely heights are tried.
const PACE: f32 = 16.0;
const CLIMB: f32 = 18.0;
const FINE: f32 = 2.0;
/// The player's box below and above its origin, and its middle above the origin, where
/// `SV_CheckWater` finds it waist deep (VEC_HULL_MIN, VEC_HULL_MAX, PHYS-8.4).
const FEET: f32 = 24.0;
const HEAD: f32 = 32.0;
const WAIST: f32 = 4.0;
/// How far a player may be from a place and have reached it.
const NEAR: f32 = 48.0;
/// `spawnflags` of a door that needs the silver or the gold key (`doors.qc`).
const SILVER_DOOR: u32 = 16;
const GOLD_DOOR: u32 = 8;

/// The keys a player holds, a bit for each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
struct Held(u8);

/// A key, by the bit it holds in [`Held`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Key {
    Silver,
    Gold,
}

impl Key {
    const fn bit(self) -> u8 {
        match self {
            Self::Silver => 1,
            Self::Gold => 2,
        }
    }

    /// The key a door's `spawnflags` ask for.
    fn of_door(spawnflags: u32) -> Option<Self> {
        if spawnflags & SILVER_DOOR != 0 {
            Some(Self::Silver)
        } else if spawnflags & GOLD_DOOR != 0 {
            Some(Self::Gold)
        } else {
            None
        }
    }

    /// The key an item's classname is (`items.qc`).
    fn of_item(classname: &[u8]) -> Option<Self> {
        match classname {
            b"item_key1" => Some(Self::Silver),
            b"item_key2" => Some(Self::Gold),
            _ => None,
        }
    }
}

impl Held {
    const fn has(self, key: Key) -> bool {
        self.0 & key.bit() != 0
    }

    const fn with(self, key: Key) -> Self {
        Self(self.0 | key.bit())
    }
}

/// A door that a key opens: its brush model and the key.
struct Locked {
    model: usize,
    key: Key,
}

/// What a compiled level is to a moving player.
pub(crate) struct Ground<'a> {
    hulls: HullSet,
    bsp: &'a Bsp<'a>,
    locked: Vec<Locked>,
    keys: Vec<([f32; 3], Key)>,
    /// The world's bounds: past its sides and under its bottom is the void, as good as solid
    /// to a walker.
    bounds: [[f32; 3]; 2],
}

/// What a check of a level found wrong.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Trap {
    /// The exit is reached from nowhere the start reaches.
    NoExit,
    /// A place the start reaches, with the keys held there, from which the exit is not.
    Stuck([i32; 3]),
    /// Walking reaches lava.
    Lava([i32; 3]),
}

fn origin(entity: &Entity) -> [f32; 3] {
    entity.origin().map(|at| at as f32)
}

impl<'a> Ground<'a> {
    /// The ground of `bsp`, compiled from `map`: brush entities are its models from 1 in the
    /// map's order.
    pub(crate) fn new(bsp: &'a Bsp<'a>, map: &MapFile) -> Self {
        let mut locked = Vec::new();
        let mut keys = Vec::new();
        let mut model: usize = 0;
        for entity in &map.entities {
            let classname = entity.classname().unwrap_or_default();
            if entity.has_brushes() {
                if classname != b"worldspawn" {
                    model = model.saturating_add(1);
                }
                if classname == b"func_door" {
                    let flags = entity
                        .value(b"spawnflags")
                        .and_then(|text| std::str::from_utf8(text).ok()?.parse().ok())
                        .unwrap_or(0);
                    if let Some(key) = Key::of_door(flags) {
                        locked.push(Locked { model, key });
                    }
                }
            } else if let Some(key) = Key::of_item(classname) {
                keys.push((origin(entity), key));
            }
        }
        let world = &bsp.models[0];
        Self {
            hulls: HullSet::new(bsp),
            bsp,
            locked,
            keys,
            bounds: [world.mins, world.maxs],
        }
    }

    fn hull(&self, index: usize, model: usize) -> (Hull<'_>, i32) {
        let [h0, h1, h2, _] = self.bsp.models[model].headnode;
        let heads = [h0, h1, h2];
        (self.hulls.hull(index, heads), heads[index])
    }

    /// Whether the player's box at `at` is in the world's solid or in a door `held` does not
    /// open.
    fn blocked(&self, held: Held, at: [f32; 3]) -> bool {
        // Past the sides or under the bottom: nothing moves up but a swimmer, so the top
        // needs no bound, and a map open to the sky stands under none.
        let [[x0, y0, z0], [x1, y1, _]] = self.bounds;
        let [x, y, z] = at;
        if x < x0 || x > x1 || y < y0 || y > y1 || z < z0 {
            return true;
        }
        let (world, head) = self.hull(1, 0);
        if world.point_contents(head, at).unwrap() == SOLID {
            return true;
        }
        self.locked.iter().any(|door| {
            !held.has(door.key) && {
                let (hull, head) = self.hull(1, door.model);
                hull.point_contents(head, at).unwrap() == SOLID
            }
        })
    }

    /// The contents of the point `at`, as the world's drawn hull has them.
    fn contents(&self, at: [f32; 3]) -> i32 {
        let (hull, head) = self.hull(0, 0);
        hull.point_contents(head, at).unwrap()
    }

    /// Whether the player's middle at `at` is in a liquid: waist deep (PHYS-8.4).
    fn swimming(&self, at: [f32; 3]) -> bool {
        LIQUIDS.contains(&self.contents([at[0], at[1], at[2] + WAIST]))
    }

    /// Whether any of the player's box at `at` is in lava, from the feet up.
    fn in_lava(&self, at: [f32; 3]) -> bool {
        let feet = at[2] - FEET + 1.0;
        let mut z = feet;
        while z <= at[2] + HEAD {
            if self.contents([at[0], at[1], z]) == LAVA {
                return true;
            }
            z += PACE;
        }
        false
    }

    /// Where the box at `[x, y, z]` comes to rest falling from there: on a floor, or at the
    /// first height in a liquid it falls into, since a swimmer is then held up.
    fn fall(&self, held: Held, [x, y, mut z]: [f32; 3]) -> [f32; 3] {
        while !self.blocked(held, [x, y, z - FINE]) && !self.swimming([x, y, z]) {
            z -= FINE;
        }
        [x, y, z]
    }

    /// Where a player at `from` stands after a pace along `step` that climbs at most `climb`;
    /// `None` when a wall is in the way.
    fn paced(&self, held: Held, from: [f32; 3], step: [f32; 2], climb: f32) -> Option<[f32; 3]> {
        let (x, y) = (from[0] + step[0], from[1] + step[1]);
        let mut z = from[2] + climb;
        // The head must clear what the feet climb.
        if self.blocked(held, [from[0], from[1], z]) {
            z = from[2];
        }
        while self.blocked(held, [x, y, z]) {
            z -= FINE;
            if z < from[2] {
                return None;
            }
        }
        Some(self.fall(held, [x, y, z]))
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

    /// The places a player at `at` holding `held` moves to in one move.
    fn moves(&self, held: Held, at: [f32; 3]) -> Vec<[f32; 3]> {
        let mut next = Vec::new();
        let flat = [[PACE, 0.0], [-PACE, 0.0], [0.0, PACE], [0.0, -PACE]];
        if self.swimming(at) {
            // A swimmer climbs out onto ground whose top is at most CLIMB_OUT above the
            // surface: a box standing there has its origin FEET above that.
            let reach = self.surface(at) + qs_map_gen::CLIMB_OUT as f32 + FEET - at[2];
            for step in flat {
                if let Some(to) = self.paced(held, at, step, reach.max(0.0)) {
                    next.push(to);
                }
            }
            for dz in [PACE, -PACE] {
                let to = [at[0], at[1], at[2] + dz];
                if !self.blocked(held, to) {
                    next.push(to);
                }
            }
        } else {
            for step in flat {
                if let Some(to) = self.paced(held, at, step, CLIMB) {
                    next.push(to);
                }
            }
        }
        next
    }

    /// The keys picked up at `at`.
    fn picked(&self, held: Held, at: [f32; 3]) -> Held {
        self.keys.iter().fold(held, |held, &(place, key)| {
            if near(at, place) {
                held.with(key)
            } else {
                held
            }
        })
    }

    /// Checks that from every place and state reached from `start` the exit, near `exit`, can
    /// still be reached, and that walking reaches no lava.
    pub(crate) fn check(&self, start: [f32; 3], exit: [f32; 3]) -> Result<(), Trap> {
        type Node = ([i32; 3], Held);
        let key = |at: [f32; 3]| [at[0] as i32, at[1] as i32, (at[2] / 8.0) as i32];
        let first = self.fall(Held::default(), start);
        let start: Node = (key(first), self.picked(Held::default(), first));
        let mut places = HashMap::from([(start, first)]);
        let mut back: HashMap<Node, Vec<Node>> = HashMap::new();
        let mut queue = VecDeque::from([start]);
        let mut exits = Vec::new();
        while let Some(node) = queue.pop_front() {
            let (_, held) = node;
            let at = places[&node];
            if self.in_lava(at) {
                return Err(Trap::Lava(node.0));
            }
            if near(at, exit) {
                exits.push(node);
            }
            for to in self.moves(held, at) {
                let next: Node = (key(to), self.picked(held, to));
                back.entry(next).or_default().push(node);
                if let std::collections::hash_map::Entry::Vacant(slot) = places.entry(next) {
                    slot.insert(to);
                    queue.push_back(next);
                }
            }
        }
        if exits.is_empty() {
            return Err(Trap::NoExit);
        }
        let mut leads: std::collections::HashSet<Node> = exits.iter().copied().collect();
        let mut queue: VecDeque<Node> = exits.into_iter().collect();
        while let Some(node) = queue.pop_front() {
            for &from in back.get(&node).into_iter().flatten() {
                if leads.insert(from) {
                    queue.push_back(from);
                }
            }
        }
        let mut stuck: Vec<[i32; 3]> = places
            .keys()
            .filter(|node| !leads.contains(node))
            .map(|(at, _)| *at)
            .collect();
        stuck.sort_unstable();
        stuck.first().map_or(Ok(()), |&at| Err(Trap::Stuck(at)))
    }
}

/// Whether `at` is within [`NEAR`] of `place` on each axis.
fn near(at: [f32; 3], place: [f32; 3]) -> bool {
    at.iter().zip(place).all(|(a, p)| (a - p).abs() <= NEAR)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::panic,
        clippy::unwrap_in_result,
        reason = "test code: a panic or a failed unwrap is a failed test"
    )]

    use qs_bsp::{Bsp, BspLoader};
    use qs_map::{Brush, Item, MapFile, Surface, TexDef};
    use qs_map_gen::{Classes, Request, Size};

    use super::{Ground, Trap};
    use crate::water_escape::{DEPTH, POOL, pool};

    /// Where a player stands on the ground west of [`pool`]'s pool, and where the exit is.
    const BESIDE: [f32; 3] = [-96.0, 0.0, 24.0];
    const EXIT: [f32; 3] = [-400.0, 0.0, 24.0];

    fn compiled(map: &[u8]) -> Bsp<'static> {
        let compiled = qs_map_bsp::Compiler::default().compile(map, None).unwrap();
        let bytes: &'static [u8] = Box::leak(compiled.bsp.into_boxed_slice());
        Bsp::read(bytes, BspLoader::default()).unwrap()
    }

    fn checked(map: &[u8], start: [f32; 3]) -> Result<(), Trap> {
        let file = MapFile::parse(map).unwrap();
        let bsp = compiled(map);
        Ground::new(&bsp, &file).check(start, EXIT)
    }

    /// A player in the middle of the pool, on its bottom.
    fn in_the_pool() -> [f32; 3] {
        [(POOL[0] / 2.0) as f32, 0.0, (-DEPTH + 25.0) as f32]
    }

    // No spec statement: L-74, a pool whose rim is at most CLIMB_OUT above its surface is no
    // trap, and one whose rim is higher is
    #[test]
    fn a_pool_with_a_high_rim_is_a_trap() {
        let rim = qs_map_gen::CLIMB_OUT;
        assert_eq!(checked(&pool("*water1", rim), in_the_pool()), Ok(()));
        let found = checked(&pool("*water1", rim + 4.0), in_the_pool());
        assert!(
            matches!(found, Err(Trap::Stuck(_) | Trap::NoExit)),
            "{found:?}"
        );
    }

    // No spec statement: L-74, a dry pit deeper than a step, with no way out, is a trap: a
    // player who walks in from beside it is stuck
    #[test]
    fn a_pit_with_no_way_out_is_a_trap() {
        let mut file = MapFile::parse(&pool("*water1", 24.0)).unwrap();
        // The liquid is the world's last brush.
        file.entities[0].items.pop();
        let found = checked(&file.to_bytes(), BESIDE);
        assert!(matches!(found, Err(Trap::Stuck(_))), "{found:?}");
    }

    /// [`pool`] of lava, with a rail around its edge when `railed`.
    fn lava(railed: bool) -> Vec<u8> {
        let mut file = MapFile::parse(&pool("*lava1", 24.0)).unwrap();
        if railed {
            let rock = Surface {
                texture: b"rock".to_vec(),
                texdef: TexDef::Standard {
                    shift: [0.0; 2],
                    rotate: 0.0,
                    scale: [1.0; 2],
                },
            };
            let [px, py] = POOL;
            let rails = [
                ([-8.0, -py - 8.0, 0.0], [0.0, py + 8.0, 32.0]),
                ([px, -py - 8.0, 0.0], [px + 8.0, py + 8.0, 32.0]),
                ([0.0, py, 0.0], [px, py + 8.0, 32.0]),
                ([0.0, -py - 8.0, 0.0], [px, -py, 32.0]),
            ];
            for (min, max) in rails {
                file.entities[0]
                    .items
                    .push(Item::Brush(Brush::cuboid(min, max, &rock).unwrap()));
            }
        }
        file.to_bytes()
    }

    // No spec statement: L-74, lava a walking player can reach is a trap, and lava behind a rail
    // higher than a step is not
    #[test]
    fn lava_beside_a_route_is_a_trap_unless_railed() {
        let found = checked(&lava(false), BESIDE);
        assert!(matches!(found, Err(Trap::Lava(_))), "{found:?}");
        assert_eq!(checked(&lava(true), BESIDE), Ok(()));
    }

    // No spec statement: L-74, a level whose locked door's key is taken away is a trap: the
    // exit is behind the door
    #[test]
    fn a_level_without_its_key_is_a_trap() {
        let textures: Vec<String> = ["wall", "floor", "ceiling", "trim", "door", "trigger"]
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        let level = (0..64)
            .map(|seed| {
                Request {
                    seed,
                    size: Size::Small,
                    textures: &textures,
                    classes: &Classes::Any,
                    name: "random",
                }
                .generate()
                .unwrap()
            })
            .find(|level| level.locked)
            .unwrap_or_else(|| panic!("no seed locks a door"));
        let start = level.rooms[0].stand.map(|at| at as f32);
        let exit = level.rooms[level.exit].stand.map(|at| at as f32);
        let check = |map: &MapFile| {
            let bsp = compiled(&map.to_bytes());
            Ground::new(&bsp, map).check(start, exit)
        };
        assert_eq!(check(&level.map), Ok(()));
        let mut keyless = MapFile::new();
        for entity in &level.map.entities {
            if entity.classname() != Some(b"item_key1".as_slice()) {
                keyless.push_entity(entity.clone());
            }
        }
        assert_eq!(check(&keyless), Err(Trap::NoExit));
    }
}
