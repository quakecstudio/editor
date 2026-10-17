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

// spec MAP-4.2: best_row picks the base axis with the largest positive dot product; a normal
// closest to the ceiling (row 1) must not be read as the floor (row 0).
#[test]
fn best_row_picks_the_largest_dot() {
    // Straight down: dot with the ceiling axis [0,0,-1] is +1, with the floor [0,0,1] is -1.
    // The largest wins (row 1, ceiling), so the s axis is [1,0,0] and t is [0,-1,0]; picking
    // the smallest dot instead would land on the floor and give the same first axes but a
    // different normal-to-row mapping, so use the east wall to separate the rows cleanly.
    let down = standard_texvecs([0.0, 0.0, -1.0], [0.0; 2], 0.0, [1.0; 2]);
    assert_eq!(down, [[1.0, 0.0, 0.0, 0.0], [0.0, -1.0, 0.0, 0.0]]);
    // A normal pointing +x is the west wall (row 2): s axis [0,1,0], t axis [0,0,-1].
    let east = standard_texvecs([1.0, 0.0, 0.0], [0.0; 2], 0.0, [1.0; 2]);
    assert_eq!(east, [[0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, 0.0]]);
}

// spec MAP-4.2: the base axes of every row, so a dropped sign in any row is caught.
#[test]
fn base_axes_all_rows() {
    assert_eq!(
        MAP_BASE_AXES,
        [
            [[0, 0, 1], [1, 0, 0], [0, -1, 0]],
            [[0, 0, -1], [1, 0, 0], [0, -1, 0]],
            [[1, 0, 0], [0, 1, 0], [0, 0, -1]],
            [[-1, 0, 0], [0, 1, 0], [0, 0, -1]],
            [[0, 1, 0], [1, 0, 0], [0, 0, -1]],
            [[0, -1, 0], [1, 0, 0], [0, 0, -1]],
        ]
    );
}

// spec MAP-3.4, MAP-3.6: a plane normal is the cross product divided by its length, so for a
// cross product whose length is not 1 the division cannot be a multiplication.
#[test]
fn plane_divides_the_cross_by_its_length() {
    // cross = t1 x t2 with t1 = p0 - p1 = [-4,0,0], t2 = p2 - p1 = [-4,4,2]; cross = [0,8,-16],
    // length = sqrt(320) ~ 17.888, so the unit normal is [0, 0.4472136, -0.8944272].
    let points = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 2.0]];
    let wide = plane(points).unwrap();
    let expect = [0.0, 8.0 / 320.0_f64.sqrt(), -16.0 / 320.0_f64.sqrt()];
    for (got, want) in wide.normal.iter().zip(expect) {
        assert!((got - want).abs() < 1e-9, "{got} != {want}");
    }
    let narrow = qbsp_plane(points.map(|p| p.map(|v| v as f32))).unwrap();
    for (got, want) in narrow.normal.iter().zip(expect) {
        assert!((f64::from(*got) - want).abs() < 1e-5, "{got} != {want}");
    }
}

// spec MAP-3.5: on_plane multiplies each coordinate by the matching normal component; with a
// normal whose components are not 0 or +-1 a division would move the point off the plane.
#[test]
fn on_plane_multiplies_by_the_normal() {
    let points = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 2.0]];
    let wide = plane(points).unwrap();
    let narrow = qbsp_plane(points.map(|p| p.map(|v| v as f32))).unwrap();
    // [0,10,5] is on this plane (10 * 0.4472 + 5 * -0.8944 = 0); dividing instead gives ~16.8.
    let on = [[0.0, 10.0, 5.0]; 3];
    assert!(on_plane(&on, &wide));
    assert!(qbsp_on_plane(&on.map(|p| p.map(|v| v as f32)), &narrow));
    let off = [[0.0, 10.0, 0.0]; 3];
    assert!(!on_plane(&off, &wide));
}

// spec MAP-4.3, MAP-4.4: a non-right-angle rotation applies both rotation terms (cos*s - sin*t
// and sin*s + cos*t), and a non-unit scale divides each axis component.
#[test]
fn texvecs_rotation_and_scale_are_applied() {
    // Floor, rotate 45 degrees, scale [2, 4]. s0 = [1,0,0], t0 = [0,-1,0]; sv = 0, tv = 1.
    // After rotation the s axis is [cos45, -sin45, 0] and t is [sin45, cos45, 0], then each is
    // divided by its scale. cos45 = sin45 = 1/sqrt(2).
    let d = core::f64::consts::FRAC_1_SQRT_2;
    let vecs = standard_texvecs([0.0, 0.0, 1.0], [5.0, -6.0], 45.0, [2.0, 4.0]);
    let expect = [[d / 2.0, d / 2.0, 0.0, 5.0], [d / 4.0, -d / 4.0, 0.0, -6.0]];
    for (row, want) in vecs.iter().zip(expect) {
        for (got, w) in row.iter().zip(want) {
            assert!((got - w).abs() < 1e-9, "{got} != {w}");
        }
    }
    let q = qbsp_texvecs([0.0, 0.0, 1.0], [5.0, -6.0], 45.0, [2.0, 4.0]);
    let df = core::f32::consts::FRAC_1_SQRT_2;
    let expect_q = [
        [df / 2.0, df / 2.0, 0.0, 5.0],
        [df / 4.0, -df / 4.0, 0.0, -6.0],
    ];
    for (row, want) in q.iter().zip(expect_q) {
        for (got, w) in row.iter().zip(want) {
            assert!((got - w).abs() < 1e-5, "{got} != {w}");
        }
    }
}

// spec MAP-5.3: Valve 220 axes are each divided by their scale, so a non-unit scale on a
// non-zero component cannot be a multiplication.
#[test]
fn valve_divides_every_component_by_its_scale() {
    let vecs = valve_texvecs([2.0, 3.0, 4.0, 7.0], [5.0, 6.0, 8.0, 9.0], [2.0, 4.0]).unwrap();
    assert_eq!(vecs, [[1.0, 1.5, 2.0, 7.0], [1.25, 1.5, 2.0, 9.0]]);
}

// spec MAP-4.4: every axis component, including a non-zero z on a wall, is divided by its
// scale. A west wall's t axis is [0,0,-1], so its z divided by a non-unit scale is non-zero.
#[test]
fn texvecs_divides_the_z_component_on_a_wall() {
    let vecs = standard_texvecs([1.0, 0.0, 0.0], [0.0; 2], 0.0, [2.0, 4.0]);
    assert_eq!(vecs, [[0.0, 0.5, 0.0, 0.0], [0.0, 0.0, -0.25, 0.0]]);
    let q = qbsp_texvecs([1.0, 0.0, 0.0], [0.0; 2], 0.0, [2.0, 4.0]);
    assert_eq!(q, [[0.0, 0.5, 0.0, 0.0], [0.0, 0.0, -0.25, 0.0]]);
}
