// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use eframe::egui::{Pos2, Rect, Vec2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_workbench::map_edit::{BrushIndex, EntityIndex};
use qs_workbench::map_uv::{Axis, FaceIndex, FaceRef, JustifyTo, UvChoice};

use super::{Frame, PickMode, UvEditor, edges, snapped};
use crate::map_scene::{BrushShape, FaceShape, Scene};
use crate::map_tools::BrushAction;
use crate::strings::Text;

/// A floor face from 0 to 64 along x and y, texels as the coordinates (MAP-4.2).
fn floor(index: usize) -> FaceShape {
    FaceShape {
        index: FaceIndex::new(index),
        plane: ([0.0, 0.0, 1.0], 0.0),
        polygon: vec![
            [0.0, 0.0, 0.0],
            [64.0, 0.0, 0.0],
            [64.0, -64.0, 0.0],
            [0.0, -64.0, 0.0],
        ],
        texture: "floor".to_owned(),
        texvecs: Some([[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0]]),
    }
}

fn face(index: usize) -> FaceRef {
    FaceRef {
        entity: EntityIndex::new(0),
        brush: BrushIndex::new(0),
        face: FaceIndex::new(index),
    }
}

fn scene() -> Scene {
    let mut scene = Scene::default();
    scene.brushes.push(BrushShape {
        entity: EntityIndex::new(0),
        brush: BrushIndex::new(0),
        lines: Vec::new(),
        vertices: Vec::new(),
        planes: Vec::new(),
        textures: Vec::new(),
        faces: vec![floor(0), floor(1)],
    });
    scene
}

// No spec statement: P6-22: Alt-click replaces the face selection and Alt-Shift-click adds or
// takes out a face; a click on nothing clears it unless Shift is held
#[test]
fn picking_replaces_or_toggles() {
    let scene = scene();
    let mut uv = UvEditor::default();
    uv.pick(Some(face(0)), &scene, PickMode::Replace);
    uv.pick(Some(face(1)), &scene, PickMode::Toggle);
    assert_eq!(uv.faces(), vec![face(0), face(1)]);
    uv.pick(Some(face(0)), &scene, PickMode::Toggle);
    assert_eq!(uv.faces(), vec![face(1)]);
    uv.pick(None, &scene, PickMode::Toggle);
    assert_eq!(uv.faces(), vec![face(1)]);
    uv.pick(Some(face(9)), &scene, PickMode::Toggle);
    assert_eq!(
        uv.faces(),
        vec![face(1)],
        "a face not in the scene is not taken"
    );
    uv.pick(None, &scene, PickMode::Replace);
    assert!(uv.is_empty());
}

// No spec statement: P6-22: a selected face whose place now holds another plane, or nothing,
// is forgotten, and so is a copied face that is gone
#[test]
fn moved_faces_are_forgotten() {
    let mut scene = scene();
    let mut uv = UvEditor::default();
    uv.pick(Some(face(0)), &scene, PickMode::Replace);
    uv.pick(Some(face(1)), &scene, PickMode::Toggle);
    uv.copied = Some(face(1));
    uv.keep_found(&scene);
    assert_eq!(uv.faces(), vec![face(0), face(1)], "unchanged faces stay");
    scene.brushes[0].faces[0].plane = ([0.0, 0.0, 1.0], 8.0);
    scene.brushes[0].faces.pop();
    uv.keep_found(&scene);
    assert!(uv.is_empty());
    assert_eq!(uv.copied, None);
}

// No spec statement: P6-22: the view frames the face in whole tiles and maps texels to points
#[test]
fn the_view_frames_whole_tiles() {
    let rect = Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::splat(216.0));
    let corners = [[8.0, 8.0], [40.0, 24.0]];
    let frame = Frame::around(&corners, Some([32.0, 32.0]), rect).unwrap();
    assert_eq!((frame.least, frame.greatest), ([0.0, 0.0], [64.0, 32.0]));
    assert_eq!(frame.scale, 200.0 / 64.0);
    assert_eq!(frame.screen_of([0.0, 0.0]), Pos2::new(18.0, 28.0));
    assert_eq!(frame.screen_of([64.0, 32.0]), Pos2::new(218.0, 128.0));
    let bare = Frame::around(&corners, None, rect).unwrap();
    assert_eq!((bare.least, bare.greatest), ([8.0, 8.0], [40.0, 24.0]));
    assert!(Frame::around(&[], None, rect).is_none());
    assert!(Frame::around(&[[3.0, 3.0]], None, rect).is_none());
    let on_edge = Frame::around(&[[0.0, 0.0], [32.0, 0.0]], Some([32.0, 16.0]), rect).unwrap();
    assert_eq!(
        on_edge.greatest,
        [32.0, 16.0],
        "a face on an edge still gets a tile"
    );
}

