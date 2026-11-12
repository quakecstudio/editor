// SPDX-License-Identifier: GPL-3.0-or-later

//! qbsp's vector arithmetic in single precision, `vec_t` being `float` (QBSP), with lengths
//! summed and divided in double precision as its `VectorLength` and `VectorNormalize` do.

/// A point or direction.
pub(crate) type Vec3 = [f32; 3];

/// The zero vector.
pub(crate) const ORIGIN: Vec3 = [0.0; 3];

/// How far apart two components may be and still be equal, `VectorCompare`'s `EQUAL_EPSILON`
/// (QBSP-5.3).
pub(crate) const EQUAL_EPSILON: f64 = 0.001;

pub(crate) fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn scale(a: Vec3, by: f32) -> Vec3 {
    a.map(|c| c * by)
}

/// `VectorMA`: `a + scale * b`, its scale a `double`, so each component is computed in double
/// precision and stored as a float.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec QBSP-3.1, QBSP-11.2: qbsp's VectorMA computes in double into a vec3_t"
)]
pub(crate) fn ma(a: Vec3, scale: f64, b: Vec3) -> Vec3 {
    [0, 1, 2].map(|i| (f64::from(a[i]) + scale * f64::from(b[i])) as f32)
}

/// `VectorSubtract (vec3_origin, a)`: `0.0 - c`, which turns `0.0` into `+0.0`, not `-0.0`.
pub(crate) fn negate(a: Vec3) -> Vec3 {
    a.map(|c| 0.0 - c)
}

pub(crate) fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// `VectorLength`: the squares summed in double precision.
pub(crate) fn length(a: Vec3) -> f64 {
    let mut sum = 0.0_f64;
    for c in a {
        sum += f64::from(c * c);
    }
    sum.sqrt()
}

/// `VectorNormalize`: each component divided by the double-precision length, stored as a
/// float; the zero vector stays as it is.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec QBSP-2.3: qbsp divides a float by a double and stores the float"
)]
pub(crate) fn normalize(a: Vec3) -> Vec3 {
    let length = length(a);
    if length == 0.0 {
        return a;
    }
    a.map(|c| (f64::from(c) / length) as f32)
}

/// `VectorCompare`: every component within [`EQUAL_EPSILON`].
pub(crate) fn same(a: Vec3, b: Vec3) -> bool {
    (0..3).all(|i| f64::from((a[i] - b[i]).abs()) <= EQUAL_EPSILON)
}

/// `Q_rint`: `floor (in + 0.5)`, the sum and the floor in double precision, the result stored
/// as a float.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec QBSP-4.4: qbsp's Q_rint returns the double floor as a vec_t"
)]
pub(crate) fn rint(value: f32) -> f32 {
    (f64::from(value) + 0.5).floor() as f32
}

/// `a` and `b` within `epsilon`, compared as qbsp compares a float difference with a `double`
/// constant.
pub(crate) fn within(a: f32, b: f32, epsilon: f64) -> bool {
    f64::from((a - b).abs()) < epsilon
}
