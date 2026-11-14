// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/tree.cc, ConvertNodeToLeaf, PruneNodes_R, PruneNodes
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/brushbsp.cc, BrushFromBounds, BrushVolume, BoxOnPlaneSide, TestBrushToPlanenum, LeafNode, BrushMostlyOnSide, SplitBrush, CheckSplitBrush, CheckPlaneAgainstVolume, DivideBounds, SplitPlaneMetric, ChooseMidPlaneFromList, SelectSplitPlane, SplitBrushList, BuildTree_r, BrushBSP

//! The brush BSP, ericw-tools' `BrushBSP` after Quake II's (QBSP-15.25 to QBSP-15.29): a tree
//! built from the brushes themselves, each node's plane a brush side's chosen by how evenly it
//! divides the node or by how few sides it splits, each brush cut along it, and each leaf the
//! contents of the brushes left in it.

use super::brush::{BspBrush, OriginalNum, Side, SideMarks, VisibleFaces};
use super::faces::{Face, FaceId};
use super::game::Contents;
use super::map::{EntityNum, SIDESPACE, SideKind, SurfFlags, Texinfos};
use super::portals::{Portal, PortalId};
use super::vec::{AXES, Bounds, BspPlane, NORMAL_EPSILON, POINT_EPSILON, Plane, PlaneNum, Planes};
use super::winding::{self, KeepOn, ON_EPSILON, Winding};

/// `PLANESIDE_EPSILON`: a box reaching no further than this past a plane is not across it.
const PLANESIDE_EPSILON: f64 = 0.001;
/// How far a brush must reach past a plane on both sides to be split by it, `SplitBrush`'s
/// commented `PLANESIDE_EPSILON` of 0.1.
const SPLIT_EPSILON: f64 = 0.1;
/// A split's polygon with fewer than three edges longer than this does not split the brush.
const TINY_SPLIT: f64 = 0.02;
/// `-microvolume`'s default: a cut piece of less volume is dropped.
const MICROVOLUME: f64 = 0.0;
/// `-maxnodesize`'s default: an automatic split of a node larger than this on an axis is the
/// cheap one.
const MAX_NODE_SIZE: f64 = 1024.0;
/// A brush reaching less than this past a plane is an epsilon brush, which the plane should not
/// cut.
const EPSILON_BRUSH: f64 = 1.0;
/// The fewest sides a sound brush has.
const MIN_SIDES: usize = 3;
/// A tetrahedron's volume is a third of its base times its height.
const THIRD: f64 = 3.0;
/// The score of a plane each brush that lies on it adds.
const FACING_SCORE: i64 = 5;
/// The score each side a plane splits takes away.
const SPLIT_SCORE: i64 = 5;
/// The score an axial plane adds.
const AXIAL_SCORE: i64 = 5;
/// The score each epsilon brush takes away.
const EPSILON_BRUSH_SCORE: i64 = 1000;
/// The score of a plane splitting a hint side when not a hint itself.
const SPLITS_HINT_SCORE: i64 = -9_999_999;
/// The score to beat.
const NO_SCORE: i64 = -99_999;
/// The split choice's passes: visible structural, invisible structural, visible detail,
/// invisible detail.
const PASSES: usize = 4;
/// For each axis, the next two, `(a + 1) % 3` and `(a + 2) % 3`.
const OTHER_AXES: [(usize, usize); AXES] = [(1, 2), (2, 0), (0, 1)];
/// The first pass of detail sides.
const DETAIL_PASS: usize = 2;

/// Which sides of a plane a brush is on, with whether it lies on the plane, `PSIDE_*`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlaneSides(u8);

impl PlaneSides {
    /// In front.
    pub(crate) const FRONT: Self = Self(1);
    /// Behind.
    pub(crate) const BACK: Self = Self(1 << 1);
    /// Across the plane.
    pub(crate) const BOTH: Self = Self(1 | 1 << 1);
    /// One of its sides is on the plane, `PSIDE_FACING`.
    const FACING: u8 = 1 << 2;

    /// Whether every side of `other` is among these.
    pub(crate) const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0 && other.0 != 0
    }

    /// Whether a side of the brush lies on the plane.
    pub(crate) const fn is_facing(self) -> bool {
        self.0 & Self::FACING != 0
    }

    const fn facing(self) -> Self {
        Self(self.0 | Self::FACING)
    }

    const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// How the tree chooses its planes, `tree_split_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeSplit {
    /// Each node's choice by its size: the cheap way for a large node.
    Auto,
    /// The plane that splits fewest sides, `PRECISE`.
    Precise,
    /// The plane that halves the node best, `FAST`.
    Fast,
}

/// A node's number in its [`Tree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NodeId(usize);

impl NodeId {
    /// Its place in the tree's nodes.
    pub(crate) const fn get(self) -> usize {
        self.0
    }
}

/// Whether a node's plane is a detail side's, which vis need not know.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Separator {
    #[default]
    Structural,
    Detail,
}

/// A leaf, `leafdata_t`.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Leaf {
    /// The contents of the brushes in it, combined.
    pub(crate) contents: Contents,
    /// The hull brushes the pieces in it were cut from.
    pub(crate) original_brushes: Vec<OriginalNum>,
    /// The fill's steps from the void to it, `outside_distance`; `None` when not reached.
    pub(crate) outside_distance: Option<u32>,
    /// The fill's steps from an occupied leaf to it, 1 for the leaf itself, `occupied`; 0 when
    /// not reached.
    pub(crate) occupied: u32,
    /// The entity in it, `occupant`.
    pub(crate) occupant: Option<EntityNum>,
    /// The faces it touches, `markfaces`.
    pub(crate) markfaces: Vec<FaceId>,
}

