// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/outside.cc, LeafSealsMap, LeafSealsForDetailFill, PointInLeaf, ClearOccupied_r, OutsideFill_Passable, DetailFill_Passable, FloodFillLeafsFromVoid, FindPortalsToVoid, WriteLeakTrail, WriteLeakLine, IsNofillEntity, MarkOccupiedLeafs, FindOccupiedLeafs, MarkVisibleBrushSides_R, OutLeafsToSolid_R, FillDetailEnclosedLeafsToDetailSolid_R, BFSFloodFillFromOccupiedLeafs, MakeLeakLine, FillOutside, FillBrushEntity, FillDetail

//! The fill from the void, ericw-tools' `FillOutside` (QBSP-15.35 to QBSP-15.38): the leaves no
//! entity reaches made solid, the leak when one does, and the brush sides seen from the leaves
//! left.

use std::collections::{HashSet, VecDeque};

use qs_map::Entity;
use qs_map::cnum::c_atoi;

use super::brush::{BspBrush, VisibleFaces};
use super::game::Contents;
use super::map::{EntityNum, Texinfos};
use super::portals::{PortalId, SideMarker};
use super::tree::{NodeId, Tree};
use super::vec::{AXES, Planes, Vec3};

/// `QBSP_EQUAL_EPSILON` as the `float` `MarkOccupiedLeafs` compares an origin with.
const ORIGIN_EPSILON: f32 = 0.0001;
/// The keys that let an entity stand in the void without a leak, `IsNofillEntity`.
const NOFILL_KEYS: [&[u8]; 2] = [b"_nofill", b"_lightgrid_hint"];

/// Which leaf a point exactly on a node's plane goes to, `prefer_sealing`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Prefer {
    /// The one that seals the map: the drawing hull.
    Sealing,
    /// The other: the clipping hulls, so a player 24 units off a floor fills nothing.
    Open,
}

/// Which way the fill goes, `filltype_t` as `-filltype auto` decides it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FillType {
    /// From the entities out, filling what they do not reach: the drawing hull.
    Inside,
    /// From the void in, filling what it reaches: the clipping hulls.
    Outside,
}

/// Which portals a fill crosses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Passage {
    /// Between leaves neither of which seals the map, `OutsideFill_Passable`.
    Outside,
    /// Between leaves neither solid nor sky, `DetailFill_Passable`.
    Detail,
}

/// An entity that can occupy a leaf, and where it stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Occupant {
    pub(crate) entity: EntityNum,
    pub(crate) origin: Vec3,
}

impl Occupant {
    /// The entities after the world that may occupy a leaf, `MarkOccupiedLeafs`'s skips
    /// (QBSP-15.35): not at the origin within `QBSP_EQUAL_EPSILON` and with no nonzero `_nofill`
    /// or `_lightgrid_hint`.
    pub(crate) fn of<'a>(entities: impl Iterator<Item = (Vec3, &'a Entity)>) -> Vec<Self> {
        entities
            .enumerate()
            .skip(1)
            .filter(|(_, (origin, source))| {
                let at_origin = origin.iter().all(|c| c.abs() <= f64::from(ORIGIN_EPSILON));
                let nofill = NOFILL_KEYS
                    .iter()
                    .any(|key| source.value(key).is_some_and(|v| c_atoi(v) != 0));
                !at_origin && !nofill
            })
            .map(|(index, (origin, _))| Self {
                entity: EntityNum::new(index),
                origin,
            })
            .collect()
    }
}

/// What the fill found, `FillOutside`'s result.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Fill {
    /// No entity stands in an open leaf: nothing is filled.
    NoOccupant,
    /// An entity reaches the void: nothing is filled. The portals from the entity to the void.
    Leak {
        entity: EntityNum,
        origin: Vec3,
        line: Vec<PortalId>,
    },
    /// The unreached leaves are solid.
    Filled,
}

impl Contents {
    /// Whether a leaf of these contents closes the detail fill, `LeafSealsForDetailFill`: any
    /// solid, detail too, or sky.
    const fn seals_for_detail_fill(self) -> bool {
        self.is_any_solid() || self.is_sky()
    }
}

impl Tree {
    fn seals(&self, leaf: NodeId) -> bool {
        self.leaf_data(leaf).is_some_and(|l| l.contents.seals_map())
    }

