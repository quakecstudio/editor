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
                json!({ "name": "floor-door", "brushes": [[0, 0], [4, 0]], "entities": [4] }),
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
            .execute(
                MAP_SAVE_PREFAB,
                json!({ "name": name, "brushes": [[0, 0]] }),
            )
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

// No spec statement: P6-23, MAP-9: a brush of a layer is kept in the prefab's world, the layer
// left out; an entity in a layer is kept without its layer key; the floor is the origin's
// height exactly, and the place's grid is the one asked for
#[test]
fn layers_stay_behind_and_grids_are_asked_for() {
    let dir = keep("layers");
    let text = std::fs::read_to_string(dir.join("maps/keep.map")).unwrap();
    let start = text.find("{\n\"classname\" \"info_player_start\"").unwrap();
    let mut text = text.get(..start).unwrap().to_owned();
    text.push_str(
        "{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\"_tb_name\" \"Bits\"\n\
         \"_tb_id\" \"7\"\n{\n\
         ( 0 0 8 ) ( 0 1 8 ) ( 0 0 9 ) wall 0 0 0 1 1\n\
         ( 8 0 8 ) ( 8 0 9 ) ( 8 1 8 ) wall 0 0 0 1 1\n\
         ( 0 0 8 ) ( 0 0 9 ) ( 1 0 8 ) wall 0 0 0 1 1\n\
         ( 0 8 8 ) ( 1 8 8 ) ( 0 8 9 ) wall 0 0 0 1 1\n\
         ( 0 0 8 ) ( 1 0 8 ) ( 0 1 8 ) wall 0 0 0 1 1\n\
         ( 0 0 24 ) ( 0 1 24 ) ( 1 0 24 ) wall 0 0 0 1 1\n}\n}\n\
         {\n\"classname\" \"light\"\n\"origin\" \"4 4 40\"\n\"_tb_layer\" \"7\"\n}\n",
    );
    std::fs::write(dir.join("maps/keep.map"), text).unwrap();
    let mut session = session(&dir);
    let saved: PrefabSaved = serde_json::from_value(
        session
            .execute(
                MAP_SAVE_PREFAB,
                json!({ "name": "bits", "brushes": [[1, 0]], "entities": [2, 2], "grid": 0 }),
            )
            .unwrap(),
    )
    .unwrap();
    let prefab = MapFile::parse(&std::fs::read(&saved.prefab.path).unwrap()).unwrap();
    assert_eq!(
        prefab.entities.len(),
        2,
        "the layer is not kept, the light once"
    );
    assert_eq!(prefab.entities[0].brushes().count(), 1);
    let light = &prefab.entities[1];
    assert_eq!(light.classname(), Some(&b"light"[..]));
    assert_eq!(light.value(b"_tb_layer"), None);
    // The box is 8 to 24 high and 0 to 8 across: the middle of its floor, with no grid, is
    // (4, 4, 8).
    assert_eq!(light.origin(), [0.0, 0.0, 32.0]);
    let before = map(&session);
    session
        .execute(
            MAP_PLACE_PREFAB,
            json!({ "path": "prefabs/bits.map", "origin": [5.0, 3.0, 1.0], "grid": 4 }),
        )
        .unwrap();
    let after = map(&session);
    assert_eq!(after.entities.last().unwrap().origin(), [4.0, 4.0, 32.0]);
    assert_eq!(after.entities.len(), before.entities.len() + 1);
}
