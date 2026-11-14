// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/portals.cc, ClusterContents, Portal_VisFlood, AddPortalToNodes, MakeHeadnodePortals, BaseWindingForNode, MakeNodePortal, SplitNodePortals, MakePortalsFromBuildportals, CalcNodeBounds, CalcTreeBounds_r, ClipNodePortalsToTree_r, MakeTreePortals_r, MakeTreePortals, FindPortalSide, MarkVisibleSides_r, MarkVisibleSides
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/outside.cc, MarkBrushSidesInvisible
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: include/common/polylib.hh, max_dist_off_plane

//! The tree's portals, ericw-tools' `MakeTreePortals` (QBSP-15.31 to QBSP-15.34): the polygons
//! between neighbouring leaves, from the six around the head down through each node, and the
//! brush sides each one shows.

use super::brush::{BspBrush, OriginalNum, VisibleFaces};
use super::game::Contents;
use super::map::{SIDESPACE, SideKind, SurfFlags, Texinfos};
use super::tree::{Leaf, NodeId, Separator, Tree};
use super::vec::{AXES, Bounds, BspPlane, Facing, POINT_EPSILON, Plane, Planes};
use super::winding::{self, KeepOn, Winding};

/// `BASE_WINDING_EPSILON`: how near a parent's plane a node's polygon is on it.
const BASE_WINDING_EPSILON: f64 = 0.001;
/// `SPLIT_WINDING_EPSILON`: how near a node's plane a portal is on it.
const SPLIT_WINDING_EPSILON: f64 = 0.001;
/// How near a node's portals a new portal's polygon is on them, a "magic number".
const NODE_PORTAL_EPSILON: f64 = 0.1;
/// How far off a portal's polygon a brush side's plane may be and still show on it.
const SIDE_ON_PORTAL: f64 = 0.1;
/// `EDGE_LENGTH_EPSILON`: a portal with fewer than three edges longer than this is tiny.
const EDGE_LENGTH_EPSILON: f64 = 0.2;

/// A portal's number in its [`Tree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PortalId(usize);

/// A brush side shown on a portal: an original brush of the hull and the side's place in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SideRef {
    pub(crate) brush: OriginalNum,
    pub(crate) side: usize,
}

/// Which portals are made: all of them, or those for vis, which stop at detail separators.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PortalType {
    Tree,
    Vis,
}

/// Whether the six portals around the head start the tree's portals; the vis portals start
/// from none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HeadPortals {
    Used,
    Unused,
}

/// A portal, `portal_t`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Portal {
    /// Its plane, facing its first node.
    pub(crate) plane: BspPlane,
    /// The node whose plane it is on; `None` for the six around the head.
    pub(crate) onnode: Option<NodeId>,
    /// Its polygon.
    pub(crate) winding: Winding,
    /// The node in front of it, then the one behind.
    pub(crate) nodes: [NodeId; 2],
    /// The brush side seen on each of its sides, once found, `sides` and `sidefound`.
    pub(crate) sides: Option<[Option<SideRef>; 2]>,
}

/// A portal being made, `buildportal_t`.
#[derive(Clone, Debug, PartialEq)]
struct BuildPortal {
    plane: BspPlane,
    onnode: Option<NodeId>,
    winding: Winding,
    nodes: [NodeId; 2],
}

impl Winding {
    /// How far its farthest point is off `plane`, `max_dist_off_plane`.
    pub(crate) fn max_dist_off_plane(&self, plane: Plane) -> f64 {
        self.points()
            .iter()
            .map(|p| plane.distance_to(*p).abs())
            .fold(0.0, f64::max)
    }
}

/// What making portals needs at hand.
pub(crate) struct Portalizer<'a> {
    pub(crate) planes: &'a Planes,
    pub(crate) extent: f64,
}