    /// Whether a fill of `passage` crosses portal `id`: never one of the head's.
    fn passable(&self, id: PortalId, passage: Passage) -> bool {
        let Some(p) = self.portal(id) else {
            return false;
        };
        if p.onnode.is_none() {
            return false;
        }
        p.nodes.iter().all(|&n| {
            self.leaf_data(n).is_some_and(|l| match passage {
                Passage::Outside => !l.contents.seals_map(),
                Passage::Detail => !l.contents.seals_for_detail_fill(),
            })
        })
    }

    /// The leaf `point` is in, `PointInLeaf` (QBSP-15.35): exactly on a node's plane, the front
    /// leaf when it seals as `prefer` wants, else the back.
    pub(crate) fn point_in_leaf(&self, planes: &Planes, point: Vec3, prefer: Prefer) -> NodeId {
        self.point_in_leaf_below(Self::HEAD, planes, point, prefer)
    }

    fn point_in_leaf_below(
        &self,
        id: NodeId,
        planes: &Planes,
        point: Vec3,
        prefer: Prefer,
    ) -> NodeId {
        let Some((planenum, [front, back], _)) = self.split_of(id) else {
            return id;
        };
        let dist = planes
            .get(planenum)
            .map_or(0.0, |p| p.plane().distance_to(point));
        if dist > 0.0 {
            return self.point_in_leaf_below(front, planes, point, prefer);
        }
        if dist < 0.0 {
            return self.point_in_leaf_below(back, planes, point, prefer);
        }
        let front = self.point_in_leaf_below(front, planes, point, prefer);
        let back = self.point_in_leaf_below(back, planes, point, prefer);
        if (prefer == Prefer::Sealing) == self.seals(front) {
            front
        } else {
            back
        }
    }

    /// Every leaf's fill state cleared, `ClearOccupied_r`.
    fn clear_occupied(&mut self) {
        for leaf in self.leaves_below(Self::HEAD) {
            if let Some(l) = self.leaf_data_mut(leaf) {
                l.outside_distance = None;
                l.occupied = 0;
                l.occupant = None;
            }
        }
    }

    /// Each open leaf's first occupant, `MarkOccupiedLeafs` (QBSP-15.35); the occupied leaves,
    /// front first, `FindOccupiedLeafs`.
    fn occupy(&mut self, planes: &Planes, occupants: &[Occupant], prefer: Prefer) -> Vec<NodeId> {
        for occupant in occupants {
            let leaf = self.point_in_leaf(planes, occupant.origin, prefer);
            if self.seals(leaf) {
                continue;
            }
            if let Some(l) = self.leaf_data_mut(leaf)
                && l.occupant.is_none()
            {
                l.occupant = Some(occupant.entity);
            }
        }
        self.leaves_below(Self::HEAD)
            .into_iter()
            .filter(|&l| self.leaf_data(l).is_some_and(|d| d.occupant.is_some()))
            .collect()
    }

    /// The leaf beyond the outside leaf's first portal, where the void fill starts.
    fn void_leaf(&self) -> Option<NodeId> {
        let outside = self.outside?;
        let first = *self.portals_of(outside).first()?;
        self.across(first, outside)
    }

    /// Each leaf's steps from the void, `FloodFillLeafsFromVoid` (QBSP-15.36): breadth first
    /// from the void leaf through the portals the outside fill crosses.
    fn flood_from_void(&mut self) {
        let Some(start) = self.void_leaf() else {
            return;
        };
        let mut queue = VecDeque::from([(start, 0_u32)]);
        let mut visited = HashSet::new();
        while let Some((node, distance)) = queue.pop_front() {
            if !visited.insert(node) {
                continue;
            }
            if let Some(l) = self.leaf_data_mut(node) {
                l.outside_distance = Some(distance);
            }
            for portal in self.portals_of(node) {
                if self.passable(portal, Passage::Outside)
                    && let Some(next) = self.across(portal, node)
                {
                    queue.push_back((next, distance.saturating_add(1)));
                }
            }
        }
    }

    /// Each leaf's steps from an occupied leaf, `BFSFloodFillFromOccupiedLeafs` (QBSP-15.36).
    fn flood_from_occupied(&mut self, leaves: &[NodeId], passage: Passage) {
        let mut queue: VecDeque<(NodeId, u32)> = leaves.iter().map(|&l| (l, 1)).collect();
        while let Some((node, distance)) = queue.pop_front() {
            let Some(l) = self.leaf_data_mut(node) else {
                continue;
            };
            if l.occupied != 0 {
                continue;
            }
            l.occupied = distance;
            for portal in self.portals_of(node) {
                if self.passable(portal, passage)
                    && let Some(next) = self.across(portal, node)
                {
                    queue.push_back((next, distance.saturating_add(1)));
                }
            }
        }
    }

