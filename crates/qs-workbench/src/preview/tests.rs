// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::MemoryLoader;
use serde_json::json;

use qs_engine::Flavour;

use super::debug::StopReason;
use super::{
    PlayState, Preview, PreviewConfig, PreviewError, PreviewStatus, SelectedEntity, mod_folder,
    place_project_assets, place_project_map,
};
use crate::ids::{Edict, WatchIndex};
use crate::{Config, Workbench, WorkbenchError};

/// A fresh directory for the test `name`.
fn root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "qs-workbench-preview-{name}-{}",
        std::process::id()
    ));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A workbench over a bare project in `root` whose `world.qc` is `world`, opened.
fn opened(root: &Path, world: &str) -> Workbench<MemoryLoader> {
    let mut loader = MemoryLoader::new();
    for (name, text) in [("progs.src", "progs.dat\nworld.qc\n"), ("world.qc", world)] {
        loader.insert(root.join(name), text.as_bytes().to_vec());
    }
    let mut workbench = Workbench::new(loader, Config::default());
    workbench.open_project(root).unwrap();
    workbench
}

/// A preview whose game is `game` and whose host is `host`.
fn preview(game: Option<PathBuf>, host: Option<PathBuf>) -> Preview {
    Preview::new(PreviewConfig {
        host,
        app: PathBuf::from("/nowhere/app"),
        game,
        extra: Vec::new(),
    })
}

// No spec statement: playing builds the open project first, so no project or a project with
// errors starts no game, and the state stays stopped
#[test]
fn play_needs_a_project_that_builds() {
    let dir = root("build");
    let mut none = Workbench::new(MemoryLoader::new(), Config::default());
    let mut game = preview(None, None);
    assert!(matches!(
        game.play(&mut none, None),
        Err(PreviewError::Build(WorkbenchError::NoProject))
    ));
    let mut broken = opened(&dir, "void() worldspawn = { nosuch(); };\n");
    let error = game.play(&mut broken, None).unwrap_err();
    assert!(matches!(error, PreviewError::NothingBuilt(_)), "{error}");
    assert!(error.to_string().contains("1 modules with errors"));
    assert_eq!(game.state(), PlayState::Stopped);
}

// No spec statement: a game folder with no game, or a host that is not there, starts nothing
#[test]
fn play_needs_a_game_and_a_host() {
    let dir = root("game");
    let mut workbench = opened(&dir, "void() worldspawn = {};\n");
    let empty = root("empty-game");
    let mut no_game = preview(Some(empty.clone()), None);
    let error = no_game.play(&mut workbench, None).unwrap_err();
    assert!(matches!(&error, PreviewError::NoGame(Some(path)) if *path == empty));
    assert!(error.to_string().contains("no Quake game here"));
    // A folder holding id1/pak0.pak is a game (FS-6.1); the host is still missing.
    let game = root("a-game");
    std::fs::create_dir_all(game.join("id1")).unwrap();
    std::fs::write(game.join("id1/pak0.pak"), b"PACK").unwrap();
    let mut no_host = preview(Some(game), Some(dir.join("no-host")));
    let error = no_host.play(&mut workbench, None).unwrap_err();
    assert!(matches!(error, PreviewError::Launch(_)), "{error}");
    assert_eq!(no_host.state(), PlayState::Stopped);
    // The run root was made, linking the build.
    assert!(dir.join("build/run/original/id1").exists());
}

