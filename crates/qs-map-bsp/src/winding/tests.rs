// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::float_cmp,
    reason = "test code: a failed unwrap or index is a failed test; the values are exact"
)]

use super::{KeepOn, PointSide, classify, clip, divide, split_face};
use crate::CompileError;
use crate::math::{Vec3, dot};
use crate::planes::Plane;

fn plane(normal: Vec3, dist: f32) -> Plane {
    Plane { normal, dist }
}

/// A 64-unit square on z = 0.
fn square() -> Vec<Vec3> {
    vec![
        [0.0, 0.0, 0.0],
        [0.0, 64.0, 0.0],
        [64.0, 64.0, 0.0],
        [64.0, 0.0, 0.0],
    ]
}

// spec QBSP-3.1: a base winding is a square of side 16384 on the plane, centred on
// `normal * dist`
#[test]
fn a_base_winding_is_a_large_square_on_its_plane() {
    let p = plane([0.0, 0.0, 1.0], 32.0);
    let w = p.base_winding();
    assert_eq!(w.len(), 4);
    assert!(w.iter().all(|point| point[2] == 32.0));
    let xs: Vec<f32> = w.iter().map(|point| point[0]).collect();
    assert_eq!(
        xs.iter().copied().fold(f32::MIN, f32::max) - xs.iter().copied().fold(f32::MAX, f32::min),
        16384.0
    );
    let centre = w.iter().fold([0.0; 3], |acc, point| {
        [
            acc[0] + point[0] / 4.0,
            acc[1] + point[1] / 4.0,
            acc[2] + point[2] / 4.0,
        ]
    });
    assert_eq!(centre, [0.0, 0.0, 32.0]);
}

// spec QBSP-3.6: of equal largest components the earlier axis decides the up vector, so a
// 45-degree plane between x and z has +z up
#[test]
fn equal_components_take_the_earlier_axis() {
    let h = std::f32::consts::FRAC_1_SQRT_2;
    let w = (plane([h, 0.0, h], 0.0)).base_winding();
    // With +z as up the winding spans y fully: up x normal lies along y.
    let ys: Vec<f32> = w.iter().map(|point| point[1]).collect();
    assert!(ys.iter().any(|&y| y > 8000.0) && ys.iter().any(|&y| y < -8000.0));
    for point in &w {
        assert!(dot(*point, [h, 0.0, h]).abs() < 0.01);
    }
}

// spec QBSP-3.2: a point is in front past ON_EPSILON, behind below -ON_EPSILON, else on
#[test]
fn points_are_classed_by_on_epsilon() {
    let sides: Vec<PointSide> = classify(
        &[
            [0.0, 0.0, 0.06],
            [0.0, 0.0, 0.04],
            [0.0, 0.0, -0.04],
            [0.0, 0.0, -0.06],
        ],
        plane([0.0, 0.0, 1.0], 0.0),
    )
    .into_iter()
    .map(|(_, side)| side)
    .collect();
    assert_eq!(
        sides,
        [
            PointSide::Front,
            PointSide::On,
            PointSide::On,
            PointSide::Back
        ]
    );
}

// spec QBSP-3.3: clipping keeps the front; all behind is removed, all in front unchanged, all on
// kept or dropped as asked
#[test]
fn clipping_keeps_the_front() {
    let w = square();
    assert_eq!(
        clip(&w, plane([0.0, 0.0, 1.0], 8.0), KeepOn::Keep).unwrap(),
        None
    );
    assert_eq!(
        clip(&w, plane([0.0, 0.0, 1.0], -8.0), KeepOn::Keep).unwrap(),
        Some(w.clone())
    );
    assert_eq!(
        clip(&w, plane([0.0, 0.0, 1.0], 0.0), KeepOn::Keep).unwrap(),
        Some(w.clone())
    );
    assert_eq!(
        clip(&w, plane([0.0, 0.0, 1.0], 0.0), KeepOn::Drop).unwrap(),
        None
    );
    let half = clip(&w, plane([1.0, 0.0, 0.0], 16.0), KeepOn::Keep)
        .unwrap()
        .unwrap();
    assert!(half.iter().all(|point| point[0] >= 16.0));
    assert_eq!(half.len(), 4);
}

// spec QBSP-3.4: dividing gives both parts; an all-on winding is all back
#[test]
fn dividing_gives_both_sides() {
    let w = square();
    let (front, back) = divide(&w, plane([1.0, 0.0, 0.0], 16.0)).unwrap();
    assert!(front.unwrap().iter().all(|point| point[0] >= 16.0));
    assert!(back.unwrap().iter().all(|point| point[0] <= 16.0));
    assert_eq!(
        divide(&w, plane([0.0, 0.0, 1.0], 0.0)).unwrap(),
        (None, Some(w.clone()))
    );
    assert_eq!(
        divide(&w, plane([0.0, 0.0, 1.0], -1.0)).unwrap(),
        (Some(w.clone()), None)
    );
}

// spec QBSP-3.5: a split face with more than MAXEDGES points on a side stops the run; with no
// limit it is kept
#[test]
fn a_split_face_past_its_limit_stops() {
    // An octagon cut across two edges: four corners and two new points on each side.
    let octagon: Vec<Vec3> = vec![
        [2.0, 0.0, 0.0],
        [4.0, 0.0, 0.0],
        [6.0, 2.0, 0.0],
        [6.0, 4.0, 0.0],
        [4.0, 6.0, 0.0],
        [2.0, 6.0, 0.0],
        [0.0, 4.0, 0.0],
        [0.0, 2.0, 0.0],
    ];
    let cut = plane([1.0, 0.0, 0.0], 3.0);
    assert_eq!(
        split_face(&octagon, cut, 6).unwrap(),
        divide(&octagon, cut).unwrap()
    );
    assert_eq!(
        split_face(&octagon, cut, 5),
        Err(CompileError::SplitFacePoints)
    );
}
