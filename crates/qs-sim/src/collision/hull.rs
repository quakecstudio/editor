// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/world.c, SV_InitBoxHull, SV_HullForBox, SV_HullForEntity, SV_HullPointContents
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/model.c, Mod_LoadClipnodes, Mod_MakeHull0, Mod_LoadBrushModel

//! Hulls: the clipnode trees a box is clipped against (WORLD-1), and the contents of a point
//! in one (WORLD-2).

use qs_bsp::Bsp;

use super::{Body, CollisionError, Solid};

/// `CONTENTS_EMPTY` (WORLD-2).
pub const CONTENTS_EMPTY: i32 = -1;
/// `CONTENTS_SOLID` (WORLD-2).
pub const CONTENTS_SOLID: i32 = -2;
/// `CONTENTS_WATER` (WORLD-2.2).
pub const CONTENTS_WATER: i32 = -3;
/// `CONTENTS_CURRENT_0`, the first current (WORLD-2.2).
pub const CONTENTS_CURRENT_0: i32 = -9;
/// `CONTENTS_CURRENT_DOWN`, the last current (WORLD-2.2).
pub const CONTENTS_CURRENT_DOWN: i32 = -14;

/// Hull 1's box, the player's (WORLD-1.2).
pub const HULL1: ([f32; 3], [f32; 3]) = ([-16.0, -16.0, -24.0], [16.0, 16.0, 32.0]);
/// Hull 2's box (WORLD-1.2).
pub const HULL2: ([f32; 3], [f32; 3]) = ([-32.0, -32.0, -24.0], [32.0, 32.0, 64.0]);
/// `size[0] < 3` picks hull 0 (WORLD-1.4).
const POINT_SIZE: f32 = 3.0;
/// `size[0] <= 32` picks hull 1 (WORLD-1.4).
const PLAYER_SIZE: f32 = 32.0;

/// A splitting plane: its normal, its distance and its type, 0 to 2 for a plane on an axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// The normal.
    pub normal: [f32; 3],
    /// The distance from the origin.
    pub dist: f32,
    /// The stored type.
    pub kind: i32,
}

impl Plane {
    /// The distance of `p` from the plane, `p[type] - dist` for a plane on an axis, else
    /// `DotProduct (normal, p) - dist` (WORLD-2.1).
    #[must_use]
    pub fn distance(&self, p: [f32; 3]) -> f32 {
        match usize::try_from(self.kind).ok().and_then(|axis| p.get(axis)) {
            Some(&value) if self.kind < 3 => value - self.dist,
            _ => {
                let [a, b, c] = self.normal;
                let [x, y, z] = p;
                a * x + b * y + c * z - self.dist
            }
        }
    }
}

/// A clipnode: its plane and its children, each a clipnode or a contents value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipNode {
    /// The index of its plane.
    pub plane: usize,
    /// Child 0, in front of the plane, and child 1, behind it.
    pub children: [i32; 2],
}

/// The clipnodes and planes of one BSP, which each of its models' hulls walk (WORLD-1.1,
/// WORLD-1.2).
#[derive(Clone, Debug, PartialEq)]
pub struct HullSet {
    planes: Vec<Plane>,
    /// Hull 0: the drawing nodes as clipnodes.
    nodes: Vec<ClipNode>,
    /// Hulls 1 and 2.
    clipnodes: Vec<ClipNode>,
}

impl HullSet {
    /// The hulls of `bsp`.
    #[must_use]
    pub fn new(bsp: &Bsp<'_>) -> Self {
        let planes = bsp
            .planes
            .iter()
            .map(|p| Plane {
                normal: p.normal,
                dist: p.dist,
                kind: p.plane_type,
            })
            .collect();
        // WORLD-1.1: a child leaf is its contents.
        let child = |c: i32| {
            if c >= 0 {
                return c;
            }
            let leaf = c.checked_neg().and_then(|n| n.checked_sub(1));
            leaf.and_then(|l| usize::try_from(l).ok())
                .and_then(|l| bsp.leafs.get(l))
                .map_or(CONTENTS_SOLID, |leaf| leaf.contents)
        };
        let nodes = bsp
            .nodes
            .iter()
            .map(|n| ClipNode {
                plane: n.planenum,
                children: n.children.map(child),
            })
            .collect();
        let clipnodes = bsp
            .clipnodes
            .iter()
            .map(|c| ClipNode {
                plane: usize::try_from(c.planenum).unwrap_or(usize::MAX),
                children: c.children,
            })
            .collect();
        Self {
            planes,
            nodes,
            clipnodes,
        }
    }

