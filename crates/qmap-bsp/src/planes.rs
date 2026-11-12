// SPDX-License-Identifier: GPL-3.0-or-later

//! The table of planes every face names (QBSP-2): each plane stored once, canonical, and a
//! face's side of it.

use crate::CompileError;
use crate::math::{Axis, Vec3, dot, length, negate};

/// How near 1 two canonical normals' dot product must be for one plane (QBSP-2.3).
pub(crate) const ANGLE_EPSILON: f64 = 0.000_01;
/// How near two distances must be for one plane (QBSP-2.3).
const DIST_EPSILON: f64 = 0.01;

/// A plane's type (QBSP-2.2, BSP-3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaneType {
    X,
    Y,
    Z,
    AnyX,
    AnyY,
    AnyZ,
}

impl PlaneType {
    /// The type's number in a BSP.
    pub(crate) const fn number(self) -> i32 {
        match self {
            Self::X => 0,
            Self::Y => 1,
            Self::Z => 2,
            Self::AnyX => 3,
            Self::AnyY => 4,
            Self::AnyZ => 5,
        }
    }

    /// The axis the type is along, or nearest.
    pub(crate) const fn axis(self) -> Axis {
        match self {
            Self::X | Self::AnyX => Axis::X,
            Self::Y | Self::AnyY => Axis::Y,
            Self::Z | Self::AnyZ => Axis::Z,
        }
    }

    /// Whether the normal is the axis itself.
    pub(crate) const fn is_axial(self) -> bool {
        matches!(self, Self::X | Self::Y | Self::Z)
    }

    /// The type of a canonical normal (QBSP-2.2, `PlaneTypeForNormal`).
    #[expect(
        clippy::float_cmp,
        reason = "spec QBSP-2.2: qbsp compares a component with exactly 1"
    )]
    pub(crate) fn of(normal: Vec3) -> Self {
        if normal[0] == 1.0 {
            return Self::X;
        }
        if normal[1] == 1.0 {
            return Self::Y;
        }
        if normal[2] == 1.0 {
            return Self::Z;
        }
        let [ax, ay, az] = normal.map(f32::abs);
        if ax >= ay && ax >= az {
            Self::AnyX
        } else if ay >= ax && ay >= az {
            Self::AnyY
        } else {
            Self::AnyZ
        }
    }
}

/// A plane: the points `p` with `dot(normal, p) == dist`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plane {
    pub(crate) normal: Vec3,
    pub(crate) dist: f32,
}

impl Plane {
    /// The same plane facing the other way: the normal subtracted from the origin, as qbsp's
    /// `VectorSubtract (vec3_origin, ...)` does, so a zero component stays `+0.0`; the distance
    /// negated.
    pub(crate) fn flipped(self) -> Self {
        Self {
            normal: negate(self.normal),
            dist: -self.dist,
        }
    }

    /// How far `point` is in front of it.
    pub(crate) fn distance(self, point: Vec3) -> f32 {
        dot(point, self.normal) - self.dist
    }

    /// The plane made canonical, `NormalizePlane` (QBSP-2.2).
    #[expect(
        clippy::float_cmp,
        reason = "spec QBSP-2.2: qbsp compares a component with exactly -1 and 1"
    )]
    fn canonical(self) -> (Self, PlaneType) {
        let mut plane = self;
        for axis in Axis::ALL {
            let n = axis.of_mut(&mut plane.normal);
            if *n == -1.0 {
                *n = 1.0;
                plane.dist = -plane.dist;
            }
        }
        let kind = PlaneType::of(plane.normal);
        if !kind.is_axial() && kind.axis().of(plane.normal) < 0.0 {
            plane = plane.flipped();
        }
        (plane, kind)
    }
}

/// Which way a face faces its stored plane (QBSP-2.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    /// The plane's way.
    Front,
    /// The other way.
    Back,
}

impl Side {
    /// The side's number in a BSP face.
    pub(crate) const fn number(self) -> i32 {
        match self {
            Self::Front => 0,
            Self::Back => 1,
        }
    }

    /// The other side.
    pub(crate) const fn other(self) -> Self {
        match self {
            Self::Front => Self::Back,
            Self::Back => Self::Front,
        }
    }
}

/// A plane of the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct PlaneId(usize);

/// The table of planes (QBSP-2).
#[derive(Clone, Debug)]
pub(crate) struct Planes {
    list: Vec<(Plane, PlaneType)>,
    limit: usize,
}

impl Planes {
    /// An empty table holding at most `limit` planes (QBSP-2.5, QBSP-14.2).
    pub(crate) const fn new(limit: usize) -> Self {
        Self {
            list: Vec::new(),
            limit,
        }
    }

    /// The stored plane.
    pub(crate) fn get(&self, id: PlaneId) -> Plane {
        self.list.get(id.0).map_or(
            Plane {
                normal: [0.0; 3],
                dist: 0.0,
            },
            |(plane, _)| *plane,
        )
    }

    /// The stored plane's type.
    pub(crate) fn kind(&self, id: PlaneId) -> PlaneType {
        self.list
            .get(id.0)
            .map_or(PlaneType::AnyZ, |(_, kind)| *kind)
    }

    /// The face's plane as it faces: the stored plane, flipped for the back side.
    pub(crate) fn facing(&self, id: PlaneId, side: Side) -> Plane {
        match side {
            Side::Front => self.get(id),
            Side::Back => self.get(id).flipped(),
        }
    }

    /// The table's number for `plane` and the side `plane` faces, storing it when new,
    /// `FindPlane`.
    ///
    /// Implements QBSP-2.2 to QBSP-2.5.
    pub(crate) fn find(&mut self, plane: Plane) -> Result<(PlaneId, Side), CompileError> {
        // `dot = VectorLength (...)` keeps the length in a float.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec QBSP-2.3: qbsp stores the double length in a vec_t"
        )]
        let length = f64::from(length(plane.normal) as f32);
        if length < 1.0 - ANGLE_EPSILON || length > 1.0 + ANGLE_EPSILON {
            return Err(CompileError::Normalization);
        }
        let (canonical, kind) = plane.canonical();
        let side = if dot(canonical.normal, plane.normal) > 0.0 {
            Side::Front
        } else {
            Side::Back
        };
        for (index, (stored, _)) in self.list.iter().enumerate() {
            if f64::from(dot(stored.normal, canonical.normal)) > 1.0 - ANGLE_EPSILON
                && f64::from((stored.dist - canonical.dist).abs()) < DIST_EPSILON
            {
                return Ok((PlaneId(index), side));
            }
        }
        if self.list.len() >= self.limit {
            return Err(CompileError::TooManyPlanes);
        }
        self.list.push((canonical, kind));
        Ok((PlaneId(self.list.len().saturating_sub(1)), side))
    }
}

#[cfg(test)]
mod tests;
