// SPDX-License-Identifier: GPL-3.0-or-later

//! The player starts of a coop or deathmatch map (L-88, MAP-39): what would spoil the game
//! before it is played, as `map.issues` lists it for the game `preview.set_game` chose
//! (START-3). Each finding quotes what id1's QuakeC does then (START-1, START-2):
//!
//! - too few `info_player_deathmatch`: `SelectSpawnPoint` takes them in turn, so players share
//!   one;
//! - no `info_player_coop` in coop: every player falls back to the `info_player_start`;
//! - a start whose player box is in a solid brush: the player is stuck there;
//! - two starts of one group close enough that `spawn_tdeath` reaches across: one telefrags
//!   the other.

use qs_map::{Brush, Entity, MapFile};
use qs_map_edit::{EntityIndex, EntityProblem, QuickFix};

use crate::{GameMode, GameOptions};

/// The players a deathmatch map is checked for when deathmatch is not the game chosen: four
/// (START-3.1).
pub const DEATHMATCH_PLAYERS: usize = 4;

/// The player's box, `VEC_HULL_MIN` and `VEC_HULL_MAX` in id1's `defs.qc` (START-1.3).
const HULL_MIN: [f64; 3] = [-16.0, -16.0, -24.0];
const HULL_MAX: [f64; 3] = [16.0, 16.0, 32.0];
/// How far above its spot `PutClientInServer` puts the player: `spot.origin + '0 0 1'`
/// (START-1.3).
const SPAWN_LIFT: f64 = 1.0;
/// How much bigger than the player's box `spawn_tdeath` makes its trigger: `mins - '1 1 1'`
/// to `maxs + '1 1 1'` (START-2.1).
const TDEATH_GROW: f64 = 1.0;
/// How much bigger than its box `SV_LinkEdict` links an entity that is not an item
/// (START-2.2).
const LINK_GROW: f64 = 1.0;
/// How far up a start in solid is looked for room, then down, a unit at a time (START-3.3).
const ROOM_SEARCH: u8 = 64;
/// How far from a start another place for it is looked for, in each of eight directions
/// (START-3.2).
const STEP_ASIDE: f64 = 64.0;
/// How far below its feet a player's place must have a floor (START-3.5).
const FLOOR_REACH: f64 = 16.0;
/// The eight directions a place beside a start is looked for in (START-3.2).
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
/// The brush entities whose brushes are not solid to a player: `func_illusionary` and the
/// extended dialect's `func_detail_illusionary` (START-3.3).
const NOT_SOLID: [&[u8]; 2] = [b"func_illusionary", b"func_detail_illusionary"];
/// The classname prefix of the brush entities whose brushes are solid, `func_group` and
/// `func_detail` among them (START-3.3).
const SOLID_PREFIX: &[u8] = b"func_";
/// The axes of a player's box.
const BOX_AXES: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// The entities a player spawns at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StartClass {
    /// `info_player_start`, single player's, and coop's after the coop starts.
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

/// The starts players of one game take in turn (START-3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    /// The `info_player_deathmatch` (START-1.2).
    Deathmatch,
    /// The `info_player_coop` and the first `info_player_start` (START-1.1).
    Coop,
}

impl Group {
    const ALL: [Self; 2] = [Self::Deathmatch, Self::Coop];
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
    /// Whether a player spawning at this start telefrags one standing at `other`: the
    /// spawner's `spawn_tdeath` trigger and the other's box, each as `SV_LinkEdict` links it,
    /// touch (START-2.2).
    fn telefrags(self, other: [f64; 3]) -> bool {
        touch(
            player_box(self.origin, TDEATH_GROW + LINK_GROW),
            player_box(other, LINK_GROW),
        )
    }
}

/// The box of a player put at a start at `origin` (START-1.3), `grow` bigger each way.
fn player_box(origin: [f64; 3], grow: f64) -> [[f64; 3]; 2] {
    let [x, y, z] = origin;
    let [lx, ly, lz] = HULL_MIN;
    let [hx, hy, hz] = HULL_MAX;
    let z = z + SPAWN_LIFT;
    [
        [x + lx - grow, y + ly - grow, z + lz - grow],
        [x + hx + grow, y + hy + grow, z + hz + grow],
    ]
}

/// Whether two boxes touch or share a volume, as `SV_TouchLinks` tests them: neither is wholly
/// past the other (START-2.2).
fn touch([a_lo, a_hi]: [[f64; 3]; 2], [b_lo, b_hi]: [[f64; 3]; 2]) -> bool {
    a_lo.iter().zip(&b_hi).all(|(a, b)| a <= b) && b_lo.iter().zip(&a_hi).all(|(b, a)| b <= a)
}

