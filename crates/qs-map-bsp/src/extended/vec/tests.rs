// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::{
    Bounds, BspPlane, DIST_EPSILON, Facing, NORMAL_EPSILON, Plane, PlaneNum, PlaneType, Planes,
    normalize,
};

fn plane(normal: [f64; 3], dist: f64) -> Plane {
    Plane { normal, dist }
}

// spec QBSP-15.1: an axial normal is its axis alone, typed X, Y or Z; any other is typed by its
// largest component, x before y before z on ties; asked to, a plane is turned to face its axis's
// positive side
#[test]
fn planes_are_normalized_and_typed() {
    let (p, turned) = BspPlane::new(plane([1e-9, -1.0, 1e-9], 8.0), Facing::AsGiven);
    assert_eq!(
        (p.plane(), p.kind(), turned),
        (plane([0.0, -1.0, 0.0], 8.0), PlaneType::Y, false)
    );
    let (p, turned) = BspPlane::new(plane([0.0, -1.0, 0.0], 8.0), Facing::Positive);
    assert_eq!((p.plane(), turned), (plane([0.0, 1.0, 0.0], -8.0), true));
    let (p, _) = BspPlane::new(plane([0.0, 0.0, 1.0], 2.0), Facing::Positive);
    assert_eq!(p.kind(), PlaneType::Z);
    let (p, turned) = BspPlane::new(plane(normalize([0.6, 0.6, 0.2]), 1.0), Facing::AsGiven);
    assert_eq!(
        (p.kind(), turned),
        (PlaneType::AnyX, false),
        "a tie goes to x"
    );
    let (p, turned) = BspPlane::new(plane(normalize([0.1, -0.9, 0.3]), 1.0), Facing::Positive);
    assert_eq!((p.kind(), turned), (PlaneType::AnyY, true));
    assert!(p.plane().normal[1] > 0.0);
    let (p, _) = BspPlane::new(plane(normalize([0.1, 0.2, -0.9]), 1.0), Facing::AsGiven);
    assert_eq!(p.kind(), PlaneType::AnyZ);
    assert!(PlaneType::X.is_axial() && !PlaneType::AnyZ.is_axial());
    assert_eq!(PlaneType::AnyY.axis(), 1);
}

// spec QBSP-15.2: each plane is added with its opposite, the one not negative along its axis
// first, and its own number is given back
// spec QBSP-15.3: a plane within half the epsilons of one in the table is that plane
#[test]
fn the_table_holds_each_plane_with_its_opposite() {
    let mut planes = Planes::default();
    assert!(planes.get(PlaneNum::new(0)).is_none());
    let down = planes.add_or_find(plane([0.0, 0.0, -1.0], 16.0));
    assert_eq!(down, PlaneNum::new(1), "the positive plane goes first");
    assert!(planes.get(PlaneNum::new(1)).is_some() && planes.get(PlaneNum::new(2)).is_none());
    let up = down.opposite();
    assert_eq!(up, PlaneNum::new(0));
    assert_eq!(
        planes.get(up).unwrap().plane(),
        plane([0.0, 0.0, 1.0], -16.0)
    );
    assert_eq!(planes.add_or_find(plane([0.0, 0.0, 1.0], -16.0)), up);
    assert_eq!(
        planes.add_or_find(plane([0.0, 0.0, -1.0], 16.0 + 0.4 * DIST_EPSILON)),
        down
    );
    let further = planes.add_or_find(plane([0.0, 0.0, -1.0], 16.0 + 0.6 * DIST_EPSILON));
    assert_eq!(further, PlaneNum::new(3));
    let tilted = normalize([1.0, 1.0, 0.0]);
    let a = planes.add_or_find(plane(tilted, 3.0));
    let nudged = [tilted[0] + 0.4 * NORMAL_EPSILON, tilted[1], tilted[2]];
    assert_eq!(planes.add_or_find(plane(nudged, 3.0)), a);
    let off = [tilted[0] + 0.6 * NORMAL_EPSILON, tilted[1], tilted[2]];
    assert_ne!(planes.add_or_find(plane(off, 3.0)), a);
    assert!(planes.get(PlaneNum::new(99)).is_none());
}

// spec QBSP-15.4: a box starts empty and widens to each point
#[test]
fn bounds_widen_to_each_point() {
    let mut b = Bounds::default();
    assert!(b.mins[0] > b.maxs[0], "empty");
    b.add([1.0, -2.0, 3.0]);
    assert_eq!((b.mins, b.maxs), ([1.0, -2.0, 3.0], [1.0, -2.0, 3.0]));
    b.add([-1.0, 5.0, 0.0]);
    assert_eq!((b.mins, b.maxs), ([-1.0, -2.0, 0.0], [1.0, 5.0, 3.0]));
}
