// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a failed unwrap, index or overflow is a failed test"
)]

use qs_bsp::SURF_DRAWTILED;

use super::light_point;
use crate::mesh::tests::{LIGHTMAP, MapSpec, bsp, samples};

/// Fifteen samples numbered 0, 10, 20, ... row by row.
fn numbered() -> Vec<u8> {
    (0..LIGHTMAP[0] * LIGHTMAP[1])
        .map(|i| u8::try_from(i * 10).unwrap())
        .collect()
}

// spec REND-4.6: the light is the face's sample below the point, at (dt>>4) rows of
// (extents>>4)+1 and ds>>4 columns, times the style value, >> 8
#[test]
fn the_light_is_the_sample_below_the_point() {
    let lighting = numbered();
    let map = bsp(&MapSpec::default(), &lighting);
    // ds 40 >> 4 = 2, dt 20 >> 4 = 1: sample 1*5 + 2 = 7.
    assert_eq!(light_point(&map, [40.0, 20.0, 10.0]), 70);
    assert_eq!(light_point(&map, [0.0, 0.0, 2000.0]), 0);
}

// spec REND-4.6: the styles' samples are summed, each at the unset style's value
#[test]
fn every_style_adds_its_sample() {
    let lighting = samples(&[10, 20]);
    let spec = MapSpec {
        styles: [0, 1, 255, 255],
        ..MapSpec::default()
    };
    assert_eq!(light_point(&bsp(&spec, &lighting), [8.0, 8.0, 1.0]), 30);
}

// spec REND-4.6: a map with no lighting lump is 255; a line that crosses no face, or ends
// above it, or a face outside its extents, is 0
#[test]
fn unlit_maps_and_misses() {
    let lighting = samples(&[64]);
    assert_eq!(
        light_point(&bsp(&MapSpec::default(), &[]), [8.0, 8.0, 1.0]),
        255
    );
    let map = bsp(&MapSpec::default(), &lighting);
    assert_eq!(light_point(&map, [8.0, 8.0, 3000.0]), 0);
    assert_eq!(light_point(&map, [8.0, 8.0, -1.0]), 0);
    assert_eq!(light_point(&map, [100.0, 8.0, 1.0]), 0);
    assert_eq!(light_point(&map, [-1.0, 8.0, 1.0]), 0);
    assert_eq!(light_point(&map, [8.0, 8.0, 1.0]), 64);
}

// spec REND-4.6: a face hit with no samples is 0, and a tiled face is passed over
#[test]
fn faces_with_no_samples_or_drawn_tiled() {
    let lighting = samples(&[64]);
    let dark = MapSpec {
        lightofs: -1,
        ..MapSpec::default()
    };
    assert_eq!(light_point(&bsp(&dark, &lighting), [8.0, 8.0, 1.0]), 0);
    let tiled = MapSpec {
        flags: SURF_DRAWTILED,
        ..MapSpec::default()
    };
    assert_eq!(light_point(&bsp(&tiled, &lighting), [8.0, 8.0, 1.0]), 0);
}

#[test]
fn a_node_that_is_its_own_child_ends() {
    let lighting = samples(&[64]);
    let mut map = bsp(&MapSpec::default(), &lighting);
    map.nodes[0].children = [0, 0];
    // The walk stops at the node count instead of recursing for ever, then finds the face.
    assert_eq!(light_point(&map, [8.0, 8.0, 1.0]), 64);
}

