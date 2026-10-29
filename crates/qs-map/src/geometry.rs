// SPDX-License-Identifier: GPL-3.0-or-later

//! Face planes and texture axes (MAP-3.4, MAP-4.2 to MAP-4.4, MAP-5.3), written from
//! `docs/spec/map.md`.
//!
//! Each computation runs in single precision from integer points, as id's qbsp does it (its
//! `vec_t` is `float`), for [`crate::qbsp`], and in double precision, as ericw-tools reads
//! `.map` numbers (MAP-3.3, MAP-4.1), for [`crate::MapFile`]: one function generic over
//! [`MapFloat`], except the plane, whose test for a degenerate normal differs (MAP-3.6).

#[cfg(test)]
mod tests;

use core::f64::consts::PI;
use core::ops::{Add, Div, Mul, Sub};

/// The float type a computation runs in: qbsp's `float` or ericw-tools' `double`.
pub trait MapFloat:
    Copy
    + PartialOrd
    + From<i8>
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + sealed::Sealed
{
    /// The value as a `double`, as C converts it.
    fn to_f64(self) -> f64;

    /// The sine and cosine of a rotation of `rotate` degrees, computed as this precision's
    /// reader computes them (MAP-4.3).
    fn rotation(rotate: Self) -> (Self, Self);
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for f32 {}
    impl Sealed for f64 {}
}

impl MapFloat for f32 {
    fn to_f64(self) -> f64 {
        f64::from(self)
    }

    /// qbsp: `ang = rotate / 180 * Q_PI` is a float divided, then multiplied by a double, and
    /// stored in a float, whose sine and cosine are taken in double and stored in floats.
    fn rotation(rotate: Self) -> (Self, Self) {
        let (sinv, cosv) = sin_cos(f64::from(rotate), || {
            f64::from(to_f32(f64::from(rotate / 180.0) * PI))
        });
        (to_f32(sinv), to_f32(cosv))
    }
}

impl MapFloat for f64 {
    fn to_f64(self) -> f64 {
        self
    }

    fn rotation(rotate: Self) -> (Self, Self) {
        sin_cos(rotate, || rotate / 180.0 * PI)
    }
}

/// `dot(a, b)`, in the order qbsp's `DotProduct` adds.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "MapFloat is sealed to f32 and f64, whose arithmetic never overflows or panics"
)]
fn dot<T: MapFloat>(a: [T; 3], b: [T; 3]) -> T {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// How far a point may lie from a plane and still be on it, qbsp's `ON_EPSILON` (MAP-3.5).
pub const MAP_ON_EPSILON: f64 = 0.05;

/// The largest component a cross product may have and still be no normal: qbsp compares
/// it with the zero vector through `VectorCompare`, whose `EQUAL_EPSILON` is 0.001
/// (MAP-3.6).
const EQUAL_EPSILON: f64 = 0.001;

/// The six base axes and their s and t axes, qbsp's `baseaxis` (MAP-4.2): floor, ceiling,
/// west, east, south and north walls.
pub const MAP_BASE_AXES: [[[i8; 3]; 3]; 6] = [
    [[0, 0, 1], [1, 0, 0], [0, -1, 0]],
    [[0, 0, -1], [1, 0, 0], [0, -1, 0]],
    [[1, 0, 0], [0, 1, 0], [0, 0, -1]],
    [[-1, 0, 0], [0, 1, 0], [0, 0, -1]],
    [[0, 1, 0], [1, 0, 0], [0, 0, -1]],
    [[0, -1, 0], [1, 0, 0], [0, 0, -1]],
];

/// A face's plane: its unit normal, pointing out of the brush, and its distance from the
/// origin (MAP-3.4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane<T> {
    /// The unit normal.
    pub normal: [T; 3],
    /// `dot(p1, normal)`.
    pub dist: T,
}

/// A texinfo's two axes and offsets, `vecs` of BSP-8.1: s axis x, y, z and offset, then the
/// same for t.
pub type TexVecs<T> = [[T; 4]; 2];

fn base_axis<T: From<i8>>(row: usize, column: usize) -> [T; 3] {
    let axis = MAP_BASE_AXES
        .get(row)
        .and_then(|axes| axes.get(column))
        .copied()
        .unwrap_or_default();
    axis.map(T::from)
}

/// The index of the first non-zero component of an axis, qbsp's `sv` and `tv` (MAP-4.3).
fn first_nonzero(axis: [i8; 3]) -> usize {
    axis.iter().position(|&value| value != 0).unwrap_or(2)
}

/// The base axes row for `normal`: the first whose axis has the largest positive dot
/// product, the floor when none is positive (MAP-4.2).
fn best_row<T: Copy + PartialOrd + From<i8>>(dots: impl Iterator<Item = T>) -> usize {
    let mut best = T::from(0);
    let mut row = 0;
    for (index, dot) in dots.enumerate() {
        if dot > best {
            best = dot;
            row = index;
        }
    }
    row
}

/// A `float` holding a `double`, as C converts on assignment.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec MAP-2.7, MAP-3.4, MAP-4.1: qbsp stores atof, sscanf and double results in \
              floats, rounding to nearest"
)]
pub(crate) const fn to_f32(value: f64) -> f32 {
    value as f32
}

