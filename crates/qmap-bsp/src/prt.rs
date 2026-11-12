// SPDX-License-Identifier: GPL-3.0-or-later

//! The portal file `vis` reads (QBSP-12).

use crate::Contents;
use crate::math::{ORIGIN, Vec3, cross, dot, normalize, rint, sub, within};
use crate::planes::Planes;
use crate::portals::{PortalId, Portals};
use crate::tree::{NodeKind, Tree};

/// The portal file's first line (QBSP-12.2).
const PORTAL_FILE: &str = "PRT1";
/// How near a whole number a coordinate is written as one (QBSP-12.4).
const WHOLE_EPSILON: f64 = 0.001;
/// How near the portal's plane its winding must face to keep the leaves' order (QBSP-12.3).
const SAME_WAY: f64 = 0.99;

/// `PlaneFromWinding`'s normal.
fn winding_normal(w: &[Vec3]) -> Vec3 {
    match w {
        [p0, p1, p2, ..] => normalize(cross(sub(*p0, *p1), sub(*p2, *p1))),
        _ => ORIGIN,
    }
}

/// `WriteFloat` (QBSP-12.4).
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec QBSP-12.4: qbsp writes the rounded value as an int"
)]
fn write_float(out: &mut String, v: f32) {
    let whole = rint(v);
    if within(v, whole, WHOLE_EPSILON) {
        out.push_str(&format!("{} ", whole as i32));
    } else {
        out.push_str(&format!("{:.6} ", f64::from(v)));
    }
}

impl Tree {
    /// The leaves in tree order, front first.
    pub(crate) fn leaves_in_order(&self) -> Vec<usize> {
        let mut order = Vec::new();
        let mut work = vec![self.head()];
        while let Some(node) = work.pop() {
            match self.nodes.get(node).map(|n| &n.kind) {
                Some(NodeKind::Decision { children, .. }) => {
                    work.push(children[1]);
                    work.push(children[0]);
                }
                Some(NodeKind::Leaf { .. }) => order.push(node),
                None => {}
            }
        }
        order
    }
}

impl Tree {
    /// `WritePortalfile`: the leaves numbered, solid ones skipped, and each portal between two
    /// leaves of the same contents written once from its front leaf (QBSP-12).
    pub(crate) fn portal_file(&self, portals: &Portals, planes: &Planes) -> Vec<u8> {
        let order = self.leaves_in_order();
        let mut numbers = vec![None; self.nodes.len()];
        let mut count = 0_usize;
        let mut portal_count = 0_usize;
        let same_contents = |id: PortalId| {
            portals
                .get(id)
                .is_some_and(|p| self.contents(p.nodes[0]) == self.contents(p.nodes[1]))
        };
        for &leaf in &order {
            if self.contents(leaf) == Some(Contents::Solid) {
                continue;
            }
            if let Some(slot) = numbers.get_mut(leaf) {
                *slot = Some(count);
            }
            count = count.saturating_add(1);
            if let Some(node) = self.nodes.get(leaf) {
                portal_count = portal_count.saturating_add(
                    node.portals
                        .iter()
                        .filter(|&&id| {
                            portals.get(id).is_some_and(|p| p.nodes[0] == leaf) && same_contents(id)
                        })
                        .count(),
                );
            }
        }
        let number = |node: usize| {
            numbers
                .get(node)
                .copied()
                .flatten()
                .map_or(-1, |n| i64::try_from(n).unwrap_or(-1))
        };
        let mut out = format!("{PORTAL_FILE}\n{count}\n{portal_count}\n");
        for &leaf in &order {
            if self.contents(leaf) == Some(Contents::Solid) {
                continue;
            }
            let Some(node) = self.nodes.get(leaf) else {
                continue;
            };
            for &id in &node.portals {
                let Some(portal) = portals.get(id) else {
                    continue;
                };
                if portal.nodes[0] != leaf || !same_contents(id) || portal.winding.is_empty() {
                    continue;
                }
                let plane = planes.get(portal.plane);
                let [first, second] =
                    if f64::from(dot(plane.normal, winding_normal(&portal.winding))) < SAME_WAY {
                        [portal.nodes[1], portal.nodes[0]]
                    } else {
                        portal.nodes
                    };
                out.push_str(&format!(
                    "{} {} {} ",
                    portal.winding.len(),
                    number(first),
                    number(second)
                ));
                for point in &portal.winding {
                    out.push('(');
                    for &c in point {
                        write_float(&mut out, c);
                    }
                    out.push_str(") ");
                }
                out.push('\n');
            }
        }
        out.into_bytes()
    }
}