    /// Hull `index` (0 to 2) of the model whose head nodes are `heads` (WORLD-1.1, WORLD-1.2).
    #[must_use]
    pub fn hull(&self, index: usize, heads: [i32; 3]) -> Hull<'_> {
        let first = heads.get(index).copied().unwrap_or(0);
        match index {
            0 => Hull {
                planes: &self.planes,
                nodes: &self.nodes,
                first,
                last: last_index(self.nodes.len()),
                clip_mins: [0.0; 3],
            },
            1 => Hull {
                planes: &self.planes,
                nodes: &self.clipnodes,
                first,
                last: last_index(self.clipnodes.len()),
                clip_mins: HULL1.0,
            },
            _ => Hull {
                planes: &self.planes,
                nodes: &self.clipnodes,
                first,
                last: last_index(self.clipnodes.len()),
                clip_mins: HULL2.0,
            },
        }
    }
}

/// `count - 1`, the last index a hull of `count` clipnodes may visit.
fn last_index(count: usize) -> i32 {
    i32::try_from(count).map_or(i32::MAX, |c| c.saturating_sub(1))
}

/// A brush model the server clips against: the hulls it uses and its bounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushModel {
    /// The [`HullSet`] of its BSP.
    pub set: usize,
    /// Its head node in hulls 0, 1 and 2.
    pub heads: [i32; 3],
    /// Its bounds.
    pub mins: [f32; 3],
    /// Its bounds.
    pub maxs: [f32; 3],
}

/// A hull: a tree of clipnodes over planes, the indices a walk may visit, and the box it was
/// built for.
#[derive(Clone, Copy, Debug)]
pub struct Hull<'a> {
    planes: &'a [Plane],
    nodes: &'a [ClipNode],
    /// `firstclipnode`.
    pub first: i32,
    /// `lastclipnode`.
    pub last: i32,
    /// `clip_mins`.
    pub clip_mins: [f32; 3],
}

impl<'a> Hull<'a> {
    /// Clipnode `num` and its plane; `name` is the function id's error names.
    ///
    /// # Errors
    ///
    /// [`CollisionError::BadNode`] outside `firstclipnode` to `lastclipnode` (WORLD-2.1,
    /// WORLD-3.2), and for a clipnode or plane the hull does not have.
    pub fn node(&self, num: i32, name: &'static str) -> Result<(ClipNode, Plane), CollisionError> {
        if num < self.first || num > self.last {
            return Err(CollisionError::BadNode(name));
        }
        let node = usize::try_from(num)
            .ok()
            .and_then(|n| self.nodes.get(n))
            .ok_or(CollisionError::BadNode(name))?;
        let plane = self
            .planes
            .get(node.plane)
            .ok_or(CollisionError::BadNode(name))?;
        Ok((*node, *plane))
    }

    /// `SV_HullPointContents`: the contents of `p` walking from clipnode `num` (WORLD-2.1).
    ///
    /// # Errors
    ///
    /// [`CollisionError::BadNode`] at a clipnode outside the hull, and when the walk visits
    /// more clipnodes than the hull has, which only a cycle can do.
    pub fn point_contents(&self, num: i32, p: [f32; 3]) -> Result<i32, CollisionError> {
        const NAME: &str = "SV_HullPointContents: bad node number";
        let mut num = num;
        let mut budget = self.nodes.len();
        while num >= 0 {
            budget = budget.checked_sub(1).ok_or(CollisionError::BadNode(NAME))?;
            let (node, plane) = self.node(num, NAME)?;
            let [front, back] = node.children;
            num = if plane.distance(p) < 0.0 { back } else { front };
        }
        Ok(num)
    }

    /// The number of clipnodes the hull has: no walk through a tree is longer.
    #[must_use]
    pub const fn size(&self) -> usize {
        self.nodes.len()
    }

    /// A hull over `planes` and `nodes`, such as a [`BoxHull`]'s.
    #[must_use]
    pub const fn over(planes: &'a [Plane], nodes: &'a [ClipNode], last: i32) -> Self {
        Self {
            planes,
            nodes,
            first: 0,
            last,
            clip_mins: [0.0; 3],
        }
    }
}

