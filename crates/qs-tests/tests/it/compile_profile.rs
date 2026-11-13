// SPDX-License-Identifier: GPL-3.0-or-later

//! Compile profiles (P6-20, MAP-28): the built-in Full profile run on a map open in the
//! workbench compiles it with qs-map-bsp, gives it visibility with qs-map-vis and lights it with
//! qs-map-light, then starts the game; here, with no game to start, the BSP it made is played in
//! `qs-sim`, the preview host's simulation, where the player walks from one room into the other.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::{Path, PathBuf};
use std::rc::Rc;

use qc_progs::Progs;
use qc_project::FsLoader;
use qs_bsp::{Bsp, BspLoader};
use qs_commands::{Registry, Session};
use qs_sim::{LoadOptions, UserCmd, World};
use qs_workbench::map_edit::MAP_OPEN;
use qs_workbench::map_profile::{FULL_PROFILE, MAP_RUN_PROFILE, ProfileRun, TaskResult};
use qs_workbench::{
    Config, HasPreview, HasWorkbench, Preview, PreviewConfig, Workbench, register, register_preview,
};
use serde_json::json;

use crate::play_from_here::map;
use crate::shape_stairs::{FRAME, progs};

/// A workbench and a preview whose game folder is not there, so no game starts.
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

impl HasPreview for Context {
    fn workbench_and_preview(&mut self) -> (&mut Workbench<FsLoader>, &mut Preview) {
        (&mut self.workbench, &mut self.preview)
    }
}

/// The two rooms written to a folder of their own; the map's path.
fn written() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("qs-it-profile-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rooms.map");
    std::fs::write(&path, map()).unwrap();
    path
}

// No spec statement: P6-20: the Full profile compiles, gives visibility to and lights the map,
// each task timed, and stops at launching with no game; the BSP it made plays: the player
// walks through the doorway into the other room
#[test]
fn the_full_profile_makes_a_map_that_plays() {
    let path = written();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    register_preview(&mut registry).unwrap();
    let nowhere = path.with_file_name("no-game-here");
    let context = Context {
        workbench: Workbench::new(FsLoader, Config::default()),
        preview: Preview::new(PreviewConfig {
            host: None,
            app: Path::new("/nowhere/app").to_path_buf(),
            game: Some(nowhere),
            extra: Vec::new(),
        }),
    };
    let mut session = Session::new(context, registry);
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    let run: ProfileRun = serde_json::from_value(
        session
            .execute(MAP_RUN_PROFILE, json!({ "profile": FULL_PROFILE }))
            .unwrap(),
    )
    .unwrap();
    let names: Vec<&str> = run.tasks.iter().map(|task| task.task.name()).collect();
    assert_eq!(names, ["bsp", "vis", "light", "launch"]);
    let results: Vec<bool> = run
        .tasks
        .iter()
        .map(|task| task.result == TaskResult::Done)
        .collect();
    assert_eq!(results, [true, true, true, false], "{run:?}");
    let bytes: &'static [u8] = Box::leak(
        std::fs::read(path.with_extension("bsp"))
            .unwrap()
            .into_boxed_slice(),
    );
    let bsp = Bsp::read(bytes, BspLoader::default()).unwrap();
    assert!(!bsp.visibility.is_empty(), "vis ran");
    assert!(!bsp.lighting.is_empty(), "light ran");
    let progs_bytes = progs();
    let progs = Progs::read(&progs_bytes).unwrap();
    let mut world = World::load(
        &progs,
        "rooms",
        &bsp,
        Rc::new(|_: &str| None),
        LoadOptions::default(),
    )
    .unwrap();
    world.settle(&progs).unwrap();
    world.connect(&progs).unwrap();
    world.frame(&progs, FRAME).unwrap();
    // Due east from the west room, through the doorway at y from -32 to 32.
    world
        .move_player(&progs, [-96.0, 0.0, 24.0], [0.0; 3])
        .unwrap();
    world.set_command(UserCmd {
        angles: [0.0, 0.0, 0.0],
        forward: 320.0,
        ..UserCmd::default()
    });
    let mut through = false;
    for _ in 0..216 {
        world.frame(&progs, FRAME).unwrap();
        let (eye, _) = world.player_view().unwrap();
        if eye[0] > 64.0 {
            through = true;
            break;
        }
    }
    assert!(through, "walked into the east room");
}
