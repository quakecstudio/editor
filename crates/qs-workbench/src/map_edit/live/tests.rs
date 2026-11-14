// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qs_map::MapFile;

use super::LiveChange;
use crate::map_edit::EntityIndex;

/// The world, a monster and a light.
const MAP: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
    {\n\"classname\" \"monster_army\"\n\"origin\" \"0 0 24\"\n\"angle\" \"90\"\n}\n\
    {\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n}\n";

fn changes(before: &str, after: &str) -> Vec<LiveChange> {
    let before = MapFile::parse(before.as_bytes()).unwrap();
    let after = MapFile::parse(after.as_bytes()).unwrap();
    LiveChange::between(&before.entities, &after.entities)
}

fn owned(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

// No spec statement: P5-07, MAP-12: an entity put in is spawned at its place with its keys
#[test]
fn an_entity_put_in_is_spawned_at_its_place() {
    let monster = "{\n\"classname\" \"monster_dog\"\n\"origin\" \"8 0 24\"\n}\n";
    let at_end = format!("{MAP}{monster}");
    assert_eq!(
        changes(MAP, &at_end),
        [LiveChange::Spawn {
            place: EntityIndex::new(3),
            classname: "monster_dog".to_owned(),
            pairs: owned(&[("origin", "8 0 24")]),
        }]
    );
    let (world, rest) = MAP.split_at(MAP.find("}\n").unwrap() + 2);
    let second = format!("{world}{monster}{rest}");
    assert_eq!(
        changes(MAP, &second),
        [LiveChange::Spawn {
            place: EntityIndex::new(1),
            classname: "monster_dog".to_owned(),
            pairs: owned(&[("origin", "8 0 24")]),
        }],
        "an undo of a delete puts the entity back where it was"
    );
    assert_eq!(
        changes(&second, MAP),
        [LiveChange::Remove {
            place: EntityIndex::new(1)
        }]
    );
}

// No spec statement: P5-07, MAP-12: an edit sends the keys that changed, a key taken out as
// empty, and nothing for an entity whose keys are the same
#[test]
fn an_edit_sends_the_keys_that_changed() {
    let moved = MAP.replace("\"0 0 24\"", "\"16 0 24\"");
    assert_eq!(
        changes(MAP, &moved),
        [LiveChange::Edit {
            place: EntityIndex::new(1),
            pairs: owned(&[("origin", "16 0 24")]),
        }]
    );
    let unturned = MAP.replace("\"angle\" \"90\"\n", "");
    assert_eq!(
        changes(MAP, &unturned),
        [LiveChange::Edit {
            place: EntityIndex::new(1),
            pairs: owned(&[("angle", "")]),
        }]
    );
    let targeted = MAP.replace("\"light\"\n", "\"light\"\n\"targetname\" \"t1\"\n");
    assert_eq!(
        changes(MAP, &targeted),
        [LiveChange::Edit {
            place: EntityIndex::new(2),
            pairs: owned(&[("targetname", "t1")]),
        }]
    );
    let respaced = MAP.replace("\"origin\" \"0 0 64\"", "\"origin\"  \"0 0 64\"");
    assert_eq!(
        changes(MAP, &respaced),
        [],
        "the same keys, written another way"
    );
    assert_eq!(changes(MAP, MAP), []);
}

// No spec statement: P5-07: lists that differ by more than one entity put in or take out the
// rest at the end
#[test]
fn lists_far_apart_change_at_the_end() {
    let world = "{\n\"classname\" \"worldspawn\"\n}\n";
    assert_eq!(
        changes(MAP, world),
        [
            LiveChange::Remove {
                place: EntityIndex::new(1)
            },
            LiveChange::Remove {
                place: EntityIndex::new(1)
            }
        ]
    );
    let spawned = changes(world, MAP);
    assert_eq!(spawned.len(), 2);
    assert!(matches!(
        spawned.last(),
        Some(LiveChange::Spawn { place, .. }) if *place == EntityIndex::new(2)
    ));
}
