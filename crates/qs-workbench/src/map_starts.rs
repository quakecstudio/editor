// SPDX-License-Identifier: GPL-3.0-or-later

//! The player starts of a coop or deathmatch map (L-88, MAP-39): what would spoil the game
//! before it is played, as `map.issues` lists it for the game `preview.set_game` chose. Each
//! finding quotes what id1's QuakeC does then, from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73
//! `qcc/v101qc/client.qc` (`SelectSpawnPoint`, `PutClientInServer`) and `triggers.qc`
//! (`spawn_tdeath`, `tdeath_touch`):
//!
//! - too few `info_player_deathmatch`: `SelectSpawnPoint` takes them in turn, so players share
//!   one;
//! - no `info_player_coop` in coop: every player falls back to the `info_player_start`;
//! - a start whose player box is in a world brush: the player is stuck there;
//! - two starts of one game whose boxes `spawn_tdeath` reaches across: one telefrags the other.

use qs_map::{Brush, Entity, MapFile};
use qs_map_edit::{EntityIndex, EntityProblem, QuickFix};

use crate::{GameMode, GameOptions};

/// The players a deathmatch map is checked for when deathmatch is not the game chosen: four.
pub const DEATHMATCH_PLAYERS: usize = 4;

/// The player's box, `VEC_HULL_MIN` and `VEC_HULL_MAX` in id1's `defs.qc`.
const HULL_MIN: [f64; 3] = [-16.0, -16.0, -24.0];
const HULL_MAX: [f64; 3] = [16.0, 16.0, 32.0];
/// How far above its spot `PutClientInServer` puts the player: `spot.origin + '0 0 1'`.
const SPAWN_LIFT: f64 = 1.0;
/// How much bigger than the player's box `spawn_tdeath` makes its trigger: `mins - '1 1 1'`
/// to `maxs + '1 1 1'`.
const TDEATH_GROW: f64 = 1.0;
/// How far up a start in solid is looked for room, then down, a unit at a time.
const ROOM_SEARCH: u8 = 64;
/// How far from a start another place for it is looked for, in each of eight directions.
const STEP_ASIDE: f64 = 64.0;
/// How far below its feet a new place must have a floor, so a start is not put in the air.
const FLOOR_REACH: f64 = 16.0;
/// The eight directions a place beside a start is looked for in.
const ASIDE: [[f64; 2]; 8] = [
    [1.0, 0.0],
    [-1.0, 0.0],
    [0.0, 1.0],
    [0.0, -1.0],
    [1.0, 1.0],
    [-1.0, 1.0],
    [1.0, -1.0],
    [-1.0, -1.0],
];

/// The entities a player spawns at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartClass {
    /// `info_player_start`, single player's, and coop's when there is no coop start.
    Start,
    /// `info_player_start2`, the start with a rune (`serverflags`).
    Start2,
    /// `info_player_coop`.
    Coop,
    /// `info_player_deathmatch`.
    Deathmatch,
    /// `testplayerstart`, a regioned level's.
    Test,
}

impl StartClass {
    /// Every class, in the order `SelectSpawnPoint` names them.
    const ALL: [Self; 5] = [
        Self::Test,
        Self::Coop,
        Self::Deathmatch,
        Self::Start2,
        Self::Start,
    ];

    const fn classname(self) -> &'static str {
        match self {
            Self::Start => "info_player_start",
            Self::Start2 => "info_player_start2",
            Self::Coop => "info_player_coop",
            Self::Deathmatch => "info_player_deathmatch",
            Self::Test => "testplayerstart",
        }
    }

    /// The class `entity` is, when it is a start.
    fn of(entity: &Entity) -> Option<Self> {
        let classname = entity.classname()?;
        Self::ALL
            .into_iter()
            .find(|class| class.classname().as_bytes() == classname)
    }
}

/// A start of the map.
#[derive(Clone, Copy, Debug)]
struct Start {
    entity: EntityIndex,
    line: u32,
    class: StartClass,
    origin: [f64; 3],
    /// The way it faces, its yaw.
    angle: f64,
}

