// SPDX-License-Identifier: GPL-3.0-or-later

//! Portals: the polygons joining the tree's leaves to each other and to the outside
//! (QBSP-8).

use crate::math::{Axis, Vec3};
use crate::planes::{Plane, PlaneId, Planes, Side};
use crate::tree::{Node, NodeKind, Tree};
use crate::winding::{KeepOn, clip, divide};
use crate::{CompileError, Contents, Warning};

/// A portal: a polygon on a plane joining the node in front of it to the one behind.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Portal {
    pub(crate) plane: PlaneId,
    /// The node in front of the plane, then the one behind.
    pub(crate) nodes: [usize; 2],
    pub(crate) winding: Vec<Vec3>,
}

/// A portal of [`Portals`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PortalId(usize);

/// A tree's portals, and the outside node the head node's face (QBSP-8.1).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Portals {
    pub(crate) list: Vec<Option<Portal>>,
    pub(crate) outside: usize,
}

impl Portals {
    /// `AddPortalToNodes`: the portal joins `front` and `back`, at the head of each's list.
    fn add(&mut self, tree: &mut Tree, portal: Portal) -> PortalId {
        let id = PortalId(self.list.len());
        let [front, back] = portal.nodes;
        self.list.push(Some(portal));
        for node in [front, back] {
            if let Some(n) = tree.nodes.get_mut(node) {
                n.portals.insert(0, id);
            }
        }
        id
    }

    /// `RemovePortalFromNode`, from both its nodes.
    fn detach(tree: &mut Tree, id: PortalId, nodes: [usize; 2]) {
        for node in nodes {
            if let Some(n) = tree.nodes.get_mut(node) {
                n.portals.retain(|&p| p != id);
            }
        }
    }

    /// The portal `id`.
    pub(crate) fn get(&self, id: PortalId) -> Option<&Portal> {
        self.list.get(id.0).and_then(Option::as_ref)
    }

    /// The portal `id`, to change.
    fn get_mut(&mut self, id: PortalId) -> Option<&mut Portal> {
        self.list.get_mut(id.0).and_then(Option::as_mut)
    }
}

impl Tree {
    /// Builds every portal of `tree`, whose head bounds are `mins` to `maxs`, `PortalizeWorld`
    /// (QBSP-8).
    pub(crate) fn portalize(
        &mut self,
        planes: &mut Planes,
        mins: Vec3,
        maxs: Vec3,
        warnings: &mut Vec<Warning>,
    ) -> Result<Portals, CompileError> {
        self.nodes.push(Node {
            kind: NodeKind::Leaf {
                contents: Contents::Solid,
                marks: Vec::new(),
            },
            mins,
            maxs,
            portals: Vec::new(),
            occupied: false,
        });
        let mut portals = Portals {
            list: Vec::new(),
            outside: self.nodes.len().saturating_sub(1),
        };
        let head = self.head();
        // `MakeHeadnodePortals` (QBSP-8.1).
        // qbsp makes them axis by axis, minimum then maximum, and numbers them `j * 3 + i`.
        let mut low: [Option<(Plane, PortalId)>; Axis::ALL.len()] = [None; Axis::ALL.len()];
        let mut high: [Option<(Plane, PortalId)>; Axis::ALL.len()] = [None; Axis::ALL.len()];
        for (axis, (low, high)) in Axis::ALL
            .into_iter()
            .zip(low.iter_mut().zip(high.iter_mut()))
        {
            let sides = [
                (
                    low,
                    Plane {
                        normal: axis.along(1.0),
                        dist: axis.of(mins),
                    },
                ),
                (
                    high,
                    Plane {
                        normal: axis.along(-1.0),
                        dist: -axis.of(maxs),
                    },
                ),
            ];
            for (slot, plane) in sides {
                let (id, side) = planes.find(plane)?;
                let nodes = match side {
                    Side::Back => [portals.outside, head],
                    Side::Front => [head, portals.outside],
                };
                let portal = portals.add(
                    self,
                    Portal {
                        plane: id,
                        nodes,
                        winding: plane.base_winding(),
                    },
                );
                *slot = Some((plane, portal));
            }
        }
        let bounding = low.into_iter().chain(high).flatten();
        let (bounding, made): (Vec<Plane>, Vec<PortalId>) = bounding.unzip();
        for (i, &id) in made.iter().enumerate() {
            let mut winding = portals.get(id).map(|p| p.winding.clone());
            for (j, &plane) in bounding.iter().enumerate() {
                if j == i {
                    continue;
                }
                winding = match winding {
                    Some(points) => clip(&points, plane, KeepOn::Keep)?,
                    None => None,
                };
            }
            if let Some(portal) = portals.get_mut(id) {
                portal.winding = winding.unwrap_or_default();
            }
        }
        let mut work = vec![head];
        while let Some(node) = work.pop() {
            if let Some([front, back]) = self.cut_node(planes, &mut portals, node, warnings)? {
                work.push(back);
                work.push(front);
            }
        }
        Ok(portals)
    }
}

