// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's edits reaching a running game (P5-07, MAP-12): the preview sends each
//! change of the open map's entities over the bridge, to a host that records what it is sent.

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
use serde_json::{Value, json};

use crate::full_build::tests::{Scratch, compiled_room};
use crate::{Config, HasWorkbench, Preview, Workbench};
use qs_map_edit::{MAP_DUPLICATE, MAP_MOVE, MAP_OPEN, MAP_PLACE, register_map_edit};

/// The world, a monster and a light.
const MAP: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
    {\n\"classname\" \"monster_army\"\n\"origin\" \"0 0 24\"\n\"angle\" \"90\"\n}\n\
    {\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n}\n";

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
        Capabilities::of(["entity/spawn", "entity/edit", "entity/remove", "map/swap"])
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
    static SOCKETS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let name = format!(
        "qs-workbench-live-{}-{}",
        std::process::id(),
        SOCKETS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
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

impl crate::HasMap for Context {
    type Project = Workbench<FsLoader>;
    type Live = Preview;

    fn map_store(&mut self) -> &mut crate::MapStore {
        self.workbench.maps_mut()
    }

    fn map_project(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn live_map(&mut self) -> Option<&mut Preview> {
        Some(&mut self.preview)
    }
}

impl HasWorkbench for Context {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }
}

// No spec statement: P5-07, MAP-12: placing a monster in the map editor spawns it in the game
// playing the map, at its place; an undo removes it, a move edits it, and a map the game is not
// on sends nothing
/// A session whose game plays the map `test`, having loaded entities of `classnames`; the
/// directory holding `test.map` and `other.map`, both [`MAP`], and what the game was sent.
fn playing(name: &str, classnames: &[&str]) -> (PathBuf, Session<Context>, Seen) {
    let dir = std::env::temp_dir().join(format!("qs-wb-live-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.map"), MAP).unwrap();
    std::fs::write(dir.join("other.map"), MAP).unwrap();
    let (client, seen) = recording_client();
    // The host's process stands in for the game's; the test binary listing its tests exits.
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let preview = Preview::playing(Running { child, client }, "TEST", classnames);
    let mut registry = Registry::new();
    register_map_edit(&mut registry).unwrap();
    let context = Context {
        workbench: Workbench::new(FsLoader, Config::default()),
        preview,
    };
    (dir, Session::new(context, registry), seen)
}

/// The classnames of [`MAP`]'s entities.
const LOADED: [&str; 3] = ["worldspawn", "monster_army", "light"];

#[test]
fn placing_a_monster_spawns_it_in_the_running_game() {
    let (dir, mut session, seen) = playing("place", &LOADED);
    let other = dir.join("other.map");
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("test.map") }))
        .unwrap();
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

// No spec statement: P5-07, BRIDGE-4.13: a map whose entities are not the ones the game loaded
// sends nothing, said once; a brush entity placed needs a compile and stops the changes after
// it
#[test]
fn a_map_out_of_step_sends_nothing() {
    let (dir, mut session, seen) = playing("apart", &["worldspawn"]);
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("test.map") }))
        .unwrap();
    for _ in 0..2 {
        session
            .execute(MAP_MOVE, json!({ "entity": 1, "by": [8.0, 0.0, 0.0] }))
            .unwrap();
    }
    assert!(seen.lock().unwrap().is_empty());
    let notes: Vec<String> = session
        .context()
        .preview
        .console()
        .map(|line| line.text.clone())
        .collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("next compile"), "{notes:?}");

    let mut loaded = LOADED.to_vec();
    loaded.push("func_door");
    let (dir, mut session, seen) = playing("brush", &loaded);
    let map = dir.join("test.map");
    let door =
        "{\n\"classname\" \"func_door\"\n{\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) door 0 0 0 1 1\n}\n}\n";
    std::fs::write(&map, format!("{MAP}{door}")).unwrap();
    session.execute(MAP_OPEN, json!({ "path": map })).unwrap();
    session
        .execute(MAP_DUPLICATE, json!({ "entity": 3 }))
        .unwrap();
    session
        .execute(MAP_MOVE, json!({ "entity": 1, "by": [8.0, 0.0, 0.0] }))
        .unwrap();
    assert!(seen.lock().unwrap().is_empty(), "the copy needs a compile");
    let notes: Vec<String> = session
        .context()
        .preview
        .console()
        .map(|line| line.text.clone())
        .collect();
    assert!(
        notes.iter().any(|n| n.contains("brush entity")),
        "{notes:?}"
    );
}

