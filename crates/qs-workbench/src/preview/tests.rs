// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::MemoryLoader;
use serde_json::json;

use super::{PlayState, Preview, PreviewConfig, PreviewError, PreviewStatus, mod_folder};
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

// No spec statement: the controls need a running game; a reload without one does nothing
#[test]
fn the_controls_need_a_running_game() {
    let mut game = preview(None, None);
    assert!(matches!(game.pause(), Err(PreviewError::NotRunning)));
    assert!(matches!(game.resume(), Err(PreviewError::NotRunning)));
    assert!(matches!(game.step(), Err(PreviewError::NotRunning)));
    assert!(matches!(game.restart(), Err(PreviewError::NotRunning)));
    let mut workbench = Workbench::new(MemoryLoader::new(), Config::default());
    assert!(game.reload(&mut workbench).unwrap().is_none());
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
    game.take_event("console/line", &json!({}));
    assert_eq!(game.console().last().unwrap().text, "");
    assert_eq!(
        PreviewStatus::of(&game),
        PreviewStatus {
            state: "playing".to_owned(),
            status: None,
        }
    );
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
    assert!(matches!(game.select(3), Err(PreviewError::NotRunning)));
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
    assert_eq!(game.pending(3, "health"), Some("6"));
    assert_eq!(game.pending(4, "health"), None);
    assert_eq!(game.pending(3, "armor"), None);
}
