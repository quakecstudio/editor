// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::{Value, json};

use super::{
    EntityIndex, MAP_DELETE, MAP_DUPLICATE, MAP_ENTITY, MAP_MOVE, MAP_OPEN, MAP_PLACE,
    MAP_REMOVE_KEY, MAP_ROTATE, MAP_SAVE, MAP_SELECT, MAP_SET_KEY,
};
use crate::{Config, OPEN_PROJECT, Workbench, register};

const CODE: &str = "entity self; .string classname, target, targetname; .float speed, angle;\n\
    .vector origin;\nvoid() worldspawn = {};\n\
    /*QUAKED monster_thing (1 0 0) (-16 -16 -24) (16 16 40)\n*/\n\
    void() monster_thing = { if (!self.speed) self.speed = 100; if (self.target) self.angle = 1; };\n";

const MAP: &str = "// a test map\n{\n\"classname\" \"worldspawn\"\n\"wad\" \"base.wad\"\n\
    {\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) floor 0 0 0 1 1\n}\n}\n\
    {\n\"classname\" \"monster_thing\"\n\"origin\" \"16 32 24\"\n\"angle\" \"350\"\n}\n\
    {\n\"classname\" \"func_door\"\n{\n\
    ( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) door 0 0 0 1 1\n\
    ( 0 0 64 ) ( 1 0 64 ) ( 0 1 64 ) door 0 0 0 1 1\n}\n}\n";

/// A fresh directory named `name` with a project of [`CODE`], `maps/test.map` holding
/// [`MAP`] and `maps/base.bsp` the spec's base vector; the project open.
fn project(name: &str) -> (PathBuf, Session<Workbench<FsLoader>>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-mapedit-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("a.qc"), CODE).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\na.qc\n").unwrap();
    std::fs::write(dir.join("maps/test.map"), MAP).unwrap();
    std::fs::write(dir.join("maps/base.bsp"), base_bsp()).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    (dir, session)
}

/// The spec's `base` BSP vector, from its `xxd` dump.
fn base_bsp() -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/spec/vectors/bsp/base.txt");
    let mut out = Vec::new();
    for line in std::fs::read_to_string(path).unwrap().lines() {
        let Some((_, rest)) = line.split_once(": ") else {
            continue;
        };
        for group in rest.get(..39).unwrap_or(rest).split_whitespace() {
            for pair in group.as_bytes().chunks(2) {
                out.push(u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap());
            }
        }
    }
    out
}

/// The open map's text as it would be saved.
fn text(session: &Session<Workbench<FsLoader>>) -> String {
    let map = session.context().map().unwrap();
    String::from_utf8(map.file().to_bytes()).unwrap()
}

// No spec statement: P5-06, MAP-4: every entity edit is a command with undo and redo, and the
// map saves as a valid .map that differs only where it was edited (MAP-7)
#[test]
fn every_entity_edit_undoes_and_the_map_saves() {
    let (dir, mut session) = project("edits");
    let path = dir.join("maps/test.map");
    let opened = session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    assert_eq!(opened["entities"], 3);
    assert_eq!(text(&session), MAP, "opening changes nothing");
    let edits: [(&str, Value, &str, &str); 7] = [
        (
            MAP_PLACE,
            json!({ "classname": "monster_thing", "origin": [8.0, -8.5, 0.0], "keys": [["speed", "50"]] }),
            "{\n\"classname\" \"func_door\"\n",
            "\n{\n\"classname\" \"monster_thing\"\n\"origin\" \"8 -8.5 0\"\n\"speed\" \"50\"\n}",
        ),
        (
            MAP_MOVE,
            json!({ "entity": 1, "by": [4.0, 0.0, -8.0] }),
            "\"origin\" \"16 32 24\"",
            "\"origin\" \"20 32 16\"",
        ),
        (
            MAP_MOVE,
            json!({ "entity": 2, "by": [0.0, 0.0, 8.0] }),
            "( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) door 0 0 0 1 1\n( 0 0 64 ) ( 1 0 64 ) ( 0 1 64 ) door",
            "( 0 0 8 ) ( 0 1 8 ) ( 1 0 8 ) door 0 0 0 1 1\n( 0 0 72 ) ( 1 0 72 ) ( 0 1 72 ) door",
        ),
        (
            MAP_ROTATE,
            json!({ "entity": 1, "degrees": 90.0 }),
            "\"angle\" \"350\"",
            "\"angle\" \"80\"",
        ),
        (
            MAP_SET_KEY,
            json!({ "entity": 1, "key": "target", "value": "t1" }),
            "\"angle\" \"350\"\n}",
            "\"angle\" \"350\"\n\"target\" \"t1\"\n}",
        ),
        (
            MAP_REMOVE_KEY,
            json!({ "entity": 1, "key": "angle" }),
            "\"angle\" \"350\"\n",
            "",
        ),
        (
            MAP_DELETE,
            json!({ "entity": 1 }),
            "{\n\"classname\" \"monster_thing\"\n\"origin\" \"16 32 24\"\n\"angle\" \"350\"\n}\n",
            "",
        ),
    ];
    for (command, params, old, new) in edits {
        let before = text(&session);
        session.execute(command, params.clone()).unwrap();
        let after = text(&session);
        if command == MAP_PLACE {
            assert_eq!(
                after,
                format!("{}{new}", before.trim_end_matches('\n')) + "\n"
            );
            assert!(after.contains(old));
        } else {
            assert_eq!(after, before.replacen(old, new, 1), "{command} {params}");
        }
        session.undo().unwrap();
        assert_eq!(text(&session), before, "{command} undoes");
        session.redo().unwrap();
        assert_eq!(text(&session), after, "{command} redoes");
        session.undo().unwrap();
    }
    assert!(!session.context().map().unwrap().is_unsaved() || text(&session) == MAP);
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 1, "key": "speed", "value": "200" }),
        )
        .unwrap();
    assert!(session.context().map().unwrap().is_unsaved());
    let saved = session.execute(MAP_SAVE, json!({})).unwrap();
    assert_eq!(saved["path"], json!(path));
    let written = std::fs::read(&path).unwrap();
    assert_eq!(
        String::from_utf8(written.clone()).unwrap(),
        MAP.replace(
            "\"angle\" \"350\"\n}",
            "\"angle\" \"350\"\n\"speed\" \"200\"\n}"
        )
    );
    let reread = qs_map::MapFile::parse(&written).unwrap();
    assert_eq!(reread.entities.len(), 3);
    assert!(!session.context().map().unwrap().is_unsaved());
}

