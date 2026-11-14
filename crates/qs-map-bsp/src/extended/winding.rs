// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/qbsp/brushbsp.hh, WindingIsTiny
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/polylib.hh, area, flip, center, bounds, remove_colinear, plane, from_plane, calc_sides, clip

//! Convex polygons in double precision, ericw-tools' `winding_t`
//! (`winding_base_t<winding_storage_heap_t<double>>`, QBSP-15.5 to QBSP-15.8).

use super::vec::{
    AXES, Bounds, DIST_EPSILON, Plane, Vec3, add, cross, dot, length, normalize, scale, sub,
};

/// `DEFAULT_ON_EPSILON`: how near a plane a point is on it (`mathlib.hh`).
pub(crate) const ON_EPSILON: f64 = 0.1;
/// Half of a whole: a triangle's area is half its cross product's length.
const HALF: f64 = 0.5;
/// A winding with this many edges of some length is not tiny.
const TINY_EDGES: usize = 3;
/// The room `clip` makes beyond a winding's points for each part, `reserve(size() + 4)`.
const CLIP_RESERVE: usize = 4;

/// Which side of a plane a point is on, `planeside_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Front,
    Back,
    On,
}

/// How many points are on each side of a plane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SideCounts {
    pub(crate) front: usize,
    pub(crate) back: usize,
    pub(crate) on: usize,
}

/// Whether a winding exactly on a plane is kept on the front, `clip`'s `keepon`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeepOn {
    /// Clipped away: a winding on the plane goes to the back.
    No,
    /// Kept on the front.
    Front,
}

/// A convex polygon, its points in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Winding(Vec<Vec3>);

impl Winding {
    /// The winding of `points`.
    pub(crate) const fn new(points: Vec<Vec3>) -> Self {
        Self(points)
    }

    /// Its points.
    pub(crate) fn points(&self) -> &[Vec3] {
        &self.0
    }

    /// How many points it has.
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether it has no point.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Its area, `area` (QBSP-15.5): half the length of each fan triangle's cross product from
    /// the first point, summed.
    pub(crate) fn area(&self) -> f64 {
        let Some((&first, rest)) = self.0.split_first() else {
            return 0.0;
        };
        rest.windows(2)
            .filter_map(|pair| match pair {
                [a, b] => Some(HALF * length(cross(sub(*a, first), sub(*b, first)))),
                _ => None,
            })
            .sum()
    }

    /// The winding facing the other way, its points in reverse order, `flip`.
    pub(crate) fn flipped(&self) -> Self {
        Self(self.0.iter().rev().copied().collect())
    }

    /// Whether it has fewer than three edges longer than `size`, `WindingIsTiny` (QBSP-15.25).
    pub(crate) fn is_tiny(&self, size: f64) -> bool {
        let n = self.0.len();
        let long = (0..n)
            .filter(|&i| {
                let at = |k: usize| self.0.get(k.checked_rem(n)?).copied();
                at(i)
                    .zip(at(i.saturating_add(1)))
                    .is_some_and(|(a, b)| length(sub(b, a)) > size)
            })
            .take(TINY_EDGES)
            .count();
        long < TINY_EDGES
    }

