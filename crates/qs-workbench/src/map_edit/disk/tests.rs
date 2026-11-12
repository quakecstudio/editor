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
use crate::map_edit::{EntityIndex, MAP_MOVE, MAP_OPEN, MAP_SAVE, MAP_SELECT, MAP_SET_KEY};
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

/// The spec's `base` BSP vector, from its `xxd` dump.
fn base_bsp() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/vectors/bsp/base.txt");
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

// No spec statement: P5-17, BSP-19.1: a BSP rebuilt under unsaved edits is a conflict read from
// its entity lump, not a parse of its bytes; a .ent another program writes beside it is no file
// map.save overwrites, and asking again finds the conflict
#[test]
fn a_bsp_and_its_ent_are_watched() {
    let (dir, mut session) = opened("bsp");
    let bsp = dir.join("maps/base.bsp");
    std::fs::write(&bsp, base_bsp()).unwrap();
    session.execute(MAP_OPEN, json!({ "path": bsp })).unwrap();
    let edit = json!({ "entity": 0, "key": "message", "value": "Mine" });
    session.execute(MAP_SET_KEY, edit.clone()).unwrap();
    let rebuilt = qs_bsp::replace_entities(
        &base_bsp(),
        b"{\n\"classname\" \"worldspawn\"\n\"message\" \"Rebuilt\"\n}\n",
        qs_bsp::BspLoader::default(),
    )
    .unwrap();
    std::fs::write(&bsp, &rebuilt).unwrap();
    assert_eq!(
        changed(&mut session, std::slice::from_ref(&bsp)).map,
        MapOnDisk::Conflict
    );
    session
        .execute(MAP_RESOLVE, json!({ "keep": "disk" }))
        .unwrap();
    assert!(text(&session).contains("Rebuilt"));

    session.execute(MAP_SET_KEY, edit).unwrap();
    let ent = dir.join("maps/base.ent");
    std::fs::write(
        &ent,
        "{\n\"classname\" \"worldspawn\"\n\"message\" \"Theirs\"\n}\n",
    )
    .unwrap();
    let refused = session
        .execute(MAP_SAVE, json!({}))
        .unwrap_err()
        .to_string();
    assert!(refused.contains("base.ent"), "{refused}");
    assert!(std::fs::read_to_string(&ent).unwrap().contains("Theirs"));
    assert_eq!(changed(&mut session, &[bsp]).map, MapOnDisk::Conflict);
    session
        .execute(MAP_RESOLVE, json!({ "keep": "mine" }))
        .unwrap();
    session.execute(MAP_SAVE, json!({})).unwrap();
    assert!(std::fs::read_to_string(&ent).unwrap().contains("Mine"));
}

// No spec statement: P5-17: QuakeC changed with a map that no longer reads is still read again
#[test]
fn quakec_is_read_again_when_the_map_is_not() {
    let (dir, mut session) = opened("broken");
    std::fs::write(dir.join("maps/test.map"), "{\n\"classname\"\n").unwrap();
    std::fs::write(
        dir.join("a.qc"),
        format!(
            "{CODE}/*QUAKED monster_new (1 0 0) (-16 -16 -24) (16 16 40)\n*/\n\
             void() monster_new = {{}};\n"
        ),
    )
    .unwrap();
    let paths = [dir.join("a.qc"), dir.join("maps/test.map")];
    assert!(
        session
            .execute(FILES_CHANGED, json!({ "paths": paths }))
            .is_err()
    );
    let found = session.execute(PROJECT_ENTITIES, json!({})).unwrap();
    assert!(found.to_string().contains("monster_new"));
}
