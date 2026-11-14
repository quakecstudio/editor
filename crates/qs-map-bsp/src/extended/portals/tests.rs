// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::{PortalType, Portalizer, SideMarker};
use crate::Hull;
use crate::extended::brush::{HullBrushes, VisibleFaces};
use crate::extended::map::LoadedMap;
use crate::extended::tree::tests::{cube, grid_brushes, world_brushes};
use crate::extended::tree::{BspInput, NodeId, PARALLEL_DEPTH, Tree, TreeSplit};
use crate::extended::vec::Plane;
use crate::extended::winding::Winding;

fn cube_tree() -> (LoadedMap, Tree, HullBrushes, VisibleFaces) {
    let (mut map, loaded, visible) = world_brushes(vec![cube([0, 0, 0], [32, 32, 32])], Hull::Draw);
    let extent = map.world_extent;
    let mut tree = BspInput {
        planes: &mut map.planes,
        texinfos: &map.texinfos,
        visible: &visible,
        extent,
    }
    .brush_bsp(loaded.bounds, &loaded.brushes, TreeSplit::Precise);
    Portalizer {
        planes: &map.planes,
        extent,
    }
    .make_tree_portals(&mut tree);
    (map, tree, loaded, visible)
}

fn solid_leaf(tree: &Tree) -> NodeId {
    tree.leaves_below(Tree::HEAD)
        .into_iter()
        .find(|&l| tree.leaf_data(l).unwrap().contents.is_solid())
        .unwrap()
}

// spec QBSP-15.31: six portals around the head join it to a solid outside leaf
// spec QBSP-15.32: every node's portal is cut to its parents and pushed down to the leaves, so
// each portal is between two leaves
#[test]
fn portals_join_leaves() {
    let (_, tree, _, _) = cube_tree();
    let outside = tree.outside.unwrap();
    assert!(tree.leaf_data(outside).unwrap().contents.is_solid());
    let to_outside = tree.portals_of(outside);
    assert!(to_outside.len() >= 6, "the six, cut down the tree");
    assert!(
        to_outside
            .iter()
            .all(|&id| tree.portal(id).unwrap().onnode.is_none())
    );
    for p in &tree.portals {
        assert!(p.winding.len() >= 3);
        assert!(p.nodes.iter().all(|&n| tree.leaf_data(n).is_some()));
        assert_ne!(p.nodes[0], p.nodes[1]);
    }
    let solid = solid_leaf(&tree);
    let around = tree.portals_of(solid);
    assert_eq!(around.len(), 6, "a portal on each face of the cube");
    let area: f64 = around
        .iter()
        .map(|&id| tree.portal(id).unwrap().winding.area())
        .sum();
    assert!((area - 6.0 * 32.0 * 32.0).abs() < 1e-6);
    for &id in &around {
        let p = tree.portal(id).unwrap();
        assert!(p.onnode.is_some());
        let other = tree.across(id, solid).unwrap();
        assert!(tree.leaf_data(other).unwrap().contents.is_empty());
        assert!(!tree.vis_floods(id));
    }
    let open = tree
        .portals
        .iter()
        .position(|p| {
            p.onnode.is_some()
                && p.nodes
                    .iter()
                    .all(|&n| tree.leaf_data(n).unwrap().contents.is_empty())
        })
        .unwrap();
    assert!(tree.vis_floods(super::PortalId(open)));
}

// spec QBSP-15.33: each leaf's bounds are around its portals, each node's around its children's
#[test]
fn leaves_are_bounded_by_their_portals() {
    let (_, tree, _, _) = cube_tree();
    let solid = tree.node(solid_leaf(&tree)).unwrap();
    assert_eq!(
        (solid.bounds.mins, solid.bounds.maxs),
        ([0.0; 3], [32.0; 3])
    );
    let head = tree.node(Tree::HEAD).unwrap();
    assert_eq!(head.bounds.mins, [-24.0; 3]);
    assert_eq!(head.bounds.maxs, [56.0; 3]);
}

// spec QBSP-15.34: a portal across a change of contents shows the brush side on it, which marks
// its map face seen; one with no change shows none
#[test]
fn portals_show_brush_sides() {
    let (map, mut tree, loaded, mut visible) = cube_tree();
    let marker = SideMarker {
        planes: &map.planes,
        texinfos: &map.texinfos,
        originals: &loaded.brushes,
    };
    marker.hide_all(&loaded.brushes, &mut visible);
    let faces: Vec<_> = loaded.brushes[0]
        .sides
        .iter()
        .map(|s| s.source.unwrap())
        .collect();
    assert!(faces.iter().all(|&f| !visible.contains(f)));
    marker.mark_visible_sides(&mut tree, &loaded.brushes, &mut visible);
    assert!(faces.iter().all(|&f| visible.contains(f)));
    let solid = solid_leaf(&tree);
    for id in tree.portals_of(solid) {
        let p = tree.portal(id).unwrap();
        let sides = p.sides.unwrap();
        assert!(sides[0].is_some(), "solid is mirrored inside by default");
        assert_eq!(sides[0], sides[1], "the cube's face, both ways");
    }
    assert!(
        tree.portals
            .iter()
            .filter(|p| p
                .nodes
                .iter()
                .all(|&n| tree.leaf_data(n).unwrap().contents.is_empty()))
            .all(|p| p.sides.is_none())
    );
}

// spec QBSP-15.32: for vis, portals stop at a detail separator
#[test]
fn vis_portals_and_distances() {
    let (map, mut tree, _, _) = cube_tree();
    let count = tree.portals.len();
    Portalizer {
        planes: &map.planes,
        extent: map.world_extent,
    }
    .make_portals(&mut tree, PortalType::Vis);
    assert_eq!(tree.portals.len(), count, "no detail here");
    let w = Winding::new(vec![[0.0, 0.0, 1.0], [1.0, 0.0, -2.0], [0.0, 1.0, 0.5]]);
    let floor = Plane {
        normal: [0.0, 0.0, 1.0],
        dist: 0.0,
    };
    assert_eq!(w.max_dist_off_plane(floor), 2.0);
}

// spec QBSP-15.32: the children portalized on threads of their own give the portals made front
// first on one thread, in the same order
#[test]
fn portals_made_on_threads_are_those_made_on_one() {
    let (mut map, loaded, visible) = grid_brushes(5);
    let extent = map.world_extent;
    let mut tree = BspInput {
        planes: &mut map.planes,
        texinfos: &map.texinfos,
        visible: &visible,
        extent,
    }
    .brush_bsp(loaded.bounds, &loaded.brushes, TreeSplit::Auto);
    let portalizer = Portalizer {
        planes: &map.planes,
        extent,
    };
    let around = portalizer.head_portals(&mut tree);
    let make = |depth| {
        portalizer.tree_portals(&tree, (Tree::HEAD, depth), PortalType::Tree, around.clone())
    };
    let threaded = make(0);
    assert!(threaded.len() > 100, "{}", threaded.len());
    assert_eq!(threaded, make(PARALLEL_DEPTH));
}