/// The most two starts may be apart across, on x and on y, and up and down, and still
/// telefrag (START-2.2).
fn telefrag_reach() -> [f64; 2] {
    let [lx, _, lz] = HULL_MIN;
    let [hx, _, hz] = HULL_MAX;
    let grown = TDEATH_GROW + LINK_GROW + LINK_GROW;
    [hx - lx + grown, hz - lz + grown]
}

/// A box's eight corners.
fn corners([lo, hi]: [[f64; 3]; 2]) -> [[f64; 3]; 8] {
    let [lx, ly, lz] = lo;
    let [hx, hy, hz] = hi;
    [
        [lx, ly, lz],
        [hx, ly, lz],
        [lx, hy, lz],
        [hx, hy, lz],
        [lx, ly, hz],
        [hx, ly, hz],
        [lx, hy, hz],
        [hx, hy, hz],
    ]
}

fn dot([a, b, c]: [f64; 3], [x, y, z]: [f64; 3]) -> f64 {
    a * x + b * y + c * z
}

/// The least and greatest of `points` along `axis`.
fn span(points: &[[f64; 3]], axis: [f64; 3]) -> [f64; 2] {
    points
        .iter()
        .map(|&point| dot(point, axis))
        .fold([f64::INFINITY, f64::NEG_INFINITY], |[lo, hi], at| {
            [lo.min(at), hi.max(at)]
        })
}

/// A solid brush (START-3.3): its corners, and the axes a box is apart from it along when it
/// is: the box's, the brush's faces' normals and each brush edge crossed with each box axis.
struct Solid {
    corners: Vec<[f64; 3]>,
    axes: Vec<[f64; 3]>,
}

impl Solid {
    /// `brush` as a player's box meets it: none for one with no volume, or a liquid, whose
    /// faces' textures all start with `*`.
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
        let mut axes: Vec<[f64; 3]> = BOX_AXES.to_vec();
        axes.extend(
            brush
                .faces
                .iter()
                .filter_map(qs_map::Face::plane)
                .map(|plane| plane.normal),
        );
        for polygon in brush.polygons() {
            let next = polygon.iter().cycle().skip(1);
            for (&[fx, fy, fz], &[tx, ty, tz]) in polygon.iter().zip(next) {
                let [ex, ey, ez] = [tx - fx, ty - fy, tz - fz];
                for [ax, ay, az] in BOX_AXES {
                    let cross = [ey * az - ez * ay, ez * ax - ex * az, ex * ay - ey * ax];
                    if dot(cross, cross) > f64::EPSILON {
                        axes.push(cross);
                    }
                }
            }
        }
        Some(Self { corners, axes })
    }

    /// Whether `area`, a box, shares a volume with the brush: along no axis are the two apart
    /// or only touching.
    fn holds(&self, area: [[f64; 3]; 2]) -> bool {
        let area = corners(area);
        self.axes.iter().all(|&axis| {
            let [a_lo, a_hi] = span(&self.corners, axis);
            let [b_lo, b_hi] = span(&area, axis);
            a_lo < b_hi && b_lo < a_hi
        })
    }
}

/// Whether `entity`'s brushes are solid to a player: the world's and a `func_` brush
/// entity's, but for the illusionary ones (START-3.3).
fn solid_entity(entity: &Entity) -> bool {
    let Some(classname) = entity.classname() else {
        return false;
    };
    classname == b"worldspawn"
        || (classname.starts_with(SOLID_PREFIX) && !NOT_SOLID.contains(&classname))
}