impl Portalizer<'_> {
    fn node_plane(&self, tree: &Tree, id: NodeId) -> Option<BspPlane> {
        let (planenum, _, _) = tree.split_of(id)?;
        self.planes.get(planenum).copied()
    }

    /// The six portals around the head, facing the outside leaf, `MakeHeadnodePortals`
    /// (QBSP-15.31): on the planes of the tree's box grown by [`SIDESPACE`], each turned to face
    /// its axis's positive side, the head in front of the minimum's and behind the maximum's,
    /// each square clipped by the other five within `-epsilon`, kept when on them.
    fn head_portals(&self, tree: &mut Tree) -> Vec<BuildPortal> {
        let grown = Bounds {
            mins: tree.bounds.mins.map(|c| c - SIDESPACE),
            maxs: tree.bounds.maxs.map(|c| c + SIDESPACE),
        };
        let outside = tree.push(Tree::leaf(
            None,
            Bounds::default(),
            Leaf {
                contents: Contents::new(Contents::SOLID),
                ..Leaf::default()
            },
        ));
        tree.outside = Some(outside);
        let mut planes = Vec::with_capacity(AXES * 2);
        let mut portals = Vec::with_capacity(AXES * 2);
        for j in 0..2 {
            for i in 0..AXES {
                let mut normal = [0.0; AXES];
                let dist = if j == 0 {
                    if let Some(n) = normal.get_mut(i) {
                        *n = 1.0;
                    }
                    grown.mins.get(i).copied().unwrap_or_default()
                } else {
                    if let Some(n) = normal.get_mut(i) {
                        *n = -1.0;
                    }
                    -grown.maxs.get(i).copied().unwrap_or_default()
                };
                let plane = Plane { normal, dist };
                let (positive, turned) = BspPlane::new(plane, Facing::Positive);
                planes.push(plane);
                portals.push(BuildPortal {
                    plane: positive,
                    onnode: None,
                    winding: Winding::from_plane(plane, self.extent).unwrap_or_default(),
                    nodes: if turned {
                        [outside, Tree::HEAD]
                    } else {
                        [Tree::HEAD, outside]
                    },
                });
            }
        }
        for (i, portal) in portals.iter_mut().enumerate() {
            for (j, plane) in planes.iter().enumerate() {
                if i == j {
                    continue;
                }
                if let Some(w) = portal.winding.clip(*plane, POINT_EPSILON, KeepOn::Front).0 {
                    portal.winding = w;
                }
            }
        }
        portals
    }

    /// Node `id`'s plane's square clipped to its side of every parent's plane,
    /// `BaseWindingForNode` (QBSP-15.32).
    fn base_winding(&self, tree: &Tree, id: NodeId) -> Option<Winding> {
        let mut w = Winding::from_plane(self.node_plane(tree, id)?.plane(), self.extent);
        let mut node = id;
        while let Some(parent) = tree.node(node).and_then(|n| n.parent) {
            let (Some(current), Some(plane), Some((_, children, _))) = (
                w.take(),
                self.node_plane(tree, parent),
                tree.split_of(parent),
            ) else {
                break;
            };
            let (front, back) = current.clip(plane.plane(), BASE_WINDING_EPSILON, KeepOn::No);
            w = if children[0] == node { front } else { back };
            node = parent;
        }
        w
    }

    /// The portal on node `id`'s plane between its children, `MakeNodePortal` (QBSP-15.32): its
    /// base polygon clipped to the inside of each portal around the node within 0.1; none when
    /// tiny.
    fn node_portal(&self, tree: &Tree, id: NodeId, around: &[BuildPortal]) -> Option<BuildPortal> {
        let mut w = self.base_winding(tree, id);
        for p in around {
            let current = w.take()?;
            let plane = if p.nodes[0] == id {
                p.plane.plane()
            } else {
                p.plane.plane().flipped()
            };
            w = current.clip(plane, NODE_PORTAL_EPSILON, KeepOn::No).0;
        }
        let w = w.filter(|w| !w.is_tiny(EDGE_LENGTH_EPSILON))?;
        let (_, children, _) = tree.split_of(id)?;
        Some(BuildPortal {
            plane: self.node_plane(tree, id)?,
            onnode: Some(id),
            winding: w,
            nodes: children,
        })
    }

    /// The portals around node `id` moved or cut onto its children, `SplitNodePortals`
    /// (QBSP-15.32): each cut by the node's plane within 0.001, kept whole in front when on it,
    /// a tiny part dropped.
    fn split_node_portals(
        &self,
        tree: &Tree,
        id: NodeId,
        portals: Vec<BuildPortal>,
    ) -> [Vec<BuildPortal>; 2] {
        let mut out = [Vec::new(), Vec::new()];
        let (Some(plane), Some((_, [f, b], _))) = (self.node_plane(tree, id), tree.split_of(id))
        else {
            return out;
        };
        for mut p in portals {
            let front_side = p.nodes[0] == id;
            let other = if front_side { p.nodes[1] } else { p.nodes[0] };
            let (front, back) = p
                .winding
                .clip(plane.plane(), SPLIT_WINDING_EPSILON, KeepOn::Front);
            let tiny = |w: Option<Winding>| w.filter(|w| !w.is_tiny(EDGE_LENGTH_EPSILON));
            let (front, back) = (tiny(front), tiny(back));
            let place = |child: NodeId| {
                if front_side {
                    [child, other]
                } else {
                    [other, child]
                }
            };
            match (front, back) {
                (None, None) => {}
                (None, Some(back)) => {
                    p.winding = back;
                    p.nodes = place(b);
                    out[1].push(p);
                }
                (Some(front), None) => {
                    p.winding = front;
                    p.nodes = place(f);
                    out[0].push(p);
                }
                (Some(front), Some(back)) => {
                    let back_part = BuildPortal {
                        winding: back,
                        nodes: place(b),
                        ..p.clone()
                    };
                    p.winding = front;
                    p.nodes = place(f);
                    out[0].push(p);
                    out[1].push(back_part);
                }
            }
        }
        out
    }

    /// Whether `id` is where portals stop: a leaf, or a detail separator for vis.
    fn stops(tree: &Tree, id: NodeId, kind: PortalType) -> bool {
        match tree.split_of(id) {
            None => true,
            Some((_, _, separator)) => kind == PortalType::Vis && separator == Separator::Detail,
        }
    }

    /// `portals` on node `id` pushed down to its leaves, `ClipNodePortalsToTree_r` (QBSP-15.32).
    fn clip_to_tree(
        &self,
        tree: &Tree,
        id: NodeId,
        kind: PortalType,
        portals: Vec<BuildPortal>,
    ) -> Vec<BuildPortal> {
        if portals.is_empty() || Self::stops(tree, id, kind) {
            return portals;
        }
        let Some((_, [f, b], _)) = tree.split_of(id) else {
            return portals;
        };
        let [front, back] = self.split_node_portals(tree, id, portals);
        let mut out = self.clip_to_tree(tree, f, kind, front);
        out.extend(self.clip_to_tree(tree, b, kind, back));
        out
    }

    /// The portals of node `id` and below from those around it, `MakeTreePortals_r`
    /// (QBSP-15.32): the node's own portal made first, those around split onto its children,
    /// each child portalized, the front first, then the node's portal pushed down the front
    /// child and then the back.
    fn tree_portals(
        &self,
        tree: &Tree,
        id: NodeId,
        kind: PortalType,
        around: Vec<BuildPortal>,
    ) -> Vec<BuildPortal> {
        if Self::stops(tree, id, kind) {
            return around;
        }
        let Some((_, [f, b], _)) = tree.split_of(id) else {
            return around;
        };
        let node_portal = self.node_portal(tree, id, &around);
        let [front, back] = self.split_node_portals(tree, id, around);
        let mut out = self.tree_portals(tree, f, kind, front);
        out.extend(self.tree_portals(tree, b, kind, back));
        if let Some(p) = node_portal {
            let half = self.clip_to_tree(tree, f, kind, vec![p]);
            out.extend(self.clip_to_tree(tree, b, kind, half));
        }
        out
    }

    /// The bounds of each node from its portals, `CalcTreeBounds_r` (QBSP-15.33): a leaf's
    /// around its portals' points, a node's around its children's; one with no volume on the
    /// first axis takes its parent's minimum as both corners.
    fn tree_bounds(tree: &mut Tree, id: NodeId) {
        let bounds = match tree.split_of(id) {
            Some((_, [f, b], _)) => {
                Self::tree_bounds(tree, f);
                Self::tree_bounds(tree, b);
                let mut bounds = tree.node(f).map(|n| n.bounds).unwrap_or_default();
                if let Some(back) = tree.node(b) {
                    bounds.union(&back.bounds);
                }
                bounds
            }
            None => {
                let mut bounds = Bounds::default();
                for &portal in tree
                    .node(id)
                    .map(|n| n.portals.as_slice())
                    .unwrap_or_default()
                {
                    if let Some(p) = tree.portals.get(portal.0) {
                        for &point in p.winding.points() {
                            bounds.add(point);
                        }
                    }
                }
                bounds
            }
        };
        let parent_mins = tree
            .node(id)
            .and_then(|n| n.parent)
            .and_then(|p| tree.node(p))
            .map(|p| p.bounds.mins);
        if let Some(node) = tree.node_mut(id) {
            node.bounds = bounds;
            if node.bounds.mins[0] >= node.bounds.maxs[0]
                && let Some(mins) = parent_mins
            {
                node.bounds = Bounds { mins, maxs: mins };
            }
        }
    }

    /// The tree's portals, `MakeTreePortals` (QBSP-15.31 to QBSP-15.33): any it had freed, the
    /// head's six made, every node's made and pushed to the leaves, each put on its two nodes,
    /// and every node's bounds found from them.
    pub(crate) fn make_tree_portals(&self, tree: &mut Tree) {
        self.make_portals(tree, PortalType::Tree);
        Self::tree_bounds(tree, Tree::HEAD);
    }

    /// The tree's portals of `kind`, its old ones freed.
    pub(crate) fn make_portals(&self, tree: &mut Tree, kind: PortalType) {
        self.make_portals_from(tree, kind, HeadPortals::Used);
    }

    /// The tree's portals of `kind` from the head's six or from none, its old ones freed.
    pub(crate) fn make_portals_from(&self, tree: &mut Tree, kind: PortalType, head: HeadPortals) {
        tree.portals.clear();
        for node in &mut tree.nodes {
            node.portals.clear();
        }
        if let Some(outside) = tree.outside.take() {
            tree.nodes.truncate(outside.get());
        }
        let mut around = self.head_portals(tree);
        if head == HeadPortals::Unused {
            around.clear();
        }
        let built = self.tree_portals(tree, Tree::HEAD, kind, around);
        for p in built {
            let id = PortalId(tree.portals.len());
            for node in p.nodes {
                if let Some(n) = tree.node_mut(node) {
                    n.portals.push(id);
                }
            }
            tree.portals.push(Portal {
                plane: p.plane,
                onnode: p.onnode,
                winding: p.winding,
                nodes: p.nodes,
                sides: None,
            });
        }
    }
}

