// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_map::MapFile;

use super::{HullBrushes, Loader, Side, VisibleFaces, is_world_brush_entity};
use crate::Hull;
use crate::extended::game::{Contents, Game};
use crate::extended::map::{EntityNum, SideKind, load};
use crate::extended::vec::Plane;
use crate::extended::winding::Winding;
use crate::fixtures::{BrushSpec, EntitySpec, MapSpec, Shape, map_text};

fn hull(spec: &MapSpec, entity: usize, hull: Hull) -> (HullBrushes, VisibleFaces) {
    let file = MapFile::parse(map_text(spec).as_bytes()).unwrap();
    let mut map = load(&file, Game::Quake);
    let mut visible = VisibleFaces::default();
    let brushes = Loader {
        file: &file,
        map: &mut map,
        game: Game::Quake,
        visible: &mut visible,
    }
    .load_entity(EntityNum::new(entity), hull);
    (brushes, visible)
}

fn only_world(brushes: Vec<BrushSpec>) -> MapSpec {
    MapSpec {
        world: brushes,
        entities: Vec::new(),
        ..MapSpec::default()
    }
}

fn wedge() -> BrushSpec {
    BrushSpec {
        shape: Shape::Prism {
            corners: vec![[0, 0], [64, 0], [0, 32]],
            bottom: 0,
            top: 16,
        },
        texture: "wall".into(),
    }
}

// spec QBSP-15.22: a brush's sides get polygons and bounds, its bevels left out of the drawing
// hull; a clipping hull moves each side out by the corner of its box the normal points to
#[test]
fn brushes_for_each_hull() {
    let spec = only_world(vec![BrushSpec::cube([0, 0, 0], [32, 32, 32], "wall")]);
    let (draw, visible) = hull(&spec, 0, Hull::Draw);
    let brush = &draw.brushes[0];
    assert_eq!(brush.sides.len(), 6);
    assert!(brush.sides.iter().all(|s| s.winding.len() == 4));
    assert_eq!(
        (brush.bounds.mins, brush.bounds.maxs),
        ([0.0; 3], [32.0; 3])
    );
    assert_eq!(draw.bounds, brush.bounds);
    assert!(visible.contains(brush.sides[0].source.unwrap()));
    assert_eq!(brush.contents, Contents::new(Contents::SOLID));

    let (player, _) = hull(&spec, 0, Hull::Player);
    let b = &player.brushes[0];
    assert_eq!(
        (b.bounds.mins, b.bounds.maxs),
        ([-16.0, -16.0, -32.0], [48.0, 48.0, 56.0])
    );
    let (large, _) = hull(&spec, 0, Hull::Large);
    let b = &large.brushes[0];
    assert_eq!(
        (b.bounds.mins, b.bounds.maxs),
        ([-32.0, -32.0, -64.0], [64.0, 64.0, 56.0])
    );

    let spec = only_world(vec![wedge()]);
    let (draw, _) = hull(&spec, 0, Hull::Draw);
    assert_eq!(draw.brushes[0].sides.len(), 5);
    let (player, _) = hull(&spec, 0, Hull::Player);
    assert_eq!(player.brushes[0].sides.len(), 7);
    assert!(
        player.brushes[0]
            .sides
            .iter()
            .all(|s| s.kind == SideKind::Given)
    );
}

