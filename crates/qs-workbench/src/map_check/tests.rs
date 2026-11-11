// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::PathBuf;

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::{Value, json};

use super::{MAP_CHECK, MAP_RENAME_KEY};
use crate::{Config, OPEN_FILE, OPEN_PROJECT, READ_FILE, RENAME, Workbench, register};

const CODE: &str = "entity self; .string classname, target, targetname; .float speed;\n\
    void() worldspawn = {};\n\
    /*QUAKED func_door (0 .5 .8) ?\n*/\n\
    void() func_door = { if (!self.speed) self.speed = 100; };\n";

const MAP: &str = "{\n\"classname\" \"worldspawn\"\n\"wad\" \"base.wad\"\n}\n\
    {\n\"classname\" \"func_door\"\n\"sped\" \"50\"\n}\n\
    {\n\"classname\" \"func_door\"\n\"speed\"   \"75\"\n}\n";

/// A project in a fresh directory named `name` with [`CODE`] and `maps/test.map` holding
/// [`MAP`], open.
fn project(name: &str) -> (PathBuf, Session<Workbench<FsLoader>>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-map-{name}-{}", std::process::id()));
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
    (dir, session)
}

// spec MAP-8.2: map.check reports a misspelled key of a map with the field to write instead
#[test]
fn a_misspelled_key_in_a_map_is_reported_with_a_fix() {
    let (dir, mut session) = project("check");
    let checked = session
        .execute(MAP_CHECK, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    assert_eq!(
        checked["diagnostics"],
        json!([{
            "line": 7,
            "message": "sped is not a field, so the game ignores it; did you mean speed?",
            "suggestion": "speed",
        }])
    );
    let missing = session.execute(MAP_CHECK, json!({ "path": dir.join("maps/none.map") }));
    assert!(missing.is_err());
}

// spec MAP-8.7: renaming a field warns that a map may name it; once confirmed, the key is
// renamed in the open maps with its value and formatting kept, unsaved, and undo puts it back
#[test]
fn renaming_a_field_rewrites_the_key_in_open_maps_after_confirmation() {
    let (dir, mut session) = project("rename");
    let map = dir.join("maps").join("test.map");
    session.execute(OPEN_FILE, json!({ "path": map })).unwrap();
    let offset = CODE.find("speed").unwrap();
    let renamed = session
        .execute(
            RENAME,
            json!({ "path": dir.join("a.qc"), "offset": offset, "name": "pace" }),
        )
        .unwrap();
    assert_eq!(renamed["caution"], json!("speed"), "a map may name it");
    let text = |session: &mut Session<Workbench<FsLoader>>| -> String {
        let read: Value = session.execute(READ_FILE, json!({ "path": map })).unwrap();
        read["text"].as_str().unwrap().to_owned()
    };
    assert_eq!(
        text(&mut session),
        MAP,
        "nothing changes before confirmation"
    );
    let done = session
        .execute(MAP_RENAME_KEY, json!({ "from": "speed", "to": "pace" }))
        .unwrap();
    assert_eq!(done["maps"], json!([{ "path": map, "pairs": 1 }]));
    assert_eq!(
        text(&mut session),
        MAP.replace("\"speed\"   ", "\"pace\"   ")
    );
    assert_eq!(std::fs::read_to_string(&map).unwrap(), MAP, "unsaved");
    session.undo().unwrap();
    assert_eq!(text(&mut session), MAP);
    let none = session
        .execute(MAP_RENAME_KEY, json!({ "from": "nonesuch", "to": "x" }))
        .unwrap();
    assert_eq!(none["maps"], json!([]));
    let bad = session.execute(MAP_RENAME_KEY, json!({ "from": "speed", "to": "a\"b" }));
    assert!(bad.is_err(), "a key cannot hold a quote");
}