// No spec statement: ADR-0008: a project for the rerelease plays on the rerelease's files,
// in the run root of that name with its `QuakeEX.kpf`; one for the original game on the
// original's; and a rerelease project on an install with no rerelease on the original's
#[test]
fn play_uses_the_game_the_project_is_for() {
    let game = root("both-games");
    for id1 in ["id1", "rerelease/id1"] {
        std::fs::create_dir_all(game.join(id1)).unwrap();
        std::fs::write(game.join(id1).join("pak0.pak"), b"PACK").unwrap();
    }
    std::fs::write(game.join("rerelease/QuakeEX.kpf"), b"kpf").unwrap();
    let rerelease = |dir: &Path| {
        let mut loader = MemoryLoader::new();
        for (name, text) in [
            (
                "quakec.toml",
                "[project]\nname = \"m\"\ndialect = \"fteqcc\"\ntarget = \"rerelease\"\n\n\
                 [[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n",
            ),
            ("progs.src", "progs.dat\nworld.qc\n"),
            ("world.qc", "void() worldspawn = {};\n"),
        ] {
            loader.insert(dir.join(name), text.as_bytes().to_vec());
        }
        let mut workbench = Workbench::new(loader, Config::default());
        workbench.open_project(dir).unwrap();
        workbench
    };
    let linked = |link: PathBuf| std::fs::canonicalize(link).unwrap();
    let dir = root("for-the-rerelease");
    let mut workbench = rerelease(&dir);
    assert_eq!(workbench.flavour(), Flavour::Rerelease);
    let mut preview_of = preview(Some(game.clone()), Some(dir.join("no-host")));
    let error = preview_of.play(&mut workbench, None).unwrap_err();
    assert!(matches!(error, PreviewError::Launch(_)), "{error}");
    assert_eq!(
        linked(dir.join("build/run/rerelease/id1")),
        linked(game.join("rerelease/id1"))
    );
    // On Windows the kpf is given to the host with `-kpf`, so the root has no link.
    assert_eq!(
        dir.join("build/run/rerelease/QuakeEX.kpf").exists(),
        !cfg!(windows)
    );
    assert!(!dir.join("build/run/original").exists());
    // The game's files are the rerelease's from then on.
    assert_eq!(
        preview_of.game_base_path().map(linked),
        Some(linked(game.join("rerelease")))
    );
    // A project for the original game, on the same install.
    let dir = root("for-the-original");
    let mut workbench = opened(&dir, "void() worldspawn = {};\n");
    assert_eq!(workbench.flavour(), Flavour::Original);
    let mut preview_of = preview(Some(game.clone()), Some(dir.join("no-host")));
    assert!(preview_of.play(&mut workbench, None).is_err());
    assert_eq!(
        linked(dir.join("build/run/original/id1")),
        linked(game.join("id1"))
    );
    // With no rerelease in the install, the rerelease project plays as the original.
    let only = root("original-only");
    std::fs::create_dir_all(only.join("id1")).unwrap();
    std::fs::write(only.join("id1/pak0.pak"), b"PACK").unwrap();
    let dir = root("rerelease-on-original");
    let mut workbench = rerelease(&dir);
    let mut preview_of = preview(Some(only.clone()), Some(dir.join("no-host")));
    assert!(preview_of.play(&mut workbench, None).is_err());
    assert_eq!(
        linked(dir.join("build/run/original/id1")),
        linked(only.join("id1"))
    );
    assert!(!dir.join("build/run/rerelease").exists());
    // With the rerelease alone, the game's files are read from it before the first Play, and
    // a project for the original game has nothing to play on.
    let alone = root("rerelease-alone");
    std::fs::create_dir_all(alone.join("id1")).unwrap();
    std::fs::write(alone.join("id1/pak0.pak"), b"PACK").unwrap();
    std::fs::write(alone.join("QuakeEX.kpf"), b"kpf").unwrap();
    let mut preview_of = preview(Some(alone.clone()), Some(dir.join("no-host")));
    assert_eq!(preview_of.game_base_path().map(linked), Some(linked(alone)));
    let dir = root("original-on-rerelease");
    let mut workbench = opened(&dir, "void() worldspawn = {};\n");
    let error = preview_of.play(&mut workbench, None).unwrap_err();
    assert!(matches!(error, PreviewError::NoGame(Some(_))), "{error}");
}

// No spec statement: the controls need a running game; a reload without one does nothing
#[test]
fn the_controls_need_a_running_game() {
    let mut game = preview(None, None);
    assert!(matches!(game.pause(), Err(PreviewError::NotRunning)));
    assert!(matches!(game.resume(), Err(PreviewError::NotRunning)));
    assert!(matches!(game.step(), Err(PreviewError::NotRunning)));
    assert!(matches!(game.restart(), Err(PreviewError::NotRunning)));
    let mut workbench = Workbench::new(MemoryLoader::new(), Config::default());
    assert!(
        game.reload(&mut workbench, super::OnLoss::Ask)
            .unwrap()
            .is_none()
    );
    game.stop();
    game.poll();
    assert_eq!(game.state(), PlayState::Stopped);
    assert_eq!(
        PreviewError::NotRunning.to_string(),
        "the game is not running"
    );
}

