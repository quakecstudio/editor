// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::{KeepOn, ON_EPSILON, Side, Winding};
use crate::extended::vec::{Plane, normalize};

fn square(z: f64) -> Winding {
    Winding::new(vec![
        [0.0, 0.0, z],
        [4.0, 0.0, z],
        [4.0, 4.0, z],
        [0.0, 4.0, z],
    ])
}

// spec QBSP-15.5: the area is the fan's half cross products summed; the centre is the mean point
#[test]
fn area_and_centre() {
    let w = square(1.0);
    assert_eq!(w.area(), 16.0);
    assert_eq!(w.center(), [2.0, 2.0, 1.0]);
    assert_eq!(Winding::default().area(), 0.0);
    let b = w.bounds();
    assert_eq!((b.mins, b.maxs), ([0.0, 0.0, 1.0], [4.0, 4.0, 1.0]));
    assert_eq!((w.len(), w.is_empty()), (4, false));
}

// spec QBSP-15.7: a plane's winding is a square of the extent each way, from the plane's point
// nearest the origin, in the corners' order; a winding's plane is its first three points'
#[test]
fn a_plane_gives_its_square() {
    let floor = Plane {
        normal: [0.0, 0.0, 1.0],
        dist: 8.0,
    };
    let w = Winding::from_plane(floor, 100.0).unwrap();
    // Up is x for a plane nearest z; right is up x normal, (0, -1, 0).
    assert_eq!(
        w.points(),
        [
            [100.0, 100.0, 8.0],
            [100.0, -100.0, 8.0],
            [-100.0, -100.0, 8.0],
            [-100.0, 100.0, 8.0]
        ]
    );
    let own = w.plane().unwrap();
    assert_eq!((own.normal, own.dist), ([0.0, 0.0, 1.0], 8.0));
    let wall = Plane {
        normal: [1.0, 0.0, 0.0],
        dist: 0.0,
    };
    let w = Winding::from_plane(wall, 10.0).unwrap();
    assert!(w.points().iter().all(|p| p[0] == 0.0));
    assert_eq!(w.area(), 400.0);
    let tilted = Plane {
        normal: normalize([1.0, 2.0, 3.0]),
        dist: 5.0,
    };
    let w = Winding::from_plane(tilted, 50.0).unwrap();
    assert!(
        w.points()
            .iter()
            .all(|p| tilted.distance_to(*p).abs() < 1e-9)
    );
    assert!(Winding::default().plane().is_none());
}

// spec QBSP-15.8: a winding across a plane is cut into both parts, an axial crossing exactly at
// the plane's distance; one wholly on a side stays whole; one on the plane goes behind unless
// kept on; a point on the plane goes to both
#[test]
fn clipping_cuts_at_the_plane() {
    let w = square(0.0);
    let x2 = Plane {
        normal: [1.0, 0.0, 0.0],
        dist: 2.0,
    };
    let (front, back) = w.clip(x2, ON_EPSILON, KeepOn::No);
    let (front, back) = (front.unwrap(), back.unwrap());
    assert!(front.points().iter().all(|p| p[0] >= 2.0));
    assert!(back.points().iter().all(|p| p[0] <= 2.0));
    assert_eq!(front.area() + back.area(), 16.0);
    assert!(front.points().contains(&[2.0, 0.0, 0.0]));
    let counts = w.counts(x2, ON_EPSILON);
    assert_eq!((counts.front, counts.back, counts.on), (2, 2, 0));
    assert_eq!(
        w.clone().clipped(x2, ON_EPSILON, KeepOn::No, Side::Front),
        Some(front)
    );
    assert_eq!(
        w.clone().clipped(x2, ON_EPSILON, KeepOn::No, Side::Back),
        Some(back)
    );
    let far = Plane {
        normal: [1.0, 0.0, 0.0],
        dist: 10.0,
    };
    assert_eq!(w.clip(far, ON_EPSILON, KeepOn::No), (None, Some(w.clone())));
    assert_eq!(
        w.clip(far.flipped(), ON_EPSILON, KeepOn::No),
        (Some(w.clone()), None)
    );
    let own = Plane {
        normal: [0.0, 0.0, 1.0],
        dist: 0.0,
    };
    assert_eq!(w.clip(own, ON_EPSILON, KeepOn::No), (None, Some(w.clone())));
    assert_eq!(
        w.clip(own, ON_EPSILON, KeepOn::Front),
        (Some(w.clone()), None)
    );
    let n = normalize([1.0, 1.0, 0.0]);
    let diagonal = Plane {
        normal: n,
        dist: n[0] * 4.0,
    };
    let (front, back) = w.clip(diagonal, ON_EPSILON, KeepOn::No);
    assert_eq!((front.unwrap().len(), back.unwrap().len()), (3, 3));
}