/// What a node is.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NodeKind {
    /// A node split by a plane, `nodedata_t`: its front child, then its back.
    Split {
        planenum: PlaneNum,
        children: [NodeId; 2],
        separator: Separator,
    },
    /// A leaf.
    Leaf(Leaf),
}

/// A node, `node_t`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Node {
    /// The box it covers.
    pub(crate) bounds: Bounds,
    /// The node it is a child of.
    pub(crate) parent: Option<NodeId>,
    /// The space it covers as a brush, until it is split.
    pub(crate) volume: Option<BspBrush>,
    /// What it is.
    pub(crate) kind: NodeKind,
    /// The portals on it, in the order they were added; `node_t::portals` is the reverse.
    pub(crate) portals: Vec<PortalId>,
    /// The faces on its plane, `facelist`.
    pub(crate) faces: Vec<FaceId>,
}

/// A tree, `tree_t`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tree {
    /// Its nodes; [`Tree::head`] is the first.
    pub(crate) nodes: Vec<Node>,
    /// The box around its brushes.
    pub(crate) bounds: Bounds,
    /// Its portals, `tree_t::portals`.
    pub(crate) portals: Vec<Portal>,
    /// The solid leaf beyond the head's portals, `outside_node`, once portals are made.
    pub(crate) outside: Option<NodeId>,
    /// Every face made, those on nodes and those merged away.
    pub(crate) faces: Vec<Face>,
}

impl Tree {
    /// The head node.
    pub(crate) const HEAD: NodeId = NodeId(0);

    /// Node `id`.
    pub(crate) fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.0)
    }

    pub(crate) fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.0)
    }

    pub(crate) fn push(&mut self, node: Node) -> NodeId {
        self.nodes.push(node);
        NodeId(self.nodes.len().saturating_sub(1))
    }

    pub(crate) fn leaf(parent: Option<NodeId>, bounds: Bounds, leaf: Leaf) -> Node {
        Node {
            bounds,
            parent,
            volume: None,
            kind: NodeKind::Leaf(leaf),
            portals: Vec::new(),
            faces: Vec::new(),
        }
    }

    /// Leaf `id`'s data.
    pub(crate) fn leaf_data(&self, id: NodeId) -> Option<&Leaf> {
        match &self.node(id)?.kind {
            NodeKind::Leaf(leaf) => Some(leaf),
            NodeKind::Split { .. } => None,
        }
    }

    /// Leaf `id`'s data, to change.
    pub(crate) fn leaf_data_mut(&mut self, id: NodeId) -> Option<&mut Leaf> {
        match &mut self.node_mut(id)?.kind {
            NodeKind::Leaf(leaf) => Some(leaf),
            NodeKind::Split { .. } => None,
        }
    }

    /// Node `id`'s plane and children, `None` for a leaf.
    pub(crate) fn split_of(&self, id: NodeId) -> Option<(PlaneNum, [NodeId; 2], Separator)> {
        match self.node(id)?.kind {
            NodeKind::Split {
                planenum,
                children,
                separator,
            } => Some((planenum, children, separator)),
            NodeKind::Leaf(_) => None,
        }
    }

    /// Every leaf below `id`, front first, the order `_r` walks take.
    pub(crate) fn leaves_below(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(at) = stack.pop() {
            match self.split_of(at) {
                Some((_, [front, back], _)) => {
                    stack.push(back);
                    stack.push(front);
                }
                None => out.push(at),
            }
        }
        out
    }
}

impl Bounds {
    /// Which sides of `plane` the box is on, `BoxOnPlaneSide` (QBSP-15.26): an axial plane's by
    /// its axis's extent beyond [`PLANESIDE_EPSILON`] of the distance; another's by the corners
    /// leading and trailing along the normal.
    fn plane_sides(&self, plane: &BspPlane) -> PlaneSides {
        let p = plane.plane();
        let mut sides = PlaneSides::default();
        if plane.kind().is_axial() {
            let axis = plane.kind().axis();
            let (lo, hi) = (
                self.mins.get(axis).copied().unwrap_or_default(),
                self.maxs.get(axis).copied().unwrap_or_default(),
            );
            if hi > p.dist + PLANESIDE_EPSILON {
                sides = sides.union(PlaneSides::FRONT);
            }
            if lo < p.dist - PLANESIDE_EPSILON {
                sides = sides.union(PlaneSides::BACK);
            }
            return sides;
        }
        let mut corners = [[0.0; AXES]; 2];
        for (i, n) in p.normal.iter().enumerate() {
            let (lo, hi) = (
                self.mins.get(i).copied().unwrap_or_default(),
                self.maxs.get(i).copied().unwrap_or_default(),
            );
            let (lead, trail) = if *n < 0.0 { (lo, hi) } else { (hi, lo) };
            if let [Some(c0), Some(c1)] = corners.each_mut().map(|c| c.get_mut(i)) {
                *c0 = lead;
                *c1 = trail;
            }
        }
        let [lead, trail] = corners.map(|c| p.distance_to(c));
        if lead >= PLANESIDE_EPSILON {
            sides = PlaneSides::FRONT;
        }
        if trail < PLANESIDE_EPSILON {
            sides = sides.union(PlaneSides::BACK);
        }
        sides
    }

