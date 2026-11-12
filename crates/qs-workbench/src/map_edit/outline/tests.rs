// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use crate::map_edit::{MAP_MOVE_TO, MAP_OPEN, MAP_OUTLINE, MAP_SET_KEY, OutlineRow, RowKind};
use crate::{Config, Workbench, register};

/// A TrenchBroom map: two linked groups each with an army, a plain group inside a layer with a
/// door in it, the layer, a light in it, and a monster and a trigger in no layer.
const MAP: &str = "{\n\"classname\" \"worldspawn\"\n{\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) floor 0 0 0 1 1\n}\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"pair\"\n\"_tb_id\" \"1\"\n\
\"_tb_linked_group_id\" \"{a}\"\n\"_tb_transformation\" \"1 0 0 128 0 1 0 0 0 0 1 0 0 0 0 1\"\n}\n\
{\n\"classname\" \"monster_army\"\n\"origin\" \"128 0 0\"\n\"_tb_group\" \"1\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"pair\"\n\"_tb_id\" \"2\"\n\
\"_tb_linked_group_id\" \"{a}\"\n}\n\
{\n\"classname\" \"monster_army\"\n\"origin\" \"0 0 0\"\n\"_tb_group\" \"2\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\"_tb_name\" \"Lights\"\n\"_tb_id\" \"3\"\n}\n\
{\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n\"_tb_layer\" \"3\"\n}\n\
{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"doors\"\n\"_tb_id\" \"4\"\n\
\"_tb_layer\" \"3\"\n}\n\
{\n\"classname\" \"func_door\"\n\"targetname\" \"gate\"\n\"_tb_group\" \"4\"\n\
{\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) door 0 0 0 1 1\n}\n}\n\
{\n\"classname\" \"trigger_once\"\n\"target\" \"gate\"\n}\n\
{\n\"classname\" \"monster_ogre\"\n\"origin\" \"64 0 0\"\n}\n";

fn open(name: &str) -> Session<Workbench<FsLoader>> {
    let dir = std::env::temp_dir().join(format!("qs-wb-outline-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("outline.map");
    std::fs::write(&path, MAP).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

fn outline(session: &mut Session<Workbench<FsLoader>>, filter: &str) -> Vec<OutlineRow> {
    let rows = session
        .execute(MAP_OUTLINE, json!({ "filter": filter }))
        .unwrap();
    serde_json::from_value(rows).unwrap()
}

/// The rows as `(entity, depth)`, in order.
fn flat(rows: &[OutlineRow]) -> Vec<(usize, usize)> {
    fn walk(rows: &[OutlineRow], depth: usize, out: &mut Vec<(usize, usize)>) {
        for row in rows {
            out.push((row.entity.get(), depth));
            walk(&row.rows, depth.saturating_add(1), out);
        }
    }
    let mut out = Vec::new();
    walk(rows, 0, &mut out);
    out
}

// No spec statement: P5-16, MAP-31: every entity, group and layer appears once: the default
// layer (the world) and then each layer; in each, its groups by name, then its entities by
// classname and targetname, with brush counts
#[test]
fn every_entity_group_and_layer_appears_once() {
    let mut session = open("once");
    let rows = outline(&mut session, "");
    let order = flat(&rows);
    assert_eq!(
        order,
        [
            (0, 0),
            (1, 1),
            (2, 2),
            (3, 1),
            (4, 2),
            (10, 1),
            (9, 1),
            (5, 0),
            (7, 1),
            (8, 2),
            (6, 1)
        ]
    );
    assert_eq!(rows.iter().map(OutlineRow::count).sum::<usize>(), 11);
    assert_eq!(rows[0].kind, RowKind::Layer);
    assert_eq!(rows[0].brushes, 1, "the world's brush");
    assert_eq!(rows[0].rows[0].kind, RowKind::LinkedGroup);
    assert_eq!(rows[1].label, "Lights");
    let doors = &rows[1].rows[0];
    assert_eq!(
        (doors.kind, doors.label.as_str()),
        (RowKind::Group, "doors")
    );
    assert_eq!(doors.rows[0].targetname.as_deref(), Some("gate"));
    assert_eq!(doors.rows[0].brushes, 1);
}

// No spec statement: P5-16: the filter keeps what holds its text, ignoring case, by
// classname, key or value, with the layers and groups around it
#[test]
fn the_filter_keeps_matches_and_what_holds_them() {
    let mut session = open("filter");
    assert_eq!(
        flat(&outline(&mut session, "GATE")),
        [(0, 0), (9, 1), (5, 0), (7, 1), (8, 2)]
    );
    assert_eq!(flat(&outline(&mut session, "ogre")), [(0, 0), (10, 1)]);
    assert!(outline(&mut session, "nothing has this").is_empty());
}

// No spec statement: P5-16, MAP-31: the outline follows every edit, undo and redo
#[test]
fn the_outline_follows_edits_undo_and_redo() {
    let mut session = open("follow");
    let before = outline(&mut session, "");
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 10, "key": "classname", "value": "monster_zombie" }),
        )
        .unwrap();
    let after = outline(&mut session, "");
    assert_eq!(after[0].rows[2].label, "monster_zombie");
    session.undo().unwrap();
    assert_eq!(outline(&mut session, ""), before);
    session.redo().unwrap();
    assert_eq!(outline(&mut session, ""), after);
}

// No spec statement: P5-16: moving a row into a group or layer writes the key TrenchBroom
// reads; into a linked group it is copied into the group linked to it; never into itself
#[test]
fn rows_move_between_layers_and_groups() {
    let mut session = open("move");
    session
        .execute(MAP_MOVE_TO, json!({ "entity": 10, "into": 5 }))
        .unwrap();
    let rows = outline(&mut session, "");
    assert!(flat(&rows[1].rows).contains(&(10, 0)), "the ogre in Lights");
    session
        .execute(MAP_MOVE_TO, json!({ "entity": 6, "into": 0 }))
        .unwrap();
    let map = session.context().map().unwrap();
    assert_eq!(map.file().entities[6].value(b"_tb_layer"), None);
    // Into a linked group: the copy in the other group follows.
    let count = session.context().map().unwrap().file().entities.len();
    session
        .execute(MAP_MOVE_TO, json!({ "entity": 9, "into": 3 }))
        .unwrap();
    assert_eq!(
        session.context().map().unwrap().file().entities.len(),
        count + 1
    );
    for (entity, into) in [(7, 7), (0, 5), (5, 2)] {
        assert!(
            session
                .execute(MAP_MOVE_TO, json!({ "entity": entity, "into": into }))
                .is_err(),
            "{entity} into {into}"
        );
    }
    let map = session.context().map().unwrap();
    assert_eq!(map.selected(), None, "moving a row selects nothing");
}
