// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_edict::EdictNumber;
use qc_progs::Progs;

use super::MapPlace;
use crate::world::tests::{Running, entity, ledge, running};

/// A thing that stays, one that removes itself in its spawn, and one that removes itself and
/// spawns a helper in its edict.
const SOURCE: &str = "void() info_thing = { self.health = 5; };\n\
    void() info_gone = { remove(self); };\n\
    void() info_helped = { remove(self); local entity e; e = spawn(); e.classname = \"helper\"; };\n";

/// The map: the world, then a thing, a self-removing one, a thing that takes its edict, an
/// entity with no classname, a thing that takes that one's, and one whose helper takes its
/// edict in its own spawn.
fn map() -> Running {
    let classless = "{\n\"origin\" \"0 0 0\"\n}\n";
    let entities = [
        entity("info_thing", "0 0 0", ""),
        entity("info_gone", "0 0 0", ""),
        entity("info_thing", "8 0 0", ""),
        classless.to_owned(),
        entity("info_thing", "16 0 0", ""),
        entity("info_helped", "24 0 0", ""),
    ]
    .concat();
    running(&ledge(), SOURCE, &entities, Vec::new())
}

fn edicts(run: &Running, count: usize) -> Vec<Option<usize>> {
    let progs = Progs::read(&run.progs).unwrap();
    (0..count)
        .map(|place| run.world.map_entity(&progs, MapPlace::new(place)))
        .collect()
}

// spec BRIDGE-4.13: each map entity is the edict its spawn left in use with its classname; one
// its spawn function freed, inhibited or classless is in no edict, and an edict a later entity
// or a helper took is not the earlier entity's
#[test]
fn each_map_entity_is_the_edict_it_spawned_in() {
    let run = map();
    assert_eq!(
        edicts(&run, 8),
        [Some(0), Some(2), None, Some(3), None, Some(4), None, None],
        "the client is edict 1; edicts are reused at once in the map's first second"
    );
    assert_eq!(
        run.world.map_classnames(),
        [
            "worldspawn",
            "info_thing",
            "info_gone",
            "info_thing",
            "",
            "info_thing",
            "info_helped"
        ]
    );
}

// spec BRIDGE-4.13: an entity freed since it spawned is in no edict, and stays so when another
// takes its edict; the list moves with an entity put in or taken out
#[test]
fn a_freed_entity_is_in_no_edict() {
    let mut run = map();
    run.frames(30, 0.1).unwrap();
    let progs_bytes = run.progs.clone();
    let progs = Progs::read(&progs_bytes).unwrap();
    run.world.remove_entity(&progs, 3).unwrap();
    assert_eq!(run.world.map_entity(&progs, MapPlace::new(3)), None);
    run.frames(10, 0.1).unwrap();
    let pairs = [("classname".to_owned(), "info_thing".to_owned())];
    let again = run.world.spawn_entity(&progs, &pairs).unwrap();
    assert_eq!(again, 3, "the freed edict is reused after half a second");
    assert_eq!(
        run.world.map_entity(&progs, MapPlace::new(3)),
        None,
        "the reused edict is not the old entity"
    );

    run.world.insert_map_entity(
        MapPlace::new(1),
        "info_thing",
        Some(EdictNumber::new(again)),
    );
    assert_eq!(run.world.map_entity(&progs, MapPlace::new(1)), Some(3));
    assert_eq!(
        run.world.map_entity(&progs, MapPlace::new(2)),
        Some(2),
        "the later entities move one on"
    );
    assert_eq!(run.world.take_map_entity(&progs, MapPlace::new(2)), Some(2));
    assert_eq!(
        run.world.map_entity(&progs, MapPlace::new(2)),
        None,
        "the self-removed one is next"
    );
    run.world
        .insert_map_entity(MapPlace::new(99), "light", None);
    assert_eq!(run.world.map_classnames().last().unwrap(), "light");
    let last = MapPlace::new(run.world.map_classnames().len() - 1);
    assert_eq!(run.world.take_map_entity(&progs, last), None);
    assert_eq!(run.world.take_map_entity(&progs, MapPlace::new(99)), None);
}
