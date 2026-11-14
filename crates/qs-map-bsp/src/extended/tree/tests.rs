// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::float_cmp,
    reason = "test code: a failed unwrap, index or comparison is a failed test"
)]

use qs_map::MapFile;

use super::{BspInput, Builder, Cutter, NodeId, NodeKind, PlaneSides, Tree, TreeSplit};
use crate::Hull;
use crate::extended::brush::{HullBrushes, Loader, SideMarks, VisibleFaces};
use crate::extended::game::{Contents, Game};
use crate::extended::map::{EntityNum, LoadedMap, Texinfos, load};
use crate::extended::vec::{Bounds, BspPlane, Facing, Plane, PlaneNum, Planes, normalize};
use crate::fixtures::{BrushSpec, MapSpec, map_text};

/// A world's brushes for `hull`, sorted, with what they were loaded from.
pub(crate) fn world_brushes(
    brushes: Vec<BrushSpec>,
    hull: Hull,
) -> (LoadedMap, HullBrushes, VisibleFaces) {
    let spec = MapSpec {
        world: brushes,
        entities: Vec::new(),
        ..MapSpec::default()
    };
    let file = MapFile::parse(map_text(&spec).as_bytes()).unwrap();
    let mut map = load(&file, Game::Quake);
    let mut visible = VisibleFaces::default();
    let loaded = Loader {
        file: &file,
        map: &mut map,
        game: Game::Quake,
        visible: &mut visible,
    }
    .load_entity(EntityNum::WORLD, hull)
    .sorted();
    (map, loaded, visible)
}

pub(crate) fn cube(mins: [i32; 3], maxs: [i32; 3]) -> BrushSpec {
    BrushSpec::cube(mins, maxs, "wall")
}

fn bounds(mins: [f64; 3], maxs: [f64; 3]) -> Bounds {
    Bounds { mins, maxs }
}

fn axial(planes: &mut Planes, axis: usize, dist: f64) -> PlaneNum {
    let mut normal = [0.0; 3];
    normal[axis] = 1.0;
    planes.add_or_find(Plane { normal, dist })
}

/// The leaf `point` falls in.
fn leaf_at(tree: &Tree, map: &LoadedMap, point: [f64; 3]) -> NodeId {
    let mut id = Tree::HEAD;
    loop {
        match &tree.node(id).unwrap().kind {
            NodeKind::Leaf(_) => return id,
            NodeKind::Split {
                planenum, children, ..
            } => {
                let plane = map.planes.get(*planenum).unwrap().plane();
                id = if plane.distance_to(point) >= 0.0 {
                    children[0]
                } else {
                    children[1]
                };
            }
        }
    }
}

fn contents_at(tree: &Tree, map: &LoadedMap, point: [f64; 3]) -> Contents {
    match &tree.node(leaf_at(tree, map, point)).unwrap().kind {
        NodeKind::Leaf(leaf) => leaf.contents,
        NodeKind::Split { .. } => panic!("a leaf"),
    }
}

// spec QBSP-15.25: a box's brush has a third of the tetrahedrons' volume; a brush across a plane
// is cut into two, each with the plane's polygon as a skip side on the node, one reaching less
// than 0.1 past it is whole on its side
#[test]
fn brushes_are_cut_by_planes() {
    let (mut map, loaded, _) = world_brushes(vec![cube([0, 0, 0], [32, 32, 32])], Hull::Draw);
    let extent = map.world_extent;
    let half = axial(&mut map.planes, 0, 16.0);
    let near = axial(&mut map.planes, 0, 31.95);
    let box_brush = Cutter::from_bounds(&mut map.planes, bounds([0.0; 3], [8.0, 4.0, 2.0]), extent);
    let cutter = Cutter {
        planes: &map.planes,
        extent,
    };
    assert!((cutter.volume(&box_brush) - 64.0).abs() < 1e-9);
    assert_eq!(box_brush.sides[0].texinfo, Texinfos::SKIP);
    assert_eq!(box_brush.bounds.maxs, [8.0, 4.0, 2.0]);
    let brush = loaded.brushes[0].clone();
    assert!((cutter.volume(&brush) - 32768.0).abs() < 1e-6);
    let [front, back] = cutter.split(brush.clone(), half);
    let (front, back) = (front.unwrap(), back.unwrap());
    assert_eq!((front.sides.len(), back.sides.len()), (6, 6));
    assert!((cutter.volume(&front) - 16384.0).abs() < 1e-6);
    assert_eq!(front.bounds.mins[0], 16.0);
    assert_eq!(back.bounds.maxs[0], 16.0);
    let mid = front.sides.last().unwrap();
    assert_eq!(mid.planenum, half.opposite());
    assert!(mid.marks.has(SideMarks::ON_NODE) && mid.source.is_none());
    assert_eq!(mid.texinfo, Texinfos::SKIP);
    assert_eq!(back.sides.last().unwrap().planenum, half);
    assert_eq!(front.original, brush.original);
    let [front, back] = cutter.split(brush.clone(), near);
    assert_eq!((front, back.is_some()), (None, true), "within 0.1");
    let [front, back] = cutter.split(brush, near.opposite());
    assert_eq!((front.is_some(), back), (true, None));
}

