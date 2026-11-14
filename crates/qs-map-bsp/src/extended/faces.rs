// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/faces.cc, FixupDetailFence_FindDetailFenceFaces, FixupMarkFaces_ProcessCluster_HasDetailFence, FixupMarkFaces_IsUsableLeaf, FixupMarkFaces_ProcessCluster_FindStorageLeaf, BoundFaces, AddBoundsToNode, FixupMarkFaces_AddFacesToLeaf, FixupMarkFaces_ProcessCluster, FixupDetailFenceMarkFaces_R, FixupDetailFence, ShouldOmitFace, MergeNodeFaces, AddMarksurfaces_r, MakeMarkFaces, SubdivideFace, SubdivideNodeFaces, FaceFromPortal, MakeFaces_r, MakeFaces
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/merge.cc, TryMerge, MergeFaceToList, MergeFaceList
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/csg.cc, NewFaceFromFace, CopyFace, UpdateFaceSphere, SplitFace

//! Faces from the portals' visible sides, ericw-tools' `MakeFaces` (QBSP-15.39 to QBSP-15.42):
//! each made on its portal's node, merged with its neighbours, cut to the lightmap's size, and
//! listed in the leaves it touches.

use super::brush::{BspBrush, FaceRef};
use super::game::Contents;
use super::map::{LoadedMap, SurfFlags, TexinfoNum};
use super::portals::SideRef;
use super::tree::{NodeId, Separator, Tree};
use std::collections::BTreeSet;

use super::vec::{
    AXES, Bounds, POINT_EPSILON, Plane, PlaneNum, Vec3, cross, dot, normalize,
    normalize_with_length, sub,
};
use super::winding::{KeepOn, Winding};

/// `CONTINUOUS_EPSILON`: two edges meeting this near a line are one.
const CONTINUOUS_EPSILON: f64 = 0.0005;
/// `QBSP_EQUAL_EPSILON`: two points this near are one in a merge.
const EQUAL_EPSILON: f64 = 0.0001;
/// The most lightmap blocks of 16 texels a legacy engine allows, `255 << lmshift`.
const LIGHTMAP_BLOCKS: f64 = 255.0;
/// The largest lightmap shift subdivision allows for, "no bugging out with legacy lighting".
const MAX_LMSHIFT: u32 = 4;
/// What a face is cut short of the subdivision size by, "always over-estimate by 16".
const SUBDIVIDE_MARGIN: f64 = 16.0;
/// The texture axes subdivision cuts along: s, then t.
const TEXTURE_AXES: usize = 2;

/// A face's number in its tree's faces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct FaceId(usize);

/// A face, `face_t`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Face {
    /// Its plane, the side of the pair it faces telling its direction.
    pub(crate) planenum: PlaneNum,
    /// Its texinfo.
    pub(crate) texinfo: TexinfoNum,
    /// The contents in front of it, then behind.
    pub(crate) contents: [Contents; 2],
    /// Its polygon.
    pub(crate) winding: Winding,
    /// The map face it was made for.
    pub(crate) original_side: FaceRef,
    /// The middle of its points, `origin`.
    pub(crate) origin: Vec3,
    /// The farthest of its points from the middle, `radius`.
    pub(crate) radius: f64,
    /// The leaves it is listed in, `markleafs`.
    pub(crate) markleafs: Vec<NodeId>,
}

impl Face {
    /// A face of `winding` with `from`'s other parts, `NewFaceFromFace` and `UpdateFaceSphere`.
    fn with_winding(from: &Self, winding: Winding) -> Self {
        let mut face = Self {
            winding,
            markleafs: Vec::new(),
            ..from.clone()
        };
        face.update_sphere();
        face
    }

    /// Its middle and radius found again, `UpdateFaceSphere`.
    fn update_sphere(&mut self) {
        self.origin = self.winding.center();
        self.radius = self
            .winding
            .points()
            .iter()
            .map(|p| {
                let d = sub(*p, self.origin);
                dot(d, d)
            })
            .fold(0.0, f64::max)
            .sqrt();
    }

