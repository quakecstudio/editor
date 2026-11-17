// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: vis/flow.cc, ClipToSeparators

//! Windings and planes in single precision, as vis keeps them (`vec_t` is `float`): a portal's
//! plane from its points (VIS-2.3), a winding clipped to a plane's front, and the separating
//! planes that clip one portal by two others (VIS-4.3, VIS-4.4).

use std::borrow::Cow;

/// `ON_EPSILON`, a `double`: each `float` distance is widened to be compared with it (VIS-2.7).
pub(crate) const VIS_ON_EPSILON: f64 = 0.1;
/// The origin, a normal of no length.
const ORIGIN: Vec3 = [0.0, 0.0, 0.0];
/// Room for the points a clip adds, as vis allows (`maxpts = numpoints + 4`).
const SPLIT_SPARE: usize = 4;
/// `EQUAL_EPSILON`, by which `VectorCompare` takes two normals as one, a `double` (VIS-4.3,
/// VIS-2.7).
const EQUAL_EPSILON: f64 = 0.001;

/// A point or direction.
pub(crate) type Vec3 = [f32; 3];

fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A plane: its unit normal and its distance from the origin along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plane {
    pub(crate) normal: Vec3,
    pub(crate) dist: f32,
}

impl Plane {
    /// The plane facing the other way.
    pub(crate) fn flipped(self) -> Self {
        Self {
            normal: self.normal.map(|c| -c),
            dist: -self.dist,
        }
    }

    /// How far `point` is in front of the plane, as a `float`, widened to be compared with
    /// `ON_EPSILON` (VIS-2.7).
    fn side(self, point: Vec3) -> f64 {
        f64::from(dot(point, self.normal) - self.dist)
    }

    /// Whether the two normals are one, each component within `EQUAL_EPSILON` (VIS-4.3).
    pub(crate) fn same_normal(self, other: Self) -> bool {
        self.normal
            .iter()
            .zip(other.normal)
            .all(|(a, b)| f64::from(a - b).abs() <= EQUAL_EPSILON)
    }
}

/// Which side of a plane a point is on, within `ON_EPSILON`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Front,
    Back,
    On,
}

/// A convex polygon's points, in order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Winding(Vec<Vec3>);

impl Winding {
    pub(crate) const fn new(points: Vec<Vec3>) -> Self {
        Self(points)
    }

    pub(crate) fn points(&self) -> &[Vec3] {
        &self.0
    }

    /// `PlaneFromWinding`: the normal `cross(p0 - p1, p2 - p1)` normalised, through `p0`
    /// (VIS-2.3); a winding of fewer than three points has none, and so no plane to clip by.
    pub(crate) fn plane(&self) -> Plane {
        let (Some(&p0), Some(&p1), Some(&p2)) = (self.0.first(), self.0.get(1), self.0.get(2))
        else {
            return Plane {
                normal: ORIGIN,
                dist: 0.0,
            };
        };
        let normal = cross(sub(p0, p1), sub(p2, p1));
        let normal = normalized(normal);
        Plane {
            normal,
            dist: dot(p0, normal),
        }
    }

    /// `ClipWinding` with `keepon` false: the part in front of `split`, a point within
    /// `ON_EPSILON` of it kept; none when no point is in front (VIS-4.3).
    #[cfg(test)]
    pub(crate) fn clipped(&self, split: Plane) -> Option<Self> {
        match self.clip(split)? {
            Clip::Whole => Some(self.clone()),
            Clip::Cut(cut) => Some(cut),
        }
    }

