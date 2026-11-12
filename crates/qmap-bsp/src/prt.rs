// SPDX-License-Identifier: GPL-3.0-or-later

//! The portal file `vis` reads (QBSP-12).

use std::fmt::Write as _;

use crate::Contents;
use crate::math::{Vec3, cross, dot, normalize, rint, sub, within};
use crate::planes::Planes;
use crate::portals::Portals;
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
        _ => [0.0; 3],
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
        let _ = write!(out, "{} ", whole as i32);
    } else {
        let _ = write!(out, "{:.6} ", f64::from(v));
    }
}

/// The leaves in tree order, front first.
fn leaves(tree: &Tree) -> Vec<usize> {
    let mut order = Vec::new();
    let mut work = vec![tree.head()];
    while let Some(node) = work.pop() {
        match tree.nodes.get(node).map(|n| &n.kind) {
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

/// `WritePortalfile`: the leaves numbered, solid ones skipped, and each portal between two
/// leaves of the same contents written once from its front leaf (QBSP-12).
pub(crate) fn portal_file(tree: &Tree, portals: &Portals, planes: &Planes) -> Vec<u8> {
    let order = leaves(tree);
    let mut numbers = vec![None; tree.nodes.len()];
    let mut count = 0_usize;
    let mut portal_count = 0_usize;
    let same_contents = |id: usize| {
        portals
            .get(id)
            .is_some_and(|p| tree.contents(p.nodes[0]) == tree.contents(p.nodes[1]))
    };
    for &leaf in &order {
        if tree.contents(leaf) == Some(Contents::Solid) {
            continue;
        }
        if let Some(slot) = numbers.get_mut(leaf) {
            *slot = Some(count);
        }
        count += 1;
        if let Some(node) = tree.nodes.get(leaf) {
            portal_count += node
                .portals
                .iter()
                .filter(|&&id| {
                    portals.get(id).is_some_and(|p| p.nodes[0] == leaf) && same_contents(id)
                })
                .count();
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
        if tree.contents(leaf) == Some(Contents::Solid) {
            continue;
        }
        let Some(node) = tree.nodes.get(leaf) else {
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
            let _ = write!(
                out,
                "{} {} {} ",
                portal.winding.len(),
                number(first),
                number(second)
            );
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
