// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::io::{BufReader, Write as _};
use std::path::PathBuf;
use std::time::Duration;

use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{GenericFilePath, ListenerOptions, ToFsName as _};
use qc_project::FsLoader;
use qs_bridge::{
    Answer, Capabilities, Host, HostInfo, Id, Message, Session as BridgeSession, read_frame,
    write_frame,
};
use qs_commands::{Registry, Session};
use qs_engine::{Client, Running};
use qs_map::MapFile;
use serde_json::{Value, json};

use super::LiveChange;
use crate::map_edit::{EntityIndex, MAP_MOVE, MAP_OPEN, MAP_PLACE, register_map_edit};
use crate::{Config, HasWorkbench, Preview, Workbench};

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

/// A host that records each request it is sent and answers `entity/spawn` with edict 9.
struct Recording(Seen);

impl Host for Recording {
    fn info(&self) -> HostInfo {
        HostInfo {
            name: "recording".to_owned(),
            version: "0".to_owned(),
            engine: "test".to_owned(),
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::of(["entity/spawn", "entity/edit", "entity/remove"])
    }

    fn call(&mut self, _id: &Id, method: &str, params: &Value) -> Answer {
        self.0
            .lock()
            .unwrap()
            .push((method.to_owned(), params.clone()));
        Answer::Now(Ok(match method {
            "entity/spawn" => json!({ "edict": 9 }),
            _ => json!({}),
        }))
    }

    fn shutdown(&mut self) {}
}

/// The requests a [`Recording`] host was sent, each its method and parameters.
type Seen = std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>;

/// A client of a [`Recording`] host, and what it records.
fn recording_client() -> (Client, Seen) {
    let name = format!("qs-workbench-live-{}", std::process::id());
    let path = if cfg!(windows) {
        PathBuf::from(format!(r"\\.\pipe\{name}"))
    } else {
        std::env::temp_dir().join(format!("{name}.sock"))
    };
    let listener = ListenerOptions::new()
        .name(path.as_os_str().to_fs_name::<GenericFilePath>().unwrap())
        .try_overwrite(true)
        .create_sync()
        .unwrap();
    let seen = std::sync::Arc::default();
    let mut host = Recording(std::sync::Arc::clone(&seen));
    std::thread::spawn(move || {
        let stream = listener.accept().unwrap();
        let mut reader = BufReader::new(&stream);
        let mut session = BridgeSession::new();
        while let Ok(Some(content)) = read_frame(&mut reader) {
            let Ok(message) = Message::parse(&content) else {
                return;
            };
            if let Some(response) = session.handle(&mut host, message).response {
                let mut out = &stream;
                write_frame(&mut out, &response.to_bytes()).unwrap();
                out.flush().unwrap();
            }
        }
    });
    let mut client = Client::connect(&path, 100, Duration::from_millis(10)).unwrap();
    client.initialize("tests").unwrap();
    (client, seen)
}

/// A workbench and a preview whose game plays the map `test`.
struct Context {
    workbench: Workbench<FsLoader>,
    preview: Preview,
}

impl HasWorkbench for Context {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn preview(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

// No spec statement: P5-07, MAP-12: placing a monster in the map editor spawns it in the game
// playing the map, at its place; an undo removes it, a move edits it, and a map the game is not
// on sends nothing
#[test]
fn placing_a_monster_spawns_it_in_the_running_game() {
    let dir = std::env::temp_dir().join(format!("qs-wb-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let map = dir.join("test.map");
    std::fs::write(&map, MAP).unwrap();
    let other = dir.join("other.map");
    std::fs::write(&other, MAP).unwrap();
    let (client, seen) = recording_client();
    // The host's process stands in for the game's; the test binary listing its tests exits.
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let preview = Preview::playing(Running { child, client }, "TEST");
    let mut registry = Registry::new();
    register_map_edit(&mut registry).unwrap();
    let context = Context {
        workbench: Workbench::new(FsLoader, Config::default()),
        preview,
    };
    let mut session = Session::new(context, registry);
    session.execute(MAP_OPEN, json!({ "path": map })).unwrap();
    session
        .execute(
            MAP_PLACE,
            json!({ "classname": "monster_dog", "origin": [8.0, 0.0, 24.0], "keys": [] }),
        )
        .unwrap();
    session
        .execute(MAP_MOVE, json!({ "entity": 3, "by": [16.0, 0.0, 0.0] }))
        .unwrap();
    session.undo().unwrap();
    session.undo().unwrap();
    let sent = std::mem::take(&mut *seen.lock().unwrap());
    let methods: Vec<&str> = sent.iter().map(|(method, _)| method.as_str()).collect();
    assert_eq!(
        methods,
        [
            "entity/spawn",
            "entity/edit",
            "entity/edit",
            "entity/remove"
        ]
    );
    assert_eq!(
        sent[0].1,
        json!({
            "classname": "monster_dog",
            "keyvalues": [{ "key": "origin", "value": "8 0 24" }],
            "map_entity": 3,
        })
    );
    assert_eq!(
        sent[1].1,
        json!({ "map_entity": 3, "keyvalues": [{ "key": "origin", "value": "24 0 24" }] })
    );
    assert_eq!(sent[3].1, json!({ "map_entity": 3 }));

    session.execute(MAP_OPEN, json!({ "path": other })).unwrap();
    session
        .execute(MAP_MOVE, json!({ "entity": 1, "by": [1.0, 0.0, 0.0] }))
        .unwrap();
    assert!(seen.lock().unwrap().is_empty(), "the game is not on other");
}