    /// The face cut by `split`, `SplitFace` (QBSP-15.41): whole on a side its sphere is wholly
    /// on, else its polygon cut within `-epsilon`, on the plane kept in front.
    fn split(self, split: Plane) -> [Option<Self>; 2] {
        let d = split.distance_to(self.origin);
        if d > self.radius {
            return [Some(self), None];
        }
        if d < -self.radius {
            return [None, Some(self)];
        }
        match self.winding.clip(split, POINT_EPSILON, KeepOn::Front) {
            (Some(_), None) => [Some(self), None],
            (None, Some(_)) => [None, Some(self)],
            (Some(front), Some(back)) => [
                Some(Self::with_winding(&self, front)),
                Some(Self::with_winding(&self, back)),
            ],
            (None, None) => [None, None],
        }
    }
}

impl Tree {
    /// Face `id`.
    pub(crate) fn face(&self, id: FaceId) -> Option<&Face> {
        self.faces.get(id.0)
    }

    fn add_face(&mut self, face: Face) -> FaceId {
        self.faces.push(face);
        FaceId(self.faces.len().saturating_sub(1))
    }
}

impl Tree {
    /// Whether a leaf below `id` has a detail fence's window as its strongest visible contents,
    /// `FixupMarkFaces_ProcessCluster_HasDetailFence`.
    fn has_detail_fence(&self, id: NodeId) -> bool {
        self.leaves_below(id).into_iter().any(|leaf| {
            self.leaf_data(leaf)
                .is_some_and(|l| l.contents.visible().flags() & Contents::WINDOW != 0)
        })
    }

    /// The first leaf below `id` whose strongest visible contents are empty or a liquid,
    /// `FixupMarkFaces_ProcessCluster_FindStorageLeaf`.
    fn storage_leaf(&self, id: NodeId) -> Option<NodeId> {
        let usable = [
            Contents::EMPTY.flags(),
            Contents::LAVA,
            Contents::SLIME,
            Contents::WATER,
        ];
        self.leaves_below(id).into_iter().find(|&leaf| {
            self.leaf_data(leaf)
                .is_some_and(|l| usable.contains(&l.contents.visible().flags()))
        })
    }

    /// The fence faces of cluster `id` listed in the nearest leaves a viewer can be in,
    /// `FixupMarkFaces_ProcessCluster` (QBSP-15.42): every face listed in a leaf below whose
    /// strongest visible contents are a detail fence's; breadth first through the vis portals
    /// vis sees through to the first clusters with a usable leaf, each such leaf listing them
    /// too and its bounds and its parents' widened to hold them.
    fn fixup_cluster(&mut self, id: NodeId) {
        if !self.has_detail_fence(id) {
            return;
        }
        let mut faces: BTreeSet<FaceId> = BTreeSet::new();
        for leaf in self.leaves_below(id) {
            if let Some(l) = self.leaf_data(leaf)
                && l.contents.visible().flags() == Contents::WINDOW
            {
                faces.extend(l.markfaces.iter().copied());
            }
        }
        if faces.is_empty() {
            return;
        }
        let mut queue = vec![id];
        let mut front = 0_usize;
        let mut visited = BTreeSet::new();
        let mut storage = Vec::new();
        while let Some(&current) = queue.get(front) {
            front = front.saturating_add(1);
            visited.insert(current);
            if let Some(leaf) = self.storage_leaf(current) {
                storage.push(leaf);
                continue;
            }
            for portal in self.portals_of(current) {
                let Some(other) = self.across(portal, current) else {
                    continue;
                };
                if visited.contains(&other) || !self.vis_floods(portal) {
                    continue;
                }
                queue.push(other);
            }
        }
        let mut bounds = Bounds::default();
        for &face in &faces {
            if let Some(f) = self.face(face) {
                for &p in f.winding.points() {
                    bounds.add(p);
                }
            }
        }
        for leaf in storage {
            if let Some(l) = self.leaf_data_mut(leaf) {
                let mut listed: BTreeSet<FaceId> = l.markfaces.iter().copied().collect();
                listed.extend(faces.iter().copied());
                l.markfaces = listed.into_iter().collect();
            }
            let mut at = Some(leaf);
            while let Some(node) = at.and_then(|n| self.node_mut(n)) {
                node.bounds.union(&bounds);
                at = node.parent;
            }
        }
    }

