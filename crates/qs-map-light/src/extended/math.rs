// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2017 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/mathlib.cc, FaceNormal, MakeInwardFacingEdgePlane, MakeInwardFacingEdgePlanes, EdgePlanes_PointInsideDist, EdgePlanes_PointInside, ProjectPointOntoPlane, ClosestPointOnPolyBoundary, InterpolateNormal, FractionOfLine, ClosestPointOnLine, ClosestPointOnLineSegment
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/qvec.hh, TriangleArea, PolyCentroid, PolyArea, Barycentric_FromPoint, Barycentric_ToPoint, vec_from_mangle

//! The vectors, planes and polygons the extended dialect's lighting works in: single precision
//! throughout, as ericw-tools' `qvec3f` (LIGHT-12, LIGHT-13).

use crate::{AXES, Vec3, to_f32};

/// A plane as its normal and its distance from the origin along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plane {
    pub(crate) normal: Vec3,
    pub(crate) dist: f32,
}

/// `POINT_EQUAL_EPSILON`: how far outside an edge a point still counts as inside (LIGHT-12.4).
pub(crate) const POINT_EQUAL_EPSILON: f32 = 0.05;
/// `ZERO_TRI_AREA_EPSILON`: a triangle smaller than this gives a polygon no normal.
const ZERO_TRI_AREA_EPSILON: f32 = 0.05;
/// Half, of a triangle's cross product.
const HALF: f32 = 0.5;
/// The corners of a triangle.
const CORNERS: f32 = 3.0;
/// Degrees to a half turn.
const HALF_TURN: f32 = 180.0;

pub(crate) fn dot(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(crate) fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub(crate) fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(crate) fn scale(a: Vec3, by: f32) -> Vec3 {
    a.map(|c| c * by)
}

pub(crate) fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(crate) fn length(a: Vec3) -> f32 {
    dot(a, a).sqrt()
}

pub(crate) fn distance(a: Vec3, b: Vec3) -> f32 {
    length(sub(a, b))
}

/// `qv::normalize`: `a` over its length; as it is when it has none.
pub(crate) fn normalize(a: Vec3) -> Vec3 {
    normalize_with_length(a).0
}

/// `qv::normalizeInPlace`: `a` over its length, and the length.
pub(crate) fn normalize_with_length(a: Vec3) -> (Vec3, f32) {
    let len = length(a);
    if len == 0.0 {
        (a, len)
    } else {
        (a.map(|c| c / len), len)
    }
}

/// Whether every component is exactly zero (`qv::emptyExact`).
pub(crate) fn is_zero(a: Vec3) -> bool {
    a.iter().all(|&c| c == 0.0)
}

/// `qv::vec_from_mangle`: the direction of a yaw and pitch in degrees, the roll unused: the
/// angles go to radians as floats and the turns are worked in `double` (LIGHT-14.2).
pub(crate) fn vec_from_mangle(mangle: Vec3) -> Vec3 {
    let to_radians = core::f32::consts::PI / HALF_TURN;
    let yaw = f64::from(mangle[0] * to_radians);
    let pitch = f64::from(mangle[1] * to_radians);
    [
        to_f32(yaw.cos() * pitch.cos()),
        to_f32(yaw.sin() * pitch.cos()),
        to_f32(pitch.sin()),
    ]
}

impl Plane {
    /// The plane facing the other way.
    pub(crate) fn flipped(self) -> Self {
        Self {
            normal: scale(self.normal, -1.0),
            dist: -self.dist,
        }
    }

    /// `DistAbovePlane`: how far `point` is in front.
    pub(crate) fn distance_to(self, point: Vec3) -> f32 {
        dot(self.normal, point) - self.dist
    }

    /// `ProjectPointOntoPlane`: `point` moved along the normal onto the plane.
    pub(crate) fn projected(self, point: Vec3) -> Vec3 {
        add(point, scale(self.normal, -self.distance_to(point)))
    }
}

/// A box by its two corners (`aabb3f`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Aabb {
    pub(crate) mins: Vec3,
    pub(crate) maxs: Vec3,
}

impl Aabb {
    /// The box that holds only `point`.
    pub(crate) const fn at(point: Vec3) -> Self {
        Self {
            mins: point,
            maxs: point,
        }
    }

    /// The box widened to hold `point`.
    pub(crate) fn with(self, point: Vec3) -> Self {
        Self {
            mins: [
                self.mins[0].min(point[0]),
                self.mins[1].min(point[1]),
                self.mins[2].min(point[2]),
            ],
            maxs: [
                self.maxs[0].max(point[0]),
                self.maxs[1].max(point[1]),
                self.maxs[2].max(point[2]),
            ],
        }
    }

