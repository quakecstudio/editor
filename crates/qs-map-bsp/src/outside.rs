// SPDX-License-Identifier: GPL-3.0-or-later

//! The outside fill: every leaf the outside reaches without passing an entity is made solid,
//! and the faces only those leaves saw are dropped (QBSP-9).

use std::collections::BTreeMap;

use crate::math::{ORIGIN, Vec3, dot, same};
use crate::planes::{PlaneId, Planes};
use crate::portals::Portals;
use crate::tree::{NodeKind, Tree};
use crate::{Contents, Face, Surface};

/// How a fill ended (QBSP-9.2, QBSP-9.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fill {
    /// The outside was filled.
    Filled,
    /// No entity stands in empty space.
    NoOccupant,
    /// The outside reaches an entity.
    Leak,
}

impl Tree {
    /// `PointInLeaf`: the leaf holding `point`, front when strictly in front (QBSP-9.1).
    pub(crate) fn point_in_leaf(&self, planes: &Planes, point: Vec3) -> usize {
        let mut node = self.head();
        while let Some(NodeKind::Decision {
            plane, children, ..
        }) = self.nodes.get(node).map(|n| &n.kind)
        {
            let p = planes.get(*plane);
            node = if dot(p.normal, point) - p.dist > 0.0 {
                children[0]
            } else {
                children[1]
            };
        }
        node
    }
}

impl Tree {
    /// The leaves the outside reaches through portals, not passing solid or sky; `None` when it
    /// reaches an occupied leaf (QBSP-9.3).
    pub(crate) fn reached(&self, portals: &Portals) -> Option<Vec<usize>> {
        let outside = portals.outside;
        let first = self.nodes.get(outside)?.portals.first()?;
        let portal = portals.get(*first)?;
        let start = if portal.nodes[1] == outside {
            portal.nodes[0]
        } else {
            portal.nodes[1]
        };
        let mut seen = vec![false; self.nodes.len()];
        let mut order = Vec::new();
        let mut work = vec![start];
        while let Some(leaf) = work.pop() {
            let Some(node) = self.nodes.get(leaf) else {
                continue;
            };
            if matches!(
                self.contents(leaf),
                Some(Contents::Solid | Contents::Sky) | None
            ) {
                continue;
            }
            if seen.get(leaf).copied().unwrap_or(true) {
                continue;
            }
            if node.occupied {
                return None;
            }
            if let Some(slot) = seen.get_mut(leaf) {
                *slot = true;
            }
            order.push(leaf);
            for &id in node.portals.iter().rev() {
                if let Some(portal) = portals.get(id) {
                    let other = if portal.nodes[0] == leaf {
                        portal.nodes[1]
                    } else {
                        portal.nodes[0]
                    };
                    work.push(other);
                }
            }
        }
        Some(order)
    }
}

impl Tree {
    /// `FillOutside` with the entities' origins after the world's: the occupants placed, the
    /// outside filled and the faces of solid leaves dropped (QBSP-9.1 to QBSP-9.4).
    pub(crate) fn fill_outside(
        &mut self,
        planes: &Planes,
        portals: &Portals,
        origins: &[Vec3],
    ) -> Fill {
        let mut inside = false;
        for &origin in origins {
            if same(origin, ORIGIN) {
                continue;
            }
            let leaf = self.point_in_leaf(planes, origin);
            if self.contents(leaf) == Some(Contents::Solid) {
                continue;
            }
            if let Some(node) = self.nodes.get_mut(leaf) {
                node.occupied = true;
            }
            inside = true;
        }
        if !inside {
            return Fill::NoOccupant;
        }
        let Some(outside) = self.reached(portals) else {
            return Fill::Leak;
        };
        for leaf in outside {
            if let Some(NodeKind::Leaf { contents, .. }) =
                self.nodes.get_mut(leaf).map(|n| &mut n.kind)
            {
                *contents = Contents::Solid;
            }
        }
        // `ClearOutFaces`: every face a solid leaf marks is emptied.
        let mut removed = Vec::new();
        for node in &self.nodes {
            if let NodeKind::Leaf {
                contents: Contents::Solid,
                marks,
            } = &node.kind
            {
                removed.extend(marks.iter().copied());
            }
        }
        for id in removed {
            if let Some(face) = self.faces.get_mut(id) {
                face.removed = true;
            }
        }
        Fill::Filled
    }
}

impl Tree {
    /// `GatherNodeFaces`: the faces left on the nodes, each plane's into a surface, as
    /// `BuildSurfaces` lists them (QBSP-9.6).
    pub(crate) fn gather(&self) -> Vec<Surface> {
        let mut valid: BTreeMap<PlaneId, Vec<Face>> = BTreeMap::new();
        let mut work = vec![self.head()];
        while let Some(node) = work.pop() {
            let Some(NodeKind::Decision {
                children, faces, ..
            }) = self.nodes.get(node).map(|n| &n.kind)
            else {
                continue;
            };
            for &id in faces {
                if let Some(kept) = self.faces.get(id).filter(|f| !f.removed) {
                    let mut face = kept.face.clone();
                    face.original = None;
                    valid.entry(face.plane).or_default().insert(0, face);
                }
            }
            work.push(children[1]);
            work.push(children[0]);
        }
        let mut surfaces = Vec::new();
        for (plane, faces) in valid {
            if !faces.is_empty() {
                surfaces.insert(0, Surface::new(plane, faces));
            }
        }
        surfaces
    }
}
