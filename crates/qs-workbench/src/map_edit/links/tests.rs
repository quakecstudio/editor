// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_project::FsLoader;
use qs_commands::Session;
use serde_json::json;

use crate::Workbench;
use crate::map_edit::tests::{project_with, text};
use crate::map_edit::{
    MAP_DUPLICATE, MAP_ENTITY, MAP_LINK, MAP_OPEN, MAP_REMOVE_KEY, MAP_SET_FLAG, MAP_SET_KEY,
};

/// QuakeC that fires entities by `target` and `killtarget`, and has a class with named
/// spawnflags.
const CODE: &str = "entity self, world; .string classname, target, killtarget, targetname;\n\
    .float spawnflags; float SILENT = 2;\n\
    entity(entity start, .string fld, string match) find = #18;\n\
    void() worldspawn = {};\n\
    void() fire = { local entity t; t = find(world, targetname, self.target);\n\
        t = find(world, targetname, self.killtarget); };\n\
    /*QUAKED path_corner (0.5 0.3 0) (-8 -8 -8) (8 8 8) START_ON\n*/\n\
    void() path_corner = { if (self.spawnflags & SILENT) fire(); };\n";

const MAP: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
    {\n\"classname\" \"path_corner\"\n\"targetname\" \"p1\"\n\"target\" \"p2\"\n}\n\
    {\n\"classname\" \"path_corner\"\n\"targetname\" \"p2\"\n\"target\" \"p3\"\n\
    \"killtarget\" \"gate\"\n}\n\
    {\n\"classname\" \"path_corner\"\n}\n";

/// The fixture's map at `maps/links.map`, open, in a project of [`CODE`].
fn open(name: &str) -> Session<Workbench<FsLoader>> {
    let (dir, mut session) = project_with(name, CODE);
    let path = dir.join("maps/links.map");
    std::fs::write(&path, MAP).unwrap();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

// No spec statement: P5-13, MAP-27: a duplicate's numbered targetname takes the next free
// number and its targeting keys (MAP-8.8) move on by as many; other keys stay; undo takes it
// back
#[test]
fn a_duplicate_numbers_its_names_onward() {
    let mut session = open("duplicate-numbers");
    session
        .execute(MAP_DUPLICATE, json!({ "entity": 2 }))
        .unwrap();
    assert!(text(&session).ends_with(
        "{\n\"classname\" \"path_corner\"\n\"targetname\" \"p3\"\n\"target\" \"p4\"\n\
         \"killtarget\" \"gate\"\n}\n"
    ));
    session
        .execute(MAP_DUPLICATE, json!({ "entity": 1 }))
        .unwrap();
    // p1 jumps to p4, past p2 and p3, so p1's target p2 moves on by as many.
    assert!(text(&session).ends_with(
        "{\n\"classname\" \"path_corner\"\n\"targetname\" \"p4\"\n\"target\" \"p5\"\n}\n"
    ));
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(text(&session), MAP);
}

// No spec statement: P5-13: a duplicate with no numbered targetname is copied as it is
#[test]
fn a_duplicate_with_no_numbered_name_is_a_plain_copy() {
    let mut session = open("duplicate-plain");
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 3, "key": "target", "value": "p9" }),
        )
        .unwrap();
    session
        .execute(MAP_DUPLICATE, json!({ "entity": 3 }))
        .unwrap();
    let map = session.context().map().unwrap();
    assert_eq!(map.file().entities[4].items, map.file().entities[3].items);
}

// No spec statement: P5-13: linking sets the key to the other entity's targetname, naming an
// unnamed one by its classname and the next free number, in one undo step
#[test]
fn linking_names_the_target_when_it_has_no_name() {
    let mut session = open("link");
    session
        .execute(
            MAP_LINK,
            json!({ "entity": 1, "key": "killtarget", "target": 2 }),
        )
        .unwrap();
    assert_eq!(
        session.context().map().unwrap().file().entities[1].value(b"killtarget"),
        Some(b"p2".as_slice())
    );
    session
        .execute(
            MAP_LINK,
            json!({ "entity": 2, "key": "target", "target": 3 }),
        )
        .unwrap();
    let map = session.context().map().unwrap();
    assert_eq!(
        map.file().entities[3].value(b"targetname"),
        Some(b"path_corner1".as_slice())
    );
    assert_eq!(
        map.file().entities[2].value(b"target"),
        Some(b"path_corner1".as_slice())
    );
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(text(&session), MAP);
    assert!(
        session
            .execute(
                MAP_LINK,
                json!({ "entity": 1, "key": "target", "target": 9 })
            )
            .is_err()
    );
    assert_eq!(text(&session), MAP, "a refused link changes nothing");
}