// spec BRIDGE-3.1, BRIDGE-3.3: state/changed sets the play state, console/line adds a line,
// at most a thousand kept; any other event changes neither
#[test]
fn events_set_the_state_and_the_console() {
    let mut game = preview(None, None);
    game.take_event(
        "state/changed",
        &json!({ "state": "paused", "reason": "pause" }),
    );
    assert_eq!(game.state(), PlayState::Paused);
    game.take_event("state/changed", &json!({ "state": "running" }));
    assert_eq!(game.state(), PlayState::Playing);
    game.take_event("edict/changed", &json!({ "edict": 1 }));
    assert_eq!(game.state(), PlayState::Playing);
    for n in 0..1005 {
        game.take_event(
            "console/line",
            &json!({ "text": format!("line {n}"), "level": "print" }),
        );
    }
    let lines: Vec<_> = game.console().collect();
    assert_eq!(lines.len(), 1000);
    assert_eq!(lines[0].text, "line 5", "the oldest dropped");
    assert_eq!(lines[999].level, "print");
    // The filter keeps the lines holding its text, in any case.
    let shown = game.console().filter(|l| l.matches(" LINE 100")).count();
    assert_eq!(shown, 6, "line 100, and line 1000 to line 1004");
    assert_eq!(game.console().filter(|l| l.matches("  ")).count(), 1000);
    assert_eq!(game.console().filter(|l| l.matches("nothing")).count(), 0);
    game.take_event("console/line", &json!({}));
    assert_eq!(game.console().last().unwrap().text, "");
    assert_eq!(
        PreviewStatus::of(&game),
        PreviewStatus {
            state: "playing".to_owned(),
            status: None,
            picked: None,
            would_reset: Vec::new(),
            stopped: None,
            watches: Vec::new(),
            held: Vec::new(),
        }
    );
}

// spec BRIDGE-3.5: edict/picked names the picked edict's classname to the next status, for the
// shell to open its spawn function; a later poll names none
#[test]
fn a_picked_edict_names_its_spawn_function() {
    let mut game = preview(None, None);
    game.take_event(
        "edict/spawned",
        &json!({ "edict": 14, "classname": "monster_ogre" }),
    );
    game.take_event("edict/picked", &json!({ "edict": 14, "time": 1.0 }));
    assert_eq!(
        PreviewStatus::of(&game).picked.as_deref(),
        Some("monster_ogre")
    );
    game.poll();
    assert_eq!(PreviewStatus::of(&game).picked, None);
    game.take_event("edict/picked", &json!({ "edict": 99, "time": 1.0 }));
    assert_eq!(
        PreviewStatus::of(&game).picked,
        None,
        "an edict not listed names nothing"
    );
}

// spec BRIDGE-4.11: a dry run's lost values are what the IDE says the reload would reset, each
// as its kind, name and reason; a report that loses nothing holds no reload
#[test]
fn a_dry_runs_losses_are_what_would_reset() {
    let report = json!({
        "migrated": 40,
        "lost": [
            { "name": "dog_think", "kind": "function", "reason": "removed, and edict 2 .think named it: now 0" },
            { "name": "count", "kind": "field", "reason": "removed" },
        ],
        "added": [{ "name": "tally", "kind": "field", "reason": "added" }],
        "breakpoints_cleared": [],
        "time": 1.0,
    });
    assert_eq!(
        super::would_reset(&report),
        [
            "function dog_think: removed, and edict 2 .think named it: now 0",
            "field count: removed",
        ]
    );
    assert!(super::would_reset(&json!({ "migrated": 3, "lost": [] })).is_empty());
    assert!(super::would_reset(&json!({})).is_empty());
    let asked: super::ReloadParams = serde_json::from_value(json!({ "on_loss": "ask" })).unwrap();
    assert_eq!(asked.on_loss, super::OnLoss::Ask);
    let plain: super::ReloadParams = serde_json::from_value(json!({})).unwrap();
    assert_eq!(plain.on_loss, super::OnLoss::Reload);
}

// No spec statement: the program plays from `<project>/build/<mod>/`, which the run root links
#[test]
fn the_mod_is_the_build_folder() {
    let project = Path::new("/p");
    assert_eq!(
        mod_folder(project, Path::new("/p/build/ember/progs.dat")),
        Some("ember".to_owned())
    );
    assert_eq!(mod_folder(project, Path::new("/p/out/progs.dat")), None);
    assert_eq!(mod_folder(project, Path::new("/p/build/progs.dat")), None);
    assert_eq!(mod_folder(project, Path::new("progs.dat")), None);
}

