// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use crate::map_edit::{MAP_BRUSH_SHAPE, MAP_OPEN};
use crate::{Config, Workbench, register};

fn open(name: &str) -> Session<Workbench<FsLoader>> {
    let dir = std::env::temp_dir().join(format!("qs-wb-shapes-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("shapes.map");
    std::fs::write(
        &path,
        "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"func_wall\"\n}\n",
    )
    .unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

fn brushes(session: &Session<Workbench<FsLoader>>, entity: usize) -> Vec<qs_map::Brush> {
    let map = session.context().map().unwrap();
    map.file().entities[entity].brushes().cloned().collect()
}

// spec MAP-10.4: a hollow cylinder is one brush a side, added in one step that undoes whole
#[test]
fn a_shape_is_added_in_one_undoable_step() {
    let mut session = open("cylinder");
    session
        .execute(
            MAP_BRUSH_SHAPE,
            json!({
                "min": [-64, -64, 0],
                "max": [64, 64, 128],
                "shape": { "kind": "cylinder", "sides": 12, "hollow": 16 },
                "texture": "stone"
            }),
        )
        .unwrap();
    let made = brushes(&session, 0);
    assert_eq!(made.len(), 12);
    assert!(made.iter().all(qs_map::Brush::is_valid));
    assert!(made[0].faces.iter().all(|f| f.texture() == b"stone"));
    session.execute("edit.undo", json!({})).unwrap();
    assert!(brushes(&session, 0).is_empty());
}

// spec MAP-10.10: stairs take the default step when none is given, into the entity asked
#[test]
fn stairs_take_the_default_step() {
    let mut session = open("stairs");
    session
        .execute(
            MAP_BRUSH_SHAPE,
            json!({
                "entity": 1,
                "min": [0, 0, 0],
                "max": [256, 64, 96],
                "shape": { "kind": "stairs", "climb": "north" }
            }),
        )
        .unwrap();
    // 64 / 32 = 2 steps by length northward.
    assert_eq!(brushes(&session, 1).len(), 2);
}

// spec MAP-10.3: a shape out of range is refused and changes nothing
#[test]
fn a_bad_shape_changes_nothing() {
    let mut session = open("bad");
    let before = session.context().map().unwrap().file().to_bytes();
    for shape in [
        json!({ "kind": "cylinder", "sides": 2 }),
        json!({ "kind": "curve", "sides": 13 }),
        json!({ "kind": "uv_spheroid", "rings": 1 }),
    ] {
        let error = session
            .execute(
                MAP_BRUSH_SHAPE,
                json!({ "min": [0, 0, 0], "max": [64, 64, 64], "shape": shape }),
            )
            .unwrap_err();
        assert!(!error.to_string().is_empty());
    }
    assert!(
        session
            .execute(
                MAP_BRUSH_SHAPE,
                json!({ "min": [0, 0, 0], "max": [64, 64, 64], "shape": { "kind": "torus" } }),
            )
            .is_err()
    );
    assert_eq!(session.context().map().unwrap().file().to_bytes(), before);
}

// spec MAP-10.9: a curve's corners snap to the grid the command is given
#[test]
fn a_curve_is_on_the_command_s_grid() {
    let mut session = open("curve");
    session
        .execute(
            MAP_BRUSH_SHAPE,
            json!({
                "min": [3, 5, 0],
                "max": [400, 400, 64],
                "grid": 8,
                "shape": { "kind": "curve", "sides": 12 }
            }),
        )
        .unwrap();
    for v in brushes(&session, 0)
        .iter()
        .flat_map(qs_map::Brush::vertices)
    {
        for c in v {
            assert!((c / 8.0 - (c / 8.0).round()).abs() < 1e-9, "{v:?}");
        }
    }
}
