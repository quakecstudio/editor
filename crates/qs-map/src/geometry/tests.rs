// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use proptest::prelude::*;

use super::{
    MAP_BASE_AXES, MAP_ON_EPSILON, on_plane, plane, qbsp_on_plane, qbsp_plane, qbsp_texvecs,
    standard_texvecs, valve_texvecs,
};

const DIAGONAL: f32 = core::f32::consts::FRAC_1_SQRT_2;

// spec MAP-4.2: the base axes, the earliest row winning a tie.
#[test]
fn base_axes() {
    assert_eq!(MAP_BASE_AXES[0], [[0, 0, 1], [1, 0, 0], [0, -1, 0]]);
    assert_eq!(MAP_BASE_AXES[5], [[0, -1, 0], [1, 0, 0], [0, 0, -1]]);
    let unit = [1.0, 1.0];
    // A tie between floor and west wall: the floor wins.
    let vecs = qbsp_texvecs([DIAGONAL, 0.0, DIAGONAL], [0.0; 2], 0.0, unit);
    assert_eq!(vecs, [[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0]]);
    // Closest to the east wall.
    let vecs = qbsp_texvecs([-0.8, 0.0, 0.6], [0.0; 2], 0.0, unit);
    assert_eq!(vecs, [[0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0]]);
    // No positive dot product: the floor.
    let vecs = qbsp_texvecs([0.0; 3], [0.0; 2], 0.0, unit);
    assert_eq!(vecs, [[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0]]);
    let vecs = standard_texvecs([0.0, -1.0, 0.0], [0.0; 2], 0.0, [1.0; 2]);
    assert_eq!(vecs, [[1.0, 0.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0]]);
}

// spec MAP-4.3: right-angle rotations are exact, others use sine and cosine.
#[test]
fn rotation() {
    let floor = [0.0, 0.0, 1.0];
    let unit = [1.0, 1.0];
    let rotated = |rotate| qbsp_texvecs(floor, [0.0; 2], rotate, unit);
    assert_eq!(rotated(90.0), [[0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
    assert_eq!(
        rotated(180.0),
        [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0]]
    );
    assert_eq!(
        rotated(270.0),
        [[0.0, -1.0, 0.0, 0.0], [-1.0, 0.0, 0.0, 0.0]]
    );
    let vecs = rotated(45.0);
    assert!((vecs[0][0] - DIAGONAL).abs() < 1e-6);
    assert!((vecs[0][1] - DIAGONAL).abs() < 1e-6);
    // 360 is not one of the exact angles; its sine is not exactly 0.
    let vecs = rotated(360.0);
    assert!((vecs[0][0] - 1.0).abs() < 1e-6);
    let wide = standard_texvecs([0.0, 0.0, 1.0], [0.0; 2], 90.0, [1.0; 2]);
    assert_eq!(wide, [[0.0, 1.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0]]);
}

// spec MAP-4.4: axes are divided by their scale, 0 counting as 1; offsets are the shifts.
#[test]
fn scale_and_shift() {
    let vecs = qbsp_texvecs([0.0, 0.0, 1.0], [3.0, -4.0], 0.0, [0.0, 4.0]);
    assert_eq!(vecs, [[1.0, 0.0, 0.0, 3.0], [0.0, -0.25, 0.0, -4.0]]);
    let vecs = standard_texvecs([0.0, 0.0, 1.0], [3.0, -4.0], 0.0, [2.0, 0.0]);
    assert_eq!(vecs, [[0.5, 0.0, 0.0, 3.0], [0.0, -1.0, 0.0, -4.0]]);
}

// spec MAP-5.3, MAP-5.4: Valve 220 axes divided by their scale; a scale of 0 is refused.
#[test]
fn valve() {
    let vecs = valve_texvecs([1.0, 0.0, 0.0, 16.0], [0.0, -1.0, 0.0, -8.0], [0.5, 2.0]);
    assert_eq!(vecs, Some([[2.0, 0.0, 0.0, 16.0], [0.0, -0.5, 0.0, -8.0]]));
    assert_eq!(valve_texvecs([1.0; 4], [1.0; 4], [0.0, 1.0]), None);
    assert_eq!(valve_texvecs([1.0; 4], [1.0; 4], [1.0, -0.0]), None);
}

// spec MAP-3.4, MAP-3.5, MAP-3.6: planes from three points, collinear points, the epsilon.
#[test]
fn planes() {
    let points = [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let wide = plane(points).unwrap();
    assert_eq!(wide.normal.map(|v| v + 0.0), [-1.0, 0.0, 0.0]);
    let narrow = qbsp_plane(points.map(|point| point.map(|v| v as f32))).unwrap();
    assert_eq!(narrow.normal.map(|v| v + 0.0), [-1.0, 0.0, 0.0]);
    assert!(plane([[0.0; 3], [1.0; 3], [2.0; 3]]).is_none());
    assert!(qbsp_plane([[0.0; 3], [1.0; 3], [2.0; 3]]).is_none());
    assert_eq!(MAP_ON_EPSILON.to_bits(), 0.05_f64.to_bits());
    let near = [[0.05, 5.0, 5.0], [-0.05, 0.0, 0.0], [0.0, 9.0, 1.0]];
    assert!(on_plane(&near, &wide));
    let far = [[0.06, 5.0, 5.0], [0.0, 0.0, 0.0], [0.0, 9.0, 1.0]];
    assert!(!on_plane(&far, &wide));
    assert!(qbsp_on_plane(&[[0.0, 7.0, 7.0]; 3], &narrow));
    assert!(!qbsp_on_plane(&[[1.0, 7.0, 7.0]; 3], &narrow));
}

proptest! {
    // spec MAP-3.4: every face plane holds its three points and has a unit normal.
    #[test]
    fn planes_hold_their_points(values in proptest::collection::vec(-1000_i32..1000, 9)) {
        let point = |index: usize| [0, 1, 2].map(|axis| f64::from(values[index * 3 + axis]));
        let points = [point(0), point(1), point(2)];
        if let Some(found) = plane(points) {
            let length: f64 = found.normal.iter().map(|v| v * v).sum::<f64>().sqrt();
            prop_assert!((length - 1.0).abs() < 1e-9);
            for point in points {
                let d: f64 = point.iter().zip(found.normal).map(|(p, n)| p * n).sum::<f64>()
                    - found.dist;
                prop_assert!(d.abs() < 1e-6);
            }
        }
    }
}