    /// The box moved by `offset`.
    pub(crate) fn moved(self, offset: Vec3) -> Self {
        Self {
            mins: add(self.mins, offset),
            maxs: add(self.maxs, offset),
        }
    }

    /// The box grown on every side by `by` times its size.
    pub(crate) fn grown(self, by: f32) -> Self {
        let size = scale(sub(self.maxs, self.mins), by);
        Self {
            mins: sub(self.mins, size),
            maxs: add(self.maxs, size),
        }
    }

    /// `disjoint`: whether the boxes are more than `epsilon` apart on some axis.
    pub(crate) fn is_apart_from(self, other: Self, epsilon: f32) -> bool {
        self.maxs
            .iter()
            .zip(other.mins)
            .any(|(max, min)| *max < min - epsilon)
            || self
                .mins
                .iter()
                .zip(other.maxs)
                .any(|(min, max)| *min > max + epsilon)
    }
}

/// `qv::TriangleArea`.
fn triangle_area(v0: Vec3, v1: Vec3, v2: Vec3) -> f32 {
    HALF * length(cross(sub(v2, v0), sub(v1, v0)))
}

/// The triangles of a polygon fanned from its first point: each with the point before and the
/// point at `i`, for `i` from 2.
fn fan(points: &[Vec3]) -> impl Iterator<Item = (Vec3, Vec3, Vec3)> + '_ {
    let first = points.first().copied().unwrap_or_default();
    points
        .windows(2)
        .skip(1)
        .filter_map(move |pair| Some((first, *pair.first()?, *pair.get(1)?)))
}

/// `FaceNormal`: the normal of the largest triangle fanned from the first point, zero when
/// none is big enough.
pub(crate) fn polygon_normal(points: &[Vec3]) -> Vec3 {
    let mut best: Option<(f32, Vec3, Vec3, Vec3)> = None;
    for (p0, p1, p2) in fan(points) {
        let area = triangle_area(p0, p1, p2);
        if best.is_none_or(|(most, ..)| area > most) {
            best = Some((area, p0, p1, p2));
        }
    }
    match best {
        Some((area, p0, p1, p2)) if area >= ZERO_TRI_AREA_EPSILON => {
            normalize(cross(sub(p2, p0), sub(p1, p0)))
        }
        _ => [0.0; AXES],
    }
}

/// `qv::PolyArea`.
pub(crate) fn polygon_area(points: &[Vec3]) -> f32 {
    fan(points).fold(0.0, |sum, (p0, p1, p2)| sum + triangle_area(p0, p1, p2))
}

/// `qv::PolyCentroid`: the triangles' centres weighed by their areas; NaN for a polygon of no
/// area.
pub(crate) fn polygon_centroid(points: &[Vec3]) -> Vec3 {
    match points {
        [] => [f32::NAN; AXES],
        [only] => *only,
        [a, b] => scale(add(*a, *b), HALF),
        _ => {
            let mut centroid = [0.0; AXES];
            let mut total = 0.0;
            for (p0, p1, p2) in fan(points) {
                let area = triangle_area(p0, p1, p2);
                let middle = scale(add(add(p0, p1), p2), 1.0 / CORNERS);
                total += area;
                centroid = add(centroid, scale(middle, area));
            }
            scale(centroid, 1.0 / total)
        }
    }
}

/// `MakeInwardFacingEdgePlane`: the plane through the edge `v0` to `v1` of a polygon facing
/// `normal`, its front the polygon's inside; none for an edge shorter than
/// [`POINT_EQUAL_EPSILON`].
pub(crate) fn edge_plane(v0: Vec3, v1: Vec3, normal: Vec3) -> Option<Plane> {
    let len = length(sub(v1, v0));
    if len < POINT_EQUAL_EPSILON {
        return None;
    }
    let along = scale(sub(v1, v0), 1.0 / len);
    let inward = cross(along, normal);
    Some(Plane {
        normal: inward,
        dist: dot(inward, v0),
    })
}

/// The edges of a polygon, each point with the next, the last with the first.
pub(crate) fn edges(points: &[Vec3]) -> impl Iterator<Item = (Vec3, Vec3)> + '_ {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(a, b)| (*a, *b))
}

/// `MakeInwardFacingEdgePlanes`: the edge planes of a polygon; none for fewer than three
/// points or a polygon with no normal.
pub(crate) fn edge_planes(points: &[Vec3]) -> Vec<Plane> {
    if points.len() < AXES {
        return Vec::new();
    }
    let normal = polygon_normal(points);
    if is_zero(normal) {
        return Vec::new();
    }
    edges(points)
        .filter_map(|(v0, v1)| edge_plane(v0, v1, normal))
        .collect()
}

