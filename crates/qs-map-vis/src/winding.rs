// SPDX-License-Identifier: GPL-3.0-or-later

//! Windings and planes in single precision, as vis keeps them (`vec_t` is `float`): a portal's
//! plane from its points (VIS-2.3), a winding clipped to a plane's front, and the separating
//! planes that clip one portal by two others (VIS-4.3, VIS-4.4).

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
    #[expect(
        clippy::float_cmp,
        reason = "spec VIS-4.3: vis puts a split point exactly on an axial plane"
    )]
    pub(crate) fn clipped(&self, split: Plane) -> Option<Self> {
        let dists: Vec<f64> = self.0.iter().map(|&p| split.side(p)).collect();
        let sides: Vec<Side> = dists
            .iter()
            .map(|&d| {
                if d > VIS_ON_EPSILON {
                    Side::Front
                } else if d < -VIS_ON_EPSILON {
                    Side::Back
                } else {
                    Side::On
                }
            })
            .collect();
        if !sides.contains(&Side::Front) {
            return None;
        }
        if !sides.contains(&Side::Back) {
            return Some(self.clone());
        }
        let count = self.0.len();
        let mut out = Vec::with_capacity(count.saturating_add(SPLIT_SPARE));
        for (i, (&p1, &side)) in self.0.iter().zip(&sides).enumerate() {
            if side == Side::On {
                out.push(p1);
                continue;
            }
            if side == Side::Front {
                out.push(p1);
            }
            let next = i.saturating_add(1).checked_rem(count).unwrap_or_default();
            let (Some(&next_side), Some(&p2), Some(&d1), Some(&d2)) = (
                sides.get(next),
                self.0.get(next),
                dists.get(i),
                dists.get(next),
            ) else {
                continue;
            };
            if next_side == Side::On || next_side == side {
                continue;
            }
            // The distances are floats; widening them kept their values.
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
        Some(Self(out))
    }

    /// `ClipToSeperators`: `target` clipped by every plane through an edge of `self`, the
    /// source, and a point of `pass` that has the source behind it and the pass in front, the
    /// back kept when `flip` (VIS-4.4); none when it is clipped away.
    pub(crate) fn separated(&self, pass: &Self, target: Self, flip: Flip) -> Option<Self> {
        let source = &self.0;
        let count = source.len();
        let mut target = target;
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
                target = target.clipped(plane)?;
            }
        }
        Some(target)
    }
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