    /// The mean of its points, `center`.
    pub(crate) fn center(&self) -> Vec3 {
        let sum = self.0.iter().fold([0.0; AXES], |acc, &p| add(acc, p));
        #[expect(
            clippy::cast_precision_loss,
            reason = "a winding's point count is far below 2^52"
        )]
        let count = self.0.len() as f64;
        scale(sum, 1.0 / count)
    }

    /// The box around its points, `bounds`.
    pub(crate) fn bounds(&self) -> Bounds {
        let mut bounds = Bounds::default();
        for &p in &self.0 {
            bounds.add(p);
        }
        bounds
    }

    /// The winding without the points that lie on a line with their neighbours,
    /// `remove_colinear` (QBSP-15.6): a point is kept when the directions in and out of it have
    /// a dot product below `1 - DIST_EPSILON`.
    pub(crate) fn remove_colinear(&mut self) {
        let n = self.0.len();
        let kept: Vec<Vec3> = (0..n)
            .filter_map(|i| {
                let at = |k: usize| self.0.get(k.checked_rem(n)?).copied();
                let (p, next, prev) = (
                    at(i)?,
                    at(i.saturating_add(1))?,
                    at(i.saturating_add(n).saturating_sub(1))?,
                );
                let v1 = normalize(sub(next, p));
                let v2 = normalize(sub(p, prev));
                (dot(v1, v2) < 1.0 - DIST_EPSILON).then_some(p)
            })
            .collect();
        if kept.len() != n {
            self.0 = kept;
        }
    }

    /// The plane of its first three points, `plane`: the normal of `(p0 - p1) x (p2 - p1)`.
    pub(crate) fn plane(&self) -> Option<Plane> {
        let [p0, p1, p2] = [self.0.first()?, self.0.get(1)?, self.0.get(2)?].map(|p| *p);
        let normal = normalize(cross(sub(p0, p1), sub(p2, p1)));
        Some(Plane {
            normal,
            dist: dot(p0, normal),
        })
    }

    /// A square on `plane` reaching `extent` from its middle each way, `from_plane`
    /// (QBSP-15.7): its up vector the z axis, or the x axis for a plane nearest z, with the
    /// normal's part taken out; its right vector `up x normal`; corners `org - right + up`,
    /// `org + right + up`, `org + right - up`, `org - right - up` in that order. `None` for a
    /// normal with no largest component.
    pub(crate) fn from_plane(plane: Plane, extent: f64) -> Option<Self> {
        let mut max = f64::MIN;
        let mut axis = None;
        for (i, c) in plane.normal.iter().enumerate() {
            if c.abs() > max {
                max = c.abs();
                axis = Some(i);
            }
        }
        let mut up = match axis? {
            0 | 1 => [0.0, 0.0, 1.0],
            _ => [1.0, 0.0, 0.0],
        };
        let v = dot(up, plane.normal);
        up = normalize(add(up, scale(plane.normal, -v)));
        let org = scale(plane.normal, plane.dist);
        let right = scale(cross(up, plane.normal), extent);
        let up = scale(up, extent);
        Some(Self(vec![
            add(sub(org, right), up),
            add(add(org, right), up),
            sub(add(org, right), up),
            sub(sub(org, right), up),
        ]))
    }

    /// Each point's distance from `plane` and its side within `on_epsilon`, `calc_sides`
    /// (QBSP-15.8), the first point's repeated after the last, and how many are on each side.
    pub(crate) fn calc_sides(
        &self,
        plane: Plane,
        on_epsilon: f64,
    ) -> (Vec<f64>, Vec<Side>, SideCounts) {
        let mut dists = Vec::with_capacity(self.0.len().saturating_add(1));
        let mut sides = Vec::with_capacity(self.0.len().saturating_add(1));
        let mut counts = SideCounts::default();
        for &p in &self.0 {
            let d = plane.distance_to(p);
            let side = if d > on_epsilon {
                counts.front = counts.front.saturating_add(1);
                Side::Front
            } else if d < -on_epsilon {
                counts.back = counts.back.saturating_add(1);
                Side::Back
            } else {
                counts.on = counts.on.saturating_add(1);
                Side::On
            };
            dists.push(d);
            sides.push(side);
        }
        if let (Some(&d), Some(&s)) = (dists.first(), sides.first()) {
            dists.push(d);
            sides.push(s);
        }
        (dists, sides, counts)
    }

    /// The winding cut by `plane` into its parts in front and behind, `clip` (QBSP-15.8): with
    /// no point in front all of it is behind, with none behind all of it in front, and a winding
    /// wholly on the plane is in front only with [`KeepOn::Front`]. Points on the plane go to
    /// both parts; each edge crossing it gives both a point where it crosses, a component of an
    /// axial normal taken as the plane's distance to avoid rounding.
    pub(crate) fn clip(
        &self,
        plane: Plane,
        on_epsilon: f64,
        keep_on: KeepOn,
    ) -> (Option<Self>, Option<Self>) {
        let (dists, sides, counts) = self.calc_sides(plane, on_epsilon);
        if keep_on == KeepOn::Front && counts.front == 0 && counts.back == 0 {
            return (Some(self.clone()), None);
        }
        if counts.front == 0 {
            return (None, Some(self.clone()));
        }
        if counts.back == 0 {
            return (Some(self.clone()), None);
        }
        let n = self.0.len();
        let mut front = Vec::with_capacity(n.saturating_add(CLIP_RESERVE));
        let mut back = Vec::with_capacity(n.saturating_add(CLIP_RESERVE));
        for (i, &p1) in self.0.iter().enumerate() {
            let (Some(&side), Some(&next_side), Some(&d1), Some(&d2)) = (
                sides.get(i),
                sides.get(i.saturating_add(1)),
                dists.get(i),
                dists.get(i.saturating_add(1)),
            ) else {
                continue;
            };
            match side {
                Side::On => {
                    front.push(p1);
                    back.push(p1);
                    continue;
                }
                Side::Front => front.push(p1),
                Side::Back => back.push(p1),
            }
            if next_side == Side::On || next_side == side {
                continue;
            }
            let Some(&p2) = i
                .saturating_add(1)
                .checked_rem(n)
                .and_then(|k| self.0.get(k))
            else {
                continue;
            };
            let t = d1 / (d1 - d2);
            let mut mid = [0.0; AXES];
            for (((m, &a), &b), &c) in mid.iter_mut().zip(&p1).zip(&p2).zip(&plane.normal) {
                #[expect(
                    clippy::float_cmp,
                    reason = "spec QBSP-15.8: an axial normal's component is exactly 1 or -1"
                )]
                let (pos, neg) = (c == 1.0, c == -1.0);
                *m = if pos {
                    plane.dist
                } else if neg {
                    -plane.dist
                } else {
                    a + t * (b - a)
                };
            }
            front.push(mid);
            back.push(mid);
        }
        (Some(Self(front)), Some(Self(back)))
    }
}

#[cfg(test)]
mod tests;
