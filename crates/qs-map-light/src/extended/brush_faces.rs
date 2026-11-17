// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2017 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/trace_embree.cc, Node_Plane, Leaf_MakeFaces, MakeFaces_r, MakeFaces
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/polylib.hh, from_plane, calc_sides, clip_front

//! The shadow of a brush model that has no faces (LIGHT-11.10): one whose brushes are all of
//! a texture that is not drawn, `skip`. Its faces are made again from the solid leaves of its
//! hull, each leaf's sides cut from the planes of the nodes above it.

use qs_bsp::BSP_CONTENTS_SOLID;

use super::math::{Plane, add, cross, dot, normalize, scale, sub};
use super::scene::Map;
use crate::{AXES, Vec3};

/// How far a side's first square reaches each way, `from_plane(faceplane, 10e6)`.
const EXTENT: f32 = 10e6;
/// The corners a cut can add to a polygon, `result.reserve(size() + 4)`.
const CUT_ROOM: usize = 4;
/// `DEFAULT_ON_EPSILON`: how near a plane a point is on it.
const ON_EPSILON: f32 = 0.1;

/// A convex polygon, its corners in order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Winding(pub(crate) Vec<Vec3>);

/// Where a corner lies of a plane.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Front,
    Back,
    On,
}

impl Winding {
    /// `from_plane`: a square on `plane`, `EXTENT` each way from where the plane is nearest
    /// the origin, its up the world's up, or x for a plane that faces up or down, laid on the
    /// plane.
    fn from_plane(plane: Plane) -> Self {
        let mut major = 0;
        let mut most = f32::MIN;
        for (axis, part) in plane.normal.iter().enumerate() {
            if part.abs() > most {
                major = axis;
                most = part.abs();
            }
        }
        let mut up = if major == 2 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        let along = dot(up, plane.normal);
        up = normalize(add(up, scale(plane.normal, -along)));
        let origin = scale(plane.normal, plane.dist);
        let right = scale(cross(up, plane.normal), EXTENT);
        let up = scale(up, EXTENT);
        Self(vec![
            add(sub(origin, right), up),
            add(add(origin, right), up),
            sub(add(origin, right), up),
            sub(sub(origin, right), up),
        ])
    }

