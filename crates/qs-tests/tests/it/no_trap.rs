// SPDX-License-Identifier: GPL-3.0-or-later

//! The no-trap check of a generated level (L-74), the shipped check of
//! `qs_workbench::map_playable` with the generator's moves: from every place and state a player
//! reaches from the start, the exit can still be reached, and walking reaches no lava. The
//! generator's moves take no jump, so a ledge or a rail the player could jump over counts as a
//! wall, and a level that passes holds no trap a jump would hide.

#![expect(
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::unwrap_in_result,
    reason = "test code: a panic, a failed unwrap or index is a failed test, and map units fit"
)]

use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Item, MapFile, Surface, TexDef};
use qs_map_gen::{Classes, Level, LockKind, Request, Size};
use qs_workbench::map_filter::Skill;
use qs_workbench::map_playable::{Ground, Moves, Trap};

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
    let bsp = compiled(map);
    Ground::new(&bsp, Moves::Generator, Skill::Normal)
        .unwrap()
        .check(start, EXIT)
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
        matches!(found, Err(Trap::Stuck(..) | Trap::NoExit)),
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
    assert!(matches!(found, Err(Trap::Stuck(..))), "{found:?}");
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

/// The first generated level, from seed 0 up, whose mission has a lock of `kind`.
fn level_with(kind: LockKind) -> Level {
    let textures: Vec<String> = ["wall", "floor", "ceiling", "trim", "door", "trigger"]
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    (0..64)
        .map(|seed| {
            Request {
                seed,
                size: Size::Medium,
                textures: &textures,
                classes: &Classes::Any,
                name: "random",
            }
            .generate()
            .unwrap()
        })
        .find(|level| level.report.locks.contains(&kind))
        .unwrap_or_else(|| panic!("no seed has a {kind:?}"))
}

/// Whether `level`, and `level` without its entities of `classname`, are trapped.
fn with_and_without(level: &Level, classname: &[u8]) -> (Result<(), Trap>, Result<(), Trap>) {
    let start = level.rooms[0].stand.map(|at| at as f32);
    let exit = level.rooms[level.exit].stand.map(|at| at as f32);
    let check = |map: &MapFile| {
        let bsp = compiled(&map.to_bytes());
        Ground::new(&bsp, Moves::Generator, Skill::Normal)
            .unwrap()
            .check(start, exit)
    };
    let mut without = MapFile::new();
    for entity in &level.map.entities {
        if entity.classname() != Some(classname) {
            without.push_entity(entity.clone());
        }
    }
    (check(&level.map), check(&without))
}

// No spec statement: L-74, a level whose locked door's key is taken away is a trap: the
// exit is behind the door
#[test]
fn a_level_without_its_key_is_a_trap() {
    for (kind, key) in [
        (LockKind::SilverKey, b"item_key1".as_slice()),
        (LockKind::GoldKey, b"item_key2".as_slice()),
    ] {
        let (with, without) = with_and_without(&level_with(kind), key);
        assert_eq!(with, Ok(()), "{kind:?}");
        assert_eq!(without, Err(Trap::NoExit), "{kind:?}");
    }
}

// No spec statement: L-74, a level whose button, or whose arena's counter, is taken away is
// a trap: the door they open stays shut
#[test]
fn a_level_without_what_opens_a_door_is_a_trap() {
    for (kind, opener) in [
        (LockKind::Button, b"func_button".as_slice()),
        (LockKind::Arena, b"trigger_counter".as_slice()),
    ] {
        let (with, without) = with_and_without(&level_with(kind), opener);
        assert_eq!(with, Ok(()), "{kind:?}");
        assert_eq!(without, Err(Trap::NoExit), "{kind:?}");
    }
}

// No spec statement: L-74, a level whose lift on the way to the exit is taken away is a
// trap, and the level with its lifts is not
#[test]
fn a_level_without_its_lift_is_a_trap() {
    let textures: Vec<String> = ["wall", "floor", "ceiling", "trim", "door", "trigger"]
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let broken = (0..64).find(|&seed| {
        let level = Request {
            seed,
            size: Size::Medium,
            textures: &textures,
            classes: &Classes::Any,
            name: "random",
        }
        .generate()
        .unwrap();
        let lifted = level
            .map
            .entities
            .iter()
            .any(|entity| entity.classname() == Some(b"func_plat".as_slice()));
        lifted && {
            let (with, without) = with_and_without(&level, b"func_plat");
            assert_eq!(with, Ok(()), "seed {seed}");
            without == Err(Trap::NoExit)
        }
    });
    assert!(
        broken.is_some(),
        "no level's lift is on the way to its exit"
    );
}