impl Start {
    /// The box of a player put at this start, a box `grow` bigger each way.
    fn player_box(self, grow: f64) -> [[f64; 3]; 2] {
        player_box_at(self.origin, grow)
    }

    /// Whether a player spawning at one start telefrags one standing at the other: the
    /// spawner's `spawn_tdeath` trigger touches the other's box.
    fn telefrags(self, other: Self) -> bool {
        overlap(self.player_box(TDEATH_GROW), other.player_box(0.0))
    }
}

/// The box of a player put at a start at `origin`, `grow` bigger each way.
fn player_box_at(origin: [f64; 3], grow: f64) -> [[f64; 3]; 2] {
    let [x, y, z] = origin;
    let [lx, ly, lz] = HULL_MIN;
    let [hx, hy, hz] = HULL_MAX;
    let z = z + SPAWN_LIFT;
    [
        [x + lx - grow, y + ly - grow, z + lz - grow],
        [x + hx + grow, y + hy + grow, z + hz + grow],
    ]
}

/// Whether two boxes share a volume; boxes that only touch do not.
fn overlap([a_lo, a_hi]: [[f64; 3]; 2], [b_lo, b_hi]: [[f64; 3]; 2]) -> bool {
    a_lo.iter().zip(&b_hi).all(|(a, b)| a < b) && b_lo.iter().zip(&a_hi).all(|(b, a)| b < a)
}

/// A solid world brush: its faces' planes and its bounds.
struct Solid {
    planes: Vec<([f64; 3], f64)>,
    bounds: [[f64; 3]; 2],
}

impl Solid {
    /// `brush` as the player's hull meets it: none for one with no volume or a liquid, whose
    /// textures start with `*`, which a player stands in.
    fn of(brush: &Brush) -> Option<Self> {
        if !brush.is_valid()
            || brush
                .faces
                .iter()
                .all(|face| face.texture().starts_with(b"*"))
        {
            return None;
        }
        let corners = brush.vertices();
        if corners.is_empty() {
            return None;
        }
        let axis = |i: usize, pick: fn(f64, f64) -> f64, start: f64| {
            corners
                .iter()
                .filter_map(|corner| corner.get(i).copied())
                .fold(start, pick)
        };
        let bounds = [
            core::array::from_fn(|i| axis(i, f64::min, f64::INFINITY)),
            core::array::from_fn(|i| axis(i, f64::max, f64::NEG_INFINITY)),
        ];
        let planes = brush
            .faces
            .iter()
            .filter_map(qs_map::Face::plane)
            .map(|plane| (plane.normal, plane.dist))
            .collect();
        Some(Self { planes, bounds })
    }

    /// Whether `area`, a box, shares a volume with the brush: their bounds overlap, and no
    /// face's plane has the whole box outside it.
    fn holds(&self, area: [[f64; 3]; 2]) -> bool {
        overlap(self.bounds, area)
            && self.planes.iter().all(|&(normal, dist)| {
                // The corner of the box deepest behind the plane.
                let [lo, hi] = area;
                let deepest: f64 = normal
                    .iter()
                    .zip(lo.iter().zip(&hi))
                    .map(|(n, (lo, hi))| n * if *n > 0.0 { lo } else { hi })
                    .sum();
                deepest < dist
            })
    }
}

/// What a check reads of the map: its starts and its world's solid brushes.
struct Starts {
    starts: Vec<Start>,
    solids: Vec<Solid>,
}

impl Starts {
    fn of(map: &MapFile) -> Self {
        let starts = map
            .entities
            .iter()
            .enumerate()
            .filter_map(|(n, entity)| {
                Some(Start {
                    entity: EntityIndex::new(n),
                    line: entity.line(),
                    class: StartClass::of(entity)?,
                    origin: entity.origin(),
                    angle: entity.angles().map_or(0.0, |[_, yaw, _]| yaw),
                })
            })
            .collect();
        let solids = map
            .world()
            .map_or_default(|world| world.brushes().filter_map(Solid::of).collect());
        Self { starts, solids }
    }

