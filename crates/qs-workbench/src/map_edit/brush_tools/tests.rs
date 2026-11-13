// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code: a failed unwrap or index is a failed test; the corners compared are \
              on the grid, exact"
)]

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::{Value, json};

use crate::map_edit::{
    MAP_BRUSH_CHAMFER, MAP_BRUSH_EXTRUDE, MAP_BRUSH_HOLLOW, MAP_BRUSH_INTERSECT, MAP_BRUSH_SCALE,
    MAP_BRUSH_SHEAR, MAP_BRUSH_SPLIT_EDGE, MAP_BRUSH_SPLIT_FACE, MAP_BRUSH_SWEEP,
    MAP_BRUSH_VERTICES, MAP_OPEN,
};
use crate::{Config, Workbench, register};

/// A world with two boxes of `rock`: one 64 units across at the origin, one beside it
/// overlapping its far half.
fn map_text() -> String {
    let mut file = qs_map::MapFile::parse(b"{\n\"classname\" \"worldspawn\"\n}\n").unwrap();
    let surface = qs_map::Surface {
        texture: b"rock".to_vec(),
        texdef: qs_map::TexDef::Standard {
            shift: [0.0, 0.0],
            rotate: 0.0,
            scale: [1.0, 1.0],
        },
    };
    for (min, max) in [
        ([0.0; 3], [64.0; 3]),
        ([32.0, 0.0, 0.0], [96.0, 64.0, 64.0]),
    ] {
        let brush = qs_map::Brush::cuboid(min, max, &surface).unwrap();
        file.entities[0].items.push(qs_map::Item::Brush(brush));
    }
    String::from_utf8(file.to_bytes()).unwrap()
}