// No spec statement: P5-06: edits that cannot be made change nothing: the world is never
// deleted, a brush entity or the world is not rotated, an entity the map does not have and a
// key holding a quote are refused, and every edit needs an open map
#[test]
fn edits_that_cannot_be_made_change_nothing() {
    let (dir, mut session) = project("refused");
    assert!(
        session.execute(MAP_DELETE, json!({ "entity": 1 })).is_err(),
        "no map"
    );
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    for (command, params) in [
        (MAP_DELETE, json!({ "entity": 0 })),
        (MAP_ROTATE, json!({ "entity": 0, "degrees": 90.0 })),
        (MAP_ROTATE, json!({ "entity": 2, "degrees": 90.0 })),
        (MAP_MOVE, json!({ "entity": 9, "by": [1.0, 0.0, 0.0] })),
        (
            MAP_SET_KEY,
            json!({ "entity": 1, "key": "a\"b", "value": "x" }),
        ),
        (MAP_SELECT, json!({ "entity": 9 })),
    ] {
        assert!(
            session.execute(command, params.clone()).is_err(),
            "{command} {params}"
        );
        assert_eq!(text(&session), MAP, "{command} changed nothing");
    }
    assert_eq!(session.undo_depth(), 0);
    assert!(
        session
            .execute(MAP_OPEN, json!({ "path": dir.join("maps/none.map") }))
            .is_err()
    );
}

// No spec statement: P5-06: the selection follows place and delete; an entity's keys are typed
// by its class, with the map's targetnames for its target keys
#[test]
fn the_selection_follows_and_keys_are_typed() {
    let (dir, mut session) = project("keys");
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    session.execute(MAP_SELECT, json!({ "entity": 2 })).unwrap();
    session.execute(MAP_DELETE, json!({ "entity": 1 })).unwrap();
    assert_eq!(
        session.context().map().unwrap().selected(),
        Some(EntityIndex::new(1))
    );
    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "info_null", "origin": [0.0, 0.0, 0.0], "keys": [["targetname", "t2"]] }),
        )
        .unwrap();
    assert_eq!(
        session.context().map().unwrap().selected(),
        Some(EntityIndex::new(2))
    );
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(
        session.context().map().unwrap().selected(),
        Some(EntityIndex::new(2))
    );
    session.redo().unwrap();
    session.redo().unwrap();
    session
        .execute(MAP_SELECT, json!({ "entity": null }))
        .unwrap();
    session.undo().unwrap();
    session.undo().unwrap();
    let keys = session.execute(MAP_ENTITY, json!({ "entity": 1 })).unwrap();
    assert_eq!(keys["classname"], "monster_thing");
    assert_eq!(
        keys["pairs"],
        json!([
            ["classname", "monster_thing"],
            ["origin", "16 32 24"],
            ["angle", "350"]
        ])
    );
    assert_eq!(
        keys["keys"],
        json!([
            { "name": "speed", "ty": "float", "default": "100" },
            { "name": "target", "ty": "string", "default": null },
        ])
    );
    assert_eq!(keys["brush"], false);
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 2, "key": "targetname", "value": "door" }),
        )
        .unwrap();
    let keys = session.execute(MAP_ENTITY, json!({ "entity": 2 })).unwrap();
    assert_eq!(keys["targetnames"], json!(["door"]));
    assert_eq!(
        keys["keys"],
        json!([]),
        "func_door is no class of this project"
    );
    assert_eq!(keys["brush"], true);
}

