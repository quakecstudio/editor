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

// spec BRIDGE-4.9: the player is moved as a teleporter moves it: the origin, the angles with
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

// spec BRIDGE-4.4: an edit held while paused is checked when it is asked for: a value of
// another type, or naming nothing, is refused then, and checking writes nothing
#[test]
fn checking_a_field_value_writes_nothing() {
    let run = running();
    let progs = Progs::read(&run.progs).unwrap();
    let check = |name: &str, value: QcValue| run.world.check_field_value(&progs, 1, name, &value);
    assert!(check("health", QcValue::Float(7.0)).is_ok());
    assert!(check("netname", QcValue::String(b"x".to_vec())).is_ok());
    assert!(check("think", QcValue::Function("tick".to_owned())).is_ok());
    assert!(matches!(
        check("think", QcValue::Function("x".to_owned())),
        Err(EditError::BadValue(_))
    ));
    assert!(matches!(
        check("health", QcValue::Vector([0.0; 3])),
        Err(EditError::BadValue(_))
    ));
    assert!(matches!(
        check("owner", QcValue::Entity(9000)),
        Err(EditError::BadValue(_))
    ));
    assert_eq!(
        run.world
            .check_field_value(&progs, 500, "health", &QcValue::Float(1.0)),
        Err(EditError::NoSuchEdict(500))
    );
    assert_eq!(run.world.float(1, "health"), 100.0);
}

// spec WORLD-8.4: a line traced through the world stops at the first entity's box or the
// world it meets; the world and an empty line name no edict
#[test]
fn a_line_finds_the_entity_it_hits() {
    let more = "void() info_box = { self.solid = 2; \
        setsize(self, '-8 -8 -8', '8 8 8'); setorigin(self, self.origin); };\n";
    let mut run = player_from(
        &ledge(),
        player_progs("self.health = 100;", "", more),
        LoadOptions::default(),
    );
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    let pairs = [
        ("classname".to_owned(), "info_box".to_owned()),
        ("origin".to_owned(), "50 0 40".to_owned()),
    ];
    let n = run.world.spawn_entity(&progs, &pairs).unwrap();
    let hit = run
        .world
        .trace_line(&progs, [50.0, -100.0, 40.0], [50.0, 100.0, 40.0])
        .unwrap();
    assert_eq!(hit.edict, Some(n));
    assert!((hit.point.unwrap()[1] + 8.0).abs() < 0.1, "{:?}", hit.point);
    let over = run
        .world
        .trace_line(&progs, [50.0, -100.0, 200.0], [50.0, 100.0, 200.0])
        .unwrap();
    assert_eq!(over.edict, None);
    assert_eq!(over.point, None, "nothing met");
    let down = run
        .world
        .trace_line(&progs, [50.0, -50.0, 100.0], [50.0, -50.0, -100.0])
        .unwrap();
    assert_eq!(down.edict, None, "the floor is the world");
    assert!(down.point.unwrap()[2].abs() < 0.1, "{:?}", down.point);
}

/// A world with spawn functions that precache, and two that link by `target`.
fn precaching() -> Running {
    let more = "entity(entity start, .string fld, string match) find = #18;\n\
        void() monster_known = { precache_model(\"progs/player.mdl\"); \
            setmodel(self, \"progs/player.mdl\"); };\n\
        void() monster_new = { precache_model(\"progs/new.mdl\"); };\n\
        void() ambient_new = { precache_sound(\"new.wav\"); };\n\
        void() info_target = {};\n\
        void() link_target = { self.enemy = find(world, targetname, self.target); };\n\
        void() func_linked = { self.nextthink = time + 0.1; self.think = link_target; };\n";
    player_from(
        &ledge(),
        player_progs("self.health = 100;", "", more),
        LoadOptions::default(),
    )
}

// spec BRIDGE-4.12: a spawn in play runs its function as at the map's load, so a precache of
// a name the map has works; a name the map has not is refused, the edict freed and the
// precache lists left as they were
#[test]
fn a_spawn_in_play_precaches_as_the_map_did() {
    let mut run = precaching();
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    let engine = run.world.builtins.mock.engine();
    assert!(
        engine
            .model_precache
            .iter()
            .any(|m| m == "progs/player.mdl")
    );
    let lists = (engine.model_precache.clone(), engine.sound_precache.clone());
    let known = [("classname".to_owned(), "monster_known".to_owned())];
    let n = run.world.spawn_entity(&progs, &known).unwrap();
    assert!(run.world.in_use(n));
    assert_ne!(
        run.world.float(n, "modelindex"),
        0.0,
        "setmodel took the model"
    );
    let edicts = run.num_edicts();
    for (classname, name) in [("monster_new", "progs/new.mdl"), ("ambient_new", "new.wav")] {
        let pairs = [("classname".to_owned(), classname.to_owned())];
        assert_eq!(
            run.world.spawn_entity(&progs, &pairs),
            Err(EditError::NotPrecached(name.to_owned()))
        );
        let engine = run.world.builtins.mock.engine();
        assert_eq!(engine.model_precache, lists.0);
        assert_eq!(engine.sound_precache, lists.1);
        let freed = usize::try_from(edicts).unwrap();
        assert!(!run.world.in_use(freed), "{classname} was freed");
    }
}

// spec BRIDGE-4.12: entities spawned in play link by target as the map's do: the think a
// spawn function sets runs on the next frames and finds an entity spawned after it
#[test]
fn entities_spawned_in_play_link_by_target() {
    let mut run = precaching();
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    let linked = [
        ("classname".to_owned(), "func_linked".to_owned()),
        ("target".to_owned(), "t1".to_owned()),
    ];
    let a = run.world.spawn_entity(&progs, &linked).unwrap();
    let target = [
        ("classname".to_owned(), "info_target".to_owned()),
        ("targetname".to_owned(), "t1".to_owned()),
    ];
    let b = run.world.spawn_entity(&progs, &target).unwrap();
    run.frames(3, 0.1).unwrap();
    let value = i32::try_from(b).unwrap() * run.edict_size();
    assert_eq!(run.world.entity_field(a, "enemy"), value);
}
