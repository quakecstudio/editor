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
