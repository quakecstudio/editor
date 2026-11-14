// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/tjunc.cc, PointOnEdge, TestEdge, Welds, HasTJuncInteraction, FindEdgeVerts_FaceBounds_R, FindEdgeVerts_FaceBounds, SplitFaceIntoFragments, AngleOfTriangle, TriangleIsValid, CreateSuperFace, triangle_exists, find_best_fan, find_seed_vertex, compress_triangles_into_fans, minimum_weight_triangulation, mwt_face, FixFaceEdges, FindFaces_r, TJunc
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/qvec.hh, indexOfLargestMagnitudeComponent, MakeTangentAndBitangentUnnormalized

//! T-junction fixing, ericw-tools' `TJunc` at its default level, `-tjunc mwt` (QBSP-15.47): each
//! face's polygon given every vertex of the faces it welds with that lies on its edges, then cut
//! into the fewest fans of its minimum-weight triangulation, each of at most 64 edges.

use std::collections::{BTreeSet, HashMap, VecDeque};

use super::faces::FaceId;
use super::game::Contents;
use super::output::Fragment;
use super::tree::{NodeId, Tree};
use super::vec::{AXES, Bounds, Planes, Vec3, cross, dot, normalize, normalize_with_length, sub};
use super::winding::ON_EPSILON;

/// `-maxedges`'s default: a fragment of more vertices is split.
const MAX_EDGES: usize = 64;
/// How far around an edge vertices are looked for, `grow(qvec3d(1.0, 1.0, 1.0))`.
const EDGE_BOX_GROWTH: f64 = 1.0;
/// The smallest corner angle in degrees a triangle may have, `TriangleIsValid(..., 0.01)`.
const ANGLE_EPSILON: f64 = 0.01;
/// Degrees in a half turn.
const HALF_TURN_DEGREES: f64 = 180.0;
/// A triangle's corners.
const CORNERS: usize = 3;

/// A face's vertices in the file, `original_vertices`, for each face written.
pub(crate) type Originals = HashMap<FaceId, Vec<u32>>;

impl Contents {
    /// Whether faces of these back contents and `other`'s share their vertices, `Welds`: the
    /// same type; never a detail wall with another; never a see-through with an opaque; never
    /// with empty.
    fn welds(self, other: Self) -> bool {
        if self.types_equal(other) {
            return true;
        }
        if self.is_detail_wall() || other.is_detail_wall() {
            return false;
        }
        if self.is_opaque() != other.is_opaque() {
            return false;
        }
        !self.is_empty() && !other.is_empty()
    }
}

/// What fixing T-junctions needs at hand: the tree, the file's vertices and planes.
pub(crate) struct TJuncs<'a> {
    pub(crate) tree: &'a Tree,
    pub(crate) vertexes: &'a [[f32; 3]],
    pub(crate) planes: &'a Planes,
}