    /// The portals from `from` down a fill's steps, each to the neighbour fewest steps away,
    /// until `done` (QBSP-15.36).
    fn descend(
        &self,
        from: NodeId,
        steps: impl Fn(NodeId) -> Option<u32>,
        done: u32,
    ) -> (Vec<PortalId>, NodeId) {
        let mut line = Vec::new();
        let mut node = from;
        while let Some(here) = steps(node)
            && here != done
        {
            let mut best: Option<(PortalId, NodeId, u32)> = None;
            for portal in self.portals_of(node) {
                if !self.passable(portal, Passage::Outside) {
                    continue;
                }
                let Some(next) = self.across(portal, node) else {
                    continue;
                };
                let Some(there) = steps(next) else {
                    continue;
                };
                if there < best.map_or(here, |b| b.2) {
                    best = Some((portal, next, there));
                }
            }
            let Some((portal, next, _)) = best else {
                break;
            };
            line.push(portal);
            node = next;
        }
        (line, node)
    }

    /// Each leaf left out of the fill made solid unless it seals the map already,
    /// `OutLeafsToSolid_R` (QBSP-15.37): for an inside fill those no occupant reaches, for an
    /// outside fill those the void reaches.
    fn fill_out_leaves(&mut self, fill: FillType) {
        for leaf in self.leaves_below(Self::HEAD) {
            let Some(l) = self.leaf_data_mut(leaf) else {
                continue;
            };
            let reached = match fill {
                FillType::Inside => l.occupied == 0,
                FillType::Outside => l.outside_distance.is_some(),
            };
            if reached && !l.contents.seals_map() {
                l.contents = Contents::new(Contents::SOLID);
            }
        }
    }

    /// Each leaf no occupant reaches through open leaves made detail solid unless solid or sky,
    /// `FillDetailEnclosedLeafsToDetailSolid_R` (QBSP-15.38).
    fn fill_detail_enclosed(&mut self) {
        for leaf in self.leaves_below(Self::HEAD) {
            if let Some(l) = self.leaf_data_mut(leaf)
                && l.occupied == 0
                && !l.contents.seals_for_detail_fill()
            {
                l.contents = Contents::new(Contents::SOLID).detail_solid();
            }
        }
    }
}

/// What a fill needs at hand.
pub(crate) struct Filler<'a> {
    pub(crate) planes: &'a Planes,
    pub(crate) texinfos: &'a Texinfos,
    pub(crate) originals: &'a [BspBrush],
}