    /// [`Self::clipped`] without a copy when nothing is cut: the sides are classified on the
    /// fly, with no list of distances or sides, and only a winding that is split is allocated.
    #[expect(
        clippy::float_cmp,
        reason = "spec VIS-4.3: vis puts a split point exactly on an axial plane"
    )]
    pub(crate) fn clip(&self, split: Plane) -> Option<Clip> {
        let side_of = |p: Vec3| {
            let d = split.side(p);
            if d > VIS_ON_EPSILON {
                Side::Front
            } else if d < -VIS_ON_EPSILON {
                Side::Back
            } else {
                Side::On
            }
        };
        let (mut front, mut back) = (false, false);
        for &p in &self.0 {
            match side_of(p) {
                Side::Front => front = true,
                Side::Back => back = true,
                Side::On => {}
            }
        }
        if !front {
            return None;
        }
        if !back {
            return Some(Clip::Whole);
        }
        let count = self.0.len();
        let mut out = Vec::with_capacity(count.saturating_add(SPLIT_SPARE));
        for (i, &p1) in self.0.iter().enumerate() {
            let side = side_of(p1);
            if side == Side::On {
                out.push(p1);
                continue;
            }
            if side == Side::Front {
                out.push(p1);
            }
            let next = i.saturating_add(1).checked_rem(count).unwrap_or_default();
            let Some(&p2) = self.0.get(next) else {
                continue;
            };
            let next_side = side_of(p2);
            if next_side == Side::On || next_side == side {
                continue;
            }
            // The distances are floats; widening them kept their values.
            let (d1, d2) = (split.side(p1), split.side(p2));
            let t = crate::to_f32(d1) / (crate::to_f32(d1) - crate::to_f32(d2));
            let mid = [0, 1, 2].map(|j| {
                let n = split.normal.get(j).copied().unwrap_or_default();
                if n == 1.0 {
                    split.dist
                } else if n == -1.0 {
                    -split.dist
                } else {
                    let (a, b) = (
                        p1.get(j).copied().unwrap_or_default(),
                        p2.get(j).copied().unwrap_or_default(),
                    );
                    a + t * (b - a)
                }
            });
            out.push(mid);
        }
        Some(Clip::Cut(Self(out)))
    }

    /// A winding, borrowed or owned, clipped to the front of `split`: itself when nothing is
    /// cut.
    pub(crate) fn clip_cow(winding: Cow<'_, Self>, split: Plane) -> Option<Cow<'_, Self>> {
        match winding.clip(split)? {
            Clip::Whole => Some(winding),
            Clip::Cut(cut) => Some(Cow::Owned(cut)),
        }
    }

    /// `ClipToSeperators`: `target` clipped by every plane through an edge of `self`, the
    /// source, and a point of `pass` that has the source behind it and the pass in front, the
    /// back kept when `flip` (VIS-4.4); none when it is clipped away.
    pub(crate) fn separated<'t>(
        &self,
        pass: &Self,
        target: Cow<'t, Self>,
        flip: Flip,
    ) -> Option<Cow<'t, Self>> {
        let mut target = Some(target);
        self.separators(pass, flip, |plane| {
            target = target.take().and_then(|t| Self::clip_cow(t, plane));
            target.is_some()
        });
        target
    }

    /// Every plane [`Self::separated`] clips by, in its order, passed to `visit` until it
    /// returns false.
    pub(crate) fn separators(&self, pass: &Self, flip: Flip, mut visit: impl FnMut(Plane) -> bool) {
        let source = &self.0;
        let count = source.len();
        for (i, &si) in source.iter().enumerate() {
            let l = i.saturating_add(1).checked_rem(count).unwrap_or_default();
            let Some(&sl) = source.get(l) else {
                continue;
            };
            let v1 = sub(sl, si);
            for (j, &pj) in pass.0.iter().enumerate() {
                let v2 = sub(pj, si);
                let normal = cross(v1, v2);
                let length = dot(normal, normal);
                if f64::from(length) < VIS_ON_EPSILON {
                    continue;
                }
                // `length = 1/sqrt(length)`: the root and the quotient in `double`, stored as a
                // `float` (VIS-4.4).
                let scale = crate::to_f32(1.0 / f64::from(length).sqrt());
                let normal = normal.map(|c| c * scale);
                let mut plane = Plane {
                    normal,
                    dist: dot(pj, normal),
                };
                // Which side the source is on, by its first point off the plane.
                let off = source
                    .iter()
                    .enumerate()
                    .filter(|&(k, _)| k != i && k != l)
                    .map(|(_, &p)| plane.side(p))
                    .find(|d| d.abs() > VIS_ON_EPSILON);
                let Some(off) = off else {
                    continue;
                };
                if off > VIS_ON_EPSILON {
                    plane = plane.flipped();
                }
                let mut ahead = 0_usize;
                let mut behind = false;
                for (k, &p) in pass.0.iter().enumerate() {
                    if k == j {
                        continue;
                    }
                    let d = plane.side(p);
                    if d < -VIS_ON_EPSILON {
                        behind = true;
                        break;
                    }
                    if d > VIS_ON_EPSILON {
                        ahead = ahead.saturating_add(1);
                    }
                }
                if behind || ahead == 0 {
                    continue;
                }
                if flip == Flip::Back {
                    plane = plane.flipped();
                }
                if !visit(plane) {
                    return;
                }
            }
        }
    }
}