/// The plane of three points as qbsp computes it, in single precision, or `None` when every
/// component of the cross product is within 0.001 of 0 (MAP-3.4, MAP-3.6).
#[must_use]
pub fn qbsp_plane(points: [[f32; 3]; 3]) -> Option<Plane<f32>> {
    let [p0, p1, p2] = points;
    let t1 = [p0[0] - p1[0], p0[1] - p1[1], p0[2] - p1[2]];
    let t2 = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
    let cross = [
        t1[1] * t2[2] - t1[2] * t2[1],
        t1[2] * t2[0] - t1[0] * t2[2],
        t1[0] * t2[1] - t1[1] * t2[0],
    ];
    if cross
        .iter()
        .all(|&value| f64::from(value).abs() <= EQUAL_EPSILON)
    {
        return None;
    }
    // qbsp's VectorNormalize sums float squares into a double.
    let mut length = 0.0_f64;
    for value in cross {
        length += f64::from(value * value);
    }
    let length = length.sqrt();
    let normal = cross.map(|value| to_f32(f64::from(value) / length));
    let dist = dot(p1, normal);
    Some(Plane { normal, dist })
}

/// The plane of three points in double precision, or `None` when they are collinear
/// (MAP-3.4, MAP-3.6).
#[must_use]
pub fn plane(points: [[f64; 3]; 3]) -> Option<Plane<f64>> {
    let [p0, p1, p2] = points;
    let t1 = [p0[0] - p1[0], p0[1] - p1[1], p0[2] - p1[2]];
    let t2 = [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]];
    let cross = [
        t1[1] * t2[2] - t1[2] * t2[1],
        t1[2] * t2[0] - t1[0] * t2[2],
        t1[0] * t2[1] - t1[1] * t2[0],
    ];
    let length = dot(cross, cross).sqrt();
    if !length.is_normal() {
        return None;
    }
    let normal = cross.map(|value| value / length);
    let dist = dot(p1, normal);
    Some(Plane { normal, dist })
}

/// Whether every point lies within [`MAP_ON_EPSILON`] of `plane`, on either side, the
/// distance compared as a `double`, as qbsp compares its `float` one (MAP-3.5).
#[must_use]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "MapFloat is sealed to f32 and f64, whose arithmetic never overflows or panics"
)]
pub fn on_plane<T: MapFloat>(points: &[[T; 3]; 3], plane: &Plane<T>) -> bool {
    points.iter().all(|&point| {
        let d = dot(point, plane.normal) - plane.dist;
        (-MAP_ON_EPSILON..=MAP_ON_EPSILON).contains(&d.to_f64())
    })
}

/// The sine and cosine of a rotation in degrees, exact for 0, 90, 180 and 270 (MAP-4.3).
#[expect(
    clippy::float_cmp,
    reason = "spec MAP-4.3: qbsp tests the rotation for exactly 0, 90, 180 and 270"
)]
fn sin_cos(rotate: f64, angle: impl FnOnce() -> f64) -> (f64, f64) {
    if rotate == 0.0 {
        (0.0, 1.0)
    } else if rotate == 90.0 {
        (1.0, 0.0)
    } else if rotate == 180.0 {
        (0.0, -1.0)
    } else if rotate == 270.0 {
        (-1.0, 0.0)
    } else {
        let angle = angle();
        (angle.sin(), angle.cos())
    }
}

/// The texture axes of a standard face (MAP-4.2 to MAP-4.4), in the precision of `T`.
#[must_use]
#[expect(
    clippy::arithmetic_side_effects,
    reason = "MapFloat is sealed to f32 and f64, whose arithmetic never overflows or panics"
)]
pub fn texvecs<T: MapFloat>(normal: [T; 3], shift: [T; 2], rotate: T, scale: [T; 2]) -> TexVecs<T> {
    let row = best_row((0..6).map(|row| dot(normal, base_axis(row, 0))));
    let mut vecs: [[T; 3]; 2] = [base_axis(row, 1), base_axis(row, 2)];
    let axes = MAP_BASE_AXES.get(row).copied().unwrap_or_default();
    let sv = first_nonzero(axes[1]);
    let tv = first_nonzero(axes[2]);
    let (sinv, cosv) = T::rotation(rotate);
    let zero = T::from(0);
    for axis in &mut vecs {
        let (s, t) = (
            axis.get(sv).copied().unwrap_or(zero),
            axis.get(tv).copied().unwrap_or(zero),
        );
        if let Some(value) = axis.get_mut(sv) {
            *value = cosv * s - sinv * t;
        }
        if let Some(value) = axis.get_mut(tv) {
            *value = sinv * s + cosv * t;
        }
    }
    // A scale of exactly 0 is 1.
    let scale = scale.map(|value| if value == zero { T::from(1) } else { value });
    let mut out = [[zero; 4]; 2];
    for ((row, axis), (scale, shift)) in out.iter_mut().zip(vecs).zip(scale.into_iter().zip(shift))
    {
        *row = [axis[0] / scale, axis[1] / scale, axis[2] / scale, shift];
    }
    out
}

/// The texture axes of a Valve 220 face: each given axis divided by its scale, with its
/// shift as the offset; the rotation is not applied (MAP-5.3). `None` when a scale is 0
/// (MAP-5.4).
#[must_use]
pub fn valve_texvecs(s: [f64; 4], t: [f64; 4], scale: [f64; 2]) -> Option<TexVecs<f64>> {
    if scale.contains(&0.0) {
        return None;
    }
    let [sx, sy] = scale;
    Some([
        [s[0] / sx, s[1] / sx, s[2] / sx, s[3]],
        [t[0] / sy, t[1] / sy, t[2] / sy, t[3]],
    ])
}