    /// Its volume, `aabb3d::volume`.
    fn volume(&self) -> f64 {
        self.mins
            .iter()
            .zip(self.maxs)
            .map(|(lo, hi)| hi - lo)
            .product()
    }

    /// The boxes holding its parts in front of and behind `split`, `DivideBounds`
    /// (QBSP-15.27): an axial plane cuts it at its distance; another narrows each axis it is
    /// not parallel to by where its edges along that axis cross the plane, the two boxes
    /// overlapping.
    fn divide(&self, split: &BspPlane) -> [Self; 2] {
        let (mut front, mut back) = (*self, *self);
        let p = split.plane();
        if split.kind().is_axial() {
            let axis = split.kind().axis();
            if let (Some(f), Some(b)) = (front.mins.get_mut(axis), back.maxs.get_mut(axis)) {
                *f = p.dist;
                *b = p.dist;
            }
            return [front, back];
        }
        let corners = [self.mins, self.maxs];
        for a in 0..AXES {
            let n = p.normal.get(a).copied().unwrap_or_default();
            if n.abs() < NORMAL_EPSILON {
                continue;
            }
            let Some(&(b, c)) = OTHER_AXES.get(a) else {
                continue;
            };
            let at = |corner: usize, axis: usize| {
                corners
                    .get(corner)
                    .and_then(|v| v.get(axis))
                    .copied()
                    .unwrap_or_default()
            };
            let (lo_a, hi_a) = (at(0, a), at(1, a));
            let (mut split_mins, mut split_maxs) = (hi_a, lo_a);
            for i in 0..2 {
                for j in 0..2 {
                    let mut corner = [0.0; AXES];
                    for (axis, from) in [(b, i), (c, j)] {
                        if let Some(v) = corner.get_mut(axis) {
                            *v = at(from, axis);
                        }
                    }
                    let mut end = |value: f64| {
                        if let Some(v) = corner.get_mut(a) {
                            *v = value;
                        }
                        p.distance_to(corner)
                    };
                    let (dist1, dist2) = (end(lo_a), end(hi_a));
                    let mid = (hi_a - lo_a) * (dist1 / (dist1 - dist2)) + lo_a;
                    split_mins = mid.min(split_mins).max(lo_a);
                    split_maxs = mid.max(split_maxs).min(hi_a);
                }
            }
            let (low, high) = if n > 0.0 {
                (&mut front.mins, &mut back.maxs)
            } else {
                (&mut back.mins, &mut front.maxs)
            };
            if let (Some(l), Some(h)) = (low.get_mut(a), high.get_mut(a)) {
                *l = split_mins;
                *h = split_maxs;
            }
        }
        [front, back]
    }

    /// How unevenly `plane` divides the box, `SplitPlaneMetric` (QBSP-15.27): the difference of
    /// its parts' volumes.
    fn split_metric(&self, plane: &BspPlane) -> f64 {
        let [front, back] = self.divide(plane);
        (front.volume() - back.volume()).abs()
    }
}

/// How the brushes' sides test against a plane, `TestBrushToPlanenum`'s counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Tested {
    /// The sides of the plane the brush is on.
    sides: PlaneSides,
    /// The visible sides it splits.
    splits: i64,
    /// Of those, the hint sides.
    hint_splits: usize,
    /// 1 when the brush reaches less than a unit past the plane, else 0.
    epsilon_brushes: i64,
}

/// What a brush cut by a plane leaves, `SplitBrush`.
#[derive(Clone, Debug, PartialEq)]
enum Cut {
    /// The whole brush is on this side.
    Whole(winding::Side),
    /// The parts in front and behind, each dropped when too small.
    Parts(Box<[Option<BspBrush>; 2]>),
}

/// How a cut piece's bounds are checked: `SplitBrush`'s every axis, or `CheckSplitBrush`'s
/// stack brush, whose `update_bounds` tests the first axis three times.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PieceCheck {
    EveryAxis,
    FirstAxis,
}

/// What building and cutting brushes needs at hand.
pub(crate) struct Cutter<'a> {
    pub(crate) planes: &'a Planes,
    pub(crate) extent: f64,
}