    /// The detail fences' faces made visible from outside them, `FixupDetailFence`
    /// (QBSP-15.42): each cluster processed, a detail separator or a leaf not under one, once
    /// the tree has its vis portals.
    pub(crate) fn fixup_detail_fence(&mut self) {
        let mut stack = vec![Self::HEAD];
        while let Some(id) = stack.pop() {
            match self.split_of(id) {
                Some((_, [front, back], Separator::Structural)) => {
                    stack.push(back);
                    stack.push(front);
                }
                _ => self.fixup_cluster(id),
            }
        }
    }
}

/// What making faces needs at hand.
pub(crate) struct FaceMaker<'a> {
    pub(crate) map: &'a LoadedMap,
    pub(crate) originals: &'a [BspBrush],
    /// `-subdivide`: the most texels a face spans before it is cut, 240 by default.
    pub(crate) subdivide: f64,
}

impl FaceMaker<'_> {
    fn flags(&self, texinfo: TexinfoNum) -> SurfFlags {
        self.map.texinfos.flags(texinfo)
    }

    fn lmshift(&self, side: FaceRef) -> u32 {
        self.map
            .entities
            .get(side.entity.get())
            .map_or(MAX_LMSHIFT, |e| e.lmshift)
    }

    fn plane(&self, planenum: PlaneNum) -> Option<Plane> {
        self.map.planes.get(planenum).map(|p| p.plane())
    }

    /// Whether `face` is left out of the BSP, `ShouldOmitFace` (QBSP-15.42): not drawn, a
    /// hint, inside sky, or in detail wall leaves only.
    pub(crate) fn omits(&self, tree: &Tree, face: &Face) -> bool {
        let flags = self.flags(face.texinfo);
        if flags.has(SurfFlags::NODRAW) || flags.has(SurfFlags::HINT) || face.contents[0].is_sky() {
            return true;
        }
        !face.markleafs.is_empty()
            && face.markleafs.iter().all(|&l| {
                tree.leaf_data(l)
                    .is_some_and(|d| d.contents.is_detail_wall())
            })
    }

    /// The face for side `pside` of portal `id`, `FaceFromPortal` (QBSP-15.39): none without a
    /// side found there; else of the side's texinfo, on its positive plane turned to face
    /// `pside`, the portal's polygon reversed on the back, with the contents of the leaf on
    /// `pside` in front.
    fn face_from_portal(
        &self,
        tree: &Tree,
        id: super::portals::PortalId,
        pside: usize,
    ) -> Option<Face> {
        let p = tree.portal(id)?;
        let SideRef { brush, side } = (*p.sides.as_ref()?.get(pside)?)?;
        let side = self.originals.get(brush.get())?.sides.get(side)?;
        let source = side.source?;
        let planenum = PlaneNum::new(side.planenum.positive().get() | pside);
        let other = 1_usize.saturating_sub(pside);
        let winding = if pside == 1 {
            p.winding.flipped()
        } else {
            p.winding.clone()
        };
        let contents = |n: usize| {
            p.nodes
                .get(n)
                .and_then(|&node| tree.leaf_data(node))
                .map_or(Contents::EMPTY, |l| l.contents)
        };
        let mut face = Face {
            planenum,
            texinfo: side.texinfo,
            contents: [contents(pside), contents(other)],
            winding,
            original_side: source,
            origin: [0.0; AXES],
            radius: 0.0,
            markleafs: Vec::new(),
        };
        face.update_sphere();
        Some(face)
    }

    /// The faces of the tree, `MakeFaces` (QBSP-15.39 to QBSP-15.41): each leaf not any solid
    /// makes a face for its side of each of its portals on the portal's node, and each node's
    /// faces are merged and subdivided once its children's are made.
    pub(crate) fn make_faces(&self, tree: &mut Tree) {
        self.make_faces_below(tree, Tree::HEAD);
    }

    fn make_faces_below(&self, tree: &mut Tree, id: NodeId) {
        if let Some((_, [front, back], _)) = tree.split_of(id) {
            self.make_faces_below(tree, front);
            self.make_faces_below(tree, back);
            let faces: Vec<Face> = tree
                .node(id)
                .map(|n| {
                    n.faces
                        .iter()
                        .filter_map(|&f| tree.face(f).cloned())
                        .collect()
                })
                .unwrap_or_default();
            let merged = self.merge_list(faces);
            let subdivided: Vec<Face> =
                merged.into_iter().flat_map(|f| self.subdivide(f)).collect();
            let ids: Vec<FaceId> = subdivided.into_iter().map(|f| tree.add_face(f)).collect();
            if let Some(node) = tree.node_mut(id) {
                node.faces = ids;
            }
            return;
        }
        if tree.leaf_data(id).is_none_or(|l| l.contents.is_any_solid()) {
            return;
        }
        for portal in tree.portals_of(id) {
            let Some(p) = tree.portal(portal) else {
                continue;
            };
            let pside = usize::from(p.nodes[1] == id);
            let onnode = p.onnode;
            if let (Some(face), Some(onnode)) = (self.face_from_portal(tree, portal, pside), onnode)
            {
                let face = tree.add_face(face);
                if let Some(node) = tree.node_mut(onnode) {
                    node.faces.push(face);
                }
            }
        }
    }

    /// The two faces made one when they can be, `TryMerge` (QBSP-15.40): on one plane with one
    /// texinfo and lightmap scale, both or neither in front of liquid or sky, with an edge of
    /// one the reverse of an edge of the other within `QBSP_EQUAL_EPSILON`, and the polygon left
    /// convex within `CONTINUOUS_EPSILON`; a point where the edges continue in a line dropped.
    fn try_merge(&self, f1: &Face, f2: &Face) -> Option<Face> {
        let (w1, w2) = (f1.winding.points(), f2.winding.points());
        let (n1, n2) = (w1.len(), w2.len());
        if n1 == 0
            || n2 == 0
            || f1.planenum != f2.planenum
            || f1.texinfo != f2.texinfo
            || self.lmshift(f1.original_side) != self.lmshift(f2.original_side)
        {
            return None;
        }
        if f1.contents[0].is_liquid() != f2.contents[0].is_liquid()
            || f1.contents[0].is_sky() != f2.contents[0].is_sky()
        {
            return None;
        }
        let at = |w: &[Vec3], n: usize, k: usize| w.get(k.checked_rem(n)?).copied();
        let close = |a: Vec3, b: Vec3| a.iter().zip(b).all(|(x, y)| (x - y).abs() <= EQUAL_EPSILON);
        let (i, j, p1, p2) = (0..n1).find_map(|i| {
            let (p1, p2) = (at(w1, n1, i)?, at(w1, n1, i.saturating_add(1))?);
            (0..n2).find_map(|j| {
                let (p3, p4) = (at(w2, n2, j)?, at(w2, n2, j.saturating_add(1))?);
                (close(p1, p4) && close(p2, p3)).then_some((i, j, p1, p2))
            })
        })?;
        let plane_normal = self.plane(f1.planenum)?.normal;
        let back = at(w1, n1, i.saturating_add(n1).saturating_sub(1))?;
        let normal = normalize(cross(plane_normal, sub(p1, back)));
        let d = dot(sub(at(w2, n2, j.saturating_add(2))?, p1), normal);
        if d > CONTINUOUS_EPSILON {
            return None;
        }
        let keep1 = d < -CONTINUOUS_EPSILON;
        let normal = normalize(cross(
            plane_normal,
            sub(at(w1, n1, i.saturating_add(2))?, p2),
        ));
        let back = at(w2, n2, j.saturating_add(n2).saturating_sub(1))?;
        let d = dot(sub(back, p2), normal);
        if d > CONTINUOUS_EPSILON {
            return None;
        }
        let keep2 = d < -CONTINUOUS_EPSILON;
        let mut points = Vec::with_capacity(n1.saturating_add(n2));
        let start1 = if keep2 { 1 } else { 2 };
        let mut k = i.saturating_add(start1).checked_rem(n1)?;
        while k != i {
            points.push(*w1.get(k)?);
            k = k.saturating_add(1).checked_rem(n1)?;
        }
        let start2 = if keep1 { 1 } else { 2 };
        let mut l = j.saturating_add(start2).checked_rem(n2)?;
        while l != j {
            points.push(*w2.get(l)?);
            l = l.saturating_add(1).checked_rem(n2)?;
        }
        Some(Face::with_winding(f1, Winding::new(points)))
    }

    /// The faces merged, `MergeFaceList` (QBSP-15.40): each tried against those kept so far in
    /// order, a merge taking the place of both and being tried again from the first.
    fn merge_list(&self, faces: Vec<Face>) -> Vec<Face> {
        let mut out: Vec<Face> = Vec::new();
        for mut face in faces {
            let mut i = 0;
            while let Some(kept) = out.get(i) {
                match self.try_merge(&face, kept) {
                    Some(merged) => {
                        out.remove(i);
                        face = merged;
                        i = 0;
                    }
                    None => i = i.saturating_add(1),
                }
            }
            out.push(face);
        }
        out
    }

    /// The face cut to the lightmap's size, `SubdivideFace` (QBSP-15.41): a face not drawn, a
    /// hint or special is left whole; otherwise along s and then t, each piece wider than
    /// `-subdivide`, at most `255 << lmshift` with the shift at most 4, is cut 16 short of that
    /// from its low end, the low piece kept and the rest cut again.
    fn subdivide(&self, face: Face) -> Vec<Face> {
        let flags = self.flags(face.texinfo);
        if flags.has(SurfFlags::NODRAW)
            || flags.has(SurfFlags::HINT)
            || flags.has(SurfFlags::SPECIAL)
        {
            return vec![face];
        }
        let Some(texinfo) = self.map.texinfos.get(face.texinfo).copied() else {
            return vec![face];
        };
        let lmshift = self.lmshift(face.original_side).min(MAX_LMSHIFT);
        let blocks = 1_u32.checked_shl(lmshift).unwrap_or(1);
        let subdiv = self.subdivide.min(LIGHTMAP_BLOCKS * f64::from(blocks));
        let mut surfaces = vec![face];
        for axis in texinfo.vecs.iter().take(TEXTURE_AXES) {
            let [x, y, z, _] = axis.map(f64::from);
            let tmp = [x, y, z];
            let mut chopped: std::collections::VecDeque<Face> = std::collections::VecDeque::new();
            let mut queue: std::collections::VecDeque<Face> = surfaces.into();
            while let Some(f) = queue.pop_front() {
                let (mut mins, mut maxs) = (f64::MAX, -f64::MAX);
                for p in f.winding.points() {
                    let v = dot(*p, tmp);
                    mins = mins.min(v);
                    maxs = maxs.max(v);
                }
                if maxs.ceil() - mins.floor() <= subdiv {
                    chopped.push_back(f);
                    continue;
                }
                let (normal, v) = normalize_with_length(tmp);
                let plane = Plane {
                    normal,
                    dist: (mins + subdiv - SUBDIVIDE_MARGIN) / v,
                };
                let [front, back] = f.split(plane);
                if let Some(front) = front {
                    queue.push_back(front);
                }
                if let Some(back) = back {
                    chopped.push_front(back);
                }
            }
            surfaces = chopped.into();
        }
        surfaces
    }

    /// Each face listed in the leaves it touches, `MakeMarkFaces` (QBSP-15.42): a copy of each
    /// node's face cut down the child its plane faces into, each piece listing the face in the
    /// leaf it reaches.
    pub(crate) fn make_mark_faces(&self, tree: &mut Tree) {
        let mut stack = vec![Tree::HEAD];
        while let Some(id) = stack.pop() {
            let Some((_, children, _)) = tree.split_of(id) else {
                continue;
            };
            let faces = tree.node(id).map(|n| n.faces.clone()).unwrap_or_default();
            for face_id in faces {
                let Some(face) = tree.face(face_id).cloned() else {
                    continue;
                };
                let side = face.planenum.get() & 1;
                if let Some(&child) = children.get(side) {
                    self.add_mark_faces(tree, face_id, face, child);
                }
            }
            stack.push(children[1]);
            stack.push(children[0]);
        }
    }

    /// `face_id` listed in each leaf below `id` that `copy` reaches, `AddMarksurfaces_r`.
    fn add_mark_faces(&self, tree: &mut Tree, face_id: FaceId, copy: Face, id: NodeId) {
        let Some((planenum, children, _)) = tree.split_of(id) else {
            if let Some(leaf) = tree.leaf_data_mut(id) {
                leaf.markfaces.push(face_id);
            }
            if let Some(face) = tree.faces.get_mut(face_id.0) {
                face.markleafs.push(id);
            }
            return;
        };
        let Some(plane) = self.plane(planenum) else {
            return;
        };
        for (piece, child) in copy.split(plane).into_iter().zip(children) {
            if let Some(piece) = piece {
                self.add_mark_faces(tree, face_id, piece, child);
            }
        }
    }
}

#[cfg(test)]
mod tests;