    fn of_class(&self, class: StartClass) -> impl Iterator<Item = &Start> {
        self.starts.iter().filter(move |start| start.class == class)
    }

    /// Whether a player at `origin` would be in a world brush.
    fn in_solid(&self, origin: [f64; 3]) -> bool {
        let area = player_box_at(origin, 0.0);
        self.solids.iter().any(|solid| solid.holds(area))
    }

    /// Whether a player at `origin` would stand: there is a floor within [`FLOOR_REACH`] below
    /// its feet.
    fn stands(&self, origin: [f64; 3]) -> bool {
        let [[lx, ly, feet], [hx, hy, _]] = player_box_at(origin, 0.0);
        let below = [[lx, ly, feet - FLOOR_REACH], [hx, hy, feet]];
        self.solids.iter().any(|solid| solid.holds(below))
    }

    /// The nearest height above, then below, `start` within [`ROOM_SEARCH`] units where its
    /// player is out of solid.
    fn room_for(&self, start: Start) -> Option<[f64; 3]> {
        let [x, y, z] = start.origin;
        let heights = (1..=ROOM_SEARCH)
            .map(|n| z + f64::from(n))
            .chain((1..=ROOM_SEARCH).map(|n| z - f64::from(n)));
        heights
            .map(|at| [x, y, at])
            .find(|&origin| !self.in_solid(origin))
    }

    /// A place [`STEP_ASIDE`] from `origin` where a player stands out of solid and telefrags
    /// none of `others`.
    fn aside(&self, origin: [f64; 3], others: &[Start]) -> Option<[f64; 3]> {
        let [x, y, z] = origin;
        ASIDE
            .iter()
            .map(|[dx, dy]| [x + dx * STEP_ASIDE, y + dy * STEP_ASIDE, z])
            .find(|&at| {
                let placed = Start {
                    entity: EntityIndex::new(0),
                    line: 0,
                    class: StartClass::Coop,
                    origin: at,
                    angle: 0.0,
                };
                !self.in_solid(at)
                    && self.stands(at)
                    && !others
                        .iter()
                        .any(|other| placed.telefrags(*other) || other.telefrags(placed))
            })
    }
}

impl GameOptions {
    /// What would spoil the open map for this game (L-88): too few deathmatch starts for the
    /// players and bots, no coop start in coop, a start in solid, and two starts close enough to
    /// telefrag; each with a quick fix where one puts it right.
    #[must_use]
    pub fn start_problems(self, map: &MapFile) -> Vec<EntityProblem> {
        let found = Starts::of(map);
        let mut problems: Vec<EntityProblem> = self.deathmatch_count(&found).into_iter().collect();
        problems.extend(self.coop_start(&found));
        problems.extend(found.starts_in_solid());
        problems.extend(found.telefrags());
        problems
    }

    /// The players and bots this game has.
    fn clients(self) -> usize {
        usize::from(self.players).saturating_add(usize::from(self.bots))
    }

    /// Too few `info_player_deathmatch` on a deathmatch map, or one played as deathmatch: for
    /// the players and bots of a deathmatch chosen, else [`DEATHMATCH_PLAYERS`].
    fn deathmatch_count(self, found: &Starts) -> Option<EntityProblem> {
        let starts: Vec<&Start> = found.of_class(StartClass::Deathmatch).collect();
        let chosen = self.mode == GameMode::Deathmatch;
        if starts.is_empty() && !chosen {
            return None;
        }
        let players = if chosen {
            self.clients()
        } else {
            DEATHMATCH_PLAYERS
        };
        if starts.len() >= players {
            return None;
        }
        let message = if starts.is_empty() {
            "no info_player_deathmatch: in deathmatch id1's SelectSpawnPoint finds none \
             (`lastspawn = find(lastspawn, classname, \"info_player_deathmatch\")`) and puts \
             every player at the info_player_start (`spot = find (world, classname, \
             \"info_player_start\")`), so each one spawning telefrags the one there \
             (`spawn_tdeath`)"
                .to_owned()
        } else {
            format!(
                "{} info_player_deathmatch for {players} players and bots: id1's \
                 SelectSpawnPoint takes them in turn (`lastspawn = find(lastspawn, classname, \
                 \"info_player_deathmatch\")`), so players share a start and one spawning \
                 telefrags whoever stands on it (`spawn_tdeath`); add {} more",
                starts.len(),
                players.saturating_sub(starts.len())
            )
        };
        Some(EntityProblem {
            entity: starts.first().map(|start| start.entity),
            line: starts.first().map_or(0, |start| start.line),
            message,
            fix: None,
            point: None,
        })
    }

