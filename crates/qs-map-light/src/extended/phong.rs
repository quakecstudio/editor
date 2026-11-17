// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2017 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/phong.cc, MakeEdgeToFaceMap, CalculateVertexNormals, Face_EdgeIndexSmoothed, FacesSmoothed, AngleBetweenVectors, AngleBetweenPoints, face_cache_t::face_cache_t

//! Phong shading (LIGHT-13): which faces are smoothed with which, each face's corner normals
//! from them, and each face's shape as sample placement uses it.

use std::collections::{BTreeSet, HashMap};

use qs_bsp::{BSP_TEX_SPECIAL, Face};

use crate::Vec3;
use crate::extended::keys::DEFAULT_PHONG_ANGLE;
use crate::extended::math::{
    Plane, add, dot, edge_planes, is_zero, length, normalize, polygon_area, polygon_centroid,
    scale, sub,
};
use crate::extended::scene::Map;

/// How far in front of a face another's middle is for the two to meet in a hollow
/// (LIGHT-13.3).
const CONCAVE: f32 = 0.1;
/// Degrees to a half turn.
const HALF_TURN: f64 = 180.0;

/// A face's shape (`face_cache_t`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Shape {
    /// Its corners.
    pub(crate) points: Vec<Vec3>,
    /// Its corners' normals, smoothed with its neighbours' (LIGHT-13.4).
    pub(crate) normals: Vec<Vec3>,
    /// Its plane, none for a face with no plane.
    pub(crate) plane: Option<Plane>,
    /// Its edges' planes, facing in.
    pub(crate) edge_planes: Vec<Plane>,
}

/// The faces' shapes and which are smoothed together.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Smoothing {
    pub(crate) shapes: Vec<Shape>,
    /// The faces each face is smoothed with (`smoothFaces`).
    smooth: Vec<BTreeSet<usize>>,
    /// The faces that walk each edge, from its first vertex to its second.
    edge_faces: HashMap<(usize, usize), Vec<usize>>,
}

/// `AngleBetweenPoints`: the angle at `corner` between `before` and `after`, in radians; 0
/// when either is at the corner.
#[expect(
    clippy::float_cmp,
    reason = "spec LIGHT-13.4: ericw-tools tests the lengths' product against exactly 0"
)]
fn corner_angle(before: Vec3, corner: Vec3, after: Vec3) -> f32 {
    let (d1, d2) = (sub(before, corner), sub(after, corner));
    let product = length(d1) * length(d2);
    if product == 0.0 {
        return 0.0;
    }
    (dot(d1, d2) / product).clamp(-1.0, 1.0).acos()
}

/// The angles a face is smoothed within: where it bulges, and where it meets in a hollow;
/// none when it is not smoothed (LIGHT-13.2).
#[expect(
    clippy::float_cmp,
    reason = "spec LIGHT-13.2: ericw-tools tests each angle against exactly 0"
)]
fn phong_angles(map: &Map<'_>, face: &Face) -> Option<(f32, f32)> {
    let flags = map.flags_of(face);
    let mut convex = flags.phong_angle;
    if convex == 0.0 && flags.phong_group != 0 {
        convex = DEFAULT_PHONG_ANGLE;
    }
    let concave = if flags.phong_angle_concave == 0.0 {
        convex
    } else {
        flags.phong_angle_concave
    };
    (convex != 0.0 || concave != 0.0).then_some((convex, concave))
}

