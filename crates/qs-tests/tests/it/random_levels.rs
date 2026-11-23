// SPDX-License-Identifier: GPL-3.0-or-later

//! The levels `qs-map-gen` makes, compiled by `qs-map-bsp` and walked in the player's hull
//! (L-48): none leaks, and a player who only walks, climbs steps and drops reaches every room
//! and the exit from the start.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    reason = "test code: a failed unwrap, panic or index is a failed test, and map units fit"
)]

use std::collections::{HashSet, VecDeque};

use qs_bsp::{Bsp, BspLoader};
use qs_map::{Diagnostic, DiagnosticKind};
use qs_map_gen::{Classes, Level, Request, Size};
use qs_sim::collision::{Hull, HullSet};

use crate::no_trap::Ground;

/// `CONTENTS_SOLID` (BSP-10.2).
const SOLID: i32 = -2;
/// How far a walk's steps are apart, the height it climbs (`STEPSIZE`, PHYS-8.3), and how
/// finely heights are tried.
const PACE: f32 = 16.0;
const CLIMB: f32 = 18.0;
const FINE: f32 = 2.0;
/// How far a walker may be from a place and have reached it.
pub(crate) const NEAR: f32 = 48.0;
/// How far behind a button's middle its wall is tried: past its 8 of depth.
const BEHIND: f64 = 16.0;
/// Below every level.
const BOTTOM: f32 = -1024.0;

/// The textures of id's levels a theme may name, and a liquid and a sky.
const ID_TEXTURES: &[&str] = &[
    "tech01_1",
    "sfloor4_1",
    "ceiling4",
    "tech03_1",
    "adoor01_2",
    "city4_7",
    "ground1_6",
    "wood1_5",
    "wmet2_1",
    "door02_2",
    "metal5_2",
    "metal5_8",
    "metal5_1",
    "rune1_1",
    "door01_2",
    "*lava1",
    "*slime0",
    "*water1",
    "sky1",
    "sky4",
    "trigger",
];
/// The same of LibreQuake's levels.
const LIBREQUAKE_TEXTURES: &[&str] = &[
    "med_csl_brk10b",
    "med_csl_flr1_1",
    "med_wood2",
    "med_csl_trm1",
    "med_door3b",
    "grk_wall1",
    "grk_flr4_2",
    "grk_bl_flt1",
    "grk_trim1",
    "grk_door3",
    "may_blok1_1",
    "may_flr1_1",
    "may_flt1_1",
    "may_trm1_1",
    "may_door2_1",
    "t_wall1ba",
    "t_flor1a",
    "t_metalsheeta",
    "t_trim1aa",
    "edoor01_1",
    "*lava1",
    "*slime1",
    "*water1",
    "sky5_blu",
    "trigger",
];

fn level(seed: u64, size: Size, textures: &[&str]) -> Level {
    let textures: Vec<String> = textures.iter().map(|name| (*name).to_owned()).collect();
    Request {
        seed,
        size,
        textures: &textures,
        classes: &Classes::Any,
        name: "random",
    }
    .generate()
    .unwrap()
}

/// Whether the player's box is in solid at `at`.
pub(crate) fn blocked(hull: &Hull<'_>, head: i32, at: [f32; 3]) -> bool {
    hull.point_contents(head, at).unwrap() == SOLID
}

/// Where a player at `from` stands after a pace along `step`: up a step of at most
/// [`CLIMB`], then down to the floor; `None` when a wall is in the way.
fn paced(hull: &Hull<'_>, head: i32, from: [f32; 3], step: [f32; 2]) -> Option<[f32; 3]> {
    let (x, y) = (from[0] + step[0], from[1] + step[1]);
    let mut z = from[2] + CLIMB;
    while blocked(hull, head, [x, y, z]) {
        z -= FINE;
        if z < from[2] {
            return None;
        }
    }
    // The head must clear what the feet climb: the box is tested whole at each height.
    while z > BOTTOM && !blocked(hull, head, [x, y, z - FINE]) {
        z -= FINE;
    }
    Some([x, y, z])
}

/// Every place a walking player reaches from `start`.
pub(crate) fn walked(hull: &Hull<'_>, head: i32, start: [f32; 3]) -> Vec<[f32; 3]> {
    let key = |at: [f32; 3]| (at[0] as i32, at[1] as i32, (at[2] / 8.0) as i32);
    let mut seen = HashSet::from([key(start)]);
    let mut queue = VecDeque::from([start]);
    let mut reached = Vec::new();
    while let Some(at) = queue.pop_front() {
        reached.push(at);
        for step in [[PACE, 0.0], [-PACE, 0.0], [0.0, PACE], [0.0, -PACE]] {
            if let Some(next) = paced(hull, head, at, step)
                && seen.insert(key(next))
            {
                queue.push_back(next);
            }
        }
    }
    reached
}

