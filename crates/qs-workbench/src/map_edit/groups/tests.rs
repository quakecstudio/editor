// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::PathBuf;

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use crate::map_edit::{
    EntityIndex, MAP_DELETE, MAP_EXPORT, MAP_HIDE, MAP_ISOLATE, MAP_LAYERS, MAP_LOCK, MAP_MOVE,
    MAP_OPEN, MAP_SAVE, MAP_SELECT, MAP_SET_KEY, MAP_SET_LAYER, MAP_SHOW_ALL, MAP_UNLOCK_ALL,
};
use crate::{Config, Workbench, register};

/// A map as TrenchBroom writes one: two linked groups from its manual, group 1 moved 128
/// along x, each with a brush and an army; and a locked layer holding a light.
const MAP: &str = "// Game: Quake\n// Format: Valve\n{\n\"classname\" \"worldspawn\"\n\
\"mapversion\" \"220\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"group\"\n\"_tb_id\" \"1\"\n\
\"_tb_linked_group_id\" \"{38b3b39d-a165-4999-985d-d40563ce51c1}\"\n\
\"_tb_transformation\" \"1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1\"\n\
{\n( 128 0 0 ) ( 128 1 0 ) ( 129 0 0 ) floor 0 0 0 1 1\n}\n}\n\
{\n\"classname\" \"monster_army\"\n\"origin\" \"128 0 0\"\n\"_tb_group\" \"1\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"group\"\n\"_tb_id\" \"2\"\n\
\"_tb_linked_group_id\" \"{38b3b39d-a165-4999-985d-d40563ce51c1}\"\n\
{\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) floor 0 0 0 1 1\n}\n}\n\
{\n\"classname\" \"monster_army\"\n\"origin\" \"0 0 0\"\n\"_tb_group\" \"2\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\"_tb_name\" \"Lights\"\n\"_tb_id\" \"3\"\n\
\"_tb_layer_locked\" \"1\"\n\"_tb_layer_omit_from_export\" \"1\"\n}\n\
{\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n\"_tb_layer\" \"3\"\n}\n";

/// [`MAP`] open in a session, in a fresh directory named `name`.
fn open(name: &str) -> (PathBuf, Session<Workbench<FsLoader>>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-groups-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("linked.map");
    std::fs::write(&path, MAP).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    (path, session)
}

/// The open map's text.
fn text(session: &Session<Workbench<FsLoader>>) -> String {
    String::from_utf8(session.context().map().unwrap().file().to_bytes()).unwrap()
}

/// The value of `key` on the open map's entity at `entity`.
fn value(session: &Session<Workbench<FsLoader>>, entity: usize, key: &str) -> Option<String> {
    let map = session.context().map()?;
    map.file().entities[entity]
        .value(key.as_bytes())
        .map(|v| String::from_utf8_lossy(v).into_owned())
}

// spec MAP-9.7: editing one copy of a linked group changes every copy, moved by its
// transformation, and one undo takes the edit and its copies back
#[test]
fn an_edit_in_a_linked_group_changes_every_copy_in_one_step() {
    let (_, mut session) = open("copies");
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 4, "key": "wait", "value": "3" }),
        )
        .unwrap();
    assert_eq!(value(&session, 2, "wait").as_deref(), Some("3"));
    session
        .execute(MAP_MOVE, json!({ "entity": 4, "by": [16.0, 0.0, 0.0] }))
        .unwrap();
    assert_eq!(value(&session, 2, "origin").as_deref(), Some("144 0 0"));
    session.undo().unwrap();
    assert_eq!(value(&session, 2, "origin").as_deref(), Some("128 0 0"));
    session.undo().unwrap();
    assert_eq!(text(&session), MAP);
    // The other way round: the copy is a source too.
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 2, "key": "wait", "value": "7" }),
        )
        .unwrap();
    assert_eq!(value(&session, 4, "wait").as_deref(), Some("7"));
}

// spec MAP-9.5: moving a group moves its brushes, what is in it and its transformation, and
// leaves the groups linked to it where they are
#[test]
fn moving_a_group_moves_its_contents_and_transformation() {
    let (_, mut session) = open("move-group");
    session
        .execute(MAP_MOVE, json!({ "entity": 3, "by": [0.0, 32.0, 0.0] }))
        .unwrap();
    assert_eq!(value(&session, 4, "origin").as_deref(), Some("0 32 0"));
    assert_eq!(
        value(&session, 3, "_tb_transformation").as_deref(),
        Some("1 0 0 0 0 1 0 32 0 0 1 0 0 0 0 1")
    );
    assert_eq!(value(&session, 2, "origin").as_deref(), Some("128 0 0"));
    session.undo().unwrap();
    assert_eq!(text(&session), MAP);
}

