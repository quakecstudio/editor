// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    reason = "test code: a failed index is a failed test"
)]

use crate::Hull;
use crate::extended::brush::BspBrush;
use crate::extended::tree::Cutter;
use crate::extended::tree::tests::{cube, world_brushes};
use crate::fixtures::BrushSpec;

fn chopped(brushes: Vec<BrushSpec>) -> (Vec<BspBrush>, f64) {
    let (map, loaded, _) = world_brushes(brushes, Hull::Draw);
    let cutter = Cutter {
        planes: &map.planes,
        extent: map.world_extent,
    };
    let out = cutter.chop(loaded.brushes);
    let volume = out.iter().map(|b| cutter.volume(b)).sum();
    (out, volume)
}

// spec QBSP-15.30: a later brush bites what it overlaps from an earlier one, leaving the union's
// volume; a swallowed brush goes; brushes that only touch are left alone
#[test]
fn overlapping_brushes_are_chopped() {
    let (out, volume) = chopped(vec![
        cube([0, 0, 0], [32, 32, 32]),
        cube([16, 0, 0], [48, 32, 32]),
    ]);
    assert_eq!(out.len(), 2);
    assert!((volume - 48.0 * 32.0 * 32.0).abs() < 1e-6);
    assert_eq!(out[0].bounds.maxs[0], 16.0, "the earlier brush was bitten");
    assert_eq!(out[1].bounds.mins[0], 16.0);

    let (out, volume) = chopped(vec![
        cube([8, 8, 8], [16, 16, 16]),
        cube([0, 0, 0], [32, 32, 32]),
    ]);
    assert_eq!(out.len(), 1, "the small brush is swallowed");
    assert!((volume - 32768.0).abs() < 1e-6);

    let (out, _) = chopped(vec![
        cube([0, 0, 0], [32, 32, 32]),
        cube([32, 0, 0], [64, 32, 32]),
    ]);
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].sides.len(), 6);
}

// spec QBSP-15.30: only solid brushes bite and are bitten, and a bite that would
// leave several pieces of each is not taken
#[test]
fn what_may_bite() {
    let water = BrushSpec::cube([16, 0, 0], [48, 32, 32], "*water");
    let (out, _) = chopped(vec![cube([0, 0, 0], [32, 32, 32]), water]);
    assert_eq!(out[0].bounds.maxs[0], 32.0);
    assert_eq!(out[1].bounds.mins[0], 16.0);

    let cross = vec![
        cube([0, 12, 0], [32, 20, 32]),
        cube([12, 0, 0], [20, 32, 32]),
    ];
    let (avoided, _) = chopped(cross);
    assert_eq!(
        avoided.len(),
        2,
        "each bite leaves two pieces, so neither is taken"
    );
}