/// `SV_InitBoxHull` and `SV_HullForBox`: a box of `mins` to `maxs` as a hull of six
/// clipnodes (WORLD-1.3).
#[derive(Clone, Debug, PartialEq)]
pub struct BoxHull {
    planes: [Plane; 6],
    nodes: [ClipNode; 6],
}

impl BoxHull {
    /// The box of `mins` to `maxs`.
    #[must_use]
    pub fn new(mins: [f32; 3], maxs: [f32; 3]) -> Self {
        let dists = [maxs[0], mins[0], maxs[1], mins[1], maxs[2], mins[2]];
        let mut planes = [Plane {
            normal: [0.0; 3],
            dist: 0.0,
            kind: 0,
        }; 6];
        let mut nodes = [ClipNode {
            plane: 0,
            children: [0; 2],
        }; 6];
        for (i, ((plane, node), dist)) in planes.iter_mut().zip(&mut nodes).zip(dists).enumerate() {
            let axis = i >> 1;
            if let Some(n) = plane.normal.get_mut(axis) {
                *n = 1.0;
            }
            plane.kind = i32::try_from(axis).unwrap_or(0);
            plane.dist = dist;
            node.plane = i;
            let inside = if i == 5 {
                CONTENTS_SOLID
            } else {
                i32::try_from(i).map_or(CONTENTS_SOLID, |n| n.saturating_add(1))
            };
            node.children = if i & 1 == 0 {
                [CONTENTS_EMPTY, inside]
            } else {
                [inside, CONTENTS_EMPTY]
            };
        }
        Self { planes, nodes }
    }

    /// The box as a hull.
    #[must_use]
    pub fn hull(&self) -> Hull<'_> {
        Hull::over(&self.planes, &self.nodes, 5)
    }
}

/// The hull a box is clipped against for one entity, and the offset its points are moved by
/// (WORLD-1.4).
#[derive(Debug)]
pub enum EntityHull<'a> {
    /// A hull of a brush model.
    Brush(Hull<'a>),
    /// A box made from the entity's bounds.
    Box(BoxHull),
}

impl EntityHull<'_> {
    /// The hull.
    #[must_use]
    pub fn hull(&self) -> Hull<'_> {
        match self {
            Self::Brush(hull) => *hull,
            Self::Box(hull) => hull.hull(),
        }
    }
}

/// `SV_HullForEntity`: the hull a box of `mins` to `maxs` is clipped against for `ent`, and
/// the offset to subtract from its points (WORLD-1.4). `model` is `ent`'s brush model, if it
/// is one, and `sets` the hull sets models name.
///
/// # Errors
///
/// [`CollisionError::SolidBspWithoutPush`] and [`CollisionError::PushWithoutBsp`], id's
/// `Sys_Error`s.
pub fn hull_for_entity<'a>(
    ent: &Body,
    model: Option<&BrushModel>,
    sets: &'a [HullSet],
    (mins, maxs): ([f32; 3], [f32; 3]),
) -> Result<(EntityHull<'a>, [f32; 3]), CollisionError> {
    if ent.solid == Solid::Bsp {
        if !ent.pusher {
            return Err(CollisionError::SolidBspWithoutPush);
        }
        let model = model.ok_or(CollisionError::PushWithoutBsp)?;
        let set = sets.get(model.set).ok_or(CollisionError::PushWithoutBsp)?;
        let size = maxs[0] - mins[0];
        let index = if size < POINT_SIZE {
            0
        } else if size <= PLAYER_SIZE {
            1
        } else {
            2
        };
        let hull = set.hull(index, model.heads);
        let offset = add(sub(hull.clip_mins, mins), ent.origin);
        return Ok((EntityHull::Brush(hull), offset));
    }
    let hull = BoxHull::new(sub(ent.mins, maxs), sub(ent.maxs, mins));
    Ok((EntityHull::Box(hull), ent.origin))
}

/// `VectorSubtract`.
pub(crate) fn sub([a, b, c]: [f32; 3], [x, y, z]: [f32; 3]) -> [f32; 3] {
    [a - x, b - y, c - z]
}

/// `VectorAdd`.
pub(crate) fn add([a, b, c]: [f32; 3], [x, y, z]: [f32; 3]) -> [f32; 3] {
    [a + x, b + y, c + z]
}