/// What a check reads of the map: its starts and its solid brushes.
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
            .entities
            .iter()
            .filter(|entity| solid_entity(entity))
            .flat_map(Entity::brushes)
            .filter_map(Solid::of)
            .collect();
        Self { starts, solids }
    }

    fn of_class(&self, class: StartClass) -> impl Iterator<Item = &Start> {
        self.starts.iter().filter(move |start| start.class == class)
    }

    /// The starts of `group`, in the order `SelectSpawnPoint` takes them (START-3.4).
    fn group(&self, group: Group) -> Vec<Start> {
        match group {
            Group::Deathmatch => self.of_class(StartClass::Deathmatch).copied().collect(),
            Group::Coop => self
                .of_class(StartClass::Coop)
                .chain(self.of_class(StartClass::Start).take(1))
                .copied()
                .collect(),
        }
    }

    /// The starts `start` takes turns with: its group's, none for a start in no group.
    fn group_of(&self, start: Start) -> Vec<Start> {
        Group::ALL
            .into_iter()
            .map(|group| self.group(group))
            .find(|starts| starts.iter().any(|other| other.entity == start.entity))
            .unwrap_or_default()
    }

    /// Whether a player at `origin` would be in a solid brush (START-3.3).
    fn in_solid(&self, origin: [f64; 3]) -> bool {
        let area = player_box(origin, 0.0);
        self.solids.iter().any(|solid| solid.holds(area))
    }

    /// Whether a player at `origin` stands: a solid brush is within [`FLOOR_REACH`] below its
    /// feet (START-3.5).
    fn stands(&self, origin: [f64; 3]) -> bool {
        let [[lx, ly, feet], [hx, hy, _]] = player_box(origin, 0.0);
        let below = [[lx, ly, feet - FLOOR_REACH], [hx, hy, feet]];
        self.solids.iter().any(|solid| solid.holds(below))
    }

    /// Whether `origin` is a place for `start`: a player there is out of solid, stands, and
    /// telefrags none of `others` but itself, nor is telefragged by them.
    fn fits(&self, origin: [f64; 3], start: Start, others: &[Start]) -> bool {
        let placed = Start { origin, ..start };
        !self.in_solid(origin)
            && self.stands(origin)
            && !others.iter().any(|other| {
                other.entity != start.entity
                    && (placed.telefrags(other.origin) || other.telefrags(origin))
            })
    }

    /// The nearest height above, then below, `start` within [`ROOM_SEARCH`] units that fits it
    /// among `others` (START-3.3).
    fn room_for(&self, start: Start, others: &[Start]) -> Option<[f64; 3]> {
        let [x, y, z] = start.origin;
        let heights = (1..=ROOM_SEARCH)
            .map(|n| z + f64::from(n))
            .chain((1..=ROOM_SEARCH).map(|n| z - f64::from(n)));
        heights
            .map(|at| [x, y, at])
            .find(|&origin| self.fits(origin, start, others))
    }

    /// The first of the eight places [`STEP_ASIDE`] from `start` that fits it among `others`
    /// (START-3.2, START-3.4).
    fn aside(&self, start: Start, others: &[Start]) -> Option<[f64; 3]> {
        let [x, y, z] = start.origin;
        ASIDE
            .iter()
            .map(|[dx, dy]| [x + dx * STEP_ASIDE, y + dy * STEP_ASIDE, z])
            .find(|&at| self.fits(at, start, others))
    }

    /// Each start whose player is in a solid brush; the fix moves it to the nearest height
    /// with room (START-3.3).
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
                fix: self
                    .room_for(*start, &self.group_of(*start))
                    .map(|origin| QuickFix::MoveTo {
                        entity: start.entity,
                        origin,
                    }),
                point: None,
            })
            .collect()
    }

    /// Each start of a group close enough to an earlier one to telefrag; the fix moves it
    /// aside where it telefrags none (START-3.4).
    fn telefrags(&self) -> Vec<EntityProblem> {
        let [across, up] = telefrag_reach();
        let mut problems = Vec::new();
        for group in Group::ALL {
            let starts = self.group(group);
            for (n, start) in starts.iter().enumerate() {
                let Some(earlier) = starts
                    .iter()
                    .take(n)
                    .find(|other| start.telefrags(other.origin))
                else {
                    continue;
                };
                problems.push(EntityProblem {
                    entity: Some(start.entity),
                    line: start.line,
                    message: format!(
                        "this {} is close enough to entity {}'s {} to telefrag: a player \
                         spawning at one (`spawn_tdeath (self.origin, self)`) kills whoever \
                         stands on the other; keep starts more than {across} units apart \
                         across or more than {up} up and down",
                        start.class.classname(),
                        earlier.entity.get(),
                        earlier.class.classname()
                    ),
                    fix: self.aside(*start, &starts).map(|origin| QuickFix::MoveTo {
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

impl GameOptions {
    /// What would spoil the open map for this game (START-3): too few deathmatch starts for
    /// the players and bots, no coop start in coop, a start in solid, and two starts close
    /// enough to telefrag; each with a quick fix where one puts it right.
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

    /// Too few `info_player_deathmatch` on a deathmatch map, or one played as deathmatch, for
    /// the players and bots of a deathmatch chosen, else [`DEATHMATCH_PLAYERS`] (START-3.1).
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

    /// No `info_player_coop` when coop is the game chosen for more than one player and bot; the
    /// fix places one beside the `info_player_start` (START-3.2).
    fn coop_start(self, found: &Starts) -> Option<EntityProblem> {
        if self.mode != GameMode::Coop
            || self.clients() < 2
            || found.of_class(StartClass::Coop).next().is_some()
        {
            return None;
        }
        let start = found.of_class(StartClass::Start).next().copied();
        let fix = start.and_then(|start| {
            // A new entity, which is none of the map's.
            let coop = Start {
                entity: EntityIndex::new(usize::MAX),
                class: StartClass::Coop,
                ..start
            };
            let origin = found.aside(coop, &[start])?;
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

#[cfg(test)]
mod tests;