impl TJuncs<'_> {
    /// Vertex `v` as the file stores it.
    fn at(&self, v: u32) -> Vec3 {
        usize::try_from(v)
            .ok()
            .and_then(|i| self.vertexes.get(i))
            .map_or([0.0; AXES], |p| p.map(f64::from))
    }

    /// Where `p` lies along the edge from `start` along `dir`, `PointOnEdge`: `None` at or
    /// beyond the ends, kept as `float`s, or more than `DEFAULT_ON_EPSILON` off the line.
    fn point_on_edge(p: Vec3, start: Vec3, dir: Vec3, from: f64, to: f64) -> Option<f64> {
        let dist = dot(sub(p, start), dir);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec QBSP-15.47: `PointOnEdge` takes its ends as `float`"
        )]
        let (from, to) = (f64::from(from as f32), f64::from(to as f32));
        if dist <= from || dist >= to {
            return None;
        }
        let exact = super::vec::add(start, super::vec::scale(dir, dist));
        let error = super::vec::length(sub(p, exact));
        (error <= ON_EPSILON).then_some(dist)
    }

    /// The edge from `p1` to `p2` with the vertices on it put in, `TestEdge`: the first vertex
    /// on it from `startvert` breaks it in two, each tested again from the next.
    #[expect(
        clippy::too_many_arguments,
        reason = "spec QBSP-15.47: `TestEdge`'s own arguments, recursed on"
    )]
    fn test_edge(
        &self,
        range: (f64, f64),
        p1: u32,
        p2: u32,
        startvert: usize,
        edge_verts: &[u32],
        edge: (Vec3, Vec3),
        superface: &mut Vec<u32>,
    ) {
        if p1 == p2 {
            return;
        }
        for (k, &j) in edge_verts.iter().enumerate().skip(startvert) {
            if j == p1 || j == p2 {
                continue;
            }
            let Some(dist) = Self::point_on_edge(self.at(j), edge.0, edge.1, range.0, range.1)
            else {
                continue;
            };
            let next = k.saturating_add(1);
            self.test_edge((range.0, dist), p1, j, next, edge_verts, edge, superface);
            self.test_edge((dist, range.1), j, p2, next, edge_verts, edge, superface);
            return;
        }
        superface.push(p1);
    }

    /// The vertices of faces welding with `face` inside `bounds`, `FindEdgeVerts_FaceBounds_R`:
    /// each node not apart from the box, its faces' vertices then its children's.
    fn edge_verts(
        &self,
        id: NodeId,
        back: Contents,
        bounds: Bounds,
        originals: &Originals,
        out: &mut Vec<u32>,
    ) {
        let (Some((_, [front, behind], _)), Some(node)) =
            (self.tree.split_of(id), self.tree.node(id))
        else {
            return;
        };
        let apart = node
            .bounds
            .mins
            .iter()
            .zip(node.bounds.maxs)
            .zip(bounds.mins.iter().zip(bounds.maxs))
            .any(|((lo, hi), (blo, bhi))| hi < *blo || *lo > bhi);
        if apart {
            return;
        }
        for face_id in &node.faces {
            let Some(face) = self.tree.face(*face_id) else {
                continue;
            };
            if !back.welds(face.contents[1]) {
                continue;
            }
            for &v in originals.get(face_id).into_iter().flatten() {
                let p = self.at(v);
                let inside = p
                    .iter()
                    .zip(bounds.mins.iter().zip(bounds.maxs))
                    .all(|(c, (lo, hi))| *c >= *lo && *c <= hi);
                if inside {
                    out.push(v);
                }
            }
        }
        self.edge_verts(front, back, bounds, originals, out);
        self.edge_verts(behind, back, bounds, originals, out);
    }

    /// The face's polygon with every vertex lying on its edges put in, `CreateSuperFace`.
    fn super_face(&self, vertices: &[u32], back: Contents, originals: &Originals) -> Vec<u32> {
        let n = vertices.len();
        let mut superface = Vec::with_capacity(n.saturating_mul(2));
        for (i, &v1) in vertices.iter().enumerate() {
            let Some(&v2) = i
                .saturating_add(1)
                .checked_rem(n)
                .and_then(|k| vertices.get(k))
            else {
                continue;
            };
            let (a, b) = (self.at(v1), self.at(v2));
            let mut bounds = Bounds::default();
            bounds.add(a);
            bounds.add(b);
            let bounds = Bounds {
                mins: bounds.mins.map(|c| c - EDGE_BOX_GROWTH),
                maxs: bounds.maxs.map(|c| c + EDGE_BOX_GROWTH),
            };
            let mut edge_verts = Vec::new();
            self.edge_verts(Tree::HEAD, back, bounds, originals, &mut edge_verts);
            let (dir, len) = normalize_with_length(sub(b, a));
            self.test_edge((0.0, len), v1, v2, 0, &edge_verts, (a, dir), &mut superface);
        }
        superface
    }

    /// Whether no corner of the triangle is under [`ANGLE_EPSILON`] degrees, `TriangleIsValid`;
    /// each angle as `AngleOfTriangle` gives it, a `float`.
    fn triangle_is_valid(&self, v0: u32, v1: u32, v2: u32) -> bool {
        let angle = |a: Vec3, b: Vec3, c: Vec3| {
            let (ab, ac) = (sub(b, a), sub(c, a));
            let num = dot(ab, ac);
            let den = dot(ab, ab).sqrt() * dot(ac, ac).sqrt();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "spec QBSP-15.47: `AngleOfTriangle` returns a `float`"
            )]
            let degrees = ((num / den).acos() * (HALF_TURN_DEGREES / core::f64::consts::PI)) as f32;
            f64::from(degrees)
        };
        let (a, b, c) = (self.at(v0), self.at(v1), self.at(v2));
        angle(a, b, c) >= ANGLE_EPSILON
            && angle(b, c, a) >= ANGLE_EPSILON
            && angle(c, a, b) >= ANGLE_EPSILON
    }

    /// The triangles of the minimum-weight triangulation of the convex polygon `points`,
    /// `minimum_weight_triangulation`: each sub-polygon's triangle of least perimeter and
    /// sub-weights, one with a corner under [`ANGLE_EPSILON`] weighing just under the most a
    /// `double` holds; read out breadth first from the edge between the first and last points,
    /// each triangle's corners in order.
    fn mwt(&self, indices: &[u32], points: &[[f64; 2]]) -> Vec<[usize; CORNERS]> {
        let n = points.len();
        let cell = |i: usize, j: usize| i.checked_add(j.checked_mul(n)?);
        let mut t = vec![0.0_f64; n.saturating_mul(n)];
        let mut k_of: Vec<Option<usize>> = vec![None; n.saturating_mul(n)];
        let distance = |a: usize, b: usize| {
            let (Some(p), Some(q)) = (points.get(a), points.get(b)) else {
                return 0.0;
            };
            ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
        };
        let index = |i: usize| indices.get(i).copied().unwrap_or_default();
        for diagonal in 0..n {
            for (i, j) in (0..n).zip(diagonal..n) {
                if j < i.saturating_add(2) {
                    continue;
                }
                let Some(ij) = cell(i, j) else { continue };
                if let Some(slot) = t.get_mut(ij) {
                    *slot = f64::MAX;
                }
                for k in i.saturating_add(1)..j {
                    let weight = if self.triangle_is_valid(index(i), index(j), index(k)) {
                        let sub_weights = cell(i, k)
                            .and_then(|c| t.get(c))
                            .zip(cell(k, j).and_then(|c| t.get(c)))
                            .map_or(0.0, |(a, b)| a + b);
                        distance(i, j) + distance(j, k) + distance(k, i) + sub_weights
                    } else {
                        f64::MAX.next_down()
                    };
                    if let Some(slot) = t.get_mut(ij)
                        && weight < *slot
                    {
                        *slot = weight;
                        if let Some(choice) = k_of.get_mut(ij) {
                            *choice = Some(k);
                        }
                    }
                }
            }
        }
        let mut triangles = Vec::with_capacity(n.saturating_sub(2));
        let mut queue = VecDeque::from([(0_usize, n.saturating_sub(1))]);
        while let Some((a, b)) = queue.pop_front() {
            if a == b {
                continue;
            }
            let Some(Some(c)) = cell(a, b).and_then(|ab| k_of.get(ab)).copied() else {
                continue;
            };
            let mut tri = [a, b, c];
            tri.sort_unstable();
            triangles.push(tri);
            queue.push_back((a, c));
            queue.push_back((c, b));
        }
        triangles
    }

    /// The face's triangles gathered into fans, `compress_triangles_into_fans`: while
    /// triangles are left, the longest fan wound from any triangle's edge; a fan of one
    /// leaves the rest as they are; otherwise the fan's vertices from its seed on, in order,
    /// its triangles removed.
    fn fans(mut triangles: Vec<[usize; CORNERS]>, vertices: &[u32]) -> Vec<Vec<u32>> {
        let n = vertices.len();
        let vertex = |i: usize| vertices.get(i).copied().unwrap_or_default();
        let exists = |triangles: &[[usize; CORNERS]], a: usize, b: usize, c: usize| {
            triangles.iter().position(|tri| {
                (0..CORNERS).any(|s| {
                    let at = |o: usize| tri.get(s.saturating_add(o) % CORNERS).copied();
                    at(0) == Some(a) && at(1) == Some(b) && at(2) == Some(c)
                })
            })
        };
        let mut out = Vec::new();
        while !triangles.is_empty() {
            let mut best: Vec<usize> = Vec::new();
            for tri in &triangles {
                for perm in 0..CORNERS {
                    let at = |o: usize| tri.get(perm.saturating_add(o) % CORNERS).copied();
                    let (Some(first), Some(mut mid), Some(mut last)) = (at(0), at(1), at(2)) else {
                        continue;
                    };
                    let mut fan = Vec::new();
                    while last != first {
                        if let Some(found) = exists(&triangles, first, mid, last) {
                            fan.push(found);
                            mid = last;
                        }
                        last = last.saturating_add(1).checked_rem(n).unwrap_or(first);
                    }
                    if best.is_empty() || fan.len() > best.len() {
                        best = fan;
                    }
                }
            }
            if best.len() <= 1 {
                for tri in triangles.drain(..) {
                    out.push(tri.iter().map(|&i| vertex(i)).collect());
                }
                break;
            }
            let seed = Self::seed(&triangles, &best);
            let key = |v: usize| if v < seed { v.saturating_add(n) } else { v };
            let mut verts: Vec<usize> = best
                .iter()
                .filter_map(|&t| triangles.get(t))
                .flatten()
                .copied()
                .collect::<BTreeSet<usize>>()
                .into_iter()
                .collect();
            verts.sort_by_key(|&v| key(v));
            out.push(verts.into_iter().map(vertex).collect());
            best.sort_unstable_by(|a, b| b.cmp(a));
            for t in best {
                if t < triangles.len() {
                    triangles.remove(t);
                }
            }
        }
        out
    }

    /// The vertex every triangle of the fan has, `find_seed_vertex`: the first triangle's
    /// corners kept while each later triangle has them, the one left; with several left the one
    /// a hash set's iteration gives first, the last of them inserted, here the largest.
    fn seed(triangles: &[[usize; CORNERS]], fan: &[usize]) -> usize {
        let Some(first) = fan.first().and_then(|&t| triangles.get(t)) else {
            return 0;
        };
        let mut verts: Vec<usize> = first.to_vec();
        for tri in fan.iter().skip(1).filter_map(|&t| triangles.get(t)) {
            verts.retain(|v| tri.contains(v));
            if let [only] = verts.as_slice() {
                return *only;
            }
        }
        verts.into_iter().max().unwrap_or_default()
    }

    /// The fragments of a polygon of more than [`MAX_EDGES`] vertices, `SplitFaceIntoFragments`
    /// over the list as `FixFaceEdges` walks it: each fragment the first 64 vertices of what is
    /// left, which keeps its first and the 64th; after each face the list's first moved to its
    /// end, ending the walk.
    fn split_large(faces: Vec<Vec<u32>>) -> Vec<Vec<u32>> {
        let mut nodes: Vec<Vec<u32>> = faces;
        let mut order: VecDeque<usize> = (0..nodes.len()).collect();
        let mut cursor = order.front().copied();
        while let Some(current) = cursor {
            while nodes.get(current).is_some_and(|f| f.len() > MAX_EDGES) {
                let Some(face) = nodes.get_mut(current) else {
                    break;
                };
                let piece: Vec<u32> = face.iter().take(MAX_EDGES).copied().collect();
                let keep_last = MAX_EDGES.saturating_sub(1);
                let rest: Vec<u32> = face.iter().skip(keep_last).copied().collect();
                face.truncate(1);
                face.extend(rest);
                nodes.push(piece);
                order.push_back(nodes.len().saturating_sub(1));
            }
            order.rotate_left(1.min(order.len()));
            cursor = order
                .iter()
                .position(|&i| i == current)
                .and_then(|p| order.get(p.saturating_add(1)))
                .copied();
        }
        order
            .into_iter()
            .filter_map(|i| nodes.get(i).cloned())
            .collect()
    }

    /// The face's tangents for its triangulation, `MakeTangentAndBitangentUnnormalized`,
    /// normalized: the normal crossed with the axes after its largest component, swapped when
    /// they would face away from it.
    fn tangents(normal: Vec3) -> (Vec3, Vec3) {
        let mut axis = 0;
        let mut largest = 0.0;
        for (i, c) in normal.iter().enumerate() {
            if c.abs() > largest {
                largest = c.abs();
                axis = i;
            }
        }
        let unit = |i: usize| {
            let mut v = [0.0; AXES];
            if let Some(c) = v.get_mut(i % AXES) {
                *c = 1.0;
            }
            v
        };
        let mut tangent = cross(normal, unit(axis.saturating_add(1)));
        let mut bitangent = cross(normal, unit(axis.saturating_add(2)));
        if dot(cross(tangent, bitangent), normal) < 0.0 {
            core::mem::swap(&mut tangent, &mut bitangent);
        }
        (normalize(tangent), normalize(bitangent))
    }

    /// The fragments of face `id`, `FixFaceEdges` at `-tjunc mwt` (QBSP-15.47): its polygon
    /// with the vertices on its edges; none when fewer than three are left; itself with three;
    /// else its minimum-weight triangulation's fans, each split to [`MAX_EDGES`].
    fn fix(&self, id: FaceId, originals: &Originals) -> Vec<Fragment> {
        let (Some(face), Some(vertices)) = (self.tree.face(id), originals.get(&id)) else {
            return Vec::new();
        };
        let superface = self.super_face(vertices, face.contents[1], originals);
        if superface.len() < CORNERS {
            return Vec::new();
        }
        if superface.len() == CORNERS {
            return vec![Fragment {
                vertices: vertices.clone(),
                output: None,
            }];
        }
        let Some(plane) = self.planes.get(face.planenum) else {
            return Vec::new();
        };
        let (u, v) = Self::tangents(plane.plane().normal);
        let points: Vec<[f64; 2]> = superface
            .iter()
            .map(|&p| {
                let at = self.at(p);
                [dot(at, u), dot(at, v)]
            })
            .collect();
        let triangles = self.mwt(&superface, &points);
        let fans = Self::fans(triangles, &superface);
        Self::split_large(fans)
            .into_iter()
            .map(|vertices| Fragment {
                vertices,
                output: None,
            })
            .collect()
    }

    /// Every written face's fragments, `TJunc` (QBSP-15.47).
    pub(crate) fn fix_all(&self, originals: &Originals) -> HashMap<FaceId, Vec<Fragment>> {
        originals
            .keys()
            .map(|&id| (id, self.fix(id, originals)))
            .collect()
    }
}

#[cfg(test)]
mod tests;