impl Cutter<'_> {
    fn plane(&self, num: PlaneNum) -> Option<Plane> {
        self.planes.get(num).map(BspPlane::plane)
    }

    /// The brush's volume, `BrushVolume` (QBSP-15.25): the tetrahedrons from the first point of
    /// its last side with a polygon to every side's polygon, summed.
    pub(crate) fn volume(&self, brush: &BspBrush) -> f64 {
        let Some(corner) = brush
            .sides
            .iter()
            .rev()
            .find_map(|s| s.winding.points().first().copied())
        else {
            return 0.0;
        };
        let total: f64 = brush
            .sides
            .iter()
            .filter(|s| !s.winding.is_empty())
            .filter_map(|s| {
                let plane = self.plane(s.planenum)?;
                Some(-plane.distance_to(corner) * s.winding.area())
            })
            .sum();
        total / THIRD
    }

    /// Which side of `plane` most of the brush is on, `BrushMostlyOnSide`: the side of its
    /// point farthest from it, the front on ties.
    fn mostly_on_side(brush: &BspBrush, plane: Plane) -> winding::Side {
        let mut max = 0.0;
        let mut side = winding::Side::Front;
        for p in brush.sides.iter().flat_map(|s| s.winding.points()) {
            let d = plane.distance_to(*p);
            if d > max {
                max = d;
                side = winding::Side::Front;
            }
            if -d > max {
                max = -d;
                side = winding::Side::Back;
            }
        }
        side
    }

    /// Whether a cut piece is sound (QBSP-15.25): three sides at least and bounds within the
    /// world's extent.
    fn piece_is_sound(&self, piece: &mut BspBrush, check: PieceCheck) -> bool {
        if piece.sides.len() < MIN_SIDES {
            return false;
        }
        let bounded = match check {
            PieceCheck::EveryAxis => piece.update_bounds(self.extent),
            PieceCheck::FirstAxis => {
                let mut bounds = Bounds::default();
                for side in &piece.sides {
                    bounds.union(&side.winding.bounds());
                }
                piece.bounds = bounds;
                let (lo, hi) = (bounds.mins[0], bounds.maxs[0]);
                lo > -self.extent && hi < self.extent && lo < self.extent && hi > -self.extent
            }
        };
        bounded
            && piece
                .bounds
                .mins
                .iter()
                .zip(piece.bounds.maxs)
                .all(|(lo, hi)| *lo >= -self.extent && hi <= self.extent)
    }

    /// `brush` cut by plane `planenum`, `SplitBrush` (QBSP-15.25): whole on one side when it
    /// reaches less than [`SPLIT_EPSILON`] past the plane on the other, or when the plane's
    /// polygon inside it is none or tiny, then on the side most of it is; otherwise each side's
    /// polygon cut, both parts given the plane's polygon as a skip side on the node, whole on
    /// one side when the other's part is unsound, and a part of too little volume dropped.
    fn cut(&self, brush: &BspBrush, planenum: PlaneNum, check: PieceCheck) -> Cut {
        let Some(split) = self.plane(planenum) else {
            return Cut::Whole(winding::Side::Front);
        };
        let (mut d_front, mut d_back) = (0.0_f64, 0.0_f64);
        for p in brush.sides.iter().flat_map(|s| s.winding.points()) {
            let d = split.distance_to(*p);
            if d > 0.0 && d > d_front {
                d_front = d;
            }
            if d < 0.0 && d < d_back {
                d_back = d;
            }
        }
        if d_front < SPLIT_EPSILON {
            return Cut::Whole(winding::Side::Back);
        }
        if d_back > -SPLIT_EPSILON {
            return Cut::Whole(winding::Side::Front);
        }
        let mut w = Winding::from_plane(split, self.extent);
        for side in &brush.sides {
            let (Some(current), Some(plane)) = (w.take(), self.plane(side.planenum)) else {
                break;
            };
            w = current.clipped(plane, ON_EPSILON, KeepOn::No, winding::Part::Back);
        }
        let Some(mid) = w.filter(|w| !w.is_tiny(TINY_SPLIT)) else {
            return Cut::Whole(Self::mostly_on_side(brush, split));
        };
        let mut parts = [(); 2].map(|()| BspBrush {
            sides: Vec::with_capacity(brush.sides.len().saturating_add(1)),
            contents: brush.contents,
            bounds: brush.bounds,
            source: brush.source,
            sort_key: brush.sort_key,
            original: brush.original,
        });
        for side in &brush.sides {
            let (front, back) = side.winding.clip(split, 0.0, KeepOn::No);
            for (part, piece) in parts.iter_mut().zip([front, back]) {
                if let Some(piece) = piece {
                    part.sides.push(Side {
                        planenum: side.planenum,
                        texinfo: side.texinfo,
                        kind: side.kind,
                        source: side.source,
                        winding: piece,
                        marks: side.marks.without(SideMarks::TESTED),
                    });
                }
            }
        }
        let [front_ok, back_ok] = parts.each_mut().map(|p| self.piece_is_sound(p, check));
        let [mut front, mut back] = parts;
        match (front_ok, back_ok) {
            (false, false) => return Cut::Parts(Box::new([None, None])),
            (true, false) => return Cut::Whole(winding::Side::Front),
            (false, true) => return Cut::Whole(winding::Side::Back),
            (true, true) => {}
        }
        for (part, (planenum, w)) in [&mut front, &mut back]
            .into_iter()
            .zip([(planenum.opposite(), mid.flipped()), (planenum, mid)])
        {
            part.sides.push(Side {
                planenum,
                texinfo: Texinfos::SKIP,
                kind: SideKind::Given,
                source: None,
                winding: w,
                marks: SideMarks::default().with(SideMarks::ON_NODE),
            });
        }
        let keep = |part: BspBrush| (self.volume(&part) >= MICROVOLUME).then_some(part);
        Cut::Parts(Box::new([keep(front), keep(back)]))
    }

    /// `brush` cut by plane `planenum` into its parts in front and behind, `SplitBrush`.
    pub(crate) fn split(&self, brush: BspBrush, planenum: PlaneNum) -> [Option<BspBrush>; 2] {
        match self.cut(&brush, planenum, PieceCheck::EveryAxis) {
            Cut::Whole(winding::Side::Back) => [None, Some(brush)],
            Cut::Whole(_) => [Some(brush), None],
            Cut::Parts(parts) => *parts,
        }
    }

    /// Whether plane `planenum` cuts `brush` into two sound parts, `CheckSplitBrush`
    /// (QBSP-15.27).
    fn splits_in_two(&self, brush: &BspBrush, planenum: PlaneNum) -> bool {
        match self.cut(brush, planenum, PieceCheck::FirstAxis) {
            Cut::Parts(parts) => matches!(*parts, [Some(_), Some(_)]),
            Cut::Whole(_) => false,
        }
    }

    /// The brush of `bounds`' six planes, `BrushFromBounds`: +x, +y, +z at the maximum, then
    /// -x, -y, -z at the minimum, with the skip texinfo.
    pub(crate) fn from_bounds(planes: &mut Planes, bounds: Bounds, extent: f64) -> BspBrush {
        let mut sides = Vec::with_capacity(AXES * 2);
        for (positive, sign) in [(true, 1.0), (false, -1.0)] {
            for axis in 0..AXES {
                let mut normal = [0.0; AXES];
                if let Some(n) = normal.get_mut(axis) {
                    *n = sign;
                }
                let dist = if positive {
                    bounds.maxs.get(axis).copied()
                } else {
                    bounds.mins.get(axis).map(|m| -m)
                };
                sides.push(Side {
                    planenum: planes.add_or_find(Plane {
                        normal,
                        dist: dist.unwrap_or_default(),
                    }),
                    texinfo: Texinfos::SKIP,
                    kind: SideKind::Given,
                    source: None,
                    winding: Winding::default(),
                    marks: SideMarks::default(),
                });
            }
        }
        let mut brush = BspBrush {
            sides,
            ..BspBrush::default()
        };
        brush.create_windings(planes, extent, &mut VisibleFaces::default());
        brush
    }
}

