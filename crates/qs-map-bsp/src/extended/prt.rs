// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/prtfile.cc, WritePortals_r, WritePTR2ClusterMapping_r, NumberLeafs_r, WritePortalfile, WritePortalFile
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/prtfile.cc, WritePortal, WritePTR2ClusterMapping, WritePortalfile

//! The portal file vis reads, ericw-tools' `WritePortalFile` (QBSP-15.52): the vis portals of
//! the pruned tree, numbered by leaf as `PRT1`, or by cluster as `PRT2` when the map has detail.

use std::collections::HashMap;

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
/// exponent of at least two digits below 1e-4 or from 1e16 (QBSP-15.52).
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

/// Which portal file is written: by leaf, or by cluster when a detail separator made one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Format {
    /// `PRT1`, numbered by leaf.
    Leaves,
    /// `PRT2`, numbered by cluster, with each cluster's leaves.
    Clusters,
}

/// The vis leaves' and clusters' numbers, `NumberLeafs_r`'s: each leaf not any solid by its
/// place, and each cluster, a detail separator not under one or a leaf not under one, by its
/// place in the walk.
struct Numbers {
    leaves: HashMap<NodeId, i64>,
    clusters: HashMap<NodeId, i64>,
    count: i64,
    /// `PRT2` when a detail separator made a cluster, else `PRT1`.
    format: Format,
}

impl Numbers {
    /// The numbers the portals are written by.
    const fn by(&self) -> &HashMap<NodeId, i64> {
        match self.format {
            Format::Leaves => &self.leaves,
            Format::Clusters => &self.clusters,
        }
    }

    /// The file's header for `portals` portals.
    fn header(&self, portals: usize) -> String {
        let leaves = self.leaves.len();
        match self.format {
            Format::Leaves => format!("PRT1\n{leaves}\n{portals}\n"),
            Format::Clusters => format!("PRT2\n{leaves}\n{}\n{portals}\n", self.count),
        }
    }

    /// The leaves in each cluster, in leaf order, a line each: `WritePTR2ClusterMapping`.
    fn cluster_map(&self) -> String {
        let mut by_cluster: Vec<Vec<i64>> =
            vec![Vec::new(); usize::try_from(self.count).unwrap_or(0)];
        let mut leaves: Vec<(i64, i64)> = self
            .leaves
            .iter()
            .filter_map(|(node, &leaf)| Some((leaf, *self.clusters.get(node)?)))
            .collect();
        leaves.sort_unstable();
        for (leaf, cluster) in leaves {
            if let Some(list) = usize::try_from(cluster)
                .ok()
                .and_then(|c| by_cluster.get_mut(c))
            {
                list.push(leaf);
            }
        }
        let mut out = String::new();
        for list in by_cluster {
            for leaf in list {
                out.push_str(&format!("{leaf} "));
            }
            out.push_str("-1\n");
        }
        out
    }
}

impl Tree {
    /// `NumberLeafs_r` (QBSP-15.52): front first, a detail separator not under another cluster
    /// starts one that every leaf below it is in; a leaf not under one is its own.
    fn number_leaves(&self) -> Numbers {
        let mut numbers = Numbers {
            leaves: HashMap::new(),
            clusters: HashMap::new(),
            count: 0,
            format: Format::Leaves,
        };
        let mut next_leaf = 0_i64;
        let mut stack: Vec<(NodeId, Option<i64>)> = vec![(Self::HEAD, None)];
        while let Some((at, mut cluster)) = stack.pop() {
            if let Some((_, [front, back], separator)) = self.split_of(at) {
                if cluster.is_none() && separator == Separator::Detail {
                    numbers.format = Format::Clusters;
                    numbers.clusters.insert(at, numbers.count);
                    cluster = Some(numbers.count);
                    numbers.count = numbers.count.saturating_add(1);
                }
                stack.push((back, cluster));
                stack.push((front, cluster));
                continue;
            }
            if self
                .leaf_data(at)
                .is_none_or(|leaf| leaf.contents.is_any_solid())
            {
                continue;
            }
            numbers.leaves.insert(at, next_leaf);
            next_leaf = next_leaf.saturating_add(1);
            let own = cluster.unwrap_or_else(|| {
                let own = numbers.count;
                numbers.count = numbers.count.saturating_add(1);
                own
            });
            numbers.clusters.insert(at, own);
        }
        numbers
    }

    /// `WritePortals_r`: for each leaf not any solid, or each cluster when the file is `PRT2`,
    /// each portal it is in front of that vis sees through, as its two numbers, swapped when
    /// the polygon faces away from the plane, and its points.
    fn write_portals(&self, numbers: &Numbers) -> Vec<String> {
        let by = numbers.by();
        let mut out = Vec::new();
        let mut stack = vec![Self::HEAD];
        while let Some(at) = stack.pop() {
            if let Some((_, [front, back], separator)) = self.split_of(at)
                && separator != Separator::Detail
            {
                stack.push(back);
                stack.push(front);
                continue;
            }
            if self
                .leaf_data(at)
                .is_some_and(|leaf| leaf.contents.is_any_solid())
            {
                continue;
            }
            for portal in self.portals_of(at) {
                let Some(p) = self.portal(portal) else {
                    continue;
                };
                if p.winding.is_empty() || p.nodes[0] != at || !self.vis_floods(portal) {
                    continue;
                }
                let number = |node: NodeId| by.get(&node).copied().unwrap_or(-1);
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
        out
    }

    /// The portal file, `WritePortalFile` (QBSP-15.52): the vis portals, which stop at detail
    /// separators, made from none around the head; `PRT1` numbered by leaf when no detail
    /// separator made a cluster, else `PRT2` numbered by cluster with the leaves of each.
    pub(crate) fn portal_file(&mut self, portalizer: &Portalizer<'_>) -> Vec<u8> {
        portalizer.make_portals_from(self, PortalType::Vis, HeadPortals::Unused);
        let numbers = self.number_leaves();
        let portals = self.write_portals(&numbers);
        let mut text = numbers.header(portals.len());
        for line in portals {
            text.push_str(&line);
            text.push('\n');
        }
        if numbers.format == Format::Clusters {
            text.push_str(&numbers.cluster_map());
        }
        text.into_bytes()
    }
}

#[cfg(test)]
mod tests;
