// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test; the values are exact"
)]

use super::{Plane, PlaneType, Planes, Side};
use crate::CompileError;

fn plane(normal: [f32; 3], dist: f32) -> Plane {
    Plane { normal, dist }
}

// spec QBSP-2.2: a normal with a component of exactly -1 is stored negated with its distance,
// and the type is the axis
// spec QBSP-2.4: the side is 1 when the face faces away from the stored plane
#[test]
fn planes_are_stored_canonical() {
    let mut planes = Planes::new(8);
    let (id, side) = planes.find(plane([-1.0, 0.0, 0.0], -32.0)).unwrap();
    assert_eq!(side, Side::Back);
    assert_eq!(planes.get(id), plane([1.0, 0.0, 0.0], 32.0));
    assert_eq!(planes.kind(id), PlaneType::X);
    let (again, side) = planes.find(plane([1.0, 0.0, 0.0], 32.0)).unwrap();
    assert_eq!((again, side), (id, Side::Front));
}

// spec QBSP-2.2: a non-axial plane is stored with its dominant component positive, typed by it
#[test]
fn non_axial_planes_face_their_dominant_axis() {
    let mut planes = Planes::new(8);
    let (id, side) = planes.find(plane([-0.6, 0.0, -0.8], 4.0)).unwrap();
    assert_eq!(side, Side::Back);
    assert_eq!(planes.get(id).normal, [0.6, 0.0, 0.8]);
    assert_eq!(planes.get(id).dist, -4.0);
    assert_eq!(planes.kind(id), PlaneType::AnyZ);
}

// spec QBSP-2.3: planes within ANGLEEPSILON and DISTEPSILON are one; further apart, two
#[test]
fn near_planes_are_one() {
    let mut planes = Planes::new(8);
    let (a, _) = planes.find(plane([0.0, 0.0, 1.0], 16.0)).unwrap();
    let (b, _) = planes.find(plane([0.0, 0.0, 1.0], 16.009)).unwrap();
    let (c, _) = planes.find(plane([0.0, 0.0, 1.0], 16.02)).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// spec QBSP-2.5: a new plane past the table's size stops the run
#[test]
fn a_full_table_refuses_a_new_plane() {
    let mut planes = Planes::new(1);
    planes.find(plane([0.0, 0.0, 1.0], 0.0)).unwrap();
    planes.find(plane([0.0, 0.0, 1.0], 0.0)).unwrap();
    assert_eq!(
        planes.find(plane([1.0, 0.0, 0.0], 0.0)),
        Err(CompileError::TooManyPlanes)
    );
}

// spec QBSP-2.3: a normal whose length is not 1 is refused
#[test]
fn an_unnormalized_plane_is_refused() {
    let mut planes = Planes::new(8);
    assert_eq!(
        planes.find(plane([0.0, 0.0, 2.0], 0.0)),
        Err(CompileError::Normalization)
    );
}