/// A preview whose game plays the map `test`, on a host that records what it is sent.
fn playing_test() -> (Preview, Seen) {
    let (client, seen) = recording_client();
    // The host's process stands in for the game's; the test binary listing its tests exits.
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    (
        Preview::playing(Running { child, client }, "TEST", &LOADED),
        seen,
    )
}

/// The fast compile's BSP and portals of the room the full compile builds.
fn room_inputs() -> (Vec<u8>, Vec<u8>) {
    let compiled = compiled_room();
    (compiled.bsp, compiled.portal_file.unwrap())
}

// spec BRIDGE-4.18: a full compile, once done, is swapped into the game on its map from the file
// it was written to, each brush entity named by its place and its model, then put where the
// played BSP is; one for a map the game is not on is dropped
#[test]
fn a_full_build_is_swapped_into_the_running_game() {
    let dir = Scratch::new("full-swap");
    let target = dir.0.join("test.bsp");
    std::fs::write(&target, b"the fast compile").unwrap();
    let (preview, seen) = &mut playing_test();
    preview.start_full_build("test".to_owned(), target.clone(), room_inputs());
    preview.wait_for_full_build();
    let sent = std::mem::take(&mut *seen.lock().unwrap());
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].0, "map/swap");
    let side = dir.0.join("test.full.bsp");
    assert_eq!(
        sent[0].1,
        json!({
            "bsp": side.to_string_lossy(),
            "models": [{ "map_entity": 3, "model": "*1" }],
        })
    );
    assert!(!side.exists(), "put in place");
    assert_ne!(std::fs::read(&target).unwrap(), b"the fast compile");
    assert!(
        preview
            .console()
            .any(|line| line.text.contains("The full compile is in")),
        "said in the console"
    );

    preview.start_full_build("other".to_owned(), dir.0.join("other.bsp"), room_inputs());
    preview.wait_for_full_build();
    assert!(
        seen.lock().unwrap().is_empty(),
        "the game is not on that map"
    );
    assert!(
        !dir.0.join("other.full.bsp").exists(),
        "its file taken away"
    );
}

// No spec statement: P7-02: a full compile is not swapped in when the map's entities were edited
// in the game while it ran, which moves their places, nor when another compile was written over
// the played BSP; the first is said in the console
#[test]
fn a_stale_full_build_is_dropped() {
    let dir = Scratch::new("full-stale");
    let target = dir.0.join("test.bsp");
    std::fs::write(&target, b"the fast compile").unwrap();
    let (preview, recorded) = &mut playing_test();
    preview.start_full_build("test".to_owned(), target.clone(), room_inputs());
    // An edit reaches the game while the build runs.
    let before = qs_map::MapFile::parse(MAP.as_bytes()).unwrap().entities;
    preview.send_map_changes(
        &dir.0.join("test.map"),
        &before,
        &[qs_map_edit::LiveChange::Remove {
            place: qs_map_edit::EntityIndex::new(2),
        }],
    );
    recorded.lock().unwrap().clear();
    preview.wait_for_full_build();
    assert!(recorded.lock().unwrap().is_empty(), "not swapped in");
    assert!(
        preview
            .console()
            .any(|line| line.text.contains("changed in the game")),
        "said in the console"
    );

    preview.start_full_build("test".to_owned(), target.clone(), room_inputs());
    std::fs::write(&target, b"another compile").unwrap();
    preview.wait_for_full_build();
    assert!(
        recorded.lock().unwrap().is_empty(),
        "not swapped over the newer compile"
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"another compile");
}

