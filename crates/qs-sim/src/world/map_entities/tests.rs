// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_edict::EdictNumber;
use qc_progs::Progs;

use super::MapPlace;
use crate::world::tests::{Running, entity, ledge, running};

/// A thing that stays, one that removes itself in its spawn, and a classless entity.
const SOURCE: &str =
    "void() info_thing = { self.health = 5; };\nvoid() info_gone = { remove(self); };\n";

/// The map: the world, then a thing, a self-removing one, a thing that takes its edict, an
/// entity with no classname and a thing that takes that one's.
fn map() -> Running {
    let classless = "{\n\"origin\" \"0 0 0\"\n}\n";
    let entities = [
        entity("info_thing", "0 0 0", ""),
        entity("info_gone", "0 0 0", ""),
        entity("info_thing", "8 0 0", ""),
        classless.to_owned(),
        entity("info_thing", "16 0 0", ""),
    ]
    .concat();
    running(&ledge(), SOURCE, &entities, Vec::new())
}

// spec BRIDGE-4.13: each map entity is the edict its spawn left in use; one its spawn function
// freed, inhibited or classless is in no edict, and an edict a later entity took is that one's
#[test]
fn each_map_entity_is_the_edict_it_spawned_in() {
    let run = map();
    let edicts: Vec<_> = (0..7)
        .map(|place| run.world.map_entity(MapPlace::new(place)))
        .collect();
    assert_eq!(
        edicts,
        [Some(0), Some(2), None, Some(3), None, Some(4), None],
        "the client is edict 1; edicts 3 and 4 are reused at once in the map's first second"
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
    assert_eq!(run.world.map_entity(MapPlace::new(3)), None);
    run.frames(10, 0.1).unwrap();
    let pairs = [("classname".to_owned(), "info_thing".to_owned())];
    let again = run.world.spawn_entity(&progs, &pairs).unwrap();
    assert_eq!(again, 3, "the freed edict is reused after half a second");
    assert_eq!(
        run.world.map_entity(MapPlace::new(3)),
        None,
        "the reused edict is not the old entity"
    );

    run.world
        .insert_map_entity(MapPlace::new(1), Some(EdictNumber::new(again)));
    assert_eq!(run.world.map_entity(MapPlace::new(1)), Some(3));
    assert_eq!(
        run.world.map_entity(MapPlace::new(2)),
        Some(2),
        "the later entities move one on"
    );
    assert_eq!(run.world.take_map_entity(MapPlace::new(2)), Some(2));
    assert_eq!(
        run.world.map_entity(MapPlace::new(2)),
        None,
        "the self-removed one is next"
    );
    run.world.insert_map_entity(MapPlace::new(99), None);
    assert_eq!(run.world.take_map_entity(MapPlace::new(99)), None);
}