    /// `clip_front`: the part in front of `plane`; none when no corner is in front of it,
    /// and the whole when none is behind.
    #[expect(
        clippy::float_cmp,
        reason = "spec LIGHT-11.10: `plane.normal[j] == 1`, an axial plane's cut is its distance"
    )]
    fn clip_front(self, plane: Plane) -> Option<Self> {
        let dists: Vec<f32> = self
            .0
            .iter()
            .map(|point| dot(*point, plane.normal) - plane.dist)
            .collect();
        let sides: Vec<Side> = dists
            .iter()
            .map(|&dist| {
                if dist > ON_EPSILON {
                    Side::Front
                } else if dist < -ON_EPSILON {
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
            return Some(self);
        }
        let mut out = Vec::with_capacity(self.0.len().saturating_add(CUT_ROOM));
        let count = self.0.len();
        for (at, ((&point, &side), &dist)) in self.0.iter().zip(&sides).zip(&dists).enumerate() {
            if side != Side::Back {
                out.push(point);
            }
            if side == Side::On {
                continue;
            }
            let next = at.saturating_add(1).checked_rem(count).unwrap_or(0);
            let (Some(&next_point), Some(&next_side), Some(&next_dist)) =
                (self.0.get(next), sides.get(next), dists.get(next))
            else {
                continue;
            };
            if next_side == Side::On || next_side == side {
                continue;
            }
            // The cut between the two corners, an axial plane's at its own distance.
            let by = dist / (dist - next_dist);
            let mut mid = [0.0; AXES];
            for (((slot, &normal), &from), &to) in mid
                .iter_mut()
                .zip(&plane.normal)
                .zip(&point)
                .zip(&next_point)
            {
                *slot = if normal == 1.0 {
                    plane.dist
                } else if normal == -1.0 {
                    -plane.dist
                } else {
                    from + by * (to - from)
                };
            }
            out.push(mid);
        }
        Some(Self(out))
    }

    /// `Leaf_MakeFaces`: the sides of the leaf the inward `planes` bound: each plane turned
    /// outward, its square cut to what is in front of every other plane; a side cut away
    /// whole is left out.
    fn sides_of(planes: &[Plane]) -> Vec<Self> {
        let mut out = Vec::new();
        for (at, plane) in planes.iter().enumerate() {
            let outward = Plane {
                normal: scale(plane.normal, -1.0),
                dist: -plane.dist,
            };
            let mut side = Some(Self::from_plane(outward));
            for (other_at, other) in planes.iter().enumerate() {
                if other_at == at {
                    continue;
                }
                side = side.and_then(|side| side.clip_front(*other));
                if side.is_none() {
                    break;
                }
            }
            out.extend(side);
        }
        out
    }
}

impl Map<'_> {
    /// `MakeFaces`: the sides of every solid leaf of hull 0 of model `model`, moved with the
    /// model (LIGHT-11.10).
    pub(crate) fn brush_faces(&self, model: usize) -> Vec<Winding> {
        let mut out = Vec::new();
        let Some(head) = self
            .bsp
            .models
            .get(model)
            .and_then(|m| m.headnode.first().copied())
        else {
            return out;
        };
        let offset = self
            .models
            .get(model)
            .map_or([0.0; AXES], |about| about.offset);
        // `MakeFaces_r`, front child first: each node to visit with the planes above it. A
        // tree deeper than it has nodes is not a tree, and is left.
        let mut pending: Vec<(i32, Vec<Plane>)> = vec![(head, Vec::new())];
        while let Some((number, planes)) = pending.pop() {
            if planes.len() > self.bsp.nodes.len() {
                continue;
            }
            let Ok(node) = usize::try_from(number) else {
                // A leaf is -1 less its number.
                let leaf = usize::try_from(number.saturating_add(1).saturating_neg()).ok();
                let solid = leaf
                    .and_then(|leaf| self.bsp.leafs.get(leaf))
                    .is_some_and(|leaf| leaf.contents == BSP_CONTENTS_SOLID);
                if solid {
                    out.extend(
                        Winding::sides_of(&planes).into_iter().map(|side| {
                            Winding(side.0.into_iter().map(|p| add(p, offset)).collect())
                        }),
                    );
                }
                continue;
            };
            let Some(node) = self.bsp.nodes.get(node) else {
                continue;
            };
            let Some(plane) = self.bsp.planes.get(node.planenum) else {
                continue;
            };
            let front = Plane {
                normal: plane.normal,
                dist: plane.dist,
            };
            let back = Plane {
                normal: scale(plane.normal, -1.0),
                dist: -plane.dist,
            };
            // The back child is pushed first, so the front one is visited first.
            for (child, plane) in [(node.children[1], back), (node.children[0], front)] {
                let mut planes = planes.clone();
                planes.push(plane);
                pending.push((child, planes));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::float_cmp,
        reason = "test code: a failed unwrap or index is a failed test, and the values are exact"
    )]

    use super::*;

    fn inward(normal: Vec3, dist: f32) -> Plane {
        Plane { normal, dist }
    }

    /// The six planes of the box from `min` to `max`, facing in.
    fn box_planes(min: Vec3, max: Vec3) -> Vec<Plane> {
        vec![
            inward([1.0, 0.0, 0.0], min[0]),
            inward([-1.0, 0.0, 0.0], -max[0]),
            inward([0.0, 1.0, 0.0], min[1]),
            inward([0.0, -1.0, 0.0], -max[1]),
            inward([0.0, 0.0, 1.0], min[2]),
            inward([0.0, 0.0, -1.0], -max[2]),
        ]
    }

    // spec LIGHT-11.10: a side's first square lies on its plane, ten million units each way.
    #[test]
    fn a_planes_square_lies_on_it() {
        let up = Winding::from_plane(inward([0.0, 0.0, 1.0], 64.0));
        assert_eq!(up.0.len(), 4);
        assert!(up.0.iter().all(|p| p[2] == 64.0));
        assert!(
            up.0.iter()
                .all(|p| p[0].abs() == 10e6 && p[1].abs() == 10e6)
        );
        let side = Winding::from_plane(inward([1.0, 0.0, 0.0], -8.0));
        assert!(side.0.iter().all(|p| p[0] == -8.0));
        // The first corner is its place less its right, the world's y, plus its up.
        assert_eq!(side.0[0], [-8.0, -10e6, 10e6]);
    }

    // spec LIGHT-11.10: a polygon is cut to what is in front of a plane; one wholly behind is
    // gone, one wholly in front or on it is kept as it is, and an axial plane cuts at its own
    // distance.
    #[test]
    fn a_polygon_is_cut_to_the_front_of_a_plane() {
        let square = || {
            Winding(vec![
                [0.0, 0.0, 0.0],
                [8.0, 0.0, 0.0],
                [8.0, 8.0, 0.0],
                [0.0, 8.0, 0.0],
            ])
        };
        let cut = square().clip_front(inward([1.0, 0.0, 0.0], 2.0)).unwrap();
        assert_eq!(
            cut.0,
            [
                [2.0, 0.0, 0.0],
                [8.0, 0.0, 0.0],
                [8.0, 8.0, 0.0],
                [2.0, 8.0, 0.0]
            ]
        );
        assert_eq!(square().clip_front(inward([1.0, 0.0, 0.0], 9.0)), None);
        assert_eq!(
            square().clip_front(inward([1.0, 0.0, 0.0], -1.0)),
            Some(square())
        );
        // Within a tenth of the plane is on it.
        assert_eq!(
            square().clip_front(inward([1.0, 0.0, 0.0], 0.05)),
            Some(square())
        );
        assert_eq!(square().clip_front(inward([1.0, 0.0, 0.0], 7.95)), None);
        let slant = normalize([1.0, 1.0, 0.0]);
        let corner = square()
            .clip_front(inward(slant, dot([6.0, 6.0, 0.0], slant)))
            .unwrap();
        assert_eq!(corner.0.len(), 3);
        assert_eq!(corner.0[1], [8.0, 8.0, 0.0]);
    }

    // spec LIGHT-11.10: a leaf's sides are its planes' squares cut by the others: a box has
    // six, each a rectangle of the box, facing out.
    #[test]
    fn a_box_leaf_has_six_sides() {
        let sides = Winding::sides_of(&box_planes([0.0, 0.0, 0.0], [16.0, 32.0, 64.0]));
        assert_eq!(sides.len(), 6);
        for side in &sides {
            assert_eq!(side.0.len(), 4);
            for point in &side.0 {
                assert!([0.0, 16.0].contains(&point[0]), "{point:?}");
                assert!([0.0, 32.0].contains(&point[1]), "{point:?}");
                assert!([0.0, 64.0].contains(&point[2]), "{point:?}");
            }
        }
        // The first side is the plane x >= 0 turned out: it lies at x = 0.
        assert!(sides[0].0.iter().all(|p| p[0] == 0.0));
        // Planes that leave nothing give no side.
        let mut none = box_planes([0.0; 3], [16.0; 3]);
        none.push(inward([1.0, 0.0, 0.0], 100.0));
        assert!(Winding::sides_of(&none).is_empty());
    }
}