/// What building a tree needs at hand.
pub(crate) struct Builder<'a> {
    pub(crate) cutter: Cutter<'a>,
    pub(crate) texinfos: &'a Texinfos,
    pub(crate) visible: &'a VisibleFaces,
}

impl Builder<'_> {
    fn is_visible(&self, side: &Side) -> bool {
        side.is_visible(self.texinfos, self.visible)
    }

    /// How `brush` lies against plane `planenum`, `TestBrushToPlanenum` (QBSP-15.26): behind
    /// and facing when a side is on the plane, in front and facing when on its opposite, else
    /// its bounds' sides; across it, the visible sides not on a node that it splits by more than
    /// 0.1 each way, and whether the brush reaches less than a unit past it.
    fn test(&self, brush: &BspBrush, planenum: PlaneNum) -> Tested {
        for side in &brush.sides {
            if side.planenum == planenum {
                return Tested {
                    sides: PlaneSides::BACK.facing(),
                    ..Tested::default()
                };
            }
            if side.planenum == planenum.opposite() {
                return Tested {
                    sides: PlaneSides::FRONT.facing(),
                    ..Tested::default()
                };
            }
        }
        let Some(plane) = self.cutter.planes.get(planenum) else {
            return Tested::default();
        };
        let sides = brush.bounds.plane_sides(plane);
        let mut tested = Tested {
            sides,
            ..Tested::default()
        };
        if sides != PlaneSides::BOTH {
            return tested;
        }
        let (mut d_front, mut d_back) = (0.0_f64, 0.0_f64);
        for side in &brush.sides {
            if side.marks.has(SideMarks::ON_NODE)
                || !self.is_visible(side)
                || side.winding.is_empty()
            {
                continue;
            }
            let (mut front, mut back) = (false, false);
            for &point in side.winding.points() {
                let d = plane.plane().distance_to(point);
                d_front = d_front.max(d);
                d_back = d_back.min(d);
                front |= d > SPLIT_EPSILON;
                back |= d < -SPLIT_EPSILON;
            }
            let flags = self.texinfos.flags(side.texinfo);
            if front && back && !flags.has(SurfFlags::HINTSKIP) {
                tested.splits = tested.splits.saturating_add(1);
                if flags.has(SurfFlags::HINT) {
                    tested.hint_splits = tested.hint_splits.saturating_add(1);
                }
            }
        }
        if (d_front > 0.0 && d_front < EPSILON_BRUSH) || (d_back < 0.0 && d_back > -EPSILON_BRUSH) {
            tested.epsilon_brushes = 1;
        }
        tested
    }

    /// Whether `side` may be tried as a splitter in `pass` (QBSP-15.28): not a bevel, with a
    /// polygon, not on a node, not a hintskip, visible on passes 0 and 2 and not on 1 and 3.
    fn may_split(&self, side: &Side, pass: usize) -> bool {
        side.kind != SideKind::Bevel
            && !side.winding.is_empty()
            && !side.marks.has(SideMarks::ON_NODE)
            && !self.texinfos.flags(side.texinfo).has(SurfFlags::HINTSKIP)
            && self.is_visible(side) == pass.is_multiple_of(2)
    }

    /// Whether plane `planenum` cuts `node`'s volume in two, `CheckPlaneAgainstVolume`; never
    /// for a node with none.
    fn cuts_volume(&self, node: &Node, planenum: PlaneNum) -> bool {
        node.volume
            .as_ref()
            .is_some_and(|v| self.cutter.splits_in_two(v, planenum))
    }

    /// The side whose plane best halves `node`, `ChooseMidPlaneFromList` (QBSP-15.27): by
    /// pass, the least uneven of the planes that cut the node's volume, an axial one before any
    /// other.
    fn mid_plane(&self, brushes: &[BspBrush], node: &Node) -> Option<PlaneNum> {
        for pass in 0..PASSES {
            let mut best_axial: Option<(f64, PlaneNum)> = None;
            let mut best_any: Option<(f64, PlaneNum)> = None;
            for brush in brushes {
                if (pass >= DETAIL_PASS) != brush.contents.is_any_detail() {
                    continue;
                }
                for side in &brush.sides {
                    if !self.may_split(side, pass) {
                        continue;
                    }
                    let positive = side.planenum.positive();
                    let Some(plane) = self.cutter.planes.get(positive) else {
                        continue;
                    };
                    if !self.cuts_volume(node, positive) {
                        continue;
                    }
                    let metric = node.bounds.split_metric(plane);
                    if best_any.is_none_or(|(m, _)| metric < m) {
                        best_any = Some((metric, side.planenum));
                    }
                    if plane.kind().is_axial() && best_axial.is_none_or(|(m, _)| metric < m) {
                        best_axial = Some((metric, side.planenum));
                    }
                }
            }
            if let Some((_, planenum)) = best_axial.or(best_any) {
                return Some(planenum.positive());
            }
        }
        None
    }

    /// The plane to split `node` by and where each brush lies against it, `SelectSplitPlane`
    /// (QBSP-15.28); `None` makes the node a leaf.
    fn select(
        &self,
        brushes: &mut [BspBrush],
        node: &Node,
        split: TreeSplit,
    ) -> Option<(PlaneNum, Vec<PlaneSides>, Separator)> {
        if brushes.is_empty() {
            return None;
        }
        let mut split = split;
        if split == TreeSplit::Auto {
            let largest = node
                .bounds
                .maxs
                .iter()
                .zip(node.bounds.mins)
                .map(|(hi, lo)| hi - lo)
                .fold(f64::MIN, f64::max);
            if largest > MAX_NODE_SIZE - POINT_EPSILON {
                split = TreeSplit::Fast;
            }
        }
        if split == TreeSplit::Fast
            && let Some(planenum) = self.mid_plane(brushes, node)
        {
            let sides = brushes
                .iter()
                .map(|b| self.test(b, planenum).sides)
                .collect();
            return Some((planenum, sides, Separator::Structural));
        }
        let mut best: Option<(PlaneNum, Vec<PlaneSides>)> = None;
        let mut best_value = NO_SCORE;
        let mut separator = Separator::Structural;
        for pass in 0..PASSES {
            for b in 0..brushes.len() {
                let side_count = brushes.get(b).map_or(0, |brush| brush.sides.len());
                if brushes
                    .get(b)
                    .is_some_and(|brush| (pass >= DETAIL_PASS) != brush.contents.is_any_detail())
                {
                    continue;
                }
                for s in 0..side_count {
                    let Some(side) = brushes.get(b).and_then(|brush| brush.sides.get(s)) else {
                        continue;
                    };
                    if side.marks.has(SideMarks::TESTED) || !self.may_split(side, pass) {
                        continue;
                    }
                    let (planenum, texinfo) = (side.planenum, side.texinfo);
                    let positive = planenum.positive();
                    let Some(plane) = self.cutter.planes.get(positive) else {
                        continue;
                    };
                    if !self.cuts_volume(node, positive) {
                        continue;
                    }
                    let value = self.score(brushes, positive, plane, texinfo);
                    if let Some((value, sides)) = value
                        && value > best_value
                    {
                        best_value = value;
                        best = Some((planenum, sides));
                    }
                }
            }
            if best.is_some() {
                if pass >= DETAIL_PASS {
                    separator = Separator::Detail;
                }
                break;
            }
        }
        for brush in brushes.iter_mut() {
            for side in &mut brush.sides {
                side.marks = side.marks.without(SideMarks::TESTED);
            }
        }
        best.map(|(planenum, sides)| (planenum.positive(), sides, separator))
    }

    /// A plane's score as a splitter and each brush's side of it (QBSP-15.28): five for each
    /// brush on it, less five for each side split and the difference of the brushes in front
    /// and behind, five more when axial, a thousand less for each epsilon brush, and never when
    /// it splits a hint side, the last brush tested deciding, unless itself a hint. Each brush
    /// on the plane has its sides on it marked tested.
    fn score(
        &self,
        brushes: &mut [BspBrush],
        positive: PlaneNum,
        plane: &BspPlane,
        texinfo: super::map::TexinfoNum,
    ) -> Option<(i64, Vec<PlaneSides>)> {
        let (mut front, mut back, mut facing, mut splits, mut epsilon) =
            (0_i64, 0_i64, 0_i64, 0_i64, 0_i64);
        let mut hint_split = false;
        let mut sides = Vec::with_capacity(brushes.len());
        for test in brushes.iter_mut() {
            let tested = self.test(test, positive);
            splits = splits.saturating_add(tested.splits);
            epsilon = epsilon.saturating_add(tested.epsilon_brushes);
            hint_split = tested.hint_splits > 0;
            sides.push(tested.sides);
            if tested.sides.is_facing() {
                facing = facing.saturating_add(1);
                for s in &mut test.sides {
                    if s.planenum.positive() == positive {
                        s.marks = s.marks.with(SideMarks::TESTED);
                    }
                }
            }
            if tested.sides.has(PlaneSides::FRONT) {
                front = front.saturating_add(1);
            }
            if tested.sides.has(PlaneSides::BACK) {
                back = back.saturating_add(1);
            }
        }
        let mut value = FACING_SCORE
            .saturating_mul(facing)
            .saturating_sub(SPLIT_SCORE.saturating_mul(splits))
            .saturating_sub(front.saturating_sub(back).saturating_abs());
        if plane.kind().is_axial() {
            value = value.saturating_add(AXIAL_SCORE);
        }
        value = value.saturating_sub(epsilon.saturating_mul(EPSILON_BRUSH_SCORE));
        if hint_split && !self.texinfos.flags(texinfo).has(SurfFlags::HINT) {
            value = SPLITS_HINT_SCORE;
        }
        Some((value, sides))
    }

    /// The brushes on each side of plane `planenum`, `SplitBrushList` (QBSP-15.29): those
    /// across it cut, those on it with their sides on it marked as on a node.
    fn split_list(
        &self,
        brushes: Vec<BspBrush>,
        sides: &[PlaneSides],
        planenum: PlaneNum,
    ) -> [Vec<BspBrush>; 2] {
        let mut out = [Vec::new(), Vec::new()];
        for (mut brush, &side) in brushes.into_iter().zip(sides) {
            if side == PlaneSides::BOTH {
                let [front, back] = self.cutter.split(brush, planenum);
                for (list, part) in out.iter_mut().zip([front, back]) {
                    if let Some(part) = part {
                        list.push(part);
                    }
                }
                continue;
            }
            if side.is_facing() {
                for s in &mut brush.sides {
                    if s.planenum.positive() == planenum {
                        s.marks = s.marks.with(SideMarks::ON_NODE);
                    }
                }
            }
            if side.has(PlaneSides::FRONT) {
                out[0].push(brush);
            } else if side.has(PlaneSides::BACK) {
                out[1].push(brush);
            }
        }
        out
    }

    /// The leaf of `brushes`, `LeafNode`: their contents combined from empty, and their
    /// originals.
    fn leaf(brushes: &[BspBrush]) -> Leaf {
        Leaf {
            contents: brushes
                .iter()
                .fold(Contents::EMPTY, |c, b| Contents::combine(c, b.contents)),
            original_brushes: brushes.iter().map(|b| b.original).collect(),
            ..Leaf::default()
        }
    }

    /// The tree below `node` from `brushes`, `BuildTree_r` (QBSP-15.29): a leaf when no plane
    /// is chosen; else the node split by it, its children's bounds cut at an axial plane facing
    /// along its axis, its volume cut between them. The two children are built at once down to
    /// [`PARALLEL_DEPTH`] while they hold [`PARALLEL_BRUSHES`] between them: each reads only the planes and its own brushes, and
    /// [`Grown::place`] numbers the nodes as building the front child first would.
    fn grow(
        &self,
        mut node: Node,
        mut brushes: Vec<BspBrush>,
        split: TreeSplit,
        depth: usize,
    ) -> Grown {
        let Some((planenum, sides, separator)) = self.select(&mut brushes, &node, split) else {
            node.kind = NodeKind::Leaf(Self::leaf(&brushes));
            node.volume = None;
            return Grown::Leaf(node);
        };
        let [front_brushes, back_brushes] = self.split_list(brushes, &sides, planenum);
        let mut child_bounds = [node.bounds, node.bounds];
        if let Some(plane) = self.cutter.planes.get(planenum).map(BspPlane::plane)
            && let Some(axis) = plane.normal.iter().position(|n| {
                #[expect(
                    clippy::float_cmp,
                    reason = "spec QBSP-15.29: `get_normal()[i] == 1.0` exactly"
                )]
                let unit = *n == 1.0;
                unit
            })
        {
            let [front, back] = &mut child_bounds;
            if let (Some(f), Some(b)) = (front.mins.get_mut(axis), back.maxs.get_mut(axis)) {
                *f = plane.dist;
                *b = plane.dist;
            }
        }
        let volumes = node
            .volume
            .take()
            .map_or([None, None], |v| self.cutter.split(v, planenum));
        let child = |bounds: Bounds, volume: Option<BspBrush>| Node {
            volume,
            ..Tree::leaf(None, bounds, Leaf::default())
        };
        let ([front_bounds, back_bounds], [front_volume, back_volume]) = (child_bounds, volumes);
        let (front, back) = (
            child(front_bounds, front_volume),
            child(back_bounds, back_volume),
        );
        let next = depth.saturating_add(1);
        let brushes = front_brushes.len().saturating_add(back_brushes.len());
        let children = if depth < PARALLEL_DEPTH && brushes >= PARALLEL_BRUSHES {
            std::thread::scope(|scope| {
                let front = scope.spawn(|| self.grow(front, front_brushes, split, next));
                let back = self.grow(back, back_brushes, split, next);
                // A panic on the front child's thread is raised here, as building it on this
                // thread would have raised it.
                let front = front
                    .join()
                    .unwrap_or_else(|payload| std::panic::resume_unwind(payload));
                [front, back]
            })
        } else {
            [
                self.grow(front, front_brushes, split, next),
                self.grow(back, back_brushes, split, next),
            ]
        };
        node.kind = NodeKind::Split {
            planenum,
            children: [Tree::HEAD; 2],
            separator,
        };
        Grown::Split {
            node,
            children: Box::new(children),
        }
    }
}