// No spec statement: P6-22: a drag moves the texture by whole texels, and snaps a tile's edge
// to the corner nearest it when one comes within reach
#[test]
fn a_drag_snaps_to_corners() {
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(80.0));
    let corners = [[0.0, 0.0], [64.0, 64.0]];
    let frame = Frame::around(&corners, Some([64.0, 64.0]), rect).unwrap();
    assert_eq!(frame.scale, 1.0);
    let tile = Some([64.0, 64.0]);
    assert_eq!(
        frame.texture_move(Vec2::new(10.4, -20.6), &corners, tile),
        [10.0, -21.0]
    );
    assert_eq!(
        frame.texture_move(Vec2::new(4.0, 59.0), &corners, tile),
        [0.0, 64.0]
    );
    assert_eq!(
        frame.texture_move(Vec2::new(4.0, 59.0), &corners, None),
        [4.0, 59.0]
    );
    assert_eq!(snapped(5.0, [0.0, 70.0].into_iter(), 64.0, 3.0), 6.0);
    assert_eq!(
        snapped(5.0, [1.0, 66.0].into_iter(), 64.0, 6.0),
        2.0,
        "the nearest wins"
    );
    assert_eq!(
        snapped(5.0, [30.0].into_iter(), 64.0, 6.0),
        5.0,
        "none in reach"
    );
    assert_eq!(edges(0.0, 64.0, 32.0, 8.0), vec![8.0, 40.0]);
    assert_eq!(edges(0.0, 64.0, 32.0, 0.0), vec![0.0, 32.0, 64.0]);
    assert_eq!(edges(0.0, 1e9, 1.0, 0.0).len(), super::MOST_TILE_EDGES);
}

/// Shows the UV editor's properties, both faces selected and `copied` copied, clicks `label`
/// and gives what it asked for.
fn clicked(label: &str, copied: Option<FaceRef>) -> Vec<BrushAction> {
    let scene = scene();
    let mut uv = UvEditor::default();
    uv.pick(Some(face(0)), &scene, PickMode::Replace);
    uv.pick(Some(face(1)), &scene, PickMode::Toggle);
    uv.copied = copied;
    let mut harness = Harness::new_ui_state(
        |ui, (uv, actions): &mut (UvEditor, Vec<BrushAction>)| {
            let size_of = |_: &str| Some([64, 64]);
            actions.extend(uv.properties(ui, &scene, &size_of));
        },
        (uv, Vec::new()),
    );
    harness.run();
    harness.state_mut().1.clear();
    harness.get_by_label(label).click();
    harness.run();
    harness.state().1.clone()
}

// No spec statement: P6-22: each button asks for its edit of every selected face, and Paste
// carries the copied face's alignment onto the others
#[test]
fn the_buttons_edit_the_selected_faces() {
    let both = vec![face(0), face(1)];
    for (text, choice) in [
        (Text::MapUvFit, UvChoice::Fit),
        (
            Text::MapUvLeft,
            UvChoice::Justify {
                to: JustifyTo::Left,
            },
        ),
        (
            Text::MapUvRight,
            UvChoice::Justify {
                to: JustifyTo::Right,
            },
        ),
        (Text::MapUvTop, UvChoice::Justify { to: JustifyTo::Top }),
        (
            Text::MapUvBottom,
            UvChoice::Justify {
                to: JustifyTo::Bottom,
            },
        ),
        (
            Text::MapUvCentre,
            UvChoice::Justify {
                to: JustifyTo::Centre,
            },
        ),
        (Text::MapUvFlipS, UvChoice::Flip { axis: Axis::S }),
        (Text::MapUvFlipT, UvChoice::Flip { axis: Axis::T }),
        (Text::MapUvReset, UvChoice::Reset),
        (Text::MapUvSnap, UvChoice::Snap),
    ] {
        assert_eq!(
            clicked(text.get(), None),
            vec![BrushAction::Uv(both.clone(), choice)],
            "{text:?}"
        );
    }
    assert_eq!(
        clicked(Text::MapPasteAlignment.get(), Some(face(1))),
        vec![BrushAction::AlignFaces(face(1), vec![face(0)])]
    );
    assert_eq!(clicked(Text::MapCopyAlignment.get(), None), Vec::new());
}