/// Checks `level` and compiles it: the map's own checks find nothing, it is sealed, and
/// every room's standing place, the exit's among them, is open to the player's box and walked
/// to from the start.
fn check(level: &Level, what: &str) {
    // The map's own checks find one thing: it names no WAD, since its textures are those of
    // the game's levels, which a compile in the IDE is given (P6-43).
    let found: Vec<Diagnostic> = level
        .map
        .check()
        .into_iter()
        .filter(|found| !matches!(found.kind, DiagnosticKind::NoWad))
        .collect();
    assert!(found.is_empty(), "{what}: {found:?}");
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&level.map.to_bytes(), None)
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    assert!(compiled.leak_trail.is_empty(), "{what}: leaks");
    assert!(
        compiled.portal_file.is_some(),
        "{what}: the outside was not filled"
    );
    let bsp = Bsp::read(&compiled.bsp, BspLoader::default()).unwrap();
    let hulls = HullSet::new(&bsp);
    let [h0, h1, h2, _] = bsp.models[0].headnode;
    let hull = hulls.hull(1, [h0, h1, h2]);
    let stand = |place: usize| level.rooms[place].stand.map(|at| at as f32);
    for (place, _) in level.rooms.iter().enumerate() {
        assert!(
            !blocked(&hull, h1, stand(place)),
            "{what}: room {place} stands in rock"
        );
    }
    // Every button is set in a wall: the rock behind it, the way it moves, is solid.
    let (drawn, head) = (hulls.hull(0, [h0, h1, h2]), h0);
    for entity in &level.map.entities {
        if entity.classname() != Some(b"func_button".as_slice()) {
            continue;
        }
        let points: Vec<[f64; 3]> = entity.brushes().flat_map(qs_map::Brush::vertices).collect();
        let middle =
            [0, 1, 2].map(|axis| points.iter().map(|p| p[axis]).sum::<f64>() / points.len() as f64);
        let angle: f64 = std::str::from_utf8(entity.value(b"angle").unwrap())
            .unwrap()
            .parse()
            .unwrap();
        let (dy, dx) = angle.to_radians().sin_cos();
        let behind =
            [middle[0] + dx * BEHIND, middle[1] + dy * BEHIND, middle[2]].map(|at| at as f32);
        assert_eq!(
            drawn.point_contents(head, behind).unwrap(),
            SOLID,
            "{what}: a button at {middle:?} is not set in a wall"
        );
    }
    let exit = stand(level.exit);
    let ground = Ground::new(&bsp, &level.map);
    assert_eq!(ground.check(stand(0), exit), Ok(()), "{what}: a trap");
    // A player who walks, swims, rides lifts, picks up keys and presses buttons.
    let reached = ground.reached(stand(0));
    for (place, room) in level.rooms.iter().enumerate() {
        let [x, y, z] = stand(place);
        let near = reached.iter().any(|at| {
            (at[0] - x).abs() <= NEAR && (at[1] - y).abs() <= NEAR && (at[2] - z).abs() <= NEAR
        });
        assert!(
            near,
            "{what}: room {place}, a {}, is not reached",
            room.shape.name()
        );
    }
}

/// Checks the levels of `seeds` in each size, painted from `textures`.
fn check_all(seeds: std::ops::Range<u64>, textures: &[&str]) {
    for seed in seeds {
        for size in [Size::Small, Size::Medium, Size::Large] {
            check(
                &level(seed, size, textures),
                &format!("seed {seed} {size:?}"),
            );
        }
    }
}

/// The seeds checked with each game's textures on every run, which a level's compile makes
/// the cost of. L-48's bar is 50 seeds in each size: `QS_RANDOM_LEVEL_SEEDS=25` checks 25
/// with each game's textures, 150 levels, and its run is recorded in the PR.
const SEEDS: u64 = 4;

/// How many seeds to check: [`SEEDS`], or what `QS_RANDOM_LEVEL_SEEDS` says.
fn seeds() -> u64 {
    std::env::var("QS_RANDOM_LEVEL_SEEDS")
        .ok()
        .and_then(|count| count.parse().ok())
        .unwrap_or(SEEDS)
}

// No spec statement: L-48, a generated level painted from id's textures compiles sealed, and a
// player who walks reaches every room and the exit from the start
#[test]
fn levels_of_ids_textures_are_sealed_and_walked() {
    check_all(0..seeds(), ID_TEXTURES);
}

// No spec statement: L-48, the same of a level painted from LibreQuake's textures
#[test]
fn levels_of_librequakes_textures_are_sealed_and_walked() {
    let seeds = seeds();
    check_all(seeds..seeds * 2, LIBREQUAKE_TEXTURES);
}