/// How deep the brush BSP and the portals build a node's two children at once: up to 2^4
/// threads.
pub(crate) const PARALLEL_DEPTH: usize = 4;
/// The fewest brushes a node's children hold for the brush BSP to build them at once: fewer
/// build faster than a thread starts.
const PARALLEL_BRUSHES: usize = 64;

/// A node built with its children, before it has a number.
enum Grown {
    Leaf(Node),
    Split {
        /// The node, its children not yet numbered.
        node: Node,
        /// Its front child, then its back.
        children: Box<[Grown; 2]>,
    },
}

impl Grown {
    /// The node put in `tree` at `id` below `parent`, numbered as `BuildTree_r` numbers them:
    /// a split node's two children pushed together, then the front's nodes, then the back's.
    fn place(self, tree: &mut Tree, id: NodeId, parent: Option<NodeId>) {
        let (mut node, children) = match self {
            Self::Leaf(node) => (node, None),
            Self::Split { node, children } => (node, Some(children)),
        };
        node.parent = parent;
        let Some(children) = children else {
            if let Some(slot) = tree.node_mut(id) {
                *slot = node;
            }
            return;
        };
        let bounds = children.each_ref().map(Self::bounds);
        let ids = bounds.map(|b| tree.push(Tree::leaf(Some(id), b, Leaf::default())));
        if let NodeKind::Split { children, .. } = &mut node.kind {
            *children = ids;
        }
        if let Some(slot) = tree.node_mut(id) {
            *slot = node;
        }
        for (child, child_id) in (*children).into_iter().zip(ids) {
            child.place(tree, child_id, Some(id));
        }
    }