// No spec statement: fields are edited only while paused, on a field the selected edict has,
// with a value of its type; selecting and picking need a running game
#[test]
fn field_edits_need_a_paused_game_and_a_field() {
    let mut game = preview(None, None);
    assert!(matches!(
        game.set_field("health", "5"),
        Err(PreviewError::NotPaused)
    ));
    game.state = PlayState::Paused;
    assert!(
        matches!(game.set_field("health", "5"), Err(PreviewError::NoField(f)) if f == "health")
    );
    game.entities.select(
        &json!({ "edict": 3, "classname": "monster_army", "fields": [
        { "name": "health", "value": { "type": "float", "value": 30 } }
    ] }),
    );
    assert!(matches!(
        game.set_field("armor", "5"),
        Err(PreviewError::NoField(_))
    ));
    let bad = game.set_field("health", "lots").unwrap_err();
    assert_eq!(bad.to_string(), "health takes a float");
    assert!(matches!(
        game.set_field("health", "5"),
        Err(PreviewError::NotRunning)
    ));
    assert!(matches!(
        game.select(Edict::new(3)),
        Err(PreviewError::NotRunning)
    ));
    assert!(matches!(game.pick(0.5, 0.5), Err(PreviewError::NotRunning)));
    assert!(matches!(game.refresh(), Err(PreviewError::NotRunning)));
    assert_eq!(
        PreviewError::NotPaused.to_string(),
        "pause the game to edit its fields"
    );
}

// spec BRIDGE-4.4: an edit made while paused is held until resume; the last one typed for a
// field is the one shown
#[test]
fn edits_held_until_resume_are_shown() {
    let mut game = preview(None, None);
    game.pending.push((3, "health".to_owned(), "5".to_owned()));
    game.pending.push((3, "health".to_owned(), "6".to_owned()));
    game.pending.push((4, "armor".to_owned(), "1".to_owned()));
    assert_eq!(game.pending(Edict::new(3), "health"), Some("6"));
    // A status lists them, for a client that reads the game's fields and finds the old values.
    let held = PreviewStatus::of(&game).held;
    assert_eq!(held.len(), 3);
    assert_eq!(
        (
            held[1].edict,
            held[1].field.as_str(),
            held[1].value.as_str()
        ),
        (Edict::new(3), "health", "6")
    );
    assert_eq!(game.pending(Edict::new(4), "health"), None);
    assert_eq!(game.pending(Edict::new(3), "armor"), None);
}

// spec BRIDGE-3.3: state/changed to paused at a breakpoint stops the debugger there, to running
// forgets it; qc/error is an error line in the console
#[test]
fn stops_reach_the_debugger_and_the_console() {
    let mut game = preview(None, None);
    game.take_event(
        "state/changed",
        &json!({ "state": "paused", "reason": "breakpoint",
        "location": { "function": "Ember_Touch", "file": "src/weapons.qc", "line": 33 } }),
    );
    assert_eq!(game.state(), PlayState::Paused);
    assert_eq!(game.debugger().stopped().unwrap().1.function, "Ember_Touch");
    // A status says where and why, for a client that only polls.
    let stopped = PreviewStatus::of(&game).stopped.unwrap();
    assert_eq!(stopped.reason, StopReason::Breakpoint);
    assert_eq!(stopped.location.line, Some(33));
    game.take_event(
        "state/changed",
        &json!({ "state": "paused", "reason": "pause" }),
    );
    game.take_event("state/changed", &json!({ "state": "running" }));
    assert!(game.debugger().stopped().is_none());
    assert_eq!(PreviewStatus::of(&game).stopped, None);
    game.take_event("qc/error", &json!({ "message": "NULL function" }));
    assert!(game.console().last().unwrap().is_error());
    assert_eq!(game.debugger().error(), Some("NULL function"));
    // Without a game a breakpoint is kept for the next; a console command needs one.
    game.toggle_breakpoint(Path::new("src/a.qc"), 3).unwrap();
    assert_eq!(game.debugger().breakpoints().len(), 1);
    assert!(matches!(
        game.console_command("impulse 9"),
        Err(PreviewError::NotRunning)
    ));
    game.watch("time").unwrap();
    assert_eq!(game.debugger().watches().len(), 1);
    assert_eq!(PreviewStatus::of(&game).watches[0].expression, "time");
    game.unwatch(WatchIndex::new(0));
    assert!(game.step_by(qs_engine::StepKind::Over).is_err());
}

