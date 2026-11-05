// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_progs::Progs;

use super::EditError;
use crate::LoadOptions;
use crate::world::QcValue;
use crate::world::tests::{Running, ledge, player_from, player_progs};

fn running() -> Running {
    let more = ".void() think;\nvoid() tick = {};\n\
        void() info_thing = { self.health = 5; };\n";
    player_from(
        &ledge(),
        player_progs("self.health = 100;", "", more),
        LoadOptions::default(),
    )
}

// spec BRIDGE-4.4: a field is written with a value of its type; another type is refused and
// nothing is written
// spec BRIDGE-4.5: an edict number not in use is refused
#[test]
fn a_field_is_written_with_a_value_of_its_type() {
    let mut run = running();
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    run.world
        .set_field_value(&progs, 1, "health", &QcValue::Float(7.0))
        .unwrap();
    assert_eq!(run.world.float(1, "health"), 7.0);
    run.world
        .set_field_value(&progs, 1, "think", &QcValue::Function("tick".to_owned()))
        .unwrap();
    run.world
        .set_field_value(&progs, 1, "origin", &QcValue::Vector([1.0, 2.0, 3.0]))
        .unwrap();
    assert_eq!(run.world.vector(1, "origin"), [1.0, 2.0, 3.0]);
    let wrong = run
        .world
        .set_field_value(&progs, 1, "health", &QcValue::Vector([0.0; 3]));
    assert!(matches!(wrong, Err(EditError::BadValue(_))));
    assert_eq!(run.world.float(1, "health"), 7.0, "nothing written");
    let unknown = run
        .world
        .set_field_value(&progs, 1, "nonesuch", &QcValue::Float(1.0));
    assert_eq!(unknown, Err(EditError::NoSuchField("nonesuch".to_owned())));
    let free = run
        .world
        .set_field_value(&progs, 500, "health", &QcValue::Float(1.0));
    assert_eq!(free, Err(EditError::NoSuchEdict(500)));
    let nowhere = run
        .world
        .set_field_value(&progs, 1, "think", &QcValue::Function("x".to_owned()));
    assert!(matches!(nowhere, Err(EditError::BadValue(_))));
}

// spec EDICT-8.6: a spawned entity's key-values are applied, then its classname's function
// runs with self the edict; a classname naming no function frees it
#[test]
fn an_entity_spawns_as_a_map_entity_does() {
    let mut run = running();
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    let pairs = [
        ("classname".to_owned(), "info_thing".to_owned()),
        ("origin".to_owned(), "8 16 24".to_owned()),
    ];
    let n = run.world.spawn_entity(&progs, &pairs).unwrap();
    assert_eq!(run.world.float(n, "health"), 5.0);
    assert_eq!(run.world.vector(n, "origin"), [8.0, 16.0, 24.0]);
    let nothing = [("classname".to_owned(), "info_nothing".to_owned())];
    assert!(matches!(
        run.world.spawn_entity(&progs, &nothing),
        Err(EditError::Spawn(_))
    ));
    run.world
        .edit_entity(&progs, n, &[("health".to_owned(), "9".to_owned())])
        .unwrap();
    assert_eq!(run.world.float(n, "health"), 9.0);
    run.world.remove_entity(&progs, n).unwrap();
    assert!(!run.world.in_use(n));
    assert_eq!(
        run.world.remove_entity(&progs, n),
        Err(EditError::NoSuchEdict(n))
    );
    assert_eq!(run.world.remove_entity(&progs, 0), Err(EditError::Kept(0)));
    assert_eq!(run.world.remove_entity(&progs, 1), Err(EditError::Kept(1)));
}

// spec BRIDGE-4.1: the player is moved as a teleporter moves it: the origin, the angles with
// fixangle, and no velocity
#[test]
fn the_player_moves_as_a_teleporter_moves_it() {
    let mut run = running();
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    run.world
        .move_player(&progs, [32.0, 16.0, 40.0], [0.0, 90.0, 0.0])
        .unwrap();
    assert_eq!(run.world.vector(1, "origin"), [32.0, 16.0, 40.0]);
    assert_eq!(run.world.vector(1, "angles"), [0.0, 90.0, 0.0]);
    assert_eq!(run.world.float(1, "fixangle"), 1.0);
}