impl Tree {
    /// Portal `id`.
    pub(crate) fn portal(&self, id: PortalId) -> Option<&Portal> {
        self.portals.get(id.0)
    }

    /// Node `id`'s portals in `node_t::portals` order, the last added first.
    pub(crate) fn portals_of(&self, id: NodeId) -> Vec<PortalId> {
        self.node(id)
            .map(|n| n.portals.iter().rev().copied().collect())
            .unwrap_or_default()
    }

    /// The node across portal `portal` from `id`.
    pub(crate) fn across(&self, portal: PortalId, id: NodeId) -> Option<NodeId> {
        let p = self.portal(portal)?;
        Some(if p.nodes[0] == id {
            p.nodes[1]
        } else {
            p.nodes[0]
        })
    }

    /// The contents of node `id` and everything below, `ClusterContents`.
    pub(crate) fn cluster_contents(&self, id: NodeId) -> Contents {
        match self.split_of(id) {
            Some((_, [f, b], _)) => self.cluster_contents(f).cluster(self.cluster_contents(b)),
            None => self.leaf_data(id).map_or(Contents::EMPTY, |l| l.contents),
        }
    }

    /// Whether vis can see through portal `id`, `Portal_VisFlood`: never the six around the
    /// head.
    pub(crate) fn vis_floods(&self, id: PortalId) -> bool {
        let Some(p) = self.portal(id) else {
            return false;
        };
        p.onnode.is_some()
            && Contents::portal_can_see_through(
                self.cluster_contents(p.nodes[0]),
                self.cluster_contents(p.nodes[1]),
            )
    }
}