impl Filler<'_> {
    fn marker(&self) -> SideMarker<'_> {
        SideMarker {
            planes: self.planes,
            texinfos: self.texinfos,
            originals: self.originals,
        }
    }

    /// The brush sides seen from open leaves, `MarkVisibleBrushSides_R` (QBSP-15.37): every map
    /// face of `brushes` unseen but hints, then for each leaf neither solid nor sky, each original
    /// brush side in a leaf across one of its portals whose positive plane is that portal's,
    /// within epsilon, seen.
    fn mark_seen_sides(&self, tree: &Tree, brushes: &[BspBrush], visible: &mut VisibleFaces) {
        self.marker().hide_all(brushes, visible);
        for leaf in tree.leaves_below(Tree::HEAD) {
            if tree
                .leaf_data(leaf)
                .is_none_or(|l| l.contents.seals_for_detail_fill())
            {
                continue;
            }
            for id in tree.portals_of(leaf) {
                let (Some(portal), Some(neighbour)) = (tree.portal(id), tree.across(id, leaf))
                else {
                    continue;
                };
                let Some(data) = tree.leaf_data(neighbour) else {
                    continue;
                };
                for original in &data.original_brushes {
                    let Some(brush) = self.originals.get(original.get()) else {
                        continue;
                    };
                    for side in &brush.sides {
                        let on_portal = self
                            .planes
                            .get(side.planenum.positive())
                            .is_some_and(|p| p.epsilon_equal(&portal.plane));
                        if let (Some(face), true) = (side.source, on_portal) {
                            visible.show(face);
                        }
                    }
                }
            }
        }
    }

    /// The world's tree filled from the void, `FillOutside` (QBSP-15.35 to QBSP-15.37): its
    /// leaves' fill state cleared and their occupants found; nothing filled without one. An
    /// inside fill floods from the occupied leaves and leaks when it reaches the void leaf, the
    /// line from the occupant; an outside fill floods from the void and leaks at the occupied
    /// leaf nearest it, the line to the void. Without a leak the leaves left out are made solid
    /// and the sides seen marked.
    pub(crate) fn fill_outside(
        &self,
        tree: &mut Tree,
        occupants: &[Occupant],
        fill: FillType,
        brushes: &[BspBrush],
        visible: &mut VisibleFaces,
    ) -> Fill {
        tree.clear_occupied();
        let prefer = match fill {
            FillType::Inside => Prefer::Sealing,
            FillType::Outside => Prefer::Open,
        };
        let occupied = tree.occupy(self.planes, occupants, prefer);
        if occupied.is_empty() {
            return Fill::NoOccupant;
        }
        let origin_of = |entity: EntityNum| {
            occupants
                .iter()
                .find(|o| o.entity == entity)
                .map_or([0.0; AXES], |o| o.origin)
        };
        let leak = match fill {
            FillType::Inside => {
                tree.flood_from_occupied(&occupied, Passage::Outside);
                tree.void_leaf().and_then(|start| {
                    let steps = |n: NodeId| {
                        tree.leaf_data(n)
                            .and_then(|l| (l.occupied > 0).then_some(l.occupied))
                    };
                    steps(start)?;
                    let (mut line, end) = tree.descend(start, steps, 1);
                    line.reverse();
                    let entity = tree.leaf_data(end)?.occupant?;
                    Some((entity, line))
                })
            }
            FillType::Outside => {
                tree.flood_from_void();
                let nearest = occupied
                    .iter()
                    .filter_map(|&l| Some((tree.leaf_data(l)?.outside_distance?, l)))
                    .min_by_key(|(d, _)| *d);
                nearest.and_then(|(_, leaf)| {
                    let entity = tree.leaf_data(leaf)?.occupant?;
                    let steps = |n: NodeId| tree.leaf_data(n).and_then(|l| l.outside_distance);
                    Some((entity, tree.descend(leaf, steps, 0).0))
                })
            }
        };
        if let Some((entity, line)) = leak {
            return Fill::Leak {
                entity,
                origin: origin_of(entity),
                line,
            };
        }
        tree.fill_out_leaves(fill);
        self.mark_seen_sides(tree, brushes, visible);
        Fill::Filled
    }

    /// A brush model's tree's sides marked as seen from its open leaves, `FillBrushEntity`.
    pub(crate) fn fill_brush_entity(
        &self,
        tree: &mut Tree,
        brushes: &[BspBrush],
        visible: &mut VisibleFaces,
    ) {
        tree.clear_occupied();
        self.mark_seen_sides(tree, brushes, visible);
    }

    /// The open pockets no occupant reaches made detail solid, `FillDetail` (QBSP-15.38): the
    /// occupants found again, flooded through leaves neither solid nor sky, the rest made detail
    /// solid and the sides seen marked; nothing without an occupant.
    pub(crate) fn fill_detail(
        &self,
        tree: &mut Tree,
        occupants: &[Occupant],
        prefer: Prefer,
        brushes: &[BspBrush],
        visible: &mut VisibleFaces,
    ) {
        tree.clear_occupied();
        let occupied = tree.occupy(self.planes, occupants, prefer);
        if occupied.is_empty() {
            return;
        }
        tree.flood_from_occupied(&occupied, Passage::Detail);
        tree.fill_detail_enclosed();
        self.mark_seen_sides(tree, brushes, visible);
    }
}

impl Tree {
    /// The leak file's points, `WriteLeakLine` with `-leakdist 0` (QBSP-15.36): from the
    /// occupant's origin to each portal's centre in turn, each segment's two ends.
    pub(crate) fn leak_points(&self, origin: Vec3, line: &[PortalId]) -> Vec<Vec3> {
        let mut out = Vec::with_capacity(line.len().saturating_mul(2));
        let mut previous = origin;
        for &id in line {
            let Some(p) = self.portal(id) else {
                continue;
            };
            let current = p.winding.center();
            out.push(previous);
            out.push(current);
            previous = current;
        }
        out
    }
}

#[cfg(test)]
mod tests;