/// `EdgePlanes_PointInsideDist`: how far inside its nearest edge `point` is, negative outside.
pub(crate) fn inside_distance(planes: &[Plane], point: Vec3) -> f32 {
    planes
        .iter()
        .fold(f32::MAX, |least, plane| least.min(plane.distance_to(point)))
}

/// `EdgePlanes_PointInside`: whether `point` is inside every edge, or within
/// [`POINT_EQUAL_EPSILON`] outside.
pub(crate) fn is_inside(planes: &[Plane], point: Vec3) -> bool {
    !planes.is_empty() && inside_distance(planes, point) >= -POINT_EQUAL_EPSILON
}

/// `FractionOfLine`: where along `v` to `w` the point nearest `p` is, 0 at `v` and 1 at `w`.
pub(crate) fn fraction_of_line(v: Vec3, w: Vec3, p: Vec3) -> f32 {
    let along = sub(w, v);
    let squared = dot(along, along);
    if squared == 0.0 {
        return 0.0;
    }
    dot(sub(p, v), along) / squared
}

/// `ClosestPointOnLineSegment`.
fn closest_on_segment(v: Vec3, w: Vec3, p: Vec3) -> Vec3 {
    let fraction = fraction_of_line(v, w, p);
    if fraction >= 1.0 {
        return w;
    }
    if fraction <= 0.0 {
        return v;
    }
    // `ClosestPointOnLine`.
    let along = normalize(sub(w, v));
    add(v, scale(along, dot(sub(p, v), along)))
}

/// `ClosestPointOnPolyBoundary`: the point of the polygon's edges nearest `point`.
pub(crate) fn closest_on_boundary(points: &[Vec3], point: Vec3) -> Vec3 {
    let mut best = (f32::MAX, [0.0; AXES]);
    for (p0, p1) in edges(points) {
        let near = closest_on_segment(p0, p1, point);
        let away = distance(near, point);
        if away < best.0 {
            best = (away, near);
        }
    }
    best.1
}

/// `InterpolateNormal`: the normal at `point` of a polygon whose corners have `normals`, from
/// the first fanned triangle that holds the point; none when no triangle does.
pub(crate) fn interpolated_normal(points: &[Vec3], normals: &[Vec3], point: Vec3) -> Option<Vec3> {
    if points.len() < AXES {
        return None;
    }
    for ((p0, p1, p2), (n0, n1, n2)) in fan(points).zip(fan(normals)) {
        let planes = edge_planes(&[p0, p1, p2]);
        if planes.len() != AXES || !is_inside(&planes, point) {
            continue;
        }
        // `Barycentric_FromPoint`.
        let (v0, v1, v2) = (sub(p1, p0), sub(p2, p0), sub(point, p0));
        let (d00, d01, d11) = (dot(v0, v0), dot(v0, v1), dot(v1, v1));
        let (d20, d21) = (dot(v2, v0), dot(v2, v1));
        let inverse = 1.0 / (d00 * d11 - d01 * d01);
        let b1 = (d11 * d20 - d01 * d21) * inverse;
        let b2 = (d00 * d21 - d01 * d20) * inverse;
        let b0 = 1.0 - b1 - b2;
        if !(b0.is_finite() && b1.is_finite() && b2.is_finite()) {
            continue;
        }
        return Some(add(add(scale(n0, b0), scale(n1, b1)), scale(n2, b2)));
    }
    None
}

#[cfg(test)]
mod tests {

    use super::*;

    const SQUARE: [Vec3; 4] = [
        [0.0, 0.0, 0.0],
        [0.0, 8.0, 0.0],
        [8.0, 8.0, 0.0],
        [8.0, 0.0, 0.0],
    ];

    // spec LIGHT-12.4: a point is inside a face within 0.05 of each edge's plane.
    #[test]
    fn a_point_is_inside_a_polygon_to_a_twentieth_past_its_edges() {
        let planes = edge_planes(&SQUARE);
        assert_eq!(planes.len(), 4);
        assert!(is_inside(&planes, [4.0, 4.0, 0.0]));
        assert!(is_inside(&planes, [-0.04, 4.0, 0.0]));
        assert!(!is_inside(&planes, [-0.06, 4.0, 0.0]));
        assert!(!is_inside(&[], [4.0, 4.0, 0.0]), "no planes hold nothing");
        assert_eq!(inside_distance(&planes, [1.0, 4.0, 0.0]), 1.0);
    }