/// What marking the visible sides needs at hand: the hull's brushes as loaded.
pub(crate) struct SideMarker<'a> {
    pub(crate) planes: &'a Planes,
    pub(crate) texinfos: &'a Texinfos,
    pub(crate) originals: &'a [BspBrush],
}

impl SideMarker<'_> {
    /// Every map face of `brushes` unseen but hints, `MarkBrushSidesInvisible` (QBSP-15.34).
    pub(crate) fn hide_all(&self, brushes: &[BspBrush], visible: &mut VisibleFaces) {
        for side in brushes.iter().flat_map(|b| &b.sides) {
            if let Some(face) = side.source {
                if self.texinfos.flags(side.texinfo).has(SurfFlags::HINT) {
                    visible.show(face);
                } else {
                    visible.hide(face);
                }
            }
        }
    }

    /// The brush sides portal `p` shows on each of its sides, `FindPortalSide` (QBSP-15.34):
    /// none when no contents change across it; else for each side's leaf, its original brushes
    /// last first, each that gives a face for the change outside or inside, each side of it not
    /// a bevel and within 0.1 of the portal: one on the portal's node's plane taken first, for
    /// the side it faces out to and, inside, the other, and the brush's search ended; otherwise
    /// the side whose plane is nearest the node's, by its normal's dot product kept as a
    /// `float`.
    fn portal_sides(&self, tree: &Tree, p: &Portal) -> Option<[Option<SideRef>; 2]> {
        let contents = |n: NodeId| tree.leaf_data(n).map_or(Contents::EMPTY, |l| l.contents);
        let visible = Contents::portal_visible(contents(p.nodes[0]), contents(p.nodes[1]));
        if visible.is_empty() {
            return None;
        }
        let onnode = p.onnode?;
        let (node_planenum, _, _) = tree.split_of(onnode)?;
        let node_plane = self.planes.get(node_planenum)?.plane();
        let mut best: [Option<SideRef>; 2] = [None, None];
        let mut exact: [Option<SideRef>; 2] = [None, None];
        let mut best_dot = 0.0_f32;
        for (j, &node) in p.nodes.iter().enumerate() {
            let other = 1_usize.saturating_sub(j);
            let Some(leaf) = tree.leaf_data(node) else {
                continue;
            };
            for &original in leaf.original_brushes.iter().rev() {
                let Some(brush) = self.originals.get(original.get()) else {
                    continue;
                };
                let outside =
                    Contents::portal_generates_face(visible, brush.contents, winding::Side::Front);
                let inside =
                    Contents::portal_generates_face(visible, brush.contents, winding::Side::Back);
                if !outside && !inside {
                    continue;
                }
                for (index, side) in brush.sides.iter().enumerate() {
                    if side.kind == SideKind::Bevel {
                        continue;
                    }
                    let Some(plane) = self.planes.get(side.planenum) else {
                        continue;
                    };
                    if p.winding.max_dist_off_plane(plane.plane()) > SIDE_ON_PORTAL {
                        continue;
                    }
                    let here = SideRef {
                        brush: original,
                        side: index,
                    };
                    if side.planenum.positive() == node_planenum {
                        for (wanted, at) in [(outside, other), (inside, j)] {
                            if let (true, Some(slot)) = (wanted, exact.get_mut(at))
                                && slot.is_none()
                            {
                                *slot = Some(here);
                            }
                        }
                        break;
                    }
                    let Some(positive) = self.planes.get(side.planenum.positive()) else {
                        continue;
                    };
                    let dot = super::vec::dot(node_plane.normal, positive.plane().normal).abs();
                    if dot > f64::from(best_dot) {
                        #[expect(
                            clippy::cast_possible_truncation,
                            reason = "spec QBSP-15.34: `bestdot` is a `float`"
                        )]
                        let narrowed = dot as f32;
                        best_dot = narrowed;
                        for (wanted, at) in [(outside, other), (inside, j)] {
                            if let (true, Some(slot)) = (wanted, best.get_mut(at)) {
                                *slot = Some(here);
                            }
                        }
                    }
                }
            }
        }
        for (b, e) in best.iter_mut().zip(exact) {
            if e.is_some() {
                *b = e;
            }
        }
        Some(best)
    }

    /// The map faces some portal shows, `MarkVisibleSides` (QBSP-15.34): every face of `brushes`
    /// unseen but hints, then for each leaf that is not empty, each of its portals but the
    /// head's has its sides found once and each with a map face marks it seen.
    pub(crate) fn mark_visible_sides(
        &self,
        tree: &mut Tree,
        brushes: &[BspBrush],
        visible: &mut VisibleFaces,
    ) {
        self.hide_all(brushes, visible);
        for leaf in tree.leaves_below(Tree::HEAD) {
            if tree.leaf_data(leaf).is_none_or(|l| l.contents.is_empty()) {
                continue;
            }
            for id in tree.portals_of(leaf) {
                let Some(p) = tree.portal(id) else {
                    continue;
                };
                if p.onnode.is_none() {
                    continue;
                }
                let sides = match p.sides {
                    Some(sides) => sides,
                    None => {
                        let found = self.portal_sides(tree, p);
                        let sides = found.unwrap_or([None, None]);
                        if found.is_some()
                            && let Some(p) = tree.portals.get_mut(id.0)
                        {
                            p.sides = Some(sides);
                        }
                        sides
                    }
                };
                for side in sides.into_iter().flatten() {
                    if let Some(face) = self
                        .originals
                        .get(side.brush.get())
                        .and_then(|b| b.sides.get(side.side))
                        .and_then(|s| s.source)
                    {
                        visible.show(face);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
