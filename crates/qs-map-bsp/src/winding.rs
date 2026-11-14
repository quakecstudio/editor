// SPDX-License-Identifier: GPL-3.0-or-later

//! Windings: convex polygons on a plane, cut by other planes (QBSP-3).

use crate::CompileError;
use crate::math::{Axis, ORIGIN, Vec3, add, cross, cyclic, dot, ma, normalize, scale, sub};
use crate::planes::Plane;

/// How far a point may lie from a plane and still be on it (QBSP-3.2).
pub(crate) const ON_EPSILON: f64 = 0.05;
/// Half the side of a plane's base winding (QBSP-3.1).
const BASE_HALF: f32 = 8192.0;
/// How many points beyond a winding's own `DivideWinding` allocates for each side.
const SPLIT_SLACK: usize = 4;
/// The most points a winding may have (QBSP-3.5).
const MAX_POINTS_ON_WINDING: usize = 64;
/// Where qbsp starts its search for a base winding's largest normal component, below any
/// (`max = -BOGUS_RANGE`).
const BOGUS_RANGE: f32 = 18000.0;

/// Where a point is against a plane (QBSP-3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointSide {
    Front,
    Back,
    On,
}

/// Each point's distance from `plane` and its side.
pub(crate) fn classify(points: &[Vec3], plane: Plane) -> Vec<(f32, PointSide)> {
    points
        .iter()
        .map(|&point| {
            let distance = plane.distance(point);
            let side = if f64::from(distance) > ON_EPSILON {
                PointSide::Front
            } else if f64::from(distance) < -ON_EPSILON {
                PointSide::Back
            } else {
                PointSide::On
            };
            (distance, side)
        })
        .collect()
}

impl Plane {
    /// A plane's base winding, `BaseWindingForPlane` (QBSP-3.1, QBSP-3.6).
    pub(crate) fn base_winding(self) -> Vec<Vec3> {
        let mut axis = 0;
        let mut largest = -BOGUS_RANGE;
        for (index, component) in self.normal.iter().enumerate() {
            if component.abs() > largest {
                largest = component.abs();
                axis = index;
            }
        }
        let mut up: Vec3 = ORIGIN;
        if axis == 2 {
            up[0] = 1.0;
        } else {
            up[2] = 1.0;
        }
        let along = dot(up, self.normal);
        up = normalize(ma(up, -f64::from(along), self.normal));
        let origin = scale(self.normal, self.dist);
        let right = scale(cross(up, self.normal), BASE_HALF);
        let up = scale(up, BASE_HALF);
        vec![
            add(sub(origin, right), up),
            add(add(origin, right), up),
            sub(add(origin, right), up),
            sub(sub(origin, right), up),
        ]
    }
}

/// The point where the edge from `p1` to `p2` crosses `plane`, at the fraction `t` of the way,
/// each coordinate whose normal component is exactly 1 or -1 set to the plane's distance
/// (QBSP-3.3).
#[expect(
    clippy::float_cmp,
    reason = "spec QBSP-3.3: qbsp compares a normal component with exactly 1 and -1"
)]
pub(crate) fn split_point(p1: Vec3, p2: Vec3, t: f32, plane: Plane) -> Vec3 {
    Axis::ALL.map(|j| {
        let n = j.of(plane.normal);
        if n == 1.0 {
            plane.dist
        } else if n == -1.0 {
            -plane.dist
        } else {
            j.of(p1) + t * (j.of(p2) - j.of(p1))
        }
    })
}

/// What keeping the front of a winding gives (QBSP-3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeepOn {
    /// A winding wholly on the plane is kept.
    Keep,
    /// A winding wholly on the plane is clipped away.
    Drop,
}

/// The winding's part in front of `plane`, `ClipWinding`; `None` when nothing is in front
/// (QBSP-3.3).
pub(crate) fn clip(
    points: &[Vec3],
    plane: Plane,
    keep_on: KeepOn,
) -> Result<Option<Vec<Vec3>>, CompileError> {
    if classify(points, plane)
        .iter()
        .all(|(_, side)| *side == PointSide::On)
    {
        return Ok(match keep_on {
            KeepOn::Keep => Some(points.to_vec()),
            KeepOn::Drop => None,
        });
    }
    Ok(divide(points, plane)?.0)
}

/// A face's parts in front of and behind `plane`, `SplitFace`: [`divide`], and a part of more
/// than `limit` points stops the run (QBSP-3.5).
pub(crate) fn split_face(
    points: &[Vec3],
    plane: Plane,
    limit: usize,
) -> Result<Parts, CompileError> {
    let parts = divide(points, plane)?;
    if [&parts.0, &parts.1]
        .into_iter()
        .flatten()
        .any(|part| part.len() > limit)
    {
        return Err(CompileError::SplitFacePoints);
    }
    Ok(parts)
}

/// A winding's parts in front of and behind a plane, either missing.
pub(crate) type Parts = (Option<Vec<Vec3>>, Option<Vec<Vec3>>);

/// The winding's parts in front of and behind `plane`, `DivideWinding` and `SplitFace`; a
/// winding with no point in front is all back, one wholly on the plane included (QBSP-3.4).
pub(crate) fn divide(points: &[Vec3], plane: Plane) -> Result<Parts, CompileError> {
    let sides = classify(points, plane);
    let fronts = sides.iter().filter(|(_, s)| *s == PointSide::Front).count();
    let backs = sides.iter().filter(|(_, s)| *s == PointSide::Back).count();
    if fronts == 0 {
        return Ok((None, Some(points.to_vec())));
    }
    if backs == 0 {
        return Ok((Some(points.to_vec()), None));
    }
    let mut front = Vec::with_capacity(points.len().saturating_add(SPLIT_SLACK));
    let mut back = Vec::with_capacity(points.len().saturating_add(SPLIT_SLACK));
    for (i, (&p1, &(d1, s1))) in points.iter().zip(&sides).enumerate() {
        let next = cyclic(i, 1, points.len());
        match s1 {
            PointSide::On => {
                front.push(p1);
                back.push(p1);
                continue;
            }
            PointSide::Front => front.push(p1),
            PointSide::Back => back.push(p1),
        }
        let Some(&(d2, s2)) = sides.get(next) else {
            continue;
        };
        if s2 == PointSide::On || s2 == s1 {
            continue;
        }
        let Some(&p2) = points.get(next) else {
            continue;
        };
        let mid = split_point(p1, p2, d1 / (d1 - d2), plane);
        front.push(mid);
        back.push(mid);
    }
    if front.len() > MAX_POINTS_ON_WINDING || back.len() > MAX_POINTS_ON_WINDING {
        return Err(CompileError::TooManyPoints);
    }
    Ok((Some(front), Some(back)))
}

#[cfg(test)]
mod tests;
