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

/// One of the three axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// The axes in order.
    pub(crate) const ALL: [Self; 3] = [Self::X, Self::Y, Self::Z];

    /// The component of `v` along the axis.
    pub(crate) const fn of(self, v: Vec3) -> f32 {
        match self {
            Self::X => v[0],
            Self::Y => v[1],
            Self::Z => v[2],
        }
    }

    /// The component of `v` along the axis, to change.
    pub(crate) const fn of_mut(self, v: &mut Vec3) -> &mut f32 {
        match self {
            Self::X => &mut v[0],
            Self::Y => &mut v[1],
            Self::Z => &mut v[2],
        }
    }

    /// The vector `value` along the axis.
    pub(crate) fn along(self, value: f32) -> Vec3 {
        let mut v = ORIGIN;
        *self.of_mut(&mut v) = value;
        v
    }

    /// The next axis, `(axis + 1) % 3`.
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::X => Self::Y,
            Self::Y => Self::Z,
            Self::Z => Self::X,
        }
    }
}

/// `f` of each pair of components.
pub(crate) fn each(a: Vec3, b: Vec3, f: impl Fn(f32, f32) -> f32) -> Vec3 {
    let ([a0, a1, a2], [b0, b1, b2]) = (a, b);
    [f(a0, b0), f(a1, b1), f(a2, b2)]
}

/// Whether `f` holds for every pair of components.
pub(crate) fn all(a: Vec3, b: Vec3, f: impl Fn(f32, f32) -> bool) -> bool {
    let ([a0, a1, a2], [b0, b1, b2]) = (a, b);
    f(a0, b0) && f(a1, b1) && f(a2, b2)
}

/// The smaller of each pair of components.
pub(crate) fn min(a: Vec3, b: Vec3) -> Vec3 {
    each(a, b, f32::min)
}

/// The larger of each pair of components.
pub(crate) fn max(a: Vec3, b: Vec3) -> Vec3 {
    each(a, b, f32::max)
}

/// The index `offset` places after `i` around a winding of `count` points, as qbsp's
/// `(i + offset) % count`; 0 for an empty winding.
pub(crate) const fn cyclic(i: usize, offset: usize, count: usize) -> usize {
    match i.wrapping_add(offset).checked_rem(count) {
        Some(index) => index,
        None => 0,
    }
}

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
    each(a, b, |a, b| (f64::from(a) + scale * f64::from(b)) as f32)
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
    all(a, b, |a, b| f64::from((a - b).abs()) <= EQUAL_EPSILON)
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
