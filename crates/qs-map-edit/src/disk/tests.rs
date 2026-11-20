// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qs_commands::{Command, Outcome, Registry, Session};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{MAP_COMPARE, MAP_RESOLVE, MapDifference, MapOnDisk, map_files_changed};
use crate::testing::{MapContext, TestGame};
use crate::{
    MAP_OPEN, MAP_SELECT, MAP_SET_KEY, MapEditError, register_map_edit, register_map_game,
};

const MAP: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
    {\n\"classname\" \"monster_thing\"\n\"origin\" \"0 0 24\"\n}\n\
    {\n\"classname\" \"monster_thing\"\n\"origin\" \"64 0 24\"\n}\n";

/// The map moved by another program: its second monster somewhere else.
const MOVED: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
    {\n\"classname\" \"monster_thing\"\n\"origin\" \"0 0 24\"\n}\n\
    {\n\"classname\" \"monster_thing\"\n\"origin\" \"128 0 24\"\n}\n";

/// The parameters of the test's `test.files_changed`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
struct ChangedParams {
    paths: Vec<PathBuf>,
}

/// `test.files_changed`: what the workbench's `files.changed` does for the open map.
struct FilesChanged;

impl Command<MapContext> for FilesChanged {
    const ID: &'static str = "test.files_changed";
    const SUMMARY: &'static str = "The map editor told of files another program wrote.";
    type Params = ChangedParams;
    type Output = MapOnDisk;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut MapContext,
        params: ChangedParams,
    ) -> Result<Outcome<MapOnDisk, MapContext>, MapEditError> {
        Ok(Outcome::outside_history(map_files_changed(
            context,
            &params.paths,
        )?))
    }
}

/// `MAP` written to a folder of its own, open in a session with a game that plays it.
fn opened(name: &str) -> (PathBuf, Session<MapContext>) {
    let dir = std::env::temp_dir().join(format!("qs-map-edit-disk-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let map = dir.join("test.map");
    std::fs::write(&map, MAP).unwrap();
    let context = MapContext {
        game: Some(TestGame {
            playing: Some(map.clone()),
            ..TestGame::default()
        }),
        ..MapContext::default()
    };
    let mut registry = Registry::new();
    register_map_edit(&mut registry).unwrap();
    register_map_game(&mut registry).unwrap();
    registry.register(FilesChanged).unwrap();
    let mut session = Session::new(context, registry);
    session.execute(MAP_OPEN, json!({ "path": map })).unwrap();
    (map, session)
}

fn changed(session: &mut Session<MapContext>, paths: &[&Path]) -> MapOnDisk {
    let output = session
        .execute("test.files_changed", json!({ "paths": paths }))
        .unwrap();
    serde_json::from_value(output).unwrap()
}

fn origin(session: &Session<MapContext>, entity: usize) -> Vec<u8> {
    session.context().map().unwrap().file().entities[entity]
        .value(b"origin")
        .unwrap()
        .to_vec()
}

fn reread(session: &Session<MapContext>) -> Vec<PathBuf> {
    session.context().game.as_ref().unwrap().reread.clone()
}

fn compare(session: &mut Session<MapContext>) -> Vec<MapDifference> {
    serde_json::from_value(session.execute(MAP_COMPARE, json!({})).unwrap()).unwrap()
}

// No spec statement: P5-17, MAP-32: with no unsaved edits, the open map's file changed by another
// program is read again, its selection kept, and the game told; another file changes nothing
#[test]
fn a_map_with_no_edits_is_read_again() {
    let (map, mut session) = opened("reload");
    session.execute(MAP_SELECT, json!({ "entity": 2 })).unwrap();
    let other = map.with_file_name("other.map");
    assert_eq!(changed(&mut session, &[&other]), MapOnDisk::Same);
    assert_eq!(
        changed(&mut session, &[&map]),
        MapOnDisk::Same,
        "the file is as read"
    );
    std::fs::write(&map, MOVED).unwrap();
    assert_eq!(changed(&mut session, &[&map]), MapOnDisk::Reloaded);
    assert_eq!(origin(&session, 2), b"128 0 24");
    let document = session.context().map().unwrap();
    assert_eq!(document.selected().map(|e| e.get()), Some(2));
    assert!(document.conflict().is_none());
    assert_eq!(reread(&session), [map]);
}

// No spec statement: P5-17, MAP-32: with unsaved edits, the changed file is kept aside: map.compare_with_disk
// lists how it differs, map.resolve_disk_change keeps mine (the edits stay, the conflict goes) or disk (the
// file is read again, the edits dropped, the game told)
#[test]
fn a_map_with_edits_waits_for_resolve() {
    let (map, mut session) = opened("conflict");
    assert_eq!(compare(&mut session), Vec::new(), "no conflict yet");
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 1, "key": "origin", "value": "8 0 24" }),
        )
        .unwrap();
    std::fs::write(&map, MOVED).unwrap();
    assert_eq!(changed(&mut session, &[&map]), MapOnDisk::Conflict);
    assert_eq!(origin(&session, 1), b"8 0 24", "the edit is kept");
    assert!(session.context().map().unwrap().conflict().is_some());
    let differences = compare(&mut session);
    assert!(!differences.is_empty(), "{differences:?}");
    assert!(
        differences.iter().any(|d| d.entity.get() == 2),
        "{differences:?}"
    );

    session
        .execute(MAP_RESOLVE, json!({ "keep": "mine" }))
        .unwrap();
    let document = session.context().map().unwrap();
    assert!(document.conflict().is_none());
    assert!(document.is_unsaved());
    assert_eq!(origin(&session, 1), b"8 0 24");
    assert_eq!(reread(&session), Vec::<PathBuf>::new());

    std::fs::write(&map, MAP).unwrap();
    assert_eq!(changed(&mut session, &[&map]), MapOnDisk::Conflict);
    session
        .execute(MAP_RESOLVE, json!({ "keep": "disk" }))
        .unwrap();
    assert_eq!(origin(&session, 1), b"0 0 24", "the edit is dropped");
    assert_eq!(origin(&session, 2), b"64 0 24");
    assert!(!session.context().map().unwrap().is_unsaved());
    assert_eq!(reread(&session), [map]);
}

// No spec statement: P5-17, MAP-32: map.compare_with_disk and map.resolve_disk_change with no map open say so
#[test]
fn no_map_is_no_map() {
    let mut session = MapContext::session();
    assert!(session.execute(MAP_COMPARE, json!({})).is_err());
    assert!(
        session
            .execute(MAP_RESOLVE, json!({ "keep": "disk" }))
            .is_err()
    );
}