fn open(name: &str) -> Session<Workbench<FsLoader>> {
    let dir = std::env::temp_dir().join(format!("qs-wb-tools-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tools.map");
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

/// Runs `command` with `params` on a fresh map: the edit changes the map and one undo takes it
/// back. Returns the brushes after the edit.
fn edits(name: &str, command: &str, params: &Value) -> Vec<qs_map::Brush> {
    let mut session = open(name);
    let before = text(&session);
    session.execute(command, params.clone()).unwrap();
    let after = world(&session);
    assert_ne!(text(&session), before, "{command} changed the map");
    assert!(after.iter().all(qs_map::Brush::is_valid));
    session.undo().unwrap();
    assert_eq!(text(&session), before, "{command} is undone in one step");
    after
}

/// Runs `command` with `params` and checks it is refused, the map unchanged.
fn refused(name: &str, command: &str, params: &Value) {
    let mut session = open(name);
    let before = text(&session);
    assert!(
        session.execute(command, params.clone()).is_err(),
        "{command} {params}"
    );
    assert_eq!(text(&session), before);
}

/// The top face of the first box: the one facing up.
fn top(session_name: &str) -> usize {
    let session = open(session_name);
    world(&session)[0]
        .faces
        .iter()
        .position(|f| f.plane().unwrap().normal[2] > 0.5)
        .unwrap()
}

// spec MAP-15.2: brushes scale and shear about the middle of their bounds or a point given; a
// shear along its own axis is refused
#[test]
fn brushes_scale_and_shear() {
    let after = edits(
        "scale",
        MAP_BRUSH_SCALE,
        &json!({ "entity": 0, "brushes": [0], "factors": [2, 1, 0.5] }),
    );
    let corners = after[0].vertices();
    assert!(corners.iter().any(|c| (c[0] + 32.0).abs() < 1e-6));
    assert!(
        corners
            .iter()
            .all(|c| c[2] >= 16.0 - 1e-6 && c[2] <= 48.0 + 1e-6)
    );
    let after = edits(
        "shear",
        MAP_BRUSH_SHEAR,
        &json!({ "entity": 0, "brushes": [0, 1], "along": "x", "by": "z", "factor": 0.5,
                 "about": [0, 0, 0] }),
    );
    assert!(after[0].vertices().iter().any(|c| c == &[32.0, 0.0, 64.0]));
    refused(
        "shear-self",
        MAP_BRUSH_SHEAR,
        &json!({ "entity": 0, "brushes": [0], "along": "x", "by": "x", "factor": 1 }),
    );
    refused(
        "scale-flat",
        MAP_BRUSH_SCALE,
        &json!({ "entity": 0, "brushes": [0], "factors": [1, 0, 1] }),
    );
}

// spec MAP-15.3: a face extrudes by a distance snapped to the grid; one through the brush is
// refused
#[test]
fn a_face_extrudes() {
    let face = top("extrude-top");
    let after = edits(
        "extrude",
        MAP_BRUSH_EXTRUDE,
        &json!({ "face": { "entity": 0, "brush": 0, "face": face }, "by": 30 }),
    );
    assert!(
        after[0].vertices().iter().any(|c| c[2] == 96.0),
        "30 snaps to 32"
    );
    refused(
        "extrude-through",
        MAP_BRUSH_EXTRUDE,
        &json!({ "face": { "entity": 0, "brush": 0, "face": face }, "by": -64 }),
    );
}

// spec MAP-15.4: a brush sweeps along a path into one brush for each step
#[test]
fn a_brush_sweeps() {
    let after = edits(
        "sweep",
        MAP_BRUSH_SWEEP,
        &json!({ "entity": 0, "brush": 0, "path": [[0, 0, 0], [0, 128, 0], [0, 128, 128]] }),
    );
    assert_eq!(after.len(), 3, "two steps and the other box");
    refused(
        "sweep-short",
        MAP_BRUSH_SWEEP,
        &json!({ "entity": 0, "brush": 0, "path": [[0, 0, 0]] }),
    );
}

// spec MAP-15.5, MAP-15.6: a brush hollows into walls, and two brushes intersect into the one
// they share; a wall too thick, or one brush alone, is refused
#[test]
fn brushes_hollow_and_intersect() {
    let after = edits(
        "hollow",
        MAP_BRUSH_HOLLOW,
        &json!({ "entity": 0, "brush": 0, "wall": 8 }),
    );
    assert!(after.len() > 2);
    refused(
        "hollow-thick",
        MAP_BRUSH_HOLLOW,
        &json!({ "entity": 0, "brush": 0, "wall": 32 }),
    );
    let after = edits(
        "intersect",
        MAP_BRUSH_INTERSECT,
        &json!({ "entity": 0, "brushes": [0, 1] }),
    );
    assert_eq!(after.len(), 1);
    let corners = after[0].vertices();
    assert!(
        corners
            .iter()
            .all(|c| c[0] >= 32.0 - 1e-6 && c[0] <= 64.0 + 1e-6)
    );
    refused(
        "intersect-one",
        MAP_BRUSH_INTERSECT,
        &json!({ "entity": 0, "brushes": [0] }),
    );
}

// spec MAP-15.7, MAP-15.8: several vertices move together, an edge and a face split by a new
// vertex; a vertex that is no vertex is refused
#[test]
fn vertices_move_and_split() {
    let after = edits(
        "vertices",
        MAP_BRUSH_VERTICES,
        &json!({ "entity": 0, "brush": 0, "moves": [
            { "from": [0, 0, 64], "to": [0, 0, 96] },
            { "from": [64, 0, 64], "to": [64, 0, 96] },
        ] }),
    );
    assert!(after[0].vertices().contains(&[64.0, 0.0, 96.0]));
    refused(
        "vertices-none",
        MAP_BRUSH_VERTICES,
        &json!({ "entity": 0, "brush": 0, "moves": [{ "from": [5, 5, 5], "to": [0, 0, 0] }] }),
    );
    let after = edits(
        "split-edge",
        MAP_BRUSH_SPLIT_EDGE,
        &json!({ "entity": 0, "brush": 0, "edge": [[0, 0, 64], [64, 0, 64]],
                 "at": { "point": { "to": [32, -16, 80] } } }),
    );
    assert_eq!(after[0].vertices().len(), 9);
    let after = edits(
        "split-edge-out",
        MAP_BRUSH_SPLIT_EDGE,
        &json!({ "entity": 0, "brush": 0, "edge": [[0, 0, 64], [64, 0, 64]],
                 "at": { "out": { "by": 16 } } }),
    );
    assert_eq!(after[0].vertices().len(), 9);
    let face = top("split-face-top");
    let after = edits(
        "split-face",
        MAP_BRUSH_SPLIT_FACE,
        &json!({ "face": { "entity": 0, "brush": 0, "face": face },
                 "at": { "point": { "to": [32, 32, 96] } } }),
    );
    assert_eq!(after[0].vertices().len(), 9);
    let after = edits(
        "split-face-out",
        MAP_BRUSH_SPLIT_FACE,
        &json!({ "face": { "entity": 0, "brush": 0, "face": face }, "at": { "out": { "by": 32 } } }),
    );
    assert!(after[0].vertices().contains(&[32.0, 32.0, 96.0]));
}

// spec MAP-15.9: an edge chamfers into bevel faces; a pair that is no edge is refused
#[test]
fn an_edge_chamfers() {
    let after = edits(
        "chamfer",
        MAP_BRUSH_CHAMFER,
        &json!({ "entity": 0, "brush": 0, "edge": [[0, 0, 64], [64, 0, 64]], "distance": 16,
                 "faces": 3 }),
    );
    assert_eq!(after[0].faces.len(), 9);
    let one = edits(
        "chamfer-one",
        MAP_BRUSH_CHAMFER,
        &json!({ "entity": 0, "brush": 0, "edge": [[0, 0, 64], [64, 0, 64]], "distance": 16 }),
    );
    assert_eq!(one[0].faces.len(), 7);
    refused(
        "chamfer-none",
        MAP_BRUSH_CHAMFER,
        &json!({ "entity": 0, "brush": 0, "edge": [[0, 0, 64], [64, 64, 64]], "distance": 16 }),
    );
}