    const fn bounds(&self) -> Bounds {
        match self {
            Self::Leaf(node) | Self::Split { node, .. } => node.bounds,
        }
    }
}

impl Tree {
    /// The tree with each node of two leaves that are both any solid made one leaf,
    /// `PruneNodes` (QBSP-15.47): children first; a detail wall leaf made solid; the merged
    /// leaf's contents combined and its originals both lists' without repeats; the node's faces
    /// dropped.
    pub(crate) fn prune(&mut self) {
        self.prune_below(Self::HEAD);
    }

    fn prune_below(&mut self, id: NodeId) {
        let Some((_, [front, back], _)) = self.split_of(id) else {
            if let Some(leaf) = self.leaf_data_mut(id)
                && leaf.contents.is_detail_wall()
            {
                leaf.contents = Contents::new(Contents::SOLID);
            }
            return;
        };
        self.prune_below(front);
        self.prune_below(back);
        let (Some(a), Some(b)) = (self.leaf_data(front), self.leaf_data(back)) else {
            return;
        };
        if !a.contents.is_any_solid() || !b.contents.is_any_solid() {
            return;
        }
        let contents = Contents::combine(a.contents, b.contents);
        let (base, other) = if a.original_brushes.len() > b.original_brushes.len() {
            (a, b)
        } else {
            (b, a)
        };
        let mut original_brushes = base.original_brushes.clone();
        original_brushes.extend(&other.original_brushes);
        original_brushes.sort_unstable();
        original_brushes.dedup();
        if let Some(node) = self.node_mut(id) {
            node.kind = NodeKind::Leaf(Leaf {
                contents,
                original_brushes,
                ..Leaf::default()
            });
            node.faces.clear();
        }
    }
}

