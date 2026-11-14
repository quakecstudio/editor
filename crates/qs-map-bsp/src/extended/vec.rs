// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/map.cc, add_plane, find_plane_nonfatal, add_or_find_plane, GetIntersection
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/qbsp.cc, normalize, epsilonEqual
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/qvec.hh, normalizeInPlace

//! Double-precision vectors, planes, bounds and the plane table, as ericw-tools' qbsp keeps
//! them in `qvec3d`, `qplane3d`, `aabb3d` and `mapdata_t::planes` (QBSP-15.1 to QBSP-15.4).

use std::collections::HashMap;

/// A point or direction, `qvec3d`.
pub(crate) type Vec3 = [f64; 3];
/// A vector's components.
pub(crate) const AXES: usize = 3;
/// Half, by which `find_plane_nonfatal` halves each epsilon.
const HALF: f64 = 0.5;

/// `NORMAL_EPSILON`: how far two planes' normals may differ and be one plane (`mathlib.hh`).
pub(crate) const NORMAL_EPSILON: f64 = 0.000_001;
/// `DIST_EPSILON`: how far two planes' distances may differ and be one plane (`mathlib.hh`).
pub(crate) const DIST_EPSILON: f64 = 0.0001;

/// The dot product.
pub(crate) fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `a - b`.
pub(crate) fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a + b`.
pub(crate) fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `v * s`.
pub(crate) fn scale(v: Vec3, s: f64) -> Vec3 {
    [v[0] * s, v[1] * s, v[2] * s]
}

/// The cross product, `qv::cross`.
pub(crate) fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The length, `qv::length`.
pub(crate) fn length(v: Vec3) -> f64 {
    dot(v, v).sqrt()
}

/// `v` scaled to length 1, `qv::normalize`: divided by its length, or itself when that is 0.
pub(crate) fn normalize(v: Vec3) -> Vec3 {
    normalize_with_length(v).0
}

/// `v` scaled to length 1 and its length before, `qv::normalizeInPlace`.
pub(crate) fn normalize_with_length(v: Vec3) -> (Vec3, f64) {
    let l = length(v);
    let zero = l == 0.0;
    if zero {
        return (v, l);
    }
    ([v[0] / l, v[1] / l, v[2] / l], l)
}

/// How near a plane a point is on it in the brush checks, qbsp's `-epsilon` option's default.
pub(crate) const POINT_EPSILON: f64 = 0.0001;

/// A plane: its normal and its distance from the origin along it, `qplane3d`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plane {
    pub(crate) normal: Vec3,
    pub(crate) dist: f64,
}

impl Plane {
    /// How far `p` is in front of the plane, `distance_to`.
    pub(crate) fn distance_to(&self, p: Vec3) -> f64 {
        dot(self.normal, p) - self.dist
    }

    /// The plane facing the other way, `-plane`.
    pub(crate) fn flipped(self) -> Self {
        Self {
            normal: scale(self.normal, -1.0),
            dist: -self.dist,
        }
    }
}

/// A plane's type, `plane_type_t`: axial along x, y or z, or nearest to one of them
/// (QBSP-15.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PlaneType {
    X,
    Y,
    Z,
    AnyX,
    AnyY,
    AnyZ,
}

impl PlaneType {
    /// The axis the type is along or nearest to.
    pub(crate) const fn axis(self) -> usize {
        match self {
            Self::X | Self::AnyX => 0,
            Self::Y | Self::AnyY => 1,
            Self::Z | Self::AnyZ => 2,
        }
    }

    /// Whether the plane is along an axis.
    pub(crate) const fn is_axial(self) -> bool {
        matches!(self, Self::X | Self::Y | Self::Z)
    }
}

/// Whether the normal leans toward its axis's negative side, a [`BspPlane`]'s flip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Facing {
    /// As given.
    AsGiven,
    /// Turned to face its axis's positive side.
    Positive,
}

impl Plane {
    /// The point where it meets `p2` and `p3`, `GetIntersection` (QBSP-15.19); `None` when the
    /// three normals' triple product is exactly 0.
    pub(crate) fn intersection(self, p2: Self, p3: Self) -> Option<Vec3> {
        let denom = dot(self.normal, cross(p2.normal, p3.normal));
        if denom == 0.0 {
            return None;
        }
        let sum = sub(
            sub(
                scale(cross(p2.normal, p3.normal), self.dist),
                scale(cross(p3.normal, self.normal), -p2.dist),
            ),
            scale(cross(self.normal, p2.normal), -p3.dist),
        );
        Some(sum.map(|c| c / denom))
    }
}

/// The axial types, each with its unit normal.
const AXIAL: [(PlaneType, Vec3); 3] = [
    (PlaneType::X, [1.0, 0.0, 0.0]),
    (PlaneType::Y, [0.0, 1.0, 0.0]),
    (PlaneType::Z, [0.0, 0.0, 1.0]),
];

/// A plane with its type, `qbsp_plane_t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BspPlane {
    plane: Plane,
    kind: PlaneType,
}

