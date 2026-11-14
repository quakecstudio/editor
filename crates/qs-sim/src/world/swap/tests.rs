// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test code: a failed unwrap or match is a failed test"
)]

use qc_progs::Progs;
use qs_bsp::Bsp;

use super::{BrushModel, SwapError};
use crate::collision::CONTENTS_SOLID;
use crate::fixtures::{CONTENTS_EMPTY, MapSpec, bsp, node, plane};
use crate::world::tests::{Running, entity, running};
use crate::world::{MapPlace, QcValue, World};

/// A door's spawn: a pushed brush model, as `func_door` sets itself up.
const DOORS: &str = "void() door = { self.solid = 4; self.movetype = 7; \
    setmodel(self, self.model); setorigin(self, self.origin); };\n";

/// Door A, `*1`, at x = 0 to 16; door B, `*2`, at x = 32 to 64.
fn two_doors() -> MapSpec {
    MapSpec {
        submodels: vec![
            ([0.0; 3], [16.0; 3]),
            ([32.0, 0.0, 0.0], [64.0, 16.0, 16.0]),
        ],
        ..MapSpec::default()
    }
}

/// The map with the two doors running: edict 2 is door A, map entity 1, and edict 3 door B,
/// map entity 2.
fn doors() -> Running {
    let entities = [
        entity("door", "0 0 0", "\"model\" \"*1\"\n"),
        entity("door", "0 0 0", "\"model\" \"*2\"\n"),
    ]
    .concat();
    running(&two_doors(), DOORS, &entities, Vec::new())
}

/// The map after door A was deleted: door B is its one brush model, `*1`, now 24 high, and
/// its floor is 30 lower.
fn after_deleting_door_a() -> Bsp<'static> {
    bsp(&MapSpec {
        planes: vec![
            plane([0.0, 0.0, 1.0], -30.0, 2),
            plane([0.0, 0.0, 1.0], -6.0, 2),
        ],
        nodes: vec![node(0, [-2, -1])],
        submodels: vec![([32.0, 0.0, 0.0], [64.0, 16.0, 24.0])],
        ..MapSpec::default()
    })
}

fn model_of(world: &World, progs: &Progs<'_>, n: usize) -> Vec<u8> {
    let fields = world.edict_fields(progs, n).unwrap();
    match &fields.iter().find(|f| f.name == "model").unwrap().value {
        QcValue::String(text) => text.clone(),
        other => panic!("model is {other:?}"),
    }
}

// spec BRIDGE-4.16: each map entity in an edict takes its brush model in the new BSP, by its
// place in the map, not its old *n: its model, modelindex, mins, maxs and size as setmodel
// sets them; the result counts the edicts given a model
// spec BRIDGE-4.15: the swap keeps every edict's fields as they were
#[test]
fn a_renumbered_brush_entity_takes_its_new_model() {
    let mut game = doors();
    let progs = Progs::read(&game.progs).unwrap();
    game.world.set_float_field(3, "health", 75.0);
    let player = game.world.vector(1, "origin");
    assert_eq!(
        game.world.float(3, "modelindex"),
        3.0,
        "*2 is model index 3"
    );
    let new = after_deleting_door_a();
    let swapped = game
        .world
        .swap_bsp(
            &progs,
            &new,
            &[(MapPlace::new(2), BrushModel::parse("*1").unwrap())],
        )
        .unwrap();
    assert_eq!(swapped, 1);
    assert_eq!(model_of(&game.world, &progs, 3), b"*1");
    assert_eq!(
        game.world.float(3, "modelindex"),
        2.0,
        "*1 is model index 2"
    );
    assert_eq!(game.world.vector(3, "mins"), [32.0, 0.0, 0.0]);
    assert_eq!(game.world.vector(3, "maxs"), [64.0, 16.0, 24.0]);
    assert_eq!(game.world.vector(3, "size"), [32.0, 16.0, 24.0]);
    assert_eq!(game.world.float(3, "health"), 75.0);
    assert_eq!(game.world.vector(1, "origin"), player);
    assert_eq!(
        model_of(&game.world, &progs, 2),
        b"*1",
        "door A is not listed"
    );
}

// spec BRIDGE-4.15: the world's collision is the new BSP's, and the brush models' bounds
// setmodel reads are its too
// spec BRIDGE-4.16: every edict in use is linked again in the new collision
#[test]
fn the_world_collides_and_links_in_the_new_bsp() {
    let mut game = doors();
    let progs = Progs::read(&game.progs).unwrap();
    let below = [100.0, 100.0, -10.0];
    let collision = |world: &World| world.builtins.collision.true_point_contents(below).ok();
    assert_eq!(collision(&game.world), Some(CONTENTS_SOLID));
    game.world
        .swap_bsp(&progs, &after_deleting_door_a(), &[])
        .unwrap();
    assert_eq!(collision(&game.world), Some(CONTENTS_EMPTY));
    for door in [2, 3] {
        assert!(
            !game.world.builtins.collision.leafs_of(door).is_empty(),
            "{door}"
        );
    }
    let bounds = &game.world.builtins.mock.engine().model_bounds;
    let star = |name: &str| bounds.iter().filter(|(key, _)| key == name).count();
    assert_eq!((star("*1"), star("*2")), (1, 0));
    assert_eq!(star("maps/test.bsp"), 1);
}

// spec BRIDGE-4.14: a model past the new BSP's last brush model, or one the map never
// precached, is refused and nothing changes
#[test]
fn a_model_the_swap_cannot_give_is_refused() {
    let mut game = doors();
    let bytes = game.progs.clone();
    let progs = Progs::read(&bytes).unwrap();
    let three = bsp(&MapSpec {
        submodels: vec![([0.0; 3], [8.0; 3]); 3],
        ..MapSpec::default()
    });
    let star = |name| BrushModel::parse(name).unwrap();
    let refused = |world: &mut World, map: &Bsp<'_>, model| {
        world.swap_bsp(&progs, map, &[(MapPlace::new(2), star(model))])
    };
    assert_eq!(
        refused(&mut game.world, &after_deleting_door_a(), "*2"),
        Err(SwapError::NoSuchModel(star("*2")))
    );
    assert_eq!(
        refused(&mut game.world, &three, "*3"),
        Err(SwapError::NotPrecached(star("*3")))
    );
    assert_eq!(game.world.float(3, "modelindex"), 3.0);
    assert_eq!(game.world.vector(3, "maxs"), [64.0, 16.0, 16.0]);
    let below = [100.0, 100.0, -10.0];
    assert_eq!(
        game.world
            .builtins
            .collision
            .true_point_contents(below)
            .ok(),
        Some(CONTENTS_SOLID)
    );
}

// spec BRIDGE-4.14: a model is *n with n a whole number from 1, no leading zero
#[test]
fn brush_models_are_star_and_a_number() {
    assert_eq!(BrushModel::parse("*12").map(BrushModel::get), Some(12));
    assert_eq!(BrushModel::parse("*1").unwrap().name(), "*1");
    for bad in ["*0", "*01", "*", "1", "*1a", "maps/e1m1.bsp", "*-1"] {
        assert_eq!(BrushModel::parse(bad), None, "{bad}");
    }
}