// No spec statement: P5-13, MAP-27: one set_key sets a key on several entities, undone at once;
// an entity it does not have refuses the whole edit
#[test]
fn a_key_is_set_on_several_entities_at_once() {
    let mut session = open("several");
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 1, "also": [2, 3], "key": "wait", "value": "2" }),
        )
        .unwrap();
    let map = session.context().map().unwrap();
    for entity in 1..4 {
        assert_eq!(
            map.file().entities[entity].value(b"wait"),
            Some(b"2".as_slice())
        );
    }
    session.undo().unwrap();
    assert_eq!(text(&session), MAP);
    assert!(
        session
            .execute(
                MAP_SET_KEY,
                json!({ "entity": 1, "also": [7], "key": "wait", "value": "2" }),
            )
            .is_err()
    );
    assert_eq!(text(&session), MAP);
}

// No spec statement: P5-13, MAP-27: map.entity gives the class's spawnflags as the QuakeC names
// them, for the spawnflag editor
#[test]
fn the_spawnflags_named_by_the_code_come_with_the_keys() {
    let mut session = open("flags");
    let keys = session.execute(MAP_ENTITY, json!({ "entity": 1 })).unwrap();
    assert_eq!(
        keys["spawnflags"],
        json!([{ "value": 1, "name": "START_ON" }, { "value": 2, "name": "SILENT" }])
    );
}

// No spec statement: P5-13: a name given by linking is the classname with the first number no
// entity has, even when the classname ends in a digit
#[test]
fn a_link_name_is_free_when_the_classname_ends_in_a_digit() {
    let mut session = open("link-digit");
    for entity in [1, 2] {
        session
            .execute(
                MAP_SET_KEY,
                json!({ "entity": entity, "key": "classname", "value": "item_armor2" }),
            )
            .unwrap();
        session
            .execute(
                MAP_REMOVE_KEY,
                json!({ "entity": entity, "key": "targetname" }),
            )
            .unwrap();
    }
    session
        .execute(
            MAP_LINK,
            json!({ "entity": 3, "key": "target", "target": 1 }),
        )
        .unwrap();
    session
        .execute(
            MAP_LINK,
            json!({ "entity": 3, "key": "killtarget", "target": 2 }),
        )
        .unwrap();
    let map = session.context().map().unwrap();
    assert_eq!(
        map.file().entities[1].value(b"targetname"),
        Some(b"item_armor21".as_slice())
    );
    assert_eq!(
        map.file().entities[2].value(b"targetname"),
        Some(b"item_armor22".as_slice())
    );
}

// No spec statement: P5-13: numbering onward keeps a number's zero padding
#[test]
fn numbering_onward_keeps_zero_padding() {
    let mut session = open("padding");
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 3, "key": "targetname", "value": "t09" }),
        )
        .unwrap();
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 3, "key": "target", "value": "t010" }),
        )
        .unwrap();
    session
        .execute(MAP_DUPLICATE, json!({ "entity": 3 }))
        .unwrap();
    let map = session.context().map().unwrap();
    assert_eq!(
        map.file().entities[4].value(b"targetname"),
        Some(b"t10".as_slice())
    );
    assert_eq!(
        map.file().entities[4].value(b"target"),
        Some(b"t011".as_slice())
    );
}

// No spec statement: P5-13, MAP-27: a spawnflag set or cleared on several entities changes only
// that bit of each entity's own flags, in one undo step; an entity it does not have refuses all
#[test]
fn a_flag_changes_only_its_bit_on_each_entity() {
    let mut session = open("flags-several");
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 2, "key": "spawnflags", "value": "256" }),
        )
        .unwrap();
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 3, "key": "spawnflags", "value": "3.0" }),
        )
        .unwrap();
    let before = text(&session);
    session
        .execute(
            MAP_SET_FLAG,
            json!({ "entity": 1, "also": [2, 3], "flag": 2, "change": "set" }),
        )
        .unwrap();
    let value = |session: &Session<Workbench<FsLoader>>, entity: usize| {
        let map = session.context().map().unwrap();
        String::from_utf8(
            map.file().entities[entity]
                .value(b"spawnflags")
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    };
    assert_eq!(value(&session, 1), "2");
    assert_eq!(value(&session, 2), "258");
    assert_eq!(value(&session, 3), "3");
    session
        .execute(
            MAP_SET_FLAG,
            json!({ "entity": 2, "flag": 256, "change": "clear" }),
        )
        .unwrap();
    assert_eq!(value(&session, 2), "2");
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(text(&session), before);
    assert!(
        session
            .execute(
                MAP_SET_FLAG,
                json!({ "entity": 1, "also": [9], "flag": 2, "change": "set" })
            )
            .is_err()
    );
    assert_eq!(text(&session), before);
}
