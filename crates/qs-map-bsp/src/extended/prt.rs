// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/prtfile.cc, WritePortals_r, WritePTR2ClusterMapping_r, CountPortals, NumberLeafs_r, WritePortalfile, WritePortalFile
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/prtfile.cc, WritePortal, WritePTR2ClusterMapping, WritePortalfile

//! The portal file vis reads, ericw-tools' `WritePortalFile` (QBSP-15.51): the vis portals of
//! the pruned tree, numbered by leaf, or by cluster in a `PRT2` file when the tree has detail.

use std::collections::{BTreeMap, HashMap};

use super::portals::{HeadPortals, PortalType, Portalizer};
use super::tree::{NodeId, Separator, Tree};
use super::vec::{NORMAL_EPSILON, dot};

/// `ANGLEEPSILON`: a portal's polygon facing away from its plane by more than this is read
/// reversed.
const ANGLE_EPSILON: f64 = NORMAL_EPSILON;
/// Below this magnitude `{fmt}` writes a number with an exponent.
const SMALLEST_PLAIN: f64 = 1e-4;
/// From this magnitude `{fmt}` writes a number with an exponent.
const LARGEST_PLAIN: f64 = 1e16;

/// `value` as `{fmt}`'s `{}` writes a `double`: the shortest digits that read back, with an
/// exponent of at least two digits below 1e-4 or from 1e16 (QBSP-15.51).
pub(crate) fn fmt_number(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude != 0.0 && (magnitude < SMALLEST_PLAIN || magnitude >= LARGEST_PLAIN) {
        let text = format!("{value:e}");
        if let Some((mantissa, exponent)) = text.split_once('e') {
            let (sign, digits) = match exponent.strip_prefix('-') {
                Some(digits) => ('-', digits),
                None => ('+', exponent),
            };
            return format!("{mantissa}e{sign}{digits:0>2}");
        }
        return text;
    }
    format!("{value}")
}

/// What a portal's two ends are numbered by: `visleafnum`, or `viscluster` with detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NumberedBy {
    Leaf,
    Cluster,
}

/// Leaf and cluster numbers, `visleafnum` and `viscluster`.
#[derive(Default)]
struct Numbering {
    leaf: HashMap<NodeId, i64>,
    cluster: HashMap<NodeId, i64>,
    leaves: i64,
    clusters: i64,
    uses_detail: bool,
}

impl Tree {
    /// `NumberLeafs_r`: each leaf not any solid numbered in order, and given its parent detail
    /// separator's cluster or a cluster of its own.
    fn number_leaves(&self, id: NodeId, cluster: Option<i64>, n: &mut Numbering) {
        if let Some((_, [front, back], separator)) = self.split_of(id) {
            let mut cluster = cluster;
            if cluster.is_none() && separator == Separator::Detail {
                n.uses_detail = true;
                let c = n.clusters;
                n.clusters = n.clusters.saturating_add(1);
                n.cluster.insert(id, c);
                cluster = Some(c);
            }
            self.number_leaves(front, cluster, n);
            self.number_leaves(back, cluster, n);
            return;
        }
        if self
            .leaf_data(id)
            .is_some_and(|l| l.contents.is_any_solid())
        {
            n.leaf.insert(id, -1);
            n.cluster.insert(id, -1);
            return;
        }
        n.leaf.insert(id, n.leaves);
        n.leaves = n.leaves.saturating_add(1);
        let c = cluster.unwrap_or_else(|| {
            let c = n.clusters;
            n.clusters = n.clusters.saturating_add(1);
            c
        });
        n.cluster.insert(id, c);
    }

    /// `WritePortals_r`: for each leaf or cluster not any solid, each portal it is in front of
    /// that vis sees through, as its two numbers, swapped when the polygon faces away from the
    /// plane, and its points.
    fn write_portals(&self, id: NodeId, by: NumberedBy, n: &Numbering, out: &mut Vec<String>) {
        if let Some((_, [front, back], separator)) = self.split_of(id)
            && separator == Separator::Structural
        {
            self.write_portals(front, by, n, out);
            self.write_portals(back, by, n, out);
            return;
        }
        if self
            .leaf_data(id)
            .is_some_and(|l| l.contents.is_any_solid())
        {
            return;
        }
        let numbers = match by {
            NumberedBy::Cluster => &n.cluster,
            NumberedBy::Leaf => &n.leaf,
        };
        for portal in self.portals_of(id) {
            let Some(p) = self.portal(portal) else {
                continue;
            };
            if p.winding.is_empty() || p.nodes[0] != id || !self.vis_floods(portal) {
                continue;
            }
            let number = |node: NodeId| numbers.get(&node).copied().unwrap_or(-1);
            let (front, back) = (number(p.nodes[0]), number(p.nodes[1]));
            let reversed = p
                .winding
                .plane()
                .is_some_and(|w| dot(p.plane.plane().normal, w.normal) < 1.0 - ANGLE_EPSILON);
            let (a, b) = if reversed {
                (back, front)
            } else {
                (front, back)
            };
            let mut line = format!("{} {a} {b} ", p.winding.len());
            for point in p.winding.points() {
                let [x, y, z] = point.map(fmt_number);
                line.push_str(&format!("({x} {y} {z}) "));
            }
            out.push(line);
        }
    }

    /// The portal file of the pruned tree, `WritePortalFile` (QBSP-15.51): its vis portals made
    /// with no portals around the head, stopping at detail separators; `PRT1` numbered by leaf
    /// without detail, else `PRT2` numbered by cluster with each cluster's leaves listed.
    pub(crate) fn portal_file(&mut self, portalizer: &Portalizer<'_>) -> Vec<u8> {
        portalizer.make_vis_portals(self);
        let mut n = Numbering::default();
        self.number_leaves(Self::HEAD, None, &mut n);
        let mut portals = Vec::new();
        let by = if n.uses_detail {
            NumberedBy::Cluster
        } else {
            NumberedBy::Leaf
        };
        self.write_portals(Self::HEAD, by, &n, &mut portals);
        let mut text = String::new();
        if n.uses_detail {
            text.push_str(&format!(
                "PRT2
{}
{}
{}
",
                n.leaves,
                n.clusters,
                portals.len()
            ));
        } else {
            text.push_str(&format!("PRT1\n{}\n{}\n", n.leaves, portals.len()));
        }
        for line in portals {
            text.push_str(&line);
            text.push('\n');
        }
        if n.uses_detail {
            let mut by_cluster: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
            let mut leaves: Vec<(i64, i64)> = n
                .leaf
                .iter()
                .filter(|(_, leaf)| **leaf >= 0)
                .filter_map(|(node, &leaf)| Some((leaf, *n.cluster.get(node)?)))
                .collect();
            leaves.sort_unstable();
            for (leaf, cluster) in leaves {
                by_cluster.entry(cluster).or_default().push(leaf);
            }
            for cluster in 0..n.clusters {
                for leaf in by_cluster.get(&cluster).into_iter().flatten() {
                    text.push_str(&format!("{leaf} "));
                }
                text.push_str("-1\n");
            }
        }
        text.into_bytes()
    }
}

impl Portalizer<'_> {
    /// The tree's vis portals, `WritePortalFile`'s: the head's six made for the outside leaf and
    /// not used, every node's made from none around the head.
    pub(crate) fn make_vis_portals(&self, tree: &mut Tree) {
        self.make_portals_from(tree, PortalType::Vis, HeadPortals::Unused);
    }
}

#[cfg(test)]
mod tests;