// spec QBSP-15.26: a box is in front of an axial plane when it reaches past it by more than
// 0.001, behind when below it; for another plane by its leading and trailing corners; a brush
// with a side on the plane is behind it and facing
#[test]
fn boxes_and_brushes_against_planes() {
    let b = bounds([0.0; 3], [32.0; 3]);
    let x = |dist| {
        BspPlane::new(
            Plane {
                normal: [1.0, 0.0, 0.0],
                dist,
            },
            Facing::AsGiven,
        )
        .0
    };
    assert_eq!(b.plane_sides(&x(16.0)), PlaneSides::BOTH);
    assert_eq!(b.plane_sides(&x(32.0)), PlaneSides::BACK);
    assert_eq!(b.plane_sides(&x(31.9995)), PlaneSides::BACK);
    assert_eq!(b.plane_sides(&x(31.99)), PlaneSides::BOTH);
    assert_eq!(b.plane_sides(&x(0.0)), PlaneSides::FRONT);
    let diagonal = |dist| {
        BspPlane::new(
            Plane {
                normal: normalize([1.0, 1.0, 0.0]),
                dist,
            },
            Facing::AsGiven,
        )
        .0
    };
    assert_eq!(b.plane_sides(&diagonal(20.0)), PlaneSides::BOTH);
    assert_eq!(b.plane_sides(&diagonal(100.0)), PlaneSides::BACK);
    assert_eq!(b.plane_sides(&diagonal(-1.0)), PlaneSides::FRONT);

    let (mut map, loaded, visible) = world_brushes(vec![cube([0, 0, 0], [32, 32, 32])], Hull::Draw);
    let top = axial(&mut map.planes, 2, 32.0);
    let middle = axial(&mut map.planes, 2, 16.0);
    let shaved = axial(&mut map.planes, 2, 31.5);
    let builder = Builder {
        cutter: Cutter {
            planes: &map.planes,
            extent: map.world_extent,
        },
        texinfos: &map.texinfos,
        visible: &visible,
    };
    let brush = &loaded.brushes[0];
    let on = builder.test(brush, top);
    assert!(on.sides.is_facing() && on.sides.has(PlaneSides::BACK));
    let under = builder.test(brush, top.opposite());
    assert!(under.sides.is_facing() && under.sides.has(PlaneSides::FRONT));
    let across = builder.test(brush, middle);
    assert_eq!((across.sides, across.splits), (PlaneSides::BOTH, 4));
    assert_eq!(across.epsilon_brushes, 0);
    assert_eq!(builder.test(brush, shaved).epsilon_brushes, 1);
}

// spec QBSP-15.27: an axial plane divides a box at its distance, another narrows the parts to
// where it crosses the box; the even split scores best
#[test]
fn planes_divide_boxes() {
    let b = bounds([0.0; 3], [32.0; 3]);
    let plane = |normal, dist| BspPlane::new(Plane { normal, dist }, Facing::AsGiven).0;
    let [front, back] = b.divide(&plane([0.0, 1.0, 0.0], 8.0));
    assert_eq!((front.mins[1], back.maxs[1]), (8.0, 8.0));
    assert!(b.split_metric(&plane([0.0, 1.0, 0.0], 16.0)) < 1e-9);
    assert!((b.split_metric(&plane([0.0, 1.0, 0.0], 8.0)) - 32.0 * 32.0 * 16.0).abs() < 1e-9);
    let n = normalize([1.0, 1.0, 0.0]);
    let [front, back] = b.divide(&plane(n, n[0] * 48.0));
    assert!((front.mins[0] - 16.0).abs() < 1e-9 && (front.mins[1] - 16.0).abs() < 1e-9);
    assert_eq!((back.maxs[0], back.maxs[1]), (32.0, 32.0));
    assert_eq!(front.maxs, b.maxs);
    let [front, back] = b.divide(&plane(normalize([-1.0, -1.0, 0.0]), -n[0] * 48.0));
    assert!((back.mins[0] - 16.0).abs() < 1e-9);
    assert_eq!(front.maxs[0], 32.0);
}

