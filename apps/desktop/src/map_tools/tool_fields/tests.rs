// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::collections::HashMap;
use std::path::Path;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use qs_workbench::map_edit::{Axis, BrushIndex, EntityIndex, Lock};
use qs_workbench::map_uv::{FaceIndex, FaceRef};
use qs_workbench::{Config, FsLoader, Workbench};

use crate::map_scene::Scene;
use crate::map_tools::{BrushAction, BrushTools, MapTool};
use crate::strings::Text;

/// The keep fixture's map's scene.
fn scene() -> Scene {
    let mut workbench = Workbench::new(FsLoader, Config::default());
    let map = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/keep/maps/keep.map");
    workbench.open_map(&map).unwrap();
    Scene::build(workbench.map().unwrap(), &HashMap::new(), &[], &mut |_| {
        None
    })
}

const WORLD: EntityIndex = EntityIndex::new(0);

/// What clicking the buttons labelled `labels`, one after another, in the Properties panel of
/// `tools` asks for.
fn clicked(tools: BrushTools, labels: &[&str]) -> Vec<BrushAction> {
    let scene = scene();
    let mut harness = Harness::new_ui_state(
        |ui, (tools, actions): &mut (BrushTools, Vec<BrushAction>)| {
            actions.extend(tools.properties(ui, &scene, None, &|_| None, &[]));
        },
        (tools, Vec::new()),
    );
    harness.run();
    for label in labels {
        harness.get_by_label(label).click();
        harness.run();
    }
    harness.state().1.clone()
}

fn chosen(brushes: &[usize]) -> BrushTools {
    BrushTools {
        chosen: brushes
            .iter()
            .map(|&b| (WORLD, BrushIndex::new(b)))
            .collect(),
        ..BrushTools::default()
    }
}

// No spec statement: P6-18: the panel hollows, scales, shears and sweeps the brush selected,
// with the fields' values and the texture lock
#[test]
fn the_panel_asks_for_brush_tools() {
    let first = BrushIndex::new(0);
    assert_eq!(
        clicked(chosen(&[0]), &[Text::MapShapeHollow.get()]),
        vec![BrushAction::Hollow(WORLD, first, 16.0)]
    );
    assert_eq!(
        clicked(chosen(&[0]), &[Text::MapScaleBrushes.get()]),
        vec![BrushAction::Scale(
            WORLD,
            vec![first],
            [1.0; 3],
            Lock::Locked
        )]
    );
    assert_eq!(
        clicked(chosen(&[0]), &[Text::MapShearBrushes.get()]),
        vec![BrushAction::Shear(
            WORLD,
            vec![first],
            Axis::X,
            Axis::Z,
            0.5,
            Lock::Locked
        )]
    );
    let sweep = format!("{} (1)", Text::MapSweep.get());
    assert_eq!(
        clicked(chosen(&[0]), &[Text::MapAddStep.get(), &sweep]),
        vec![BrushAction::Sweep(
            WORLD,
            first,
            vec![[0.0; 3], [0.0; 3]],
            Lock::Locked
        )]
    );
}

// No spec statement: P6-18: Intersect asks for two or more brushes of one entity, and is off
// for one
#[test]
fn intersect_takes_two_brushes() {
    assert_eq!(
        clicked(chosen(&[0, 1]), &[Text::MapIntersect.get()]),
        vec![BrushAction::Intersect(
            WORLD,
            vec![BrushIndex::new(0), BrushIndex::new(1)]
        )]
    );
    assert!(clicked(chosen(&[0]), &[Text::MapIntersect.get()]).is_empty());
}

// No spec statement: P6-18: with one face picked the panel extrudes or splits it
#[test]
fn a_picked_face_extrudes_and_splits() {
    let face = FaceRef {
        entity: WORLD,
        brush: BrushIndex::new(0),
        face: FaceIndex::new(0),
    };
    let tools = || {
        let mut tools = BrushTools::default();
        tools.select_face(face, &scene());
        tools
    };
    assert_eq!(
        clicked(tools(), &[Text::MapExtrude.get()]),
        vec![BrushAction::Extrude(face, 16.0, Lock::Locked)]
    );
    assert_eq!(
        clicked(tools(), &[Text::MapSplitFace.get()]),
        vec![BrushAction::SplitFace(face, 16.0)]
    );
}

// No spec statement: P6-18: with two corners picked in the Vertex tool the panel splits or
// chamfers the edge between them
#[test]
fn two_picked_corners_split_and_chamfer() {
    let edge = [[0.0, 0.0, 64.0], [64.0, 0.0, 64.0]];
    let tools = || BrushTools {
        tool: MapTool::Vertex,
        corners: edge.to_vec(),
        ..chosen(&[0])
    };
    let first = BrushIndex::new(0);
    assert_eq!(
        clicked(tools(), &[Text::MapSplitEdge.get()]),
        vec![BrushAction::SplitEdge(WORLD, first, edge, 16.0)]
    );
    assert_eq!(
        clicked(tools(), &[Text::MapChamfer.get()]),
        vec![BrushAction::Chamfer(WORLD, first, edge, 16.0, 1)]
    );
}