// No spec statement: with no game the IDE's window is remembered for the next game, which opens
// where it docks, and the toolbar says the window docks
#[test]
fn docking_with_no_game_is_kept_for_the_next() {
    let mut game = preview(None, None);
    let frame = super::dock::DockFrame {
        x: 0.0,
        y: 0.0,
        width: 1200.0,
        height: 900.0,
        screen_width: Some(2560.0),
        focused: true,
    };
    game.dock(frame).unwrap();
    assert_eq!(game.dock.0, Some(frame));
    assert_eq!(game.dock.1, None, "nothing was sent");
    assert!(game.docks());
}

// No spec statement: P6-44: a pick whose ray hits no edict answers no edict, not the one
// selected before; one that hits answers the edict it selected
#[test]
fn a_pick_that_misses_answers_no_edict() {
    let mut game = preview(None, None);
    game.entities
        .select(&json!({ "edict": 14, "classname": "door", "fields": [] }));
    let selected = SelectedEntity::of(&game);
    assert_eq!(
        selected.classname.as_deref(),
        Some("door"),
        "an edict is selected"
    );
    let missed = SelectedEntity::picked(&game, None);
    assert_eq!((missed.edict, missed.classname), (None, None));
    let hit = SelectedEntity::picked(&game, Some(14));
    assert_eq!(hit.classname.as_deref(), Some("door"));
}

// No spec statement: L-47, ADR-0008: Play puts the project's own assets in the mod's folder,
// where the game reads them: a model, a sound, a file in a folder of a folder and a compiled
// map that is an item's model; its map sources, what a compile leaves beside a map, and the
// programs at the folder's top are not touched
#[test]
fn a_projects_assets_are_put_where_the_game_reads_them() {
    let project = root("place-assets");
    let built = project.join("build").join("mod");
    let write = |file: &str, bytes: &[u8]| {
        let path = project.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    };
    let placed = |file: &str| std::fs::read(built.join(file)).ok();
    write("progs/brute.mdl", b"model one");
    write("sound/brute/sight.wav", b"sound");
    write("maps/room.map", b"a source");
    write("maps/room.bsp", b"a map");
    write("maps/room.prt", b"left by the compile");
    write("maps/b_box.bsp", b"an item");
    write("maps/b_box.lit", b"its light");
    write("notes.txt", b"not an asset");
    std::fs::create_dir_all(&built).unwrap();
    std::fs::write(built.join("progs.dat"), b"the program").unwrap();
    place_project_assets(&project, "mod", None).unwrap();
    for (file, bytes) in [
        ("progs/brute.mdl", b"model one".as_slice()),
        ("sound/brute/sight.wav", b"sound"),
        ("maps/room.bsp", b"a map"),
        ("maps/b_box.bsp", b"an item"),
        ("maps/b_box.lit", b"its light"),
        ("progs.dat", b"the program"),
    ] {
        assert_eq!(placed(file).as_deref(), Some(bytes), "{file}");
    }
    for file in ["maps/room.map", "maps/room.prt", "notes.txt"] {
        assert!(placed(file).is_none(), "{file}");
    }

    // A project with no assets places nothing and fails nothing.
    let bare = root("place-none");
    place_project_assets(&bare, "mod", None).unwrap();
    assert!(!bare.join("build").exists());
}