impl Winding {
    /// Whether the winding's points go round `plane`'s normal the other way from the way a
    /// plane through an edge and a point in front of `plane` has the winding behind it: the
    /// sum of the crosses of its edges from its first point lies against the normal.
    fn turns_against(&self, plane: Plane) -> bool {
        let Some((&first, rest)) = self.0.split_first() else {
            return false;
        };
        let round = rest
            .windows(2)
            .filter_map(|pair| match pair {
                [a, b] => Some(cross(sub(*a, first), sub(*b, first))),
                _ => None,
            })
            .fold(ORIGIN, |sum, c| {
                [sum[0] + c[0], sum[1] + c[1], sum[2] + c[2]]
            });
        dot(round, plane.normal) < 0.0
    }

    /// ericw-tools' `ClipToSeparators`, which a `PRT2` flow clips by (VIS-7.6): for each edge
    /// of `self`, the source, which lies in `plane`, the first point of `pass` off that plane
    /// whose plane through the edge has the rest of the pass in front of it, the back kept
    /// when `flip`; each passed to `visit` until it returns false.
    pub(crate) fn separators_from(
        &self,
        plane: Plane,
        pass: &Self,
        flip: Flip,
        mut visit: impl FnMut(Plane) -> bool,
    ) {
        let source = &self.0;
        let count = source.len();
        // ericw-tools turns a portal's winding with its plane, so the plane faces the way the
        // points go round; here a portal and the one looking back through it share their
        // points (VIS-2.2), so which way they go round the plane is found.
        let turned = self.turns_against(plane);
        for (i, &si) in source.iter().enumerate() {
            let l = i.saturating_add(1).checked_rem(count).unwrap_or_default();
            let Some(&sl) = source.get(l) else {
                continue;
            };
            let v1 = sub(sl, si);
            for (j, &pj) in pass.0.iter().enumerate() {
                // Which side of the source's plane the point is on is the side of the
                // separating plane the source is on.
                let d = if turned {
                    -plane.side(pj)
                } else {
                    plane.side(pj)
                };
                if d.abs() <= VIS_ON_EPSILON {
                    continue;
                }
                let normal = cross(v1, sub(pj, si));
                let length = dot(normal, normal);
                if f64::from(length) < VIS_ON_EPSILON {
                    continue;
                }
                let scale = crate::to_f32(1.0 / f64::from(length).sqrt());
                let normal = normal.map(|c| c * scale);
                let mut separator = Plane {
                    normal,
                    dist: dot(pj, normal),
                };
                if d < -VIS_ON_EPSILON {
                    separator = separator.flipped();
                }
                let mut ahead = 0_usize;
                let mut behind = false;
                for (k, &p) in pass.0.iter().enumerate() {
                    if k == j {
                        continue;
                    }
                    let d = separator.side(p);
                    if d < -VIS_ON_EPSILON {
                        behind = true;
                        break;
                    }
                    if d > VIS_ON_EPSILON {
                        ahead = ahead.saturating_add(1);
                    }
                }
                if behind || ahead == 0 {
                    continue;
                }
                if flip == Flip::Back {
                    separator = separator.flipped();
                }
                if !visit(separator) {
                    return;
                }
                break;
            }
        }
    }
}

/// What clipping a winding to a plane's front leaves: all of it, or a new, smaller winding.
pub(crate) enum Clip {
    Whole,
    Cut(Winding),
}

/// Which side of a separating plane is kept (VIS-4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Flip {
    /// The side the pass is on.
    Front,
    /// The other.
    Back,
}

/// `normal` scaled to length 1; as it is when it has none.
/// `VectorNormalize` sums the squares in `double`.
fn normalized(normal: Vec3) -> Vec3 {
    let length = normal
        .iter()
        .fold(0.0_f64, |sum, &c| sum + f64::from(c * c))
        .sqrt();
    if length == 0.0 {
        return normal;
    }
    normal.map(|c| crate::to_f32(f64::from(c) / length))
}