impl Smoothing {
    /// `CalculateVertexNormals`: every face's shape, with its corners' normals (LIGHT-13).
    pub(crate) fn new(map: &Map<'_>) -> Self {
        let faces = &map.bsp.faces;
        let vertices: Vec<Vec<usize>> = faces.iter().map(|face| map.vertices(face)).collect();
        let points: Vec<Vec<Vec3>> = faces.iter().map(|face| map.points(face)).collect();
        let planes: Vec<Plane> = faces.iter().map(|face| map.plane(face)).collect();

        // `MakeEdgeToFaceMap`.
        let mut edge_faces: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        let mut vertex_faces: HashMap<usize, Vec<usize>> = HashMap::new();
        for (index, corners) in vertices.iter().enumerate() {
            for (&v0, &v1) in corners.iter().zip(corners.iter().cycle().skip(1)) {
                vertex_faces.entry(v0).or_default().push(index);
                if v0 == v1 {
                    continue;
                }
                let walkers = edge_faces.entry((v0, v1)).or_default();
                if !walkers.contains(&index) {
                    walkers.push(index);
                }
            }
        }

        let special = |face: &Face| {
            map.bsp
                .texinfo
                .get(face.texinfo)
                .map(|texinfo| texinfo.flags & BSP_TEX_SPECIAL)
        };
        let mut smooth: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); faces.len()];
        for (index, face) in faces.iter().enumerate() {
            let Some((convex, concave)) = phong_angles(map, face) else {
                continue;
            };
            let (Some(plane), Some(corners), Some(set)) =
                (planes.get(index), vertices.get(index), smooth.get_mut(index))
            else {
                continue;
            };
            let group = map.flags_of(face).phong_group;
            for vertex in corners {
                for &other in vertex_faces.get(vertex).map_or(&[][..], Vec::as_slice) {
                    if other == index {
                        continue;
                    }
                    let (Some(face2), Some(plane2), Some(points2)) =
                        (faces.get(other), planes.get(other), points.get(other))
                    else {
                        continue;
                    };
                    let Some((convex2, concave2)) = phong_angles(map, face2) else {
                        continue;
                    };
                    // Faces of which only one is `TEX_SPECIAL` are never smoothed together.
                    if let (Some(a), Some(b)) = (special(face), special(face2))
                        && a != b
                    {
                        continue;
                    }
                    let hollow = plane.distance_to(polygon_centroid(points2)) > CONCAVE;
                    let threshold = if hollow {
                        concave.min(concave2)
                    } else {
                        convex.min(convex2)
                    };
                    if group != map.flags_of(face2).phong_group {
                        continue;
                    }
                    let widest = (f64::from(threshold) * core::f64::consts::PI / HALF_TURN).cos();
                    if f64::from(dot(plane.normal, plane2.normal)) >= widest {
                        set.insert(other);
                    }
                }
            }
        }

        let mut shapes = Vec::with_capacity(faces.len());
        for (index, face) in faces.iter().enumerate() {
            let corners = vertices.get(index).cloned().unwrap_or_default();
            let own = points.get(index).cloned().unwrap_or_default();
            let plane = planes.get(index).copied();
            let normal = plane.map_or([0.0; 3], |p| p.normal);
            let mut normals = Vec::new();
            if face.numedges >= 3 {
                // Each face's normal counts at each of its corners by its area and the
                // corner's angle: this face first, then those it is smoothed with in order.
                let mut smoothed: HashMap<usize, Vec3> = HashMap::new();
                let with = smooth.get(index).into_iter().flatten().copied();
                for other in core::iter::once(index).chain(with) {
                    let (Some(points2), Some(corners2), Some(plane2)) =
                        (points.get(other), vertices.get(other), planes.get(other))
                    else {
                        continue;
                    };
                    let area = polygon_area(points2);
                    let count = points2.len();
                    for (at, (&vertex, &here)) in corners2.iter().zip(points2).enumerate() {
                        let before = at.checked_add(count).and_then(|n| n.checked_sub(1));
                        let before = before
                            .and_then(|n| n.checked_rem(count))
                            .and_then(|n| points2.get(n));
                        let after = at
                            .checked_add(1)
                            .and_then(|n| n.checked_rem(count))
                            .and_then(|n| points2.get(n));
                        let (Some(&before), Some(&after)) = (before, after) else {
                            continue;
                        };
                        let mut weight = area * corner_angle(before, here, after);
                        if !weight.is_finite() {
                            weight = 0.0;
                        }
                        let sum = smoothed.entry(vertex).or_insert([0.0; 3]);
                        *sum = add(*sum, scale(plane2.normal, weight));
                    }
                }
                for vertex in &corners {
                    let sum = smoothed.get(vertex).copied().unwrap_or_default();
                    // Corners on a line give no area to weigh by: the face's own normal.
                    normals.push(if is_zero(sum) || length(sum) == 0.0 {
                        normal
                    } else {
                        normalize(sum)
                    });
                }
            }
            shapes.push(Shape {
                edge_planes: edge_planes(&own),
                points: own,
                normals,
                plane,
            });
        }
        Self {
            shapes,
            smooth,
            edge_faces,
        }
    }

    /// `FacesSmoothed`.
    fn smoothed(&self, face: usize, other: usize) -> bool {
        self.smooth
            .get(face)
            .is_some_and(|set| set.contains(&other))
    }

    /// `Face_EdgeIndexSmoothed`: the face across `face`'s edge numbered `edge` that it is
    /// smoothed with, or that is on its plane (LIGHT-12.5).
    pub(crate) fn across(&self, map: &Map<'_>, face: usize, edge: usize) -> Option<usize> {
        let this = map.face(face)?;
        let v0 = map.vertex_at(this, edge)?;
        let next = edge.checked_add(1)?.checked_rem(this.numedges)?;
        let v1 = map.vertex_at(this, next)?;
        self.edge_faces
            .get(&(v1, v0))?
            .iter()
            .copied()
            .find(|&other| {
                other != face
                    && map.face(other).is_some_and(|that| {
                        self.smoothed(face, other)
                            || (that.planenum == this.planenum && that.side == this.side)
                    })
            })
    }
}