// spec QBSP-15.28: the precise choice takes the plane that splits fewest sides, axial first; the
// fast one the plane that halves the node best; a plane is never one already on a node
// spec QBSP-15.29: the tree's leaves hold the combined contents of the brushes in them, and its
// head covers the brushes and the side space
#[test]
fn trees_of_brushes() {
    let (mut map, loaded, visible) = world_brushes(
        vec![
            cube([0, 0, 0], [32, 32, 32]),
            BrushSpec::cube([64, 0, 0], [96, 32, 32], "*water"),
        ],
        Hull::Draw,
    );
    let extent = map.world_extent;
    for split in [TreeSplit::Precise, TreeSplit::Fast, TreeSplit::Auto] {
        let tree = BspInput {
            planes: &mut map.planes,
            texinfos: &map.texinfos,
            visible: &visible,
            extent,
        }
        .brush_bsp(loaded.bounds, &loaded.brushes, split);
        let head = tree.node(Tree::HEAD).unwrap();
        assert_eq!(head.bounds.mins, [-24.0; 3]);
        assert_eq!(head.bounds.maxs, [120.0, 56.0, 56.0]);
        assert!(head.volume.is_none());
        assert!(contents_at(&tree, &map, [16.0, 16.0, 16.0]).is_solid());
        assert!(contents_at(&tree, &map, [80.0, 16.0, 16.0]).is_liquid());
        assert!(contents_at(&tree, &map, [48.0, 16.0, 16.0]).is_empty());
        assert!(contents_at(&tree, &map, [16.0, 16.0, 40.0]).is_empty());
        let mut planes = Vec::new();
        for node in &tree.nodes {
            if let NodeKind::Split { planenum, .. } = node.kind {
                assert_eq!(planenum, planenum.positive());
                assert!(!planes.contains(&planenum) || split != TreeSplit::Precise);
                planes.push(planenum);
            }
        }
        assert!(planes.len() >= 6, "{split:?}");
    }
    let empty = BspInput {
        planes: &mut map.planes,
        texinfos: &map.texinfos,
        visible: &visible,
        extent,
    }
    .brush_bsp(loaded.bounds, &[], TreeSplit::Fast);
    assert_eq!(empty.nodes.len(), 3);
    let NodeKind::Split { planenum, .. } = empty.node(Tree::HEAD).unwrap().kind else {
        panic!("a split head");
    };
    assert_eq!(planenum.get(), 0);
}

// spec QBSP-15.29: a node's children's boxes are cut at its plane when it faces along an axis
#[test]
fn children_bounds_follow_the_plane() {
    let (mut map, loaded, visible) = world_brushes(vec![cube([0, 0, 0], [32, 32, 32])], Hull::Draw);
    let extent = map.world_extent;
    let tree = BspInput {
        planes: &mut map.planes,
        texinfos: &map.texinfos,
        visible: &visible,
        extent,
    }
    .brush_bsp(loaded.bounds, &loaded.brushes, TreeSplit::Precise);
    let NodeKind::Split {
        planenum, children, ..
    } = tree.node(Tree::HEAD).unwrap().kind
    else {
        panic!("a split head");
    };
    let plane = map.planes.get(planenum).unwrap().plane();
    let axis = plane.normal.iter().position(|n| *n == 1.0).unwrap();
    let front = tree.node(children[0]).unwrap();
    let back = tree.node(children[1]).unwrap();
    assert_eq!(front.bounds.mins[axis], plane.dist);
    assert_eq!(back.bounds.maxs[axis], plane.dist);
    assert_eq!(front.parent, Some(Tree::HEAD));
    let leaves = tree
        .nodes
        .iter()
        .filter(|n| matches!(n.kind, NodeKind::Leaf(_)))
        .count();
    assert_eq!(leaves, 7, "the cube and an empty leaf beyond each side");
}