    #[test]
    fn a_polygon_has_the_normal_area_and_centre_of_its_fan() {
        assert_eq!(polygon_normal(&SQUARE), [0.0, 0.0, 1.0]);
        assert_eq!(polygon_area(&SQUARE), 64.0);
        assert_eq!(polygon_centroid(&SQUARE), [4.0, 4.0, 0.0]);
        let line = [[0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        assert!(is_zero(polygon_normal(&line)), "no triangle has an area");
        assert!(edge_planes(&line).is_empty());
        assert!(polygon_centroid(&[]).iter().all(|c| c.is_nan()));
        assert_eq!(polygon_centroid(&[[2.0, 0.0, 0.0]]), [2.0, 0.0, 0.0]);
        assert_eq!(
            polygon_centroid(&[[2.0, 0.0, 0.0], [4.0, 0.0, 0.0]]),
            [3.0, 0.0, 0.0]
        );
    }

    // spec LIGHT-12.5: the nearest point of a face's edges is found edge by edge.
    #[test]
    fn the_nearest_boundary_point_is_on_the_nearest_edge_or_corner() {
        assert_eq!(
            closest_on_boundary(&SQUARE, [4.0, -3.0, 0.0]),
            [4.0, 0.0, 0.0]
        );
        assert_eq!(
            closest_on_boundary(&SQUARE, [-2.0, -2.0, 0.0]),
            [0.0, 0.0, 0.0]
        );
        assert_eq!(
            fraction_of_line([0.0; 3], [0.0; 3], [1.0, 0.0, 0.0]),
            0.0,
            "an edge of no length"
        );
    }

    // spec LIGHT-13.5: a point's normal is its triangle's corners' normals by its barycentric
    // coordinates.
    #[test]
    fn a_normal_is_interpolated_from_the_corners_of_the_points_triangle() {
        let up = [0.0, 0.0, 1.0];
        let tilted = [1.0, 0.0, 0.0];
        let normals = [up, up, tilted, tilted];
        let at_corner = interpolated_normal(&SQUARE, &normals, [0.0, 0.0, 0.0]).unwrap_or_default();
        assert_eq!(at_corner, up);
        let middle = interpolated_normal(&SQUARE, &normals, [4.0, 4.0, 0.0]).unwrap_or_default();
        assert_eq!(middle, [0.5, 0.0, 0.5]);
        assert_eq!(
            interpolated_normal(&SQUARE, &normals, [40.0, 4.0, 0.0]),
            None
        );
        assert_eq!(
            interpolated_normal(&SQUARE[..2], &normals[..2], [0.0; 3]),
            None
        );
    }

    // spec LIGHT-14.2: a mangle is yaw then pitch, in degrees.
    #[test]
    fn a_mangle_is_a_yaw_and_a_pitch() {
        let near = |a: Vec3, b: Vec3| distance(a, b) < 1e-6;
        assert!(near(vec_from_mangle([0.0, 0.0, 0.0]), [1.0, 0.0, 0.0]));
        assert!(near(vec_from_mangle([90.0, 0.0, 0.0]), [0.0, 1.0, 0.0]));
        assert!(near(vec_from_mangle([0.0, -90.0, 0.0]), [0.0, 0.0, -1.0]));
        assert!(near(vec_from_mangle([0.0, 90.0, 45.0]), [0.0, 0.0, 1.0]));
    }

    #[test]
    fn boxes_widen_move_grow_and_are_apart() {
        let unit = Aabb::at([0.0; 3]).with([1.0, 2.0, 4.0]);
        assert_eq!(unit.maxs, [1.0, 2.0, 4.0]);
        assert_eq!(unit.moved([1.0, 0.0, 0.0]).mins, [1.0, 0.0, 0.0]);
        let grown = unit.grown(0.25);
        assert_eq!(grown.mins, [-0.25, -0.5, -1.0]);
        assert_eq!(grown.maxs, [1.25, 2.5, 5.0]);
        let far = Aabb::at([3.0, 0.0, 0.0]);
        assert!(unit.is_apart_from(far, 0.0));
        assert!(far.is_apart_from(unit, 0.0));
        assert!(!unit.is_apart_from(far, 2.5), "within the epsilon");
        assert!(!unit.is_apart_from(Aabb::at([1.0, 1.0, 1.0]), 0.0));
    }

    #[test]
    fn a_plane_measures_projects_and_flips() {
        let plane = Plane {
            normal: [0.0, 0.0, 1.0],
            dist: 4.0,
        };
        assert_eq!(plane.distance_to([1.0, 2.0, 7.0]), 3.0);
        assert_eq!(plane.projected([1.0, 2.0, 7.0]), [1.0, 2.0, 4.0]);
        assert_eq!(plane.flipped().distance_to([1.0, 2.0, 7.0]), -3.0);
        assert_eq!(normalize_with_length([0.0, 3.0, 4.0]).1, 5.0);
        assert_eq!(cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]);
    }
}
