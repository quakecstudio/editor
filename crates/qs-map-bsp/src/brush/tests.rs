// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_map::qbsp::{QbspMap, load_map_file};

use super::BrushSet;
use crate::fixtures::{BrushSpec, MapSpec, map_text};
use crate::planes::Planes;
use crate::{Contents, Dialect, Hull, ModelKind};

fn world(brushes: Vec<BrushSpec>) -> QbspMap {
    let spec = MapSpec {
        world: brushes,
        entities: Vec::new(),
        ..MapSpec::default()
    };
    load_map_file(map_text(&spec).as_bytes()).unwrap()
}

fn load(map: &QbspMap, hull: Hull) -> BrushSet {
    let limits = Dialect::Id.limits();
    let mut planes = Planes::new(limits.planes);
    let mut warnings = Vec::new();
    BrushSet::load(
        &map.entities[0],
        map,
        hull,
        ModelKind::World,
        &mut planes,
        &mut warnings,
        limits,
    )
    .unwrap()
}

/// A 32-unit cube at `x` along the x axis.
fn cube_at(x: i32, texture: &str) -> BrushSpec {
    BrushSpec::cube([x, 0, 0], [x + 32, 32, 32], texture)
}

// spec QBSP-4.9: the liquids come first in reverse file order, then the solids in file order
#[test]
fn csg_order_is_liquids_reversed_then_solids() {
    let map = world(vec![
        cube_at(0, "s1"),
        cube_at(64, "*water1"),
        cube_at(128, "s2"),
        cube_at(192, "*water2"),
    ]);
    let set = load(&map, Hull::Draw);
    let order: Vec<f32> = set.brushes.iter().map(|b| b.mins[0]).collect();
    assert_eq!(order, [192.0, 64.0, 0.0, 128.0]);
    assert_eq!(set.mins, [0.0; 3]);
    assert_eq!(set.maxs, [224.0, 32.0, 32.0]);
}

// spec QBSP-4.1: a brush's contents come from its texture: `*lava`, `*slime`, other `*` names
// water, `sky` names sky, others solid
#[test]
fn contents_come_from_the_texture() {
    let map = world(vec![
        cube_at(0, "*lava1"),
        cube_at(64, "*slime0"),
        cube_at(128, "*04water"),
        cube_at(192, "sky4"),
        cube_at(256, "metal"),
    ]);
    let set = load(&map, Hull::Draw);
    let mut contents: Vec<(f32, Contents)> = set
        .brushes
        .iter()
        .map(|b| (b.mins[0], b.contents))
        .collect();
    contents.sort_by(|a, b| a.0.total_cmp(&b.0));
    let contents: Vec<Contents> = contents.into_iter().map(|(_, c)| c).collect();
    assert_eq!(
        contents,
        [
            Contents::Lava,
            Contents::Slime,
            Contents::Water,
            Contents::Sky,
            Contents::Solid
        ]
    );
}

// spec QBSP-4.2: the clipping hulls leave the liquids out
// spec QBSP-4.7: in the clipping hulls every face takes texinfo 0
#[test]
fn clipping_hulls_leave_liquids_out() {
    let map = world(vec![
        cube_at(0, "*water"),
        cube_at(64, "b"),
        cube_at(128, "c"),
    ]);
    for hull in [Hull::Player, Hull::Large] {
        let set = load(&map, hull);
        assert_eq!(set.brushes.len(), 2, "{hull:?}");
        assert!(
            set.brushes
                .iter()
                .flat_map(|b| &b.faces)
                .all(|f| f.texinfo == 0)
        );
    }
    let set = load(&map, Hull::Draw);
    assert_eq!(set.brushes.len(), 3);
    assert!(
        set.brushes
            .iter()
            .flat_map(|b| &b.faces)
            .any(|f| f.texinfo != 0)
    );
}

// spec QBSP-4.3: each face's polygon is clipped by the other faces' planes, a box's faces each a
// quad
// spec QBSP-4.4: a brush's bounds are its points' extremes
#[test]
fn a_box_has_six_quads_and_its_bounds() {
    let map = world(vec![BrushSpec::cube([-8, 0, 16], [24, 40, 48], "a")]);
    let set = load(&map, Hull::Draw);
    let brush = &set.brushes[0];
    assert_eq!(brush.faces.len(), 6);
    assert!(brush.faces.iter().all(|f| f.points.len() == 4));
    assert_eq!(brush.mins, [-8.0, 0.0, 16.0]);
    assert_eq!(brush.maxs, [24.0, 40.0, 48.0]);
}

// spec QBSP-5.1: hull 1's box is (-16, -16, -32) to (16, 16, 24), hull 2's (-32, -32, -64) to
// (32, 32, 24)
// spec QBSP-5.2: each plane is pushed out by the box's corner on its side
// spec QBSP-5.5: the grown brush's faces are built again
#[test]
fn clipping_hulls_grow_brushes_by_their_box() {
    let map = world(vec![BrushSpec::cube([0, 0, 0], [64, 64, 64], "a")]);
    let player = load(&map, Hull::Player);
    assert_eq!(player.brushes[0].mins, [-16.0, -16.0, -32.0]);
    assert_eq!(player.brushes[0].maxs, [80.0, 80.0, 88.0]);
    let large = load(&map, Hull::Large);
    assert_eq!(large.brushes[0].mins, [-32.0, -32.0, -64.0]);
    assert_eq!(large.brushes[0].maxs, [96.0, 96.0, 88.0]);
}