fn side(points: Vec<[f64; 3]>) -> Side {
    let spec = only_world(vec![BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall")]);
    let mut side = hull(&spec, 0, Hull::Draw).0.brushes[0].sides[0].clone();
    side.winding = Winding::new(points);
    side
}

// spec QBSP-15.23: a polygon of fewer than three points is none; a point next to its neighbour
// is dropped; a point outside an edge makes the polygon none
#[test]
fn polygons_are_checked() {
    let floor = Plane {
        normal: [0.0, 0.0, 1.0],
        dist: 0.0,
    };
    let mut s = side(vec![[0.0; 3], [1.0, 0.0, 0.0]]);
    s.check(floor);
    assert!(s.winding.is_empty());
    // Clockwise seen from the normal's side, as a plane's square is (QBSP-15.7).
    let mut s = side(vec![
        [0.0, 0.0, 0.0],
        [0.0, 8.0, 0.0],
        [8.0, 8.0, 0.0],
        [8.0, 0.000_01, 0.0],
        [8.0, 0.0, 0.0],
    ]);
    s.check(floor);
    assert_eq!(s.winding.len(), 4);
    let mut s = side(vec![
        [0.0, 0.0, 0.0],
        [0.0, 8.0, 0.0],
        [2.0, 2.0, 0.0],
        [8.0, 0.0, 0.0],
    ]);
    s.check(floor);
    assert!(s.winding.is_empty(), "not convex");
    let mut s = side(vec![[0.0, 0.0, 0.0], [0.0, 8.0, 0.0], [8.0, 0.0, 0.0]]);
    s.check(floor);
    assert_eq!(s.winding.len(), 3);
}

// spec QBSP-15.23: a side is seen when its map face is, a skip side never
#[test]
fn sides_are_seen_through_their_faces() {
    let spec = only_world(vec![
        BrushSpec::cube([0, 0, 0], [8, 8, 8], "wall"),
        BrushSpec::cube([16, 0, 0], [24, 8, 8], "skip"),
    ]);
    let file = MapFile::parse(map_text(&spec).as_bytes()).unwrap();
    let mut map = load(&file, Game::Quake);
    let mut visible = VisibleFaces::default();
    let brushes = Loader {
        file: &file,
        map: &mut map,
        game: Game::Quake,
        visible: &mut visible,
    }
    .load_entity(EntityNum::WORLD, Hull::Draw);
    let wall = &brushes.brushes[0].sides[0];
    let skip = &brushes.brushes[1].sides[0];
    assert!(wall.is_visible(&map.texinfos, &visible));
    assert!(!skip.is_visible(&map.texinfos, &visible));
    let face = wall.source.unwrap();
    visible.hide(face);
    assert!(!wall.is_visible(&map.texinfos, &visible));
    visible.show(face);
    assert!(wall.is_visible(&map.texinfos, &visible));
}

// spec QBSP-15.24: the world takes the world brush entities' brushes, detail by class in the
// drawing hull; clip brushes only bound the drawing hull; hint brushes are empty there and in no
// other hull; liquids and illusionary detail are in no clipping hull; a brush entity's brushes
// are solid
#[test]
fn each_hull_sees_its_contents() {
    let mut spec = MapSpec {
        world: vec![
            BrushSpec::cube([0, 0, 0], [8, 8, 8], "clip"),
            BrushSpec::cube([16, 0, 0], [24, 8, 8], "hint"),
            BrushSpec::cube([32, 0, 0], [40, 8, 8], "*water"),
        ],
        entities: vec![
            EntitySpec::brushes(
                "FUNC_DETAIL",
                vec![BrushSpec::cube([48, 0, 0], [56, 8, 8], "wall")],
            ),
            EntitySpec::brushes(
                "func_detail_illusionary",
                vec![BrushSpec::cube([64, 0, 0], [72, 8, 8], "wall")],
            ),
            EntitySpec::brushes(
                "func_wall",
                vec![BrushSpec::cube([80, 0, 0], [88, 8, 8], "*slime")],
            ),
        ],
        ..MapSpec::default()
    };
    let (draw, _) = hull(&spec, 0, Hull::Draw);
    let contents: Vec<Contents> = draw.brushes.iter().map(|b| b.contents).collect();
    assert_eq!(contents.len(), 4, "the clip brush only bounds the hull");
    assert_eq!(draw.bounds.mins[0], 0.0);
    assert_eq!(contents[0], Contents::EMPTY, "hint");
    assert!(contents[1].is_liquid());
    assert!(contents[2].is_detail_solid());
    assert!(contents[3].is_detail_illusionary());
    let (player, _) = hull(&spec, 0, Hull::Player);
    let solid = Contents::new(Contents::SOLID);
    assert_eq!(player.brushes.len(), 2, "clip and detail");
    assert!(player.brushes.iter().all(|b| b.contents == solid));
    let (wall, _) = hull(&spec, 3, Hull::Draw);
    assert_eq!(wall.brushes[0].contents, solid.detail_fence());

    spec.world_pairs = vec![("_super_detail".into(), "1".into())];
    assert_eq!(hull(&spec, 0, Hull::Draw).0.brushes.len(), 2);
    assert_eq!(hull(&spec, 0, Hull::Player).0.brushes.len(), 2);

    let file = MapFile::parse(map_text(&spec).as_bytes()).unwrap();
    assert!(is_world_brush_entity(&file.entities[1]));
    assert!(!is_world_brush_entity(&file.entities[3]));
}
