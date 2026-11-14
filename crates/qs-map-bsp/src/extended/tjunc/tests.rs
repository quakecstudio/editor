// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    reason = "test code: a failed index is a failed test"
)]

use super::{MAX_EDGES, TJuncs};
use crate::extended::game::Contents;
use crate::extended::tree::Tree;
use crate::extended::vec::{Bounds, Planes, dot, normalize_with_length};

fn tree() -> Tree {
    Tree {
        nodes: Vec::new(),
        bounds: Bounds::default(),
        portals: Vec::new(),
        outside: None,
        faces: Vec::new(),
    }
}

// spec QBSP-15.47: a point on an edge within DEFAULT_ON_EPSILON and strictly between its ends,
// as `float`s, is on it
#[test]
fn points_on_edges() {
    let on = TJuncs::point_on_edge([5.0, 0.05, 0.0], [0.0; 3], [1.0, 0.0, 0.0], 0.0, 10.0);
    assert_eq!(on, Some(5.0));
    assert_eq!(
        TJuncs::point_on_edge([5.0, 0.2, 0.0], [0.0; 3], [1.0, 0.0, 0.0], 0.0, 10.0),
        None
    );
    assert_eq!(
        TJuncs::point_on_edge([10.0, 0.0, 0.0], [0.0; 3], [1.0, 0.0, 0.0], 0.0, 10.0),
        None,
        "an end is not on the edge"
    );
}

// spec QBSP-15.47: back contents weld with their own type, never a detail wall with another,
// never see-through with opaque, never with empty
#[test]
fn which_faces_weld() {
    let solid = Contents::new(Contents::SOLID);
    let water = Contents::new(Contents::WATER | Contents::TRANSLUCENT);
    assert!(solid.welds(solid));
    assert!(solid.welds(Contents::new(Contents::SKY)));
    assert!(!solid.welds(solid.detail_wall()));
    assert!(!solid.welds(water));
    assert!(!solid.welds(Contents::EMPTY));
}

// spec QBSP-15.47: an edge is broken at each vertex on it, and the polygon triangulated into the
// fewest fans of its minimum-weight triangulation, each of at most 64 vertices
#[test]
fn edges_are_broken_and_faces_fanned() {
    let t = tree();
    let planes = Planes::default();
    let vertexes = [
        [0.0, 0.0, 0.0],
        [0.0, 10.0, 0.0],
        [10.0, 10.0, 0.0],
        [10.0, 0.0, 0.0],
        [5.0, 0.0, 0.0],
    ];
    let fixer = TJuncs {
        tree: &t,
        vertexes: &vertexes,
        planes: &planes,
    };
    let mut superface = Vec::new();
    let a = fixer.at(3);
    let (dir, len) = normalize_with_length([-10.0, 0.0, 0.0]);
    fixer.test_edge((0.0, len), 3, 0, 0, &[4, 0, 3], (a, dir), &mut superface);
    assert_eq!(superface, [3, 4], "the edge broken at vertex 4");
    let pentagon = [0, 1, 2, 3, 4];
    let points: Vec<[f64; 2]> = pentagon
        .iter()
        .map(|&v| {
            let p = fixer.at(v);
            [p[0], p[1]]
        })
        .collect();
    let triangles = fixer.mwt(&pentagon, &points);
    assert_eq!(triangles.len(), 3);
    let fans = TJuncs::fans(triangles, &pentagon);
    let total: usize = fans.iter().map(|f| f.len() - 2).sum();
    assert_eq!(total, 3, "the fans cover the three triangles");
    assert!(fans.len() <= 2);
    assert!(!fixer.triangle_is_valid(0, 4, 3), "a flat triangle");
    assert!(fixer.triangle_is_valid(0, 1, 2));
    let big: Vec<u32> = (0..100).collect();
    let pieces = TJuncs::split_large(vec![big]);
    assert!(pieces.iter().all(|p| p.len() <= MAX_EDGES));
    assert_eq!(pieces.len(), 2);
    assert_eq!(
        pieces[0].len(),
        MAX_EDGES,
        "the list's first moved to its end"
    );
    assert_eq!(pieces[1].len(), 100 - MAX_EDGES + 2);
    let (u, v) = TJuncs::tangents([0.0, 0.0, 1.0]);
    assert_eq!(dot(u, v), 0.0);
}