// No spec statement: P5-06, MAP-22: a shipped BSP's entities are edited and saved to a .ent
// override beside it, or into the BSP's entity lump
#[test]
fn a_shipped_maps_entities_save_to_an_ent_or_its_lump() {
    let (dir, mut session) = project("bsp");
    let bsp = dir.join("maps/base.bsp");
    let original = std::fs::read(&bsp).unwrap();
    session.execute(MAP_OPEN, json!({ "path": bsp })).unwrap();
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 0, "key": "message", "value": "Edited" }),
        )
        .unwrap();
    let saved = session.execute(MAP_SAVE, json!({})).unwrap();
    let ent = dir.join("maps/base.ent");
    assert_eq!(saved["path"], json!(ent));
    let ent_text = std::fs::read(&ent).unwrap();
    assert!(!ent_text.contains(&0), "a .ent holds no NUL");
    let entities = qs_map::MapFile::parse(&ent_text).unwrap();
    assert_eq!(entities.entities[0].value(b"message"), Some(&b"Edited"[..]));
    assert_eq!(
        std::fs::read(&bsp).unwrap(),
        original,
        "the BSP is untouched"
    );
    let saved = session.execute(MAP_SAVE, json!({ "bsp": "lump" })).unwrap();
    assert_eq!(saved["path"], json!(bsp));
    let rewritten = std::fs::read(&bsp).unwrap();
    let read = qs_bsp::Bsp::read(&rewritten, qs_bsp::BspLoader::default()).unwrap();
    assert_eq!(read.entities(), ent_text.as_slice());
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 0, "key": "message", "value": "Again" }),
        )
        .unwrap();
    session.execute(MAP_SAVE, json!({ "bsp": "lump" })).unwrap();
    let again = qs_bsp::Bsp::read(&std::fs::read(&bsp).unwrap(), qs_bsp::BspLoader::default())
        .unwrap()
        .entities()
        .to_vec();
    assert!(String::from_utf8_lossy(&again).contains("\"Again\""));
}

// No spec statement: P5-06: turning an entity with angles turns its yaw, kept from 0 to 360
#[test]
fn angles_turn_by_their_yaw() {
    let (dir, mut session) = project("angles");
    let path = dir.join("maps/angles.map");
    std::fs::write(
        &path,
        "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"info_null\"\n\"angles\" \"10 -90 5\"\n}\n\
         {\n\"classname\" \"info_null\"\n}\n",
    )
    .unwrap();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
        .execute(MAP_ROTATE, json!({ "entity": 1, "degrees": -45.0 }))
        .unwrap();
    session
        .execute(MAP_ROTATE, json!({ "entity": 2, "degrees": 450.0 }))
        .unwrap();
    assert!(
        text(&session).contains("\"angles\" \"10 225 5\""),
        "{}",
        text(&session)
    );
    assert!(
        text(&session).contains("\"angle\" \"90\""),
        "{}",
        text(&session)
    );
}

// No spec statement: P5-06: duplicating copies an entity's keys and brushes to the end of the
// map and selects the copy; the world is not duplicated; each edit changes the revision
#[test]
fn duplicating_copies_keys_and_brushes() {
    let (dir, mut session) = project("duplicate");
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    let first = session.context().map().unwrap().revision();
    let done = session
        .execute(MAP_DUPLICATE, json!({ "entity": 2 }))
        .unwrap();
    assert_eq!(done["entity"], 3);
    let map = session.context().map().unwrap();
    assert_ne!(map.revision(), first);
    assert_eq!(map.selected(), Some(EntityIndex::new(3)));
    assert_eq!(map.file().entities[3].items, map.file().entities[2].items);
    assert!(text(&session).ends_with(
        "{\n\"classname\" \"func_door\"\n{\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) door 0 0 0 1 1\n\
         ( 0 0 64 ) ( 1 0 64 ) ( 0 1 64 ) door 0 0 0 1 1\n}\n}\n"
    ));
    assert!(
        session
            .execute(MAP_DUPLICATE, json!({ "entity": 0 }))
            .is_err()
    );
    session.undo().unwrap();
    assert_eq!(text(&session), MAP);
}
