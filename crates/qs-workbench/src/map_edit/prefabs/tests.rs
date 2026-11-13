// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::path::{Path, PathBuf};

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use qs_map::MapFile;
use serde_json::json;

use super::{MAP_PLACE_PREFAB, MAP_PREFABS, MAP_SAVE_PREFAB, PrefabSaved, Prefabs};
use crate::map_edit::MAP_OPEN;
use crate::preview::Preview;
use crate::{Config, HasWorkbench, Workbench, register};

struct Context {
    workbench: Workbench<FsLoader>,
}

impl HasWorkbench for Context {
    type Loader = FsLoader;

    fn workbench(&mut self) -> &mut Workbench<FsLoader> {
        &mut self.workbench
    }

    fn preview(&mut self) -> Option<&mut Preview> {
        None
    }
}

/// A copy of the desktop's keep project in a folder of its own named `name`.
fn keep(name: &str) -> PathBuf {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/fixtures/keep");
    let dir = std::env::temp_dir()
        .join(format!("qs-wb-prefabs-{}", std::process::id()))
        .join(name);
    let _gone = std::fs::remove_dir_all(&dir);
    for file in ["progs.src", "defs.qc", "world.qc", "maps/keep.map"] {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixture.join(file), to).unwrap();
    }
    dir
}

/// A session with the copy of the keep project at `dir` and its map open.
fn session(dir: &Path) -> Session<Context> {
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let context = Context {
        workbench: Workbench::new(FsLoader, Config::default()),
    };
    let mut session = Session::new(context, registry);
    session
        .execute("project.open", json!({ "path": dir }))
        .unwrap();
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/keep.map") }))
        .unwrap();
    session
}

fn map(session: &Session<Context>) -> MapFile {
    session.context().workbench.map().unwrap().file().clone()
}

/// The least and greatest corner of every brush of `file`.
fn bounds(file: &MapFile) -> [[f64; 3]; 2] {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for corner in file
        .entities
        .iter()
        .flat_map(qs_map::Entity::brushes)
        .flat_map(qs_map::Brush::vertices)
    {
        for i in 0..3 {
            lo[i] = lo[i].min(corner[i]);
            hi[i] = hi[i].max(corner[i]);
        }
    }
    [lo, hi]
}

// No spec statement: P6-23, MAP-33: the selection saved as a prefab keeps the world's brushes
// in its world and the entities whole, around the middle of their floor on the grid; it is
// listed; placing it copies it in at a point on the grid in one step that undoes
#[test]
fn a_selection_saved_as_a_prefab_is_placed_again() {
    let dir = keep("save-place");
    let mut session = session(&dir);
    // The keep's floor, brush 0 of the world, from -256 to 256 across and -16 to 0 up, and
    // its door, entity 4.
    let saved: PrefabSaved = serde_json::from_value(
        session
            .execute(
                MAP_SAVE_PREFAB,
                json!({ "name": "floor-door", "brushes": [0], "entities": [4] }),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(saved.prefab.path, dir.join("prefabs/floor-door.map"));
    let prefab = MapFile::parse(&std::fs::read(&saved.prefab.path).unwrap()).unwrap();
    assert_eq!(prefab.entities.len(), 2);
    assert_eq!(prefab.entities[0].brushes().count(), 1);
    assert_eq!(prefab.entities[1].classname(), Some(&b"func_door"[..]));
    assert_eq!(prefab.entities[1].value(b"targetname"), Some(&b"gate"[..]));
    let [lo, hi] = bounds(&prefab);
    assert_eq!(
        (lo[2], (lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0),
        (0.0, 0.0, 0.0)
    );
    let listed: Prefabs =
        serde_json::from_value(session.execute(MAP_PREFABS, json!({})).unwrap()).unwrap();
    assert_eq!(listed.prefabs.len(), 1);
    assert_eq!(listed.prefabs[0].name, "floor-door");
    let before = map(&session);
    session
        .execute(
            MAP_PLACE_PREFAB,
            json!({ "path": "prefabs/floor-door.map", "origin": [1030.0, 7.0, 255.0] }),
        )
        .unwrap();
    let after = map(&session);
    assert_eq!(
        after.entities[0].brushes().count(),
        before.entities[0].brushes().count() + 1
    );
    assert_eq!(after.entities.len(), before.entities.len() + 1);
    let placed = after.entities.last().unwrap();
    assert_eq!(placed.classname(), Some(&b"func_door"[..]));
    let mut alone = MapFile::new();
    alone.push_entity(placed.clone());
    let [plo, phi] = bounds(&alone);
    // The origin snaps to the 16 grid: 1024, 0, 256.
    let mut door_only = MapFile::new();
    door_only.push_entity(prefab.entities[1].clone());
    let [olo, ohi] = bounds(&door_only);
    assert_eq!(plo, [olo[0] + 1024.0, olo[1], olo[2] + 256.0]);
    assert_eq!(phi, [ohi[0] + 1024.0, ohi[1], ohi[2] + 256.0]);
    session.execute("edit.undo", json!({})).unwrap();
    assert_eq!(map(&session), before);
}

// No spec statement: P6-23: a bad name, no selection, no project and a prefab that is not
// there are refused
#[test]
fn bad_prefabs_are_refused() {
    let dir = keep("refused");
    let mut session = session(&dir);
    for name in ["", "../out", "a b"] {
        let error = session
            .execute(MAP_SAVE_PREFAB, json!({ "name": name, "brushes": [0] }))
            .unwrap_err();
        assert!(error.to_string().contains("prefab name"), "{error}");
    }
    let error = session
        .execute(MAP_SAVE_PREFAB, json!({ "name": "none" }))
        .unwrap_err();
    assert!(error.to_string().contains("nothing is selected"), "{error}");
    let error = session
        .execute(
            MAP_PLACE_PREFAB,
            json!({ "path": "prefabs/missing.map", "origin": [0.0, 0.0, 0.0] }),
        )
        .unwrap_err();
    assert!(error.to_string().contains("missing.map"), "{error}");
    let listed: Prefabs =
        serde_json::from_value(session.execute(MAP_PREFABS, json!({})).unwrap()).unwrap();
    assert!(listed.prefabs.is_empty(), "no prefabs folder lists none");
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut bare = Session::new(
        Context {
            workbench: Workbench::new(FsLoader, Config::default()),
        },
        registry,
    );
    assert!(bare.execute(MAP_PREFABS, json!({})).is_err());
}