impl Tree {
    /// `CutNodePortals_r` at one node: its own portal made and its portals divided between its
    /// children; the children, or `None` at a leaf (QBSP-8.2, QBSP-8.3).
    pub(crate) fn cut_node(
        &mut self,
        planes: &Planes,
        portals: &mut Portals,
        node: usize,
        warnings: &mut Vec<Warning>,
    ) -> Result<Option<[usize; 2]>, CompileError> {
        let Some(Node {
            kind:
                NodeKind::Decision {
                    plane: plane_id,
                    children: [front, back],
                    ..
                },
            portals: list,
            ..
        }) = self.nodes.get(node).cloned()
        else {
            return Ok(None);
        };
        let plane = planes.get(plane_id);
        let mut winding = Some(plane.base_winding());
        for &id in &list {
            let Some(portal) = portals.get(id) else {
                continue;
            };
            let clip_plane = if portal.nodes[0] == node {
                planes.get(portal.plane)
            } else {
                planes.get(portal.plane).flipped()
            };
            winding = match winding {
                Some(points) => clip(&points, clip_plane, KeepOn::Keep)?,
                None => None,
            };
            if winding.is_none() {
                warnings.push(Warning::PortalClippedAway);
                break;
            }
        }
        if let Some(points) = winding {
            portals.add(
                self,
                Portal {
                    plane: plane_id,
                    nodes: [front, back],
                    winding: points,
                },
            );
        }
        for id in list {
            let Some(portal) = portals.get(id).cloned() else {
                continue;
            };
            let side = if portal.nodes[0] == node {
                Side::Front
            } else {
                Side::Back
            };
            let [ahead, behind] = portal.nodes;
            let other = match side {
                Side::Front => behind,
                Side::Back => ahead,
            };
            Portals::detach(self, id, portal.nodes);
            let (front_part, back_part) = divide(&portal.winding, plane)?;
            let joined = |child: usize| match side {
                Side::Front => [child, other],
                Side::Back => [other, child],
            };
            match (front_part, back_part) {
                (None, _) => {
                    let replaced = Portal {
                        nodes: joined(back),
                        ..portal
                    };
                    portals.reattach(self, id, replaced);
                }
                (Some(_), None) => {
                    let replaced = Portal {
                        nodes: joined(front),
                        ..portal
                    };
                    portals.reattach(self, id, replaced);
                }
                (Some(front_points), Some(back_points)) => {
                    let kept = Portal {
                        nodes: joined(front),
                        winding: front_points,
                        ..portal.clone()
                    };
                    portals.reattach(self, id, kept);
                    portals.add(
                        self,
                        Portal {
                            nodes: joined(back),
                            winding: back_points,
                            ..portal
                        },
                    );
                }
            }
        }
        Ok(Some([front, back]))
    }
}

impl Portals {
    /// Puts portal `id`, changed, back on its nodes' lists, at their heads.
    fn reattach(&mut self, tree: &mut Tree, id: PortalId, portal: Portal) {
        let [front, back] = portal.nodes;
        if let Some(slot) = self.list.get_mut(id.0) {
            *slot = Some(portal);
        }
        for node in [front, back] {
            if let Some(n) = tree.nodes.get_mut(node) {
                n.portals.insert(0, id);
            }
        }
    }
}