/// What building a tree from brushes takes besides them.
pub(crate) struct BspInput<'a> {
    pub(crate) planes: &'a mut Planes,
    pub(crate) texinfos: &'a Texinfos,
    pub(crate) visible: &'a VisibleFaces,
    pub(crate) extent: f64,
}

impl BspInput<'_> {
    /// The tree of `brushes`, whose entity's bounds are `bounds`, `BrushBSP` (QBSP-15.29): with
    /// no brush a node on plane 0 with two empty leaves; else the head covering every brush and
    /// [`SIDESPACE`] around, its volume the brush of that box, each side's node mark cleared.
    pub(crate) fn brush_bsp(self, bounds: Bounds, brushes: &[BspBrush], split: TreeSplit) -> Tree {
        self.brush_bsp_from(bounds, brushes, split, 0)
    }

    /// [`BspInput::brush_bsp`] with the head built at `depth`: from [`PARALLEL_DEPTH`] on, on
    /// this thread alone.
    fn brush_bsp_from(
        self,
        bounds: Bounds,
        brushes: &[BspBrush],
        split: TreeSplit,
        depth: usize,
    ) -> Tree {
        let mut tree = Tree {
            nodes: Vec::new(),
            bounds,
            portals: Vec::new(),
            outside: None,
            faces: Vec::new(),
        };
        if brushes.is_empty() {
            let head = tree.push(Tree::leaf(None, bounds, Leaf::default()));
            let children = [
                tree.push(Tree::leaf(Some(head), Bounds::default(), Leaf::default())),
                tree.push(Tree::leaf(Some(head), Bounds::default(), Leaf::default())),
            ];
            if let Some(node) = tree.node_mut(head) {
                node.kind = NodeKind::Split {
                    planenum: PlaneNum::new(0),
                    children,
                    separator: Separator::Structural,
                };
            }
            return tree;
        }
        let mut brushes = brushes.to_vec();
        for brush in &mut brushes {
            for side in &mut brush.sides {
                side.marks = side.marks.without(SideMarks::ON_NODE);
            }
            tree.bounds.union(&brush.bounds);
        }
        let head_bounds = Bounds {
            mins: tree.bounds.mins.map(|c| c - SIDESPACE),
            maxs: tree.bounds.maxs.map(|c| c + SIDESPACE),
        };
        let volume = Cutter::from_bounds(self.planes, head_bounds, self.extent);
        let head = tree.push(Tree::leaf(None, head_bounds, Leaf::default()));
        let builder = Builder {
            cutter: Cutter {
                planes: self.planes,
                extent: self.extent,
            },
            texinfos: self.texinfos,
            visible: self.visible,
        };
        let node = Node {
            volume: Some(volume),
            ..Tree::leaf(None, head_bounds, Leaf::default())
        };
        builder
            .grow(node, brushes, split, depth)
            .place(&mut tree, head, None);
        tree
    }
}

#[cfg(test)]
pub(crate) mod tests;