    /// No `info_player_coop` when coop is the game chosen; the fix places one beside the
    /// `info_player_start` where a player has room.
    fn coop_start(self, found: &Starts) -> Option<EntityProblem> {
        if self.mode != GameMode::Coop || found.of_class(StartClass::Coop).next().is_some() {
            return None;
        }
        let start = found.of_class(StartClass::Start).next().copied();
        let fix = start.and_then(|start| {
            let origin = found.aside(start.origin, &[start])?;
            Some(QuickFix::AddCoopStart {
                origin,
                angle: start.angle,
            })
        });
        Some(EntityProblem {
            entity: start.map(|start| start.entity),
            line: start.map_or(0, |start| start.line),
            message: "no info_player_coop: in coop id1's SelectSpawnPoint falls back to the \
                      info_player_start (`if (lastspawn == world) lastspawn = find (lastspawn, \
                      classname, \"info_player_start\");`), so every player spawns on it and \
                      telefrags the one there (`spawn_tdeath`)"
                .to_owned(),
            fix,
            point: None,
        })
    }
}

impl Starts {
    /// Each start whose player is in a world brush; the fix moves it to the nearest height
    /// with room.
    fn starts_in_solid(&self) -> Vec<EntityProblem> {
        self.starts
            .iter()
            .filter(|start| self.in_solid(start.origin))
            .map(|start| EntityProblem {
                entity: Some(start.entity),
                line: start.line,
                message: format!(
                    "this {} is in solid: id1's PutClientInServer puts the player at it \
                     (`self.origin = spot.origin + '0 0 1'`) with the box '-16 -16 -24' to \
                     '16 16 32', and a player there is stuck",
                    start.class.classname()
                ),
                fix: self.room_for(*start).map(|origin| QuickFix::MoveTo {
                    entity: start.entity,
                    origin,
                }),
                point: None,
            })
            .collect()
    }

    /// Each start of the deathmatch or the coop starts close enough to an earlier one of its
    /// class to telefrag a player on it; the fix moves it aside where it telefrags none.
    fn telefrags(&self) -> Vec<EntityProblem> {
        let mut problems = Vec::new();
        for class in [StartClass::Deathmatch, StartClass::Coop] {
            let starts: Vec<Start> = self.of_class(class).copied().collect();
            for (n, start) in starts.iter().enumerate() {
                let Some(earlier) = starts
                    .iter()
                    .take(n)
                    .find(|other| start.telefrags(**other) || other.telefrags(*start))
                else {
                    continue;
                };
                let others: Vec<Start> = starts
                    .iter()
                    .enumerate()
                    .filter(|(m, _)| *m != n)
                    .map(|(_, other)| *other)
                    .collect();
                problems.push(EntityProblem {
                    entity: Some(start.entity),
                    line: start.line,
                    message: format!(
                        "this {} is close enough to entity {}'s to telefrag: a player spawning \
                         at one (`spawn_tdeath (self.origin, self)`) kills whoever stands on the \
                         other; keep starts 33 units apart across or 57 up and down",
                        class.classname(),
                        earlier.entity.get()
                    ),
                    fix: self
                        .aside(start.origin, &others)
                        .map(|origin| QuickFix::MoveTo {
                            entity: start.entity,
                            origin,
                        }),
                    point: None,
                });
            }
        }
        problems
    }
}

#[cfg(test)]
mod tests;
