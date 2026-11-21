// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::MemoryLoader;

use super::{ScenarioRunError, image_path};
use crate::preview::{Preview, PreviewConfig, PreviewError};
use crate::{Config, Workbench};

/// A fresh directory for the test `name`.
fn dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "qs-workbench-scenario-{name}-{}",
        std::process::id()
    ));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn preview() -> Preview {
    Preview::new(PreviewConfig {
        host: None,
        app: PathBuf::from("/nowhere/app"),
        game: Some(PathBuf::from("/nowhere/game")),
        extra: Vec::new(),
    })
}

// spec SCN-2.6: a screenshot is written beside its scenario as <scenario>-<name>.png
#[test]
fn a_screenshot_is_named_after_its_scenario() {
    assert_eq!(
        image_path(Path::new("/p/scenarios/bounce.scenario"), "hall"),
        PathBuf::from("/p/scenarios/bounce-hall.png")
    );
    assert_eq!(
        image_path(Path::new("bounce"), "a-1"),
        PathBuf::from("bounce-a-1.png")
    );
}

// spec SCN-2.6: scenario.run reads the file first, refusing one it cannot read or that does
// not read as SCN-1 says, with its path, then needs a project that builds
#[test]
fn a_scenario_is_read_before_the_project_builds() {
    let root = dir("read");
    let mut workbench = Workbench::new(MemoryLoader::new(), Config::default());
    let missing = root.join("missing.scenario");
    let error = preview()
        .run_scenario(&mut workbench, &missing)
        .unwrap_err();
    assert!(matches!(error, ScenarioRunError::Read(..)), "{error}");
    assert!(error.to_string().contains("missing.scenario"));
    let bad = root.join("bad.scenario");
    std::fs::write(&bad, "map a\nfly\n").unwrap();
    let error = preview().run_scenario(&mut workbench, &bad).unwrap_err();
    assert!(matches!(error, ScenarioRunError::Parse(..)), "{error}");
    assert!(
        error.to_string().ends_with("line 2: unknown line"),
        "{error}"
    );
    let good = root.join("good.scenario");
    std::fs::write(&good, "map start\nframes 1\n").unwrap();
    let error = preview().run_scenario(&mut workbench, &good).unwrap_err();
    assert!(
        matches!(error, ScenarioRunError::Preview(PreviewError::Build(_))),
        "{error}"
    );
}

// spec SCN-2.7: a scenario on a map the project compiled finds its BSP, as
// Play does: the one in the project's maps is put where the game reads maps, with the
// project's other files, before the game's files are looked for
#[test]
fn a_scenario_plays_a_map_the_project_compiled() {
    let root = dir("own-map");
    let mut loader = MemoryLoader::new();
    for (name, text) in [
        ("progs.src", "progs.dat\nworld.qc\n"),
        ("world.qc", "void() worldspawn = {};\n"),
    ] {
        loader.insert(root.join(name), text.as_bytes().to_vec());
    }
    let mut workbench = Workbench::new(loader, Config::default());
    workbench.open_project(&root).unwrap();
    std::fs::create_dir_all(root.join("maps")).unwrap();
    std::fs::write(root.join("maps/room.bsp"), b"the project's own").unwrap();
    std::fs::create_dir_all(root.join("progs")).unwrap();
    std::fs::write(root.join("progs/thing.mdl"), b"a model").unwrap();
    let scenario = root.join("room.scenario");
    std::fs::write(&scenario, "map room\nframes 1\n").unwrap();
    // There is no game at /nowhere/game, so nothing is played; the map is in place by then.
    let error = preview()
        .run_scenario(&mut workbench, &scenario)
        .unwrap_err();
    assert!(matches!(error, ScenarioRunError::Preview(_)), "{error}");
    let placed: Vec<PathBuf> = std::fs::read_dir(root.join("build"))
        .unwrap()
        .filter_map(|entry| Some(entry.ok()?.path().join("maps/room.bsp")))
        .filter(|path| path.is_file())
        .collect();
    assert_eq!(placed.len(), 1, "{placed:?}");
    let placed = placed.first().unwrap();
    assert_eq!(std::fs::read(placed).unwrap(), b"the project's own");
    // And the project's other files a game reads, as Play places them.
    let model = placed.with_file_name("..").join("progs/thing.mdl");
    assert_eq!(std::fs::read(model).unwrap(), b"a model");

    // The BSP beside the open map of the scenario's name is the one played, wherever the
    // map is; one beside an open map of another name is not.
    let elsewhere = root.join("fresh");
    std::fs::create_dir_all(&elsewhere).unwrap();
    for name in ["arena", "other"] {
        std::fs::write(elsewhere.join(format!("{name}.bsp")), name).unwrap();
    }
    let arena = elsewhere.join("arena.map");
    std::fs::write(&arena, "{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    workbench.maps_mut().open_map(&arena).unwrap();
    for (name, there) in [("arena", true), ("other", false)] {
        let scenario = root.join(format!("{name}.scenario"));
        std::fs::write(&scenario, format!("map {name}\nframes 1\n")).unwrap();
        assert!(preview().run_scenario(&mut workbench, &scenario).is_err());
        let played = placed.with_file_name(format!("{name}.bsp"));
        assert_eq!(played.is_file(), there, "{name}");
    }
}
