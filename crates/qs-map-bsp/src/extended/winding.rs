// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/qbsp/brushbsp.hh, WindingIsTiny
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/polylib.hh, area, flip, center, bounds, plane, from_plane, calc_sides, clip

//! Convex polygons in double precision, ericw-tools' `winding_t`
//! (`winding_base_t<winding_storage_heap_t<double>>`, QBSP-15.5 to QBSP-15.8).

use super::vec::{AXES, Bounds, Plane, Vec3, add, cross, dot, length, normalize, scale, sub};

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

    /// How many of its points are on each side of `plane` within `on_epsilon`, `calc_sides`'s
    /// counts (QBSP-15.8).
    pub(crate) fn counts(&self, plane: Plane, on_epsilon: f64) -> SideCounts {
        let mut counts = SideCounts::default();
        for &p in &self.0 {
            match Side::of(plane.distance_to(p), on_epsilon) {
                Side::Front => counts.front = counts.front.saturating_add(1),
                Side::Back => counts.back = counts.back.saturating_add(1),
                Side::On => counts.on = counts.on.saturating_add(1),
            }
        }
        counts
    }

    /// Where the winding lies against `plane`, by `clip`'s first tests (QBSP-15.8): wholly in
    /// front or behind, or across it.
    fn lie(&self, plane: Plane, on_epsilon: f64, keep_on: KeepOn) -> Lie {
        let counts = self.counts(plane, on_epsilon);
        if keep_on == KeepOn::Front && counts.front == 0 && counts.back == 0 {
            return Lie::Whole(Side::Front);
        }
        if counts.front == 0 {
            return Lie::Whole(Side::Back);
        }
        if counts.back == 0 {
            return Lie::Whole(Side::Front);
        }
        Lie::Across
    }

    /// The points of its parts in front of and behind `plane`, into those of `parts` given, as
    /// `clip` cuts it (QBSP-15.8): each point's side within `on_epsilon`, the first point's
    /// after the last.
    fn cut_into(&self, plane: Plane, on_epsilon: f64, parts: [Option<&mut Vec<Vec3>>; 2]) {
        let [mut front, mut back] = parts;
        let n = self.0.len();
        let Some(&first) = self.0.first() else {
            return;
        };
        // Each point's distance is computed once: an edge's far end is the next edge's near
        // one, and the first point is the last edge's far end.
        let d_first = plane.distance_to(first);
        let mut far = d_first;
        for (i, &p1) in self.0.iter().enumerate() {
            let next = i.saturating_add(1);
            let (p2, d2) = if next == n {
                (first, d_first)
            } else {
                let Some(&p2) = self.0.get(next) else {
                    continue;
                };
                (p2, plane.distance_to(p2))
            };
            let d1 = far;
            far = d2;
            let (side, next_side) = (Side::of(d1, on_epsilon), Side::of(d2, on_epsilon));
            match side {
                Side::On => {
                    front
                        .iter_mut()
                        .chain(back.iter_mut())
                        .for_each(|v| v.push(p1));
                    continue;
                }
                Side::Front => front.iter_mut().for_each(|v| v.push(p1)),
                Side::Back => back.iter_mut().for_each(|v| v.push(p1)),
            }
            if next_side == Side::On || next_side == side {
                continue;
            }
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
            front
                .iter_mut()
                .chain(back.iter_mut())
                .for_each(|v| v.push(mid));
        }
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
        match self.lie(plane, on_epsilon, keep_on) {
            Lie::Whole(Side::Back) => (None, Some(self.clone())),
            Lie::Whole(_) => (Some(self.clone()), None),
            Lie::Across => {
                let room = self.0.len().saturating_add(CLIP_RESERVE);
                let (mut front, mut back) = (Vec::with_capacity(room), Vec::with_capacity(room));
                self.cut_into(plane, on_epsilon, [Some(&mut front), Some(&mut back)]);
                (Some(Self(front)), Some(Self(back)))
            }
        }
    }

    /// The `part` of the winding `plane` cuts, as [`Winding::clip`] gives it, without
    /// making the other part or copying a winding wholly on that side.
    pub(crate) fn clipped(
        self,
        plane: Plane,
        on_epsilon: f64,
        keep_on: KeepOn,
        part: Part,
    ) -> Option<Self> {
        match self.lie(plane, on_epsilon, keep_on) {
            Lie::Whole(Side::Back) => (part == Part::Back).then_some(self),
            Lie::Whole(_) => (part == Part::Front).then_some(self),
            Lie::Across => {
                let mut points = Vec::with_capacity(self.0.len().saturating_add(CLIP_RESERVE));
                let parts = match part {
                    Part::Front => [Some(&mut points), None],
                    Part::Back => [None, Some(&mut points)],
                };
                self.cut_into(plane, on_epsilon, parts);
                Some(Self(points))
            }
        }
    }
}

/// Which part of a cut winding [`Winding::clipped`] gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    /// The part in front of the plane.
    Front,
    /// The part behind it.
    Back,
}

/// Where a winding lies against a plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lie {
    /// Wholly on this side.
    Whole(Side),
    /// Across it.
    Across,
}

impl Side {
    /// The side a point `d` from a plane is on, within `on_epsilon` of it on it.
    fn of(d: f64, on_epsilon: f64) -> Self {
        if d > on_epsilon {
            Self::Front
        } else if d < -on_epsilon {
            Self::Back
        } else {
            Self::On
        }
    }
}

#[cfg(test)]
mod tests;
