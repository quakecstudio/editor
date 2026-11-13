// SPDX-License-Identifier: GPL-3.0-or-later

//! `TestLine`: whether a line between two points crosses only empty space of the world model's
//! tree, which only solid leaves block (LIGHT-4.1).

use qs_bsp::{BSP_CONTENTS_SOLID, BSP_PLANE_X, BSP_PLANE_Y, BSP_PLANE_Z, Bsp};

use crate::Vec3;

/// `ON_EPSILON`, within which a line's end is on a node's plane, a `double`: each `float`
/// distance is widened to be compared with it (LIGHT-4.1, LIGHT-4.9).
const LIGHT_ON_EPSILON: f64 = 0.1;

/// Which of a plane's normal's components its distance is taken along.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
    Z,
    Any,
}

/// What a node's child is: another node, or a leaf's contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Child {
    Node(usize),
    Contents(i32),
}

/// A node of the tree as `MakeTnode` keeps it: its plane and children.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Tnode {
    axis: Axis,
    normal: Vec3,
    dist: f32,
    children: [Child; 2],
}

/// The world's tree, for tracing lines (`MakeTnodes`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tree {
    nodes: Vec<Tnode>,
}

/// A split line's far part, still to trace: where it ends, the node and the side its near part
/// took.
struct Pending {
    back: Vec3,
    node: usize,
    side: usize,
}

impl Tree {
    /// The tree from node 0, each leaf child its leaf's contents (LIGHT-4.1).
    pub(crate) fn new(bsp: &Bsp<'_>) -> Self {
        let child = |c: i32| match usize::try_from(c) {
            Ok(node) => Child::Node(node),
            Err(_) => {
                let leaf = usize::try_from(c.saturating_neg().saturating_sub(1)).ok();
                let contents = leaf
                    .and_then(|leaf| bsp.leafs.get(leaf))
                    .map_or(BSP_CONTENTS_SOLID, |leaf| leaf.contents);
                Child::Contents(contents)
            }
        };
        let nodes = bsp
            .nodes
            .iter()
            .map(|node| {
                let plane = bsp.planes.get(node.planenum);
                let axis = match plane.map(|p| p.plane_type) {
                    Some(BSP_PLANE_X) => Axis::X,
                    Some(BSP_PLANE_Y) => Axis::Y,
                    Some(BSP_PLANE_Z) => Axis::Z,
                    _ => Axis::Any,
                };
                Tnode {
                    axis,
                    normal: plane.map_or([0.0; crate::AXES], |p| p.normal),
                    dist: plane.map_or(0.0, |p| p.dist),
                    children: node.children.map(child),
                }
            })
            .collect();
        Self { nodes }
    }

    /// `TestLine`: whether the line from `start` to `stop` reaches its end. A line wholly in
    /// front of a node's plane, by more than `-ON_EPSILON`, goes to the front; else one wholly
    /// behind by less than `ON_EPSILON` to the back; else it is split there, the part on the
    /// start's side first (LIGHT-4.1). The stack of split parts has no limit (LIGHT-4.8).
    pub(crate) fn test_line(&self, start: Vec3, stop: Vec3) -> bool {
        let mut front = start;
        let mut back = stop;
        let mut stack: Vec<Pending> = Vec::new();
        let mut child = Child::Node(0);
        loop {
            while let Child::Contents(contents) = child {
                if contents == BSP_CONTENTS_SOLID {
                    return false;
                }
                let Some(pending) = stack.pop() else {
                    return true;
                };
                front = back;
                back = pending.back;
                child = self
                    .nodes
                    .get(pending.node)
                    .and_then(|n| n.children.get(1_usize.saturating_sub(pending.side)))
                    .copied()
                    .unwrap_or(Child::Contents(BSP_CONTENTS_SOLID));
            }
            let Child::Node(index) = child else {
                continue;
            };
            let Some(node) = self.nodes.get(index) else {
                return false;
            };
            let (f, b) = (node.side(front), node.side(back));
            let (wide_f, wide_b) = (f64::from(f), f64::from(b));
            let [ahead, behind] = node.children;
            if wide_f > -LIGHT_ON_EPSILON && wide_b > -LIGHT_ON_EPSILON {
                child = ahead;
                continue;
            }
            if wide_f < LIGHT_ON_EPSILON && wide_b < LIGHT_ON_EPSILON {
                child = behind;
                continue;
            }
            let side = usize::from(f < 0.0);
            let t = f / (f - b);
            stack.push(Pending {
                back,
                node: index,
                side,
            });
            back = [0, 1, 2].map(|i| {
                let (a, z) = (
                    front.get(i).copied().unwrap_or_default(),
                    back.get(i).copied().unwrap_or_default(),
                );
                a + t * (z - a)
            });
            child = node.children.get(side).copied().unwrap_or(behind);
        }
    }
}

impl Tree {
    /// `CastRay`: the distance between the points when the line between them reaches, 1 for two
    /// equal points; none when it is blocked (LIGHT-4.1).
    pub(crate) fn cast_ray(&self, from: Vec3, to: Vec3) -> Option<f32> {
        if !self.test_line(from, to) {
            return None;
        }
        let mut t = 0.0_f32;
        for (a, b) in from.iter().zip(&to) {
            let d = b - a;
            t += d * d;
        }
        if t == 0.0 {
            t = 1.0;
        }
        Some(crate::to_f32(f64::from(t).sqrt()))
    }
}

impl Tnode {
    /// How far `point` is in front of the node's plane, an axial plane by its axis alone.
    fn side(&self, point: Vec3) -> f32 {
        let [x, y, z] = point;
        match self.axis {
            Axis::X => x - self.dist,
            Axis::Y => y - self.dist,
            Axis::Z => z - self.dist,
            Axis::Any => {
                let [a, b, c] = self.normal;
                (x * a + y * b + z * c) - self.dist
            }
        }
    }
}

#[cfg(test)]
impl Tree {
    /// A chain of nodes on the planes `x = dists[i]`, each one's front the next node (the
    /// last's empty) and its back empty, but the first node's back `first_back`.
    pub(crate) fn chain(dists: &[f32], first_back: i32) -> Self {
        let nodes = dists
            .iter()
            .enumerate()
            .map(|(i, &dist)| Tnode {
                axis: Axis::X,
                normal: [1.0, 0.0, 0.0],
                dist,
                children: [
                    if i.saturating_add(1) < dists.len() {
                        Child::Node(i.saturating_add(1))
                    } else {
                        Child::Contents(qs_bsp::BSP_CONTENTS_EMPTY)
                    },
                    Child::Contents(if i == 0 {
                        first_back
                    } else {
                        qs_bsp::BSP_CONTENTS_EMPTY
                    }),
                ],
            })
            .collect();
        Self { nodes }
    }
}
