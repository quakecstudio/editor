// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/v101qc/client.qc, PlayerJump, PlayerPreThink

//! How high a ledge a player gets onto with a jump (L-77): id's `PlayerJump`, whose
//! `self.velocity_z = self.velocity_z + 270` lifts a player on the ground, run in `qs-sim` on
//! ledges compiled by `qs-map-bsp` at each height, the player running into the ledge with jump
//! held; and how wide a gap a running jump clears, the player running at it and jumping at its
//! edge. The playability check of a map (`qs_workbench::map_playable`) takes a player's jump as
//! [`qs_workbench::map_playable::JUMP`] and [`qs_workbench::map_playable::LEAP`].

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::rc::Rc;

use qc_progs::Progs;
use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Item, MapFile};
use qs_sim::{LoadOptions, UserCmd, World};
use qs_workbench::map_playable::{JUMP, LEAP};

use crate::shape_stairs::{EYE, FRAME};
use crate::water_escape::{progs, surface};

/// How far ahead of the player start the ledge's face is: just past the player's box, 16
/// across from its origin.
const FACE: f64 = 17.0;
/// How long the player has to get onto the ledge: three seconds of frames.
const FRAMES: usize = 216;
/// The frames the jump is released for first, so that `PlayerPreThink` sets
/// `FL_JUMPRELEASED` and `PlayerJump` takes the press after it.
const RELEASED: usize = 2;
/// The heights of ledge tried, every unit.
const LEDGES: std::ops::RangeInclusive<u32> = 30..=60;

/// A floor with a ledge `height` high across it, its face [`FACE`] east of the player start.
fn ledge(height: f64) -> Vec<u8> {
    let mut file = MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n\
          {\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n}\n",
    )
    .unwrap();
    let rock = surface("rock");
    let cuboid = |min: [f64; 3], max: [f64; 3]| Brush::cuboid(min, max, &rock).unwrap();
    let brushes = [
        cuboid([-256.0, -128.0, -16.0], [256.0, 128.0, 0.0]),
        cuboid([FACE, -128.0, 0.0], [256.0, 128.0, height]),
    ];
    file.entities[0]
        .items
        .extend(brushes.into_iter().map(Item::Brush));
    file.to_bytes()
}

/// Whether a player against a ledge `height` high, running into it with jump held, stands on
/// it within [`FRAMES`].
fn jumps_onto(height: f64) -> bool {
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&ledge(height), None)
        .unwrap();
    let bytes: &'static [u8] = Box::leak(compiled.bsp.into_boxed_slice());
    let bsp = Bsp::read(bytes, BspLoader::default()).unwrap();
    let progs_bytes = progs();
    let progs = Progs::read(&progs_bytes).unwrap();
    let mut world = World::load(
        &progs,
        "ledge",
        &bsp,
        Rc::new(|_: &str| None),
        LoadOptions::default(),
    )
    .unwrap();
    world.settle(&progs).unwrap();
    world.connect(&progs).unwrap();
    let run = UserCmd {
        forward: 320.0,
        ..UserCmd::default()
    };
    // On the ledge the origin is 24 above its top and the eye 22 above that.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the ledges are whole units"
    )]
    let on_top = (height as f32) + 24.0 + EYE - 1.0;
    for frame in 0..FRAMES {
        let buttons = if frame < RELEASED { 0 } else { 2 };
        world.set_command(UserCmd { buttons, ..run });
        world.frame(&progs, FRAME).unwrap();
        let (eye, _) = world.player_view().unwrap();
        if f64::from(eye[0]) > FACE + 16.0 && eye[2] > on_top {
            return true;
        }
    }
    false
}

// No spec statement: L-77, a jump (PlayerJump's 270 up, under sv_gravity 800, PHYS-8) takes
// a player onto a ledge up to JUMP high and not onto one a unit higher, and onto every ledge
// lower
#[test]
fn a_jump_gets_onto_a_ledge_up_to_jump_and_not_higher() {
    let climbed: Vec<u32> = LEDGES.filter(|&h| jumps_onto(f64::from(h))).collect();
    let highest = climbed.iter().copied().max().unwrap();
    assert_eq!(
        f64::from(highest),
        f64::from(JUMP),
        "ledges climbed: {climbed:?}"
    );
    let lowest = *LEDGES.start();
    assert_eq!(
        climbed,
        (lowest..=highest).collect::<Vec<_>>(),
        "every lower ledge is climbed too"
    );
}