// No spec statement: L-47, ADR-0008: a placed file is placed again when the project's file is
// no longer the one it was made from, newer or older, and not otherwise; the files of the
// map being played are left to the play
#[test]
fn an_asset_is_placed_again_when_its_file_changes() {
    let project = root("place-again");
    let built = project.join("build").join("mod");
    let write = |file: &str, bytes: &[u8]| {
        let path = project.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    };
    let dated = |file: &str, at: std::time::SystemTime| {
        std::fs::File::options()
            .write(true)
            .open(project.join(file))
            .unwrap()
            .set_modified(at)
            .unwrap();
    };
    let placed = |file: &str| std::fs::read(built.join(file)).ok();
    let now = std::time::SystemTime::now();
    write("progs/brute.mdl", b"model one");
    write("maps/room.bsp", b"a map");
    place_project_assets(&project, "mod", None).unwrap();

    // The project's file as it was: what is in the folder stays, whatever it is.
    std::fs::write(built.join("progs/brute.mdl"), b"model built").unwrap();
    place_project_assets(&project, "mod", None).unwrap();
    assert_eq!(
        placed("progs/brute.mdl").as_deref(),
        Some(b"model built".as_slice())
    );
    // A file of the same size from years ago takes its place: older, and placed.
    write("progs/brute.mdl", b"model old");
    dated(
        "progs/brute.mdl",
        now - std::time::Duration::from_secs(600_000_000),
    );
    place_project_assets(&project, "mod", None).unwrap();
    assert_eq!(
        placed("progs/brute.mdl").as_deref(),
        Some(b"model old".as_slice())
    );
    // And a newer one.
    write("progs/brute.mdl", b"model two");
    dated("progs/brute.mdl", now + std::time::Duration::from_secs(60));
    place_project_assets(&project, "mod", None).unwrap();
    assert_eq!(
        placed("progs/brute.mdl").as_deref(),
        Some(b"model two".as_slice())
    );
    // A placed file that is gone is placed again.
    std::fs::remove_file(built.join("progs/brute.mdl")).unwrap();
    place_project_assets(&project, "mod", None).unwrap();
    assert_eq!(
        placed("progs/brute.mdl").as_deref(),
        Some(b"model two".as_slice())
    );

    // The map being played is the play's to place: a changed one is not placed here, and is
    // on the next Play of another map.
    std::fs::write(built.join("maps/room.bsp"), b"placed for the play").unwrap();
    write("maps/room.bsp", b"compiled again");
    place_project_assets(&project, "mod", Some("room")).unwrap();
    assert_eq!(
        placed("maps/room.bsp").as_deref(),
        Some(b"placed for the play".as_slice())
    );
    place_project_assets(&project, "mod", Some("other")).unwrap();
    assert_eq!(
        placed("maps/room.bsp").as_deref(),
        Some(b"compiled again".as_slice())
    );
}

// No spec statement: L-47, ADR-0008: Play takes away only what it placed and the project no
// longer has: a file put in the mod's folder by hand stays, a file named anew in another
// case is there under its new name, and a list that names a file outside the folder removes
// nothing
#[test]
fn only_what_play_placed_is_taken_away() {
    let project = root("place-removal");
    let built = project.join("build").join("mod");
    let write = |file: &str, bytes: &[u8]| {
        let path = project.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    };
    let placed = |file: &str| std::fs::read(built.join(file)).ok();
    write("progs/brute.mdl", b"model");
    write("sound/brute/sight.wav", b"sound");
    // Put there by hand, as was the only way before Play placed anything.
    std::fs::create_dir_all(built.join("progs")).unwrap();
    std::fs::write(built.join("progs/byhand.mdl"), b"by hand").unwrap();
    place_project_assets(&project, "mod", None).unwrap();

    std::fs::remove_file(project.join("sound/brute/sight.wav")).unwrap();
    place_project_assets(&project, "mod", None).unwrap();
    assert!(placed("sound/brute/sight.wav").is_none());
    assert_eq!(
        placed("progs/byhand.mdl").as_deref(),
        Some(b"by hand".as_slice())
    );
    assert_eq!(
        placed("progs/brute.mdl").as_deref(),
        Some(b"model".as_slice())
    );

    // Named anew in another case: there under its new name on any file system, where one
    // that tells the two names apart no longer has the old one.
    std::fs::rename(
        project.join("progs/brute.mdl"),
        project.join("progs/Brute.mdl"),
    )
    .unwrap();
    place_project_assets(&project, "mod", None).unwrap();
    assert_eq!(
        placed("progs/Brute.mdl").as_deref(),
        Some(b"model".as_slice())
    );
    let names: Vec<String> = std::fs::read_dir(built.join("progs"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"Brute.mdl".to_owned()), "{names:?}");
    assert!(!names.contains(&"brute.mdl".to_owned()), "{names:?}");

    // A list that names a file outside the folder, or no file at all, removes nothing.
    let outside = project.join("notes.txt");
    std::fs::write(&outside, b"kept").unwrap();
    let list = built.join(".placed");
    let mut text = std::fs::read_to_string(&list).unwrap();
    text.push_str("../../notes.txt\t4\t0\n/etc/hosts\t1\t1\nno numbers\n\t1\t1\n");
    std::fs::write(&list, text).unwrap();
    place_project_assets(&project, "mod", None).unwrap();
    assert_eq!(std::fs::read(&outside).unwrap(), b"kept");
    assert_eq!(
        placed("progs/byhand.mdl").as_deref(),
        Some(b"by hand".as_slice())
    );
}

