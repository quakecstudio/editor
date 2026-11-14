// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/prtfile.cc, WritePortals_r, NumberLeafs_r, WritePortalfile, WritePortalFile
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: common/prtfile.cc, WritePortal, WritePortalfile

//! The portal file vis reads, ericw-tools' `WritePortalFile` (QBSP-15.51): the portals of the
//! pruned tree, numbered by leaf as `PRT1`, which `qs-map-vis` reads.

use std::collections::HashMap;

use super::portals::{HeadPortals, PortalType, Portalizer};
use super::tree::{NodeId, Tree};
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

impl Tree {
    /// `NumberLeafs_r` with no cluster: each leaf not any solid numbered in order.
    fn number_leaves(&self) -> HashMap<NodeId, i64> {
        let mut numbers = HashMap::new();
        let mut next = 0_i64;
        for leaf in self.leaves_below(Self::HEAD) {
            if self
                .leaf_data(leaf)
                .is_some_and(|l| !l.contents.is_any_solid())
            {
                numbers.insert(leaf, next);
                next = next.saturating_add(1);
            }
        }
        numbers
    }

    /// `WritePortals_r` by leaf: for each leaf not any solid, each portal it is in front of that
    /// vis sees through, as its two leaf numbers, swapped when the polygon faces away from the
    /// plane, and its points.
    fn write_portals(&self, numbers: &HashMap<NodeId, i64>) -> Vec<String> {
        let mut out = Vec::new();
        for leaf in self.leaves_below(Self::HEAD) {
            if !numbers.contains_key(&leaf) {
                continue;
            }
            for portal in self.portals_of(leaf) {
                let Some(p) = self.portal(portal) else {
                    continue;
                };
                if p.winding.is_empty() || p.nodes[0] != leaf || !self.vis_floods(portal) {
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
        out
    }

    /// The portal file of the pruned tree, `WritePortalFile` (QBSP-15.51): `PRT1`, numbered by
    /// leaf, its portals made from none around the head and through detail separators too, then
    /// the vis portals, which stop at them, made for the detail fence fixup.
    pub(crate) fn portal_file(&mut self, portalizer: &Portalizer<'_>) -> Vec<u8> {
        portalizer.make_portals_from(self, PortalType::Tree, HeadPortals::Unused);
        let numbers = self.number_leaves();
        let portals = self.write_portals(&numbers);
        let mut text = format!("PRT1\n{}\n{}\n", numbers.len(), portals.len());
        for line in portals {
            text.push_str(&line);
            text.push('\n');
        }
        portalizer.make_vis_portals(self);
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