/// How far back from a gap's edge the player starts, to be running at full speed there.
const RUN_UP: f64 = 256.0;
/// The widths of gap tried, every 4 units.
const GAPS: std::ops::RangeInclusive<u32> = 32..=64;
const GAP_STEP: f64 = 4.0;

/// A floor with a gap `width` wide across it, a pit below, its near edge at x 0 and the player
/// start [`RUN_UP`] back from it.
fn gap(width: f64) -> Vec<u8> {
    let mut file = MapFile::parse(
        format!(
            "{{\n\"classname\" \"worldspawn\"\n}}\n\
             {{\n\"classname\" \"info_player_start\"\n\"origin\" \"{} 0 24\"\n}}\n",
            -RUN_UP
        )
        .as_bytes(),
    )
    .unwrap();
    let rock = surface("rock");
    let cuboid = |min: [f64; 3], max: [f64; 3]| Brush::cuboid(min, max, &rock).unwrap();
    let brushes = [
        cuboid([-512.0, -128.0, -528.0], [512.0 + width, 128.0, -512.0]),
        cuboid([-512.0, -128.0, -16.0], [0.0, 128.0, 0.0]),
        cuboid([width, -128.0, -16.0], [512.0 + width, 128.0, 0.0]),
    ];
    file.entities[0]
        .items
        .extend(brushes.into_iter().map(Item::Brush));
    file.to_bytes()
}

/// Whether a player running at a gap `width` wide, and jumping as its box's middle reaches the
/// edge, lands on the far side.
fn leaps(width: f64) -> bool {
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&gap(width), None)
        .unwrap();
    let bytes: &'static [u8] = Box::leak(compiled.bsp.into_boxed_slice());
    let bsp = Bsp::read(bytes, BspLoader::default()).unwrap();
    let progs_bytes = progs();
    let progs = Progs::read(&progs_bytes).unwrap();
    let mut world = World::load(
        &progs,
        "gap",
        &bsp,
        Rc::new(|_: &str| None),
        LoadOptions::default(),
    )
    .unwrap();
    world.settle(&progs).unwrap();
    world.connect(&progs).unwrap();
    for _ in 0..FRAMES {
        let (eye, _) = world.player_view().unwrap();
        // Jump from the last moment the box still stands on the edge: its back 16 short of it.
        let buttons = if f64::from(eye[0]) >= 16.0 - GAP_STEP {
            2
        } else {
            0
        };
        world.set_command(UserCmd {
            forward: 320.0,
            buttons,
            ..UserCmd::default()
        });
        world.frame(&progs, FRAME).unwrap();
        let (eye, _) = world.player_view().unwrap();
        if f64::from(eye[0]) > width + 16.0 && eye[2] > 24.0 + EYE - 1.0 {
            return true;
        }
        if eye[2] < 0.0 {
            return false;
        }
    }
    false
}

// No spec statement: L-77, a running jump (PlayerJump's 270 up at sv_maxspeed 320, under
// sv_gravity 800, PHYS-9.2, PHYS-6.4) clears a gap up to LEAP wide and not one wider
#[test]
fn a_running_jump_clears_a_gap_up_to_leap_and_not_wider() {
    let cleared: Vec<u32> = GAPS.filter(|&g| leaps(f64::from(g) * GAP_STEP)).collect();
    let widest = cleared.iter().copied().max().unwrap();
    assert_eq!(
        f64::from(widest) * GAP_STEP,
        f64::from(LEAP),
        "gaps cleared: {cleared:?}"
    );
    assert_eq!(
        cleared,
        (*GAPS.start()..=widest).collect::<Vec<_>>(),
        "every narrower gap is cleared too"
    );
}