// No spec statement: P6-44, BSP-18.2, BSP-19.1: Play of a map the project compiled finds its
// BSP: the one beside the open map of that name, else the one in the project's maps, is put in
// the mod's maps when none is there or an older one, with its `.lit` and `.ent`, and those of
// an older compile are removed; a map the project did not compile, and a name that is no
// file's name, change nothing
#[test]
fn a_compiled_map_is_put_where_the_game_reads_it() {
    let project = root("place-map");
    let maps = project.join("maps");
    let played = project.join("build").join("mod").join("maps");
    std::fs::create_dir_all(&maps).unwrap();
    place_project_map(&project, "mod", "none", None).unwrap();
    assert!(!played.exists(), "no map of that name was compiled");

    std::fs::write(maps.join("room.bsp"), b"bsp one").unwrap();
    std::fs::write(maps.join("room.lit"), b"lit one").unwrap();
    std::fs::write(maps.join("room.ent"), b"ent one").unwrap();
    place_project_map(&project, "mod", "room", None).unwrap();
    for (file, bytes) in [
        ("room.bsp", "bsp one"),
        ("room.lit", "lit one"),
        ("room.ent", "ent one"),
    ] {
        assert_eq!(
            std::fs::read_to_string(played.join(file)).unwrap(),
            bytes,
            "{file}"
        );
    }

    // A copy as new as the compile is kept: what a play from the map view wrote there.
    std::fs::write(played.join("room.bsp"), b"lit by play").unwrap();
    place_project_map(&project, "mod", "room", None).unwrap();
    assert_eq!(
        std::fs::read(played.join("room.bsp")).unwrap(),
        b"lit by play"
    );

    // An older copy is replaced, and with a compile that has no `.lit` the old one goes.
    let long_ago = std::time::SystemTime::UNIX_EPOCH;
    std::fs::File::options()
        .write(true)
        .open(played.join("room.bsp"))
        .unwrap()
        .set_modified(long_ago)
        .unwrap();
    std::fs::remove_file(maps.join("room.lit")).unwrap();
    place_project_map(&project, "mod", "room", None).unwrap();
    assert_eq!(std::fs::read(played.join("room.bsp")).unwrap(), b"bsp one");
    assert!(!played.join("room.lit").exists());
    assert!(played.join("room.ent").exists());

    // The BSP beside the open map of that name is the one played, wherever the map is.
    let elsewhere = project.join("fresh");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::write(elsewhere.join("arena.bsp"), b"arena").unwrap();
    place_project_map(&project, "mod", "arena", Some(&elsewhere)).unwrap();
    assert_eq!(std::fs::read(played.join("arena.bsp")).unwrap(), b"arena");

    // A map open where the game reads maps is there already: copied onto itself it would be
    // emptied, as a newer `.ent` beside it once asked.
    std::fs::write(played.join("home.bsp"), b"home").unwrap();
    std::fs::File::options()
        .write(true)
        .open(played.join("home.bsp"))
        .unwrap()
        .set_modified(long_ago)
        .unwrap();
    std::fs::write(played.join("home.ent"), b"ent").unwrap();
    place_project_map(&project, "mod", "home", Some(&played)).unwrap();
    assert_eq!(std::fs::read(played.join("home.bsp")).unwrap(), b"home");
    assert_eq!(std::fs::read(played.join("home.ent")).unwrap(), b"ent");

    // A name that is a path is the host's to refuse: nothing is read or written for it.
    let outside = project.join("outside.bsp");
    std::fs::write(&outside, b"outside").unwrap();
    place_project_map(&project, "mod", "../outside", None).unwrap();
    place_project_map(&project, "mod", "sub/room", None).unwrap();
    assert!(
        !project
            .join("build")
            .join("mod")
            .join("outside.bsp")
            .exists()
    );
    assert_eq!(
        std::fs::read_dir(&played).unwrap().count(),
        5,
        "room's two, arena and home's two"
    );
}