// No spec statement: P7-02: one full compile runs at a time: one asked for while another runs
// starts when it is done, the other's result dropped
#[test]
fn a_full_build_asked_for_while_one_runs_replaces_it() {
    let dir = Scratch::new("full-queue");
    let target = dir.0.join("test.bsp");
    std::fs::write(&target, b"the fast compile").unwrap();
    let (preview, seen) = &mut playing_test();
    preview.start_full_build("test".to_owned(), dir.0.join("first.bsp"), room_inputs());
    preview.start_full_build("test".to_owned(), target.clone(), room_inputs());
    preview.wait_for_full_build();
    let sent = std::mem::take(&mut *seen.lock().unwrap());
    assert_eq!(sent.len(), 1, "only the last is swapped in: {sent:?}");
    assert_eq!(
        sent[0].1["bsp"],
        json!(dir.0.join("test.full.bsp").to_string_lossy())
    );
    assert!(
        !dir.0.join("first.full.bsp").exists(),
        "the first's file taken away"
    );
}

/// An embedded host that hands out a [`Recording`] host's client, and counts.
struct Window {
    shows: bool,
    counts: std::rc::Rc<std::cell::RefCell<(Vec<qs_engine::LaunchSpec>, u32)>>,
}

impl qs_engine::EmbeddedHost for Window {
    fn shows(&self) -> bool {
        self.shows
    }

    fn start(
        &mut self,
        spec: &qs_engine::LaunchSpec,
        _client_name: &str,
    ) -> Result<Client, qs_engine::LaunchError> {
        self.counts.borrow_mut().0.push(spec.clone());
        Ok(recording_client().0)
    }

    fn stop(&mut self) {
        self.counts.borrow_mut().1 += 1;
    }
}

// No spec statement: ENG-10, ADR-0028: a Play starts the game through the app's embedded host
// when it shows the game, with no host program and no window to place, and Stop stops it there;
// when it does not, the preview host's process is asked for as before
#[test]
fn a_play_starts_the_game_in_the_apps_own_window_when_it_shows_it() {
    use crate::preview::{PlayState, PreviewConfig, PreviewError};
    let dir = std::env::temp_dir().join(format!("qs-wb-embedded-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    let project = dir.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("progs.src"), "progs.dat\nworld.qc\n").unwrap();
    std::fs::write(project.join("world.qc"), "void() worldspawn = {};\n").unwrap();
    // A folder holding id1/pak0.pak is a game (FS-6.1).
    let game = dir.join("game");
    std::fs::create_dir_all(game.join("id1")).unwrap();
    std::fs::write(game.join("id1/pak0.pak"), b"PACK").unwrap();
    let config = || PreviewConfig {
        host: Some(dir.join("no-host")),
        app: PathBuf::from("/nowhere/app"),
        game: Some(game.clone()),
        extra: vec!["--nosound".to_owned()],
    };
    let mut workbench = Workbench::new(FsLoader, Config::default());
    workbench.open_project(&project).unwrap();

    let counts = std::rc::Rc::default();
    let window = |shows| {
        Box::new(Window {
            shows,
            counts: std::rc::Rc::clone(&counts),
        })
    };
    let mut hidden = Preview::with_embedded(config(), window(false));
    let error = hidden.play(&mut workbench, None).unwrap_err();
    assert!(matches!(error, PreviewError::Launch(_)), "{error}");
    assert!(counts.borrow().0.is_empty(), "the window was not asked");

    let mut shown = Preview::with_embedded(config(), window(true));
    shown
        .play(&mut workbench, Some("start".to_owned()))
        .unwrap();
    assert_eq!(shown.state(), PlayState::Playing);
    assert!(shown.is_embedded());
    {
        let counts = counts.borrow();
        let spec = &counts.0[0];
        assert_eq!(spec.map.as_deref(), Some("start"));
        assert_eq!(spec.extra, ["--nosound"]);
        assert_eq!(spec.host, PathBuf::new(), "no host program is looked for");
    }
    shown.stop();
    assert_eq!(shown.state(), PlayState::Stopped);
    assert!(!shown.is_embedded());
    assert_eq!(counts.borrow().1, 1, "the window stopped its game");
    let _gone = std::fs::remove_dir_all(&dir);
}
