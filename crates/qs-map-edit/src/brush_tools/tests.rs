// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code: a failed unwrap or index is a failed test; the corners compared are \
              on the grid, exact"
)]

use qs_commands::Session;
use serde_json::{Value, json};

use crate::testing::MapContext;
use crate::{
    MAP_BRUSH_CHAMFER, MAP_BRUSH_EXTRUDE, MAP_BRUSH_HOLLOW, MAP_BRUSH_INTERSECT, MAP_BRUSH_SCALE,
    MAP_BRUSH_SHEAR, MAP_BRUSH_SPLIT_EDGE, MAP_BRUSH_SPLIT_FACE, MAP_BRUSH_SWEEP,
    MAP_BRUSH_VERTICES, MAP_OPEN,
};

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

fn open(name: &str) -> Session<MapContext> {
    let dir = std::env::temp_dir().join(format!("qs-wb-tools-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tools.map");
    std::fs::write(&path, map_text()).unwrap();
    let mut session = MapContext::session();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

fn text(session: &Session<MapContext>) -> String {
    String::from_utf8(session.context().map().unwrap().file().to_bytes()).unwrap()
}

/// The world's brushes.
fn world(session: &Session<MapContext>) -> Vec<qs_map::Brush> {
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
    let twice = edits(
        "scale-twice",
        MAP_BRUSH_SCALE,
        &json!({ "entity": 0, "brushes": [0, 0], "factors": [2, 2, 2], "about": [0, 0, 0] }),
    );
    assert!(
        twice[0].vertices().contains(&[128.0, 128.0, 128.0]),
        "scaled once"
    );
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

/// A session on a map of one world box, 0 to 64, and three point entities: one facing 30
/// degrees at 32 0 32, one facing straight up at 0 64 0, and one with `angles` 10 30 0.
fn arranged(name: &str) -> Session<MapContext> {
    let dir = std::env::temp_dir().join(format!("qs-wb-arrange-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("arrange.map");
    let mut text = map_text();
    text.push_str(
        "{\n\"classname\" \"info_null\"\n\"origin\" \"32 0 32\"\n\"angle\" \"30\"\n}\n\
         {\n\"classname\" \"info_null\"\n\"origin\" \"0 64 0\"\n\"angle\" \"-1\"\n}\n\
         {\n\"classname\" \"info_null\"\n\"origin\" \"64 64 64\"\n\"angles\" \"10 30 0\"\n}\n",
    );
    std::fs::write(&path, text).unwrap();
    let mut session = MapContext::session();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

/// Entity `entity`'s value of `key`.
fn key(session: &Session<MapContext>, entity: usize, key: &str) -> String {
    let map = session.context().map().unwrap();
    let value = map.file().entities[entity].value(key.as_bytes()).unwrap();
    String::from_utf8(value.to_vec()).unwrap()
}

// No spec statement: P7-08: map.flip mirrors a selection along an axis through the middle of its
// box, as TrenchBroom's Flip: brushes and point entities' origins, a yaw across x to 180 less it
// and across y to its negative, an up facing to down across z; one undo takes it back
#[test]
fn a_selection_flips_along_each_axis() {
    use crate::MAP_FLIP;

    // The box of the world's first brush, 0 to 64, and the entity at 32 0 32: middle 32 32 32.
    let selection = json!({ "entities": [1], "brushes": [[0, 0]] });
    let mut session = arranged("flip-x");
    let before = text(&session);
    let mut params = selection.clone();
    params["axis"] = json!("y");
    session.execute(MAP_FLIP, params).unwrap();
    assert_eq!(key(&session, 1, "origin"), "32 64 32");
    assert_eq!(key(&session, 1, "angle"), "330");
    session.undo().unwrap();
    assert_eq!(text(&session), before);
    let mut params = selection;
    params["axis"] = json!("x");
    session.execute(MAP_FLIP, params).unwrap();
    assert_eq!(key(&session, 1, "origin"), "32 0 32");
    assert_eq!(key(&session, 1, "angle"), "150");
    assert!(world(&session).iter().all(qs_map::Brush::is_valid));
    // Straight up turns down across z; `angles` keeps its yaw and negates its pitch.
    let mut session = arranged("flip-z");
    session
        .execute(MAP_FLIP, json!({ "entities": [2, 3], "axis": "z" }))
        .unwrap();
    assert_eq!(key(&session, 2, "angle"), "-2");
    assert_eq!(key(&session, 3, "angles"), "-10 30 0");
    assert_eq!(key(&session, 3, "origin"), "64 64 0");
}

// No spec statement: P7-08: map.turn turns a selection about the vertical through the middle of
// its box: a right angle keeps brushes on the grid, a point entity's yaw turns with it and a
// facing straight up stays; nothing selected, or the world as a whole entity, is refused
#[test]
fn a_selection_turns_about_the_vertical() {
    use crate::MAP_TURN;

    let mut session = arranged("turn");
    session
        .execute(
            MAP_TURN,
            json!({ "entities": [1, 2], "brushes": [[0, 0]], "degrees": 90 }),
        )
        .unwrap();
    // The middle of 0 0 0 to 64 64 64 is 32 32 32: 32 0 32 turns to 64 32 32 and 0 64 0 to
    // 0 0 0.
    assert_eq!(key(&session, 1, "origin"), "64 32 32");
    assert_eq!(key(&session, 1, "angle"), "120");
    assert_eq!(key(&session, 2, "origin"), "0 0 0");
    assert_eq!(key(&session, 2, "angle"), "-1");
    let corners = world(&session)[0].vertices();
    assert!(
        corners.iter().flatten().all(|c| c.fract() == 0.0),
        "{corners:?}"
    );
    session
        .execute(MAP_TURN, json!({ "entities": [3], "degrees": 45 }))
        .unwrap();
    assert_eq!(key(&session, 3, "angles"), "10 75 0");
    for bad in [
        json!({ "degrees": 90 }),
        json!({ "entities": [0], "degrees": 90 }),
        json!({ "entities": [99], "degrees": 90 }),
    ] {
        assert!(session.execute(MAP_TURN, bad.clone()).is_err(), "{bad}");
    }
}

// No spec statement: P7-08: map.replace_texture puts one texture in another's place, in any
// case, on the brushes given or on every brush, as TrenchBroom's Replace Material; one undo
// takes it back, and a texture no face has is refused
#[test]
fn a_texture_is_replaced_on_the_brushes_or_everywhere() {
    use crate::{MAP_REPLACE_TEXTURE, Replaced};

    let mut session = open("replace");
    let before = text(&session);
    let replaced: Replaced = serde_json::from_value(
        session
            .execute(
                MAP_REPLACE_TEXTURE,
                json!({ "from": "ROCK", "to": "metal", "brushes": [[0, 1]] }),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(replaced.faces, 6);
    let brushes = world(&session);
    assert!(brushes[0].faces.iter().all(|f| f.texture() == b"rock"));
    assert!(brushes[1].faces.iter().all(|f| f.texture() == b"metal"));
    session.undo().unwrap();
    assert_eq!(text(&session), before);
    session
        .execute(
            MAP_REPLACE_TEXTURE,
            json!({ "from": "rock", "to": "metal" }),
        )
        .unwrap();
    assert!(
        world(&session)
            .iter()
            .flat_map(|b| &b.faces)
            .all(|f| f.texture() == b"metal")
    );
    assert!(
        session
            .execute(MAP_REPLACE_TEXTURE, json!({ "from": "rock", "to": "x" }))
            .is_err(),
        "no face has rock now"
    );
}