/// Three nodes: node 0's plane is z = 4 with node 1 in front and node 2 behind; node 1's is
/// z = 8 and holds both faces, node 2's is z = -8 and holds face 0.
/// Face 0 starts at texel 16 and spans 64 by 32, its samples 10 + i; face 1 starts at 0 and
/// spans 128 by 128, its samples 100 + i. The texture axes slope: `s = x + z + 2`,
/// `t = y + 4`.
fn tree(lighting: &[u8]) -> qs_bsp::Bsp<'_> {
    let mut map = bsp(&MapSpec::default(), lighting);
    let plane = map.planes[0];
    map.planes = vec![
        qs_bsp::Plane { dist: 4.0, ..plane },
        qs_bsp::Plane { dist: 8.0, ..plane },
        qs_bsp::Plane {
            dist: -8.0,
            ..plane
        },
    ];
    let node = map.nodes[0];
    map.nodes = vec![
        qs_bsp::Node {
            planenum: 0,
            children: [1, 2],
            numfaces: 0,
            ..node
        },
        qs_bsp::Node {
            planenum: 1,
            numfaces: 2,
            ..node
        },
        qs_bsp::Node {
            planenum: 2,
            numfaces: 1,
            ..node
        },
    ];
    map.texinfo[0].vecs = [[1.0, 0.0, 1.0, 2.0], [0.0, 1.0, 0.0, 4.0]];
    let first = map.faces[0];
    map.faces[0].texturemins = [16, 16];
    map.faces.push(qs_bsp::Face {
        texturemins: [0, 0],
        extents: [128, 128],
        lightmap_size: [9, 9],
        lightofs: 15,
        ..first
    });
    map
}

fn tree_lighting() -> Vec<u8> {
    (0..15_u8)
        .map(|i| 10 + i)
        .chain((0..81_u8).map(|i| 100 + i))
        .collect()
}

// spec REND-4.6: the walk takes the near side first, crossing each plane at front/(front-back)
// of the way, and the face's s and t include the texinfo's offsets
#[test]
fn the_walk_crosses_each_plane_where_the_line_does() {
    let lighting = tree_lighting();
    let map = tree(&lighting);
    let light = |x: f32, y: f32, z: f32| light_point(&map, [x, y, z]);
    // At z = 8: s = x + 10, t = y + 4; face 0's sample ((t-16)>>4)*5 + ((s-16)>>4).
    assert_eq!(light(21.0, 12.0, 10.0), 10);
    assert_eq!(light(22.0, 12.0, 10.0), 11);
    assert_eq!(light(22.0, 12.0, 300.0), 11);
    // Starting on node 1's plane: front is 0, which is not behind it.
    assert_eq!(light(22.0, 12.0, 8.0), 11);
    // Ending on it: back is 0, the same side as the start, so nothing is crossed.
    assert_eq!(light(22.0, 12.0, 2056.0), 0);
    // Below node 0's plane both ends are behind it: node 2, crossed at z = -8, s = x - 6.
    assert_eq!(light(22.0, 12.0, 2.0), 10);
    assert_eq!(light(38.0, 12.0, 2.0), 11);
    // On node 0's plane: front is 0, so node 1 is the near side, where the line from the
    // plane to itself crosses nothing; the far side, node 2, is hit.
    assert_eq!(light(38.0, 12.0, 4.0), 11);
}

// spec REND-4.6: a face is hit when s - texturemins and t - texturemins are each from 0 to its
// extents; otherwise the next face is tried
#[test]
fn a_face_is_hit_within_its_extents_inclusive() {
    let lighting = tree_lighting();
    let map = tree(&lighting);
    let light = |x: f32, y: f32| light_point(&map, [x, y, 10.0]);
    // s = 16 and t = 16, face 0's first texel.
    assert_eq!(light(6.0, 12.0), 10);
    // s = 15: before face 0, so face 1's sample (15>>4) + (16>>4)*9.
    assert_eq!(light(5.0, 12.0), 109);
    // t = 15: before face 0, so face 1's (16>>4) + (15>>4)*9.
    assert_eq!(light(6.0, 11.0), 101);
    // s = 80, face 0's last: ds 64.
    assert_eq!(light(70.0, 12.0), 14);
    // s = 81: past face 0, face 1's (81>>4) + 9.
    assert_eq!(light(71.0, 12.0), 114);
    // t = 48, face 0's last row: dt 32.
    assert_eq!(light(6.0, 44.0), 20);
    // t = 49: past face 0, face 1's (16>>4) + (49>>4)*9.
    assert_eq!(light(6.0, 45.0), 128);
}
