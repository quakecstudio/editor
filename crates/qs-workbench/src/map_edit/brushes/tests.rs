// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use crate::map_edit::{
    MAP_BRUSH_CLIP, MAP_BRUSH_CREATE, MAP_BRUSH_DELETE, MAP_BRUSH_MERGE, MAP_BRUSH_SUBTRACT,
    MAP_BRUSH_VERTEX, MAP_OPEN,
};
use crate::{Config, Workbench, register};

/// A world with one 64-unit box of `rock`, made by `Brush::cuboid`, and a door entity with no
/// brushes.
fn map_text() -> String {
    let mut file = qs_map::MapFile::parse(
        b"{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"func_door\"\n}\n",
    )
    .unwrap();
    let surface = qs_map::Surface {
        texture: b"rock".to_vec(),
        texdef: qs_map::TexDef::Standard {
            shift: [0.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    };
    let brush = qs_map::Brush::cuboid([0.0; 3], [64.0; 3], &surface).unwrap();
    file.entities[0].items.push(qs_map::Item::Brush(brush));
    String::from_utf8(file.to_bytes()).unwrap()
}

fn open(name: &str) -> Session<Workbench<FsLoader>> {
    let dir = std::env::temp_dir().join(format!("qs-wb-brushes-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("brushes.map");
    std::fs::write(&path, map_text()).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

fn text(session: &Session<Workbench<FsLoader>>) -> String {
    String::from_utf8(session.context().map().unwrap().file().to_bytes()).unwrap()
}

/// The world's brushes.
fn world(session: &Session<Workbench<FsLoader>>) -> Vec<qs_map::Brush> {
    let map = session.context().map().unwrap();
    map.file().entities[0].brushes().cloned().collect()
}

// No spec statement: P6-08: a box brush is created on the grid with the map's texture, in the
// world or an entity, and undone in one step; one with no volume is refused
#[test]
fn a_box_is_created_on_the_grid() {
    let mut session = open("create");
    session
        .execute(
            MAP_BRUSH_CREATE,
            json!({ "min": [70, 3, -5], "max": [130, 60, 60] }),
        )
        .unwrap();
    let brushes = world(&session);
    assert_eq!(brushes.len(), 2);
    let made = &brushes[1];
    assert!(made.is_valid());
    assert!(made.vertices().contains(&[64.0, 0.0, 0.0]));
    assert!(made.vertices().contains(&[128.0, 64.0, 64.0]));
    assert!(made.faces.iter().all(|face| face.texture() == b"rock"));
    session
        .execute(
            MAP_BRUSH_CREATE,
            json!({ "entity": 1, "min": [0, 0, 0], "max": [8, 8, 8], "texture": "door1", "grid": 0 }),
        )
        .unwrap();
    let door = &session.context().map().unwrap().file().entities[1];
    assert_eq!(door.brushes().next().unwrap().faces[0].texture(), b"door1");
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(text(&session), map_text());
    assert!(
        session
            .execute(
                MAP_BRUSH_CREATE,
                json!({ "min": [0, 0, 0], "max": [64, 64, 4] })
            )
            .is_err(),
        "4 snaps to 0: no height"
    );
    assert_eq!(text(&session), map_text());
}

// No spec statement: P6-08: a clip keeps the part behind the plane, in front of it, or both;
// a plane that leaves nothing is refused, and so is a brush that is not there
#[test]
fn a_brush_is_clipped() {
    let mut session = open("clip");
    // x = 16, facing +x.
    let points = [[16, 0, 0], [16, 0, 1], [16, 1, 0]];
    session
        .execute(
            MAP_BRUSH_CLIP,
            json!({ "entity": 0, "brush": 0, "points": points, "keep": "both" }),
        )
        .unwrap();
    assert_eq!(world(&session).len(), 2);
    assert!(world(&session).iter().all(qs_map::Brush::is_valid));
    session.undo().unwrap();
    session
        .execute(
            MAP_BRUSH_CLIP,
            json!({ "entity": 0, "brush": 0, "points": points }),
        )
        .unwrap();
    let kept = &world(&session)[0];
    assert!(kept.vertices().iter().all(|v| v[0] <= 16.0));
    session.undo().unwrap();
    let outside = [[200, 0, 0], [200, 0, 1], [200, 1, 0]];
    assert!(
        session
            .execute(
                MAP_BRUSH_CLIP,
                json!({ "entity": 0, "brush": 0, "points": outside, "keep": "front" })
            )
            .is_err()
    );
    assert!(
        session
            .execute(
                MAP_BRUSH_CLIP,
                json!({ "entity": 0, "brush": 5, "points": points })
            )
            .is_err()
    );
    assert_eq!(text(&session), map_text());
}

// No spec statement: P6-08: a vertex moves to a point on the grid; one moved inside is refused
#[test]
fn a_vertex_is_moved() {
    let mut session = open("vertex");
    session
        .execute(
            MAP_BRUSH_VERTEX,
            json!({ "entity": 0, "brush": 0, "from": [64, 64, 64], "to": [66, 63, 97] }),
        )
        .unwrap();
    let moved = &world(&session)[0];
    assert!(moved.is_valid());
    assert!(moved.vertices().contains(&[64.0, 64.0, 96.0]));
    session.undo().unwrap();
    assert!(
        session
            .execute(
                MAP_BRUSH_VERTEX,
                json!({ "entity": 0, "brush": 0, "from": [64, 64, 64], "to": [32, 32, 32] })
            )
            .is_err()
    );
    assert_eq!(text(&session), map_text());
}

// No spec statement: P6-08: a brush subtracted from the others of its entity leaves the parts
// outside it and goes; undo puts all back
#[test]
fn a_brush_is_subtracted() {
    let mut session = open("subtract");
    session
        .execute(
            MAP_BRUSH_CREATE,
            json!({ "min": [32, 32, -16], "max": [96, 96, 80] }),
        )
        .unwrap();
    session
        .execute(MAP_BRUSH_SUBTRACT, json!({ "entity": 0, "brush": 1 }))
        .unwrap();
    let left = world(&session);
    assert!(left.len() >= 2);
    assert!(left.iter().all(qs_map::Brush::is_valid));
    assert!(
        left.iter()
            .flat_map(qs_map::Brush::vertices)
            .all(|v| v[0] <= 32.0 || v[1] <= 32.0),
        "nothing of the cutter's corner is left"
    );
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(text(&session), map_text());
}

// No spec statement: P6-08: brushes merge into their hull in the first's place; fewer than two
// is refused; a brush is deleted
#[test]
fn brushes_are_merged_and_deleted() {
    let mut session = open("merge");
    session
        .execute(
            MAP_BRUSH_CREATE,
            json!({ "min": [64, 0, 0], "max": [128, 64, 64] }),
        )
        .unwrap();
    session
        .execute(MAP_BRUSH_MERGE, json!({ "entity": 0, "brushes": [1, 0] }))
        .unwrap();
    let merged = world(&session);
    assert_eq!(merged.len(), 1);
    assert!(merged[0].vertices().contains(&[128.0, 64.0, 64.0]));
    assert!(
        session
            .execute(MAP_BRUSH_MERGE, json!({ "entity": 0, "brushes": [0] }))
            .is_err()
    );
    session
        .execute(MAP_BRUSH_DELETE, json!({ "entity": 0, "brush": 0 }))
        .unwrap();
    assert!(world(&session).is_empty());
    for _ in 0..3 {
        session.undo().unwrap();
    }
    assert_eq!(text(&session), map_text());
}