impl BspPlane {
    /// `plane` normalized as `qbsp_plane_t::normalize` does (QBSP-15.1): a normal with a
    /// component exactly 1 or -1 is that axis alone; any other is typed by its largest
    /// component, x before y before z on ties; with [`Facing::Positive`] a plane facing its
    /// axis's negative side is turned; whether it was turned.
    pub(crate) fn new(plane: Plane, facing: Facing) -> (Self, bool) {
        let flip = facing == Facing::Positive;
        for (component, (kind, unit)) in plane.normal.into_iter().zip(AXIAL) {
            #[expect(
                clippy::float_cmp,
                reason = "spec QBSP-15.1: `plane.normal[i] == 1.0` exactly"
            )]
            let (positive, negative) = (component == 1.0, component == -1.0);
            if positive || negative {
                let turned = negative && flip;
                let (normal, dist) = if positive || turned {
                    (unit, if turned { -plane.dist } else { plane.dist })
                } else {
                    (scale(unit, -1.0), plane.dist)
                };
                return (
                    Self {
                        plane: Plane { normal, dist },
                        kind,
                    },
                    turned,
                );
            }
        }
        let [ax, ay, az] = plane.normal.map(f64::abs);
        let (nearest, kind) = if ax >= ay && ax >= az {
            (plane.normal[0], PlaneType::AnyX)
        } else if ay >= ax && ay >= az {
            (plane.normal[1], PlaneType::AnyY)
        } else {
            (plane.normal[2], PlaneType::AnyZ)
        };
        if flip && nearest < 0.0 {
            return (
                Self {
                    plane: plane.flipped(),
                    kind,
                },
                true,
            );
        }
        (Self { plane, kind }, false)
    }

    /// The plane.
    pub(crate) const fn plane(&self) -> Plane {
        self.plane
    }

    /// Its type.
    pub(crate) const fn kind(&self) -> PlaneType {
        self.kind
    }

    /// The plane facing the other way, its type the same.
    pub(crate) fn flipped(self) -> Self {
        Self {
            plane: self.plane.flipped(),
            kind: self.kind,
        }
    }

    /// Whether `other` is the same plane within [`NORMAL_EPSILON`] and [`DIST_EPSILON`],
    /// `qv::epsilonEqual` (QBSP-15.14): two axial planes only when of one type and facing one
    /// way, their distances within epsilon; any other pair when the distances and each normal
    /// component are.
    pub(crate) fn epsilon_equal(&self, other: &Self) -> bool {
        let near = |a: f64, b: f64, epsilon: f64| (a - b).abs() <= epsilon;
        if self.kind.is_axial() && other.kind.is_axial() {
            let axis = self.kind.axis();
            let same_way = self.plane.normal.get(axis) == other.plane.normal.get(axis);
            return self.kind == other.kind
                && same_way
                && near(self.plane.dist, other.plane.dist, DIST_EPSILON);
        }
        near(self.plane.dist, other.plane.dist, DIST_EPSILON)
            && self
                .plane
                .normal
                .iter()
                .zip(other.plane.normal)
                .all(|(a, b)| near(*a, b, NORMAL_EPSILON))
    }
}

/// An axis-aligned box, `aabb3d`; empty until a point is added.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    pub(crate) mins: Vec3,
    pub(crate) maxs: Vec3,
}

impl Default for Bounds {
    /// The empty box: `aabb3d`'s default, its minimum the largest value and its maximum the
    /// smallest, so any point widens it.
    fn default() -> Self {
        Self {
            mins: [f64::MAX; AXES],
            maxs: [f64::MIN; AXES],
        }
    }
}

impl Bounds {
    /// The box widened to hold `p`, `aabb3d::operator+=`.
    pub(crate) fn add(&mut self, p: Vec3) {
        for ((lo, hi), c) in self.mins.iter_mut().zip(self.maxs.iter_mut()).zip(p) {
            *lo = lo.min(c);
            *hi = hi.max(c);
        }
    }

    /// The box widened to hold `other`, `aabb3d::unionWith_in_place`.
    pub(crate) fn union(&mut self, other: &Self) {
        for (((lo, hi), olo), ohi) in self
            .mins
            .iter_mut()
            .zip(self.maxs.iter_mut())
            .zip(other.mins)
            .zip(other.maxs)
        {
            *lo = lo.min(olo);
            *hi = hi.max(ohi);
        }
    }

