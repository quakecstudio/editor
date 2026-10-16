// SPDX-License-Identifier: GPL-3.0-or-later

//! Face planes and texture axes (MAP-3.4, MAP-4.2 to MAP-4.4, MAP-5.3), written from
//! `docs/spec/map.md`.
//!
//! Each computation comes twice: in single precision from integer points, as id's qbsp does
//! it (its `vec_t` is `float`), for [`crate::qbsp`]; and in double precision, as
//! ericw-tools reads `.map` numbers (MAP-3.3, MAP-4.1), for [`crate::MapFile`].

#[cfg(test)]
mod tests;

use core::f64::consts::PI;

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
    reason = "spec MAP-3.4: qbsp stores double results in float, rounding to nearest"
)]
const fn to_f32(value: f64) -> f32 {
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
    let dist = p1[0] * normal[0] + p1[1] * normal[1] + p1[2] * normal[2];
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
    let length = (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if !length.is_normal() {
        return None;
    }
    let normal = cross.map(|value| value / length);
    let dist = p1[0] * normal[0] + p1[1] * normal[1] + p1[2] * normal[2];
    Some(Plane { normal, dist })
}

/// Whether every point lies within [`MAP_ON_EPSILON`] of `plane`, on either side (MAP-3.5).
#[must_use]
pub fn on_plane(points: &[[f64; 3]; 3], plane: &Plane<f64>) -> bool {
    points.iter().all(|point| {
        let d =
            point[0] * plane.normal[0] + point[1] * plane.normal[1] + point[2] * plane.normal[2]
                - plane.dist;
        (-MAP_ON_EPSILON..=MAP_ON_EPSILON).contains(&d)
    })
}

/// [`on_plane`] as qbsp computes it: `float` points and distance, compared with the
/// `double` epsilon (MAP-3.5).
#[must_use]
pub fn qbsp_on_plane(points: &[[f32; 3]; 3], plane: &Plane<f32>) -> bool {
    points.iter().all(|point| {
        let d =
            point[0] * plane.normal[0] + point[1] * plane.normal[1] + point[2] * plane.normal[2]
                - plane.dist;
        (-MAP_ON_EPSILON..=MAP_ON_EPSILON).contains(&f64::from(d))
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

/// The texture axes of a standard face as qbsp computes them, in single precision
/// (MAP-4.2 to MAP-4.4).
#[must_use]
pub fn qbsp_texvecs(
    normal: [f32; 3],
    shift: [f32; 2],
    rotate: f32,
    scale: [f32; 2],
) -> TexVecs<f32> {
    let row = best_row((0..6).map(|row| {
        let axis: [f32; 3] = base_axis(row, 0);
        normal[0] * axis[0] + normal[1] * axis[1] + normal[2] * axis[2]
    }));
    let mut vecs: [[f32; 3]; 2] = [base_axis(row, 1), base_axis(row, 2)];
    let axes = MAP_BASE_AXES.get(row).copied().unwrap_or_default();
    let sv = first_nonzero(axes[1]);
    let tv = first_nonzero(axes[2]);
    // qbsp: `ang = rotate / 180 * Q_PI` is a float divided, then multiplied by a double, and
    // stored in a float, whose sine and cosine are taken in double and stored in floats.
    let (sinv, cosv) = sin_cos(f64::from(rotate), || {
        f64::from(to_f32(f64::from(rotate / 180.0) * PI))
    });
    let (sinv, cosv) = (to_f32(sinv), to_f32(cosv));
    for axis in &mut vecs {
        let (s, t) = (
            axis.get(sv).copied().unwrap_or_default(),
            axis.get(tv).copied().unwrap_or_default(),
        );
        if let Some(value) = axis.get_mut(sv) {
            *value = cosv * s - sinv * t;
        }
        if let Some(value) = axis.get_mut(tv) {
            *value = sinv * s + cosv * t;
        }
    }
    let scale = scale.map(|value| if value == 0.0 { 1.0 } else { value });
    let mut out = [[0.0; 4]; 2];
    for ((row, axis), (scale, shift)) in out.iter_mut().zip(vecs).zip(scale.into_iter().zip(shift))
    {
        *row = [axis[0] / scale, axis[1] / scale, axis[2] / scale, shift];
    }
    out
}

/// The texture axes of a standard face in double precision (MAP-4.2 to MAP-4.4).
#[must_use]
pub fn standard_texvecs(
    normal: [f64; 3],
    shift: [f64; 2],
    rotate: f64,
    scale: [f64; 2],
) -> TexVecs<f64> {
    let row = best_row((0..6).map(|row| {
        let axis: [f64; 3] = base_axis(row, 0);
        normal[0] * axis[0] + normal[1] * axis[1] + normal[2] * axis[2]
    }));
    let mut vecs: [[f64; 3]; 2] = [base_axis(row, 1), base_axis(row, 2)];
    let axes = MAP_BASE_AXES.get(row).copied().unwrap_or_default();
    let sv = first_nonzero(axes[1]);
    let tv = first_nonzero(axes[2]);
    let (sinv, cosv) = sin_cos(rotate, || rotate / 180.0 * PI);
    for axis in &mut vecs {
        let (s, t) = (
            axis.get(sv).copied().unwrap_or_default(),
            axis.get(tv).copied().unwrap_or_default(),
        );
        if let Some(value) = axis.get_mut(sv) {
            *value = cosv * s - sinv * t;
        }
        if let Some(value) = axis.get_mut(tv) {
            *value = sinv * s + cosv * t;
        }
    }
    let scale = scale.map(|value| if value == 0.0 { 1.0 } else { value });
    let mut out = [[0.0; 4]; 2];
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
