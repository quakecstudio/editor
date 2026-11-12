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

use super::{FILES_CHANGED, FilesChanged, MAP_COMPARE, MAP_RESOLVE, MapDifference, MapOnDisk};
use crate::entity_classes::PROJECT_ENTITIES;
use crate::map_edit::{EntityIndex, MAP_MOVE, MAP_OPEN, MAP_SAVE, MAP_SELECT};
use crate::{Config, OPEN_PROJECT, Workbench, register};

const CODE: &str = "entity self; .string classname; .vector origin;\nvoid() worldspawn = {};\n\
    /*QUAKED monster_thing (1 0 0) (-16 -16 -24) (16 16 40)\n*/\nvoid() monster_thing = {};\n";

const MAP: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
    {\n\"classname\" \"monster_thing\"\n\"origin\" \"0 0 24\"\n}\n\
    {\n\"classname\" \"monster_thing\"\n\"origin\" \"64 0 24\"\n}\n";

/// A project of [`CODE`] with `maps/test.map` holding [`MAP`], open, and the map open.
fn opened(name: &str) -> (PathBuf, Session<Workbench<FsLoader>>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-disk-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("a.qc"), CODE).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\na.qc\n").unwrap();
    std::fs::write(dir.join("maps/test.map"), MAP).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    (dir, session)
}

/// What the watcher, faked by the test, reports: `paths` changed.
fn changed(session: &mut Session<Workbench<FsLoader>>, paths: &[PathBuf]) -> FilesChanged {
    serde_json::from_value(
        session
            .execute(FILES_CHANGED, json!({ "paths": paths }))
            .unwrap(),
    )
    .unwrap()
}

fn text(session: &Session<Workbench<FsLoader>>) -> String {
    String::from_utf8(session.context().map().unwrap().file().to_bytes()).unwrap()
}

// No spec statement: P5-17, MAP-32: a script rewriting the open map with no unsaved edits
// reloads it with no prompt, keeping the selection; an unchanged file changes nothing
#[test]
fn a_map_with_no_unsaved_edits_reloads() {
    let (dir, mut session) = opened("reload");
    let map = dir.join("maps/test.map");
    session.execute(MAP_SELECT, json!({ "entity": 2 })).unwrap();
    assert_eq!(
        changed(&mut session, std::slice::from_ref(&map)).map,
        MapOnDisk::Same
    );
    let rewritten = MAP.replace("\"64 0 24\"", "\"96 0 24\"");
    std::fs::write(&map, &rewritten).unwrap();
    let result = changed(&mut session, &[map]);
    assert_eq!(result.map, MapOnDisk::Reloaded);
    assert!(!result.quakec);
    assert_eq!(text(&session), rewritten);
    let document = session.context().map().unwrap();
    assert_eq!(document.selected(), Some(EntityIndex::new(2)));
    assert!(!document.is_unsaved());
}

// No spec statement: P5-17, MAP-32: a rewrite under unsaved edits keeps them and waits; the
// differences are listed; saving does not overwrite the newer file until mine is kept, and disk
// takes the file
#[test]
fn unsaved_edits_are_never_overwritten() {
    let (dir, mut session) = opened("conflict");
    let map = dir.join("maps/test.map");
    session
        .execute(MAP_MOVE, json!({ "entity": 1, "by": [8.0, 0.0, 0.0] }))
        .unwrap();
    let mine = text(&session);
    let theirs = MAP.replace("\"64 0 24\"", "\"96 0 24\"");
    std::fs::write(&map, &theirs).unwrap();
    assert_eq!(
        changed(&mut session, std::slice::from_ref(&map)).map,
        MapOnDisk::Conflict
    );
    assert_eq!(text(&session), mine, "the edits are kept");
    let differences: Vec<MapDifference> =
        serde_json::from_value(session.execute(MAP_COMPARE, json!({})).unwrap()).unwrap();
    let said: Vec<String> = differences.iter().map(|d| d.text.clone()).collect();
    assert_eq!(
        said,
        [
            "monster_thing: origin \"0 0 24\"",
            "monster_thing: origin \"96 0 24\""
        ]
    );
    let refused = session
        .execute(MAP_SAVE, json!({}))
        .unwrap_err()
        .to_string();
    assert!(refused.contains("another program changed it"), "{refused}");
    assert_eq!(std::fs::read_to_string(&map).unwrap(), theirs);

    session
        .execute(MAP_RESOLVE, json!({ "keep": "mine" }))
        .unwrap();
    session.execute(MAP_SAVE, json!({})).unwrap();
    assert_eq!(std::fs::read_to_string(&map).unwrap(), mine);

    session
        .execute(MAP_MOVE, json!({ "entity": 1, "by": [8.0, 0.0, 0.0] }))
        .unwrap();
    std::fs::write(&map, &theirs).unwrap();
    assert_eq!(changed(&mut session, &[map]).map, MapOnDisk::Conflict);
    session
        .execute(MAP_RESOLVE, json!({ "keep": "disk" }))
        .unwrap();
    assert_eq!(text(&session), theirs);
    assert!(session.context().map().unwrap().conflict().is_none());
}

// No spec statement: P5-17, MAP-32: changed QuakeC is read again, and the entity definitions
// derived from it follow
#[test]
fn changed_quakec_refreshes_the_entity_definitions() {
    let (dir, mut session) = opened("quakec");
    let classes = |session: &mut Session<Workbench<FsLoader>>| -> Vec<String> {
        let found = session.execute(PROJECT_ENTITIES, json!({})).unwrap();
        found["classes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|class| class["name"].as_str().unwrap().to_owned())
            .collect()
    };
    let before = classes(&mut session);
    assert!(before.contains(&"monster_thing".to_owned()), "{before:?}");
    assert!(!before.contains(&"monster_new".to_owned()));
    std::fs::write(
        dir.join("a.qc"),
        format!(
            "{CODE}/*QUAKED monster_new (1 0 0) (-16 -16 -24) (16 16 40)\n*/\n\
             void() monster_new = {{}};\n"
        ),
    )
    .unwrap();
    let result = changed(&mut session, &[dir.join("a.qc")]);
    assert!(result.quakec);
    assert_eq!(result.map, MapOnDisk::Same);
    let after = classes(&mut session);
    assert!(after.contains(&"monster_new".to_owned()), "{after:?}");
}