// spec MAP-9.9: an entity in a locked layer is not edited or deleted; unlocking the layer, an
// edit like any other, lets it be; an export leaves the layer out
#[test]
fn a_locked_layer_refuses_edits_and_an_export_leaves_it_out() {
    let (path, mut session) = open("layers");
    let layers = session.execute(MAP_LAYERS, json!({})).unwrap();
    assert_eq!(
        layers,
        json!([{ "entity": 5, "name": "Lights", "hidden": false, "locked": true,
                 "omit_from_export": true, "entities": 1 }])
    );
    assert!(
        session
            .execute(
                MAP_SET_KEY,
                json!({ "entity": 6, "key": "light", "value": "200" })
            )
            .is_err()
    );
    assert!(session.execute(MAP_DELETE, json!({ "entity": 6 })).is_err());
    assert_eq!(text(&session), MAP, "a refused edit changes nothing");
    let exported = path.with_file_name("exported.map");
    session
        .execute(MAP_EXPORT, json!({ "path": exported }))
        .unwrap();
    let written = std::fs::read_to_string(&exported).unwrap();
    assert!(
        !written.contains("Lights") && !written.contains("\"light\""),
        "{written}"
    );
    session
        .execute(
            MAP_SET_LAYER,
            json!({ "layer": 5, "setting": "locked", "switch": "off" }),
        )
        .unwrap();
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 6, "key": "light", "value": "200" }),
        )
        .unwrap();
    assert!(
        session
            .execute(
                MAP_SET_LAYER,
                json!({ "layer": 4, "setting": "hidden", "switch": "on" })
            )
            .is_err(),
        "no layer"
    );
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(text(&session), MAP);
}

// No spec statement: P5-14, MAP-26: hide, isolate and lock any selection in the editor; a
// hidden layer hides what is in it; the map does not change
#[test]
fn a_selection_is_hidden_isolated_and_locked() {
    let (_, mut session) = open("view");
    let hidden = |session: &Session<Workbench<FsLoader>>| {
        let map = session.context().map().unwrap();
        (1..7)
            .filter(|&e| map.is_hidden(EntityIndex::new(e)))
            .collect::<Vec<_>>()
    };
    session
        .execute(MAP_HIDE, json!({ "entities": [2, 4] }))
        .unwrap();
    assert_eq!(hidden(&session), [2, 4]);
    session
        .execute(MAP_ISOLATE, json!({ "entities": [4] }))
        .unwrap();
    assert_eq!(hidden(&session), [1, 2, 3, 5, 6]);
    session.execute(MAP_SHOW_ALL, json!({})).unwrap();
    assert!(hidden(&session).is_empty());
    session
        .execute(
            MAP_SET_LAYER,
            json!({ "layer": 5, "setting": "hidden", "switch": "on" }),
        )
        .unwrap();
    assert_eq!(hidden(&session), [6]);
    session
        .execute(MAP_LOCK, json!({ "entities": [1] }))
        .unwrap();
    assert!(
        session
            .execute(MAP_MOVE, json!({ "entity": 1, "by": [8.0, 0.0, 0.0] }))
            .is_err()
    );
    assert!(
        session
            .execute(
                MAP_SET_KEY,
                json!({ "entity": 4, "key": "wait", "value": "1" })
            )
            .is_err(),
        "the copy it would change is locked"
    );
    session.execute(MAP_UNLOCK_ALL, json!({})).unwrap();
    session
        .execute(MAP_MOVE, json!({ "entity": 1, "by": [8.0, 0.0, 0.0] }))
        .unwrap();
    assert!(
        session
            .execute(MAP_HIDE, json!({ "entities": [99] }))
            .is_err()
    );
}

// No spec statement: P5-14: a TrenchBroom map with linked groups and layers saves with them
// intact after an edit, and the selection follows an entity a copy moved
#[test]
fn a_trenchbroom_map_saves_with_its_groups_and_layers() {
    let (path, mut session) = open("save");
    session.execute(MAP_SELECT, json!({ "entity": 6 })).unwrap();
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 4, "key": "wait", "value": "2" }),
        )
        .unwrap();
    session.execute(MAP_SAVE, json!({})).unwrap();
    let saved = std::fs::read_to_string(&path).unwrap();
    let expected = MAP.replace(
        "\"_tb_group\" \"1\"\n}",
        "\"_tb_group\" \"1\"\n\"wait\" \"2\"\n}",
    );
    let expected = expected.replace(
        "\"origin\" \"0 0 0\"\n\"_tb_group\" \"2\"\n}",
        "\"origin\" \"0 0 0\"\n\"_tb_group\" \"2\"\n\"wait\" \"2\"\n}",
    );
    assert_eq!(saved, expected);
    assert_eq!(
        session.context().map().unwrap().selected(),
        Some(EntityIndex::new(6))
    );
}