    /// Its middle, `aabb3d::centroid`.
    pub(crate) fn centroid(&self) -> Vec3 {
        scale(add(self.mins, self.maxs), HALF)
    }

    /// Whether any point was added.
    pub(crate) fn is_empty(&self) -> bool {
        self.mins.iter().zip(self.maxs).any(|(lo, hi)| *lo > hi)
    }
}

/// A plane's number in the [`Planes`] table; its opposite is the number with the lowest bit
/// flipped (`planenum ^ 1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PlaneNum(usize);

impl PlaneNum {
    /// The plane at `index` of the table.
    pub(crate) const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its place in the table.
    pub(crate) const fn get(self) -> usize {
        self.0
    }

    /// The plane of the pair facing its axis's positive side, `planenum & ~1`.
    pub(crate) const fn positive(self) -> Self {
        Self(self.0 & !1)
    }

    /// The opposite plane, `planenum ^ 1`.
    pub(crate) const fn opposite(self) -> Self {
        Self(self.0 ^ 1)
    }
}

/// The plane table, `mapdata_t::planes`: each plane with its opposite next to it, the one facing
/// its axis's positive side first, and the planes found again within [`NORMAL_EPSILON`] and
/// [`DIST_EPSILON`] (QBSP-15.2, QBSP-15.3).
#[derive(Debug, Default)]
pub(crate) struct Planes {
    planes: Vec<BspPlane>,
    /// The table's planes by their distance in [`DIST_EPSILON`] cells, standing in for the
    /// spatial hash `find_intersection` searches.
    cells: HashMap<i64, Vec<usize>>,
}

impl Planes {
    /// The plane `num`.
    pub(crate) fn get(&self, num: PlaneNum) -> Option<&BspPlane> {
        self.planes.get(num.get())
    }

    /// How many planes the table holds.
    pub(crate) fn len(&self) -> usize {
        self.planes.len()
    }

    /// Whether the table is empty.
    pub(crate) fn is_empty(&self) -> bool {
        self.planes.is_empty()
    }

    /// The cell of a distance.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a distance within the world's extent, in DIST_EPSILON cells, fits an i64"
    )]
    fn cell(dist: f64) -> i64 {
        (dist / DIST_EPSILON).floor() as i64
    }

    /// The plane of the table within half [`NORMAL_EPSILON`] of `plane`'s normal in each
    /// component and half [`DIST_EPSILON`] of its distance, `find_plane_nonfatal`
    /// (QBSP-15.3); the first added when several are.
    pub(crate) fn find(&self, plane: Plane) -> Option<PlaneNum> {
        let (half_normal, half_dist) = (NORMAL_EPSILON * HALF, DIST_EPSILON * HALF);
        let mut best: Option<usize> = None;
        let near = Self::cell(plane.dist);
        for cell in [near.saturating_sub(1), near, near.saturating_add(1)] {
            for &index in self.cells.get(&cell).into_iter().flatten() {
                let Some(candidate) = self.planes.get(index) else {
                    continue;
                };
                let c = candidate.plane;
                let inside = c
                    .normal
                    .iter()
                    .zip(plane.normal)
                    .all(|(a, b)| (a - b).abs() <= half_normal)
                    && (c.dist - plane.dist).abs() <= half_dist;
                if inside && best.is_none_or(|b| index < b) {
                    best = Some(index);
                }
            }
        }
        best.map(PlaneNum::new)
    }

    /// `plane`'s number, added with its opposite when the table has no plane within epsilon of
    /// it, `add_or_find_plane` (QBSP-15.2, QBSP-15.3).
    pub(crate) fn add_or_find(&mut self, plane: Plane) -> PlaneNum {
        if let Some(found) = self.find(plane) {
            return found;
        }
        let (positive, _) = BspPlane::new(plane, Facing::AsGiven);
        let negative = BspPlane::new(plane.flipped(), Facing::AsGiven).0;
        let first = self.planes.len();
        // The pair's first plane faces its axis's positive side; `plane` is whichever it is.
        let second = first.saturating_add(1);
        let leans_negative = positive
            .plane
            .normal
            .get(positive.kind.axis())
            .is_some_and(|c| *c < 0.0);
        let (front, back, given) = if leans_negative {
            (negative, positive, second)
        } else {
            (positive, negative, first)
        };
        for (index, bsp) in [(first, front), (second, back)] {
            self.planes.push(bsp);
            self.cells
                .entry(Self::cell(bsp.plane.dist))
                .or_default()
                .push(index);
        }
        PlaneNum::new(given)
    }
}

#[cfg(test)]
mod tests;
