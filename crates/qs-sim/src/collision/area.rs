// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/world.c, SV_CreateAreaNode, SV_ClearWorld, SV_UnlinkEdict, SV_TouchLinks, SV_FindTouchedLeafs, SV_LinkEdict, SV_ClipToLinks, SV_MoveBounds, SV_Move, SV_TestEntityPosition
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/common.c, ClearLink, RemoveLink, InsertLinkBefore
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/mathlib.c, BoxOnPlaneSide

//! Area nodes (WORLD-5), linking entities into them and touching triggers (WORLD-6), and
//! moving a box past the entities linked in them (WORLD-7).

use super::hull::{CONTENTS_SOLID, Plane, add, sub};
use super::trace::{Trace, clip_move_to_entity};
use super::{CollisionError, Host, Solid};

/// `AREA_DEPTH`: the depth of the leaf area nodes (WORLD-5.1).
pub const AREA_DEPTH: usize = 4;
/// `MAX_ENT_LEAFS`: the leaves an entity records (WORLD-6.3).
pub const MAX_ENT_LEAFS: usize = 16;
/// How far an item's box is widened on x and y, "to make items easier to pick up"
/// (WORLD-6.2).
const ITEM_REACH: f32 = 15.0;
/// How far any other box is widened, "because movement is clipped an epsilon away from an
/// actual edge" (WORLD-6.2).
const EDGE_REACH: f32 = 1.0;
/// The box a missile is clipped against monsters with (WORLD-7.1).
const MISSILE_BOX: f32 = 15.0;
/// `FL_ITEM` (WORLD-6.2).
pub const FL_ITEM: i32 = 256;
/// `FL_MONSTER` (WORLD-7.3).
pub const FL_MONSTER: i32 = 32;

/// The axis an area node divides on: x or y, never z (WORLD-5.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Axis {
    X,
    Y,
}

impl Axis {
    /// `v[axis]`.
    const fn of(self, [x, y, _]: [f32; 3]) -> f32 {
        match self {
            Self::X => x,
            Self::Y => y,
        }
    }

    /// `v` with `v[axis]` set to `value`.
    const fn with(self, [x, y, z]: [f32; 3], value: f32) -> [f32; 3] {
        match self {
            Self::X => [value, y, z],
            Self::Y => [x, value, z],
        }
    }
}

/// One area node: its dividing plane, if it is not a leaf, and its two lists (WORLD-5).
#[derive(Clone, Debug, PartialEq)]
struct AreaNode {
    /// The axis and distance of its plane, and its children; `None` for a leaf.
    split: Option<(Axis, f32, [usize; 2])>,
    triggers: Vec<usize>,
    solids: Vec<usize>,
}

/// Which of a node's lists an entity is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum List {
    Triggers,
    Solids,
}

/// The area nodes, and where each entity is linked (WORLD-5).
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    nodes: Vec<AreaNode>,
    /// Each entity's node and list.
    links: Vec<Option<(usize, List)>>,
}

impl Area {
    /// `SV_ClearWorld`'s nodes over the world's box (WORLD-5.1).
    #[must_use]
    pub fn new(mins: [f32; 3], maxs: [f32; 3]) -> Self {
        let mut area = Self {
            nodes: Vec::new(),
            links: Vec::new(),
        };
        area.create(0, mins, maxs);
        area
    }

    /// `SV_CreateAreaNode`: the node at `depth` over `mins` to `maxs`, numbered before its
    /// children; returns its number.
    fn create(&mut self, depth: usize, mins: [f32; 3], maxs: [f32; 3]) -> usize {
        let number = self.nodes.len();
        self.nodes.push(AreaNode {
            split: None,
            triggers: Vec::new(),
            solids: Vec::new(),
        });
        if depth == AREA_DEPTH {
            return number;
        }
        let [sx, sy, _] = sub(maxs, mins);
        let axis = if sx > sy { Axis::X } else { Axis::Y };
        let dist = 0.5 * (axis.of(maxs) + axis.of(mins));
        let maxs1 = axis.with(maxs, dist);
        let mins2 = axis.with(mins, dist);
        let deeper = depth.saturating_add(1);
        let front = self.create(deeper, mins2, maxs);
        let back = self.create(deeper, mins, maxs1);
        if let Some(node) = self.nodes.get_mut(number) {
            node.split = Some((axis, dist, [front, back]));
        }
        number
    }

    /// The number of area nodes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether there are no area nodes; never, once made.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Node `node`'s axis (0 for x, 1 for y), distance and children, or `None` for a leaf.
    #[must_use]
    pub fn split(&self, node: usize) -> Option<(usize, f32, [usize; 2])> {
        let (axis, dist, children) = self.nodes.get(node)?.split?;
        Some((usize::from(axis == Axis::Y), dist, children))
    }

    /// Links `entity` at the end of `node`'s triggers or solids, for a test or a host that
    /// places an entity itself (WORLD-5.2).
    pub fn insert_at(&mut self, entity: usize, node: usize, trigger: bool) {
        self.unlink(entity);
        let list = if trigger {
            List::Triggers
        } else {
            List::Solids
        };
        self.insert(entity, node, list);
    }

    /// The node `entity` is linked in, and whether as a trigger.
    #[must_use]
    pub fn link_of(&self, entity: usize) -> Option<(usize, bool)> {
        let (node, list) = self.links.get(entity).copied().flatten()?;
        Some((node, list == List::Triggers))
    }

    /// The entities in `node`'s triggers or solids, in order.
    #[must_use]
    pub fn listed(&self, node: usize, triggers: bool) -> &[usize] {
        self.nodes
            .get(node)
            .map_or(&[], |n| if triggers { &n.triggers } else { &n.solids })
    }

    fn list_mut(&mut self, node: usize, list: List) -> Option<&mut Vec<usize>> {
        let node = self.nodes.get_mut(node)?;
        Some(match list {
            List::Triggers => &mut node.triggers,
            List::Solids => &mut node.solids,
        })
    }

    /// `InsertLinkBefore` the list's head: at the end (WORLD-5.2).
    fn insert(&mut self, entity: usize, node: usize, list: List) {
        if let Some(items) = self.list_mut(node, list) {
            items.push(entity);
        }
        if self.links.len() <= entity {
            self.links.resize(entity.saturating_add(1), None);
        }
        if let Some(link) = self.links.get_mut(entity) {
            *link = Some((node, list));
        }
    }

    /// `SV_UnlinkEdict`: takes `entity` out of its list, if it is in one (WORLD-6.8).
    pub fn unlink(&mut self, entity: usize) {
        let Some((node, list)) = self.links.get_mut(entity).and_then(Option::take) else {
            return;
        };
        if let Some(items) = self.list_mut(node, list) {
            items.retain(|&e| e != entity);
        }
    }

    /// The entity after `entity` in `node`'s `list`, if `entity` is there.
    fn after(&self, node: usize, list: List, entity: usize) -> Option<usize> {
        let items = self.listed(node, list == List::Triggers);
        let at = items.iter().position(|&e| e == entity)?;
        items.get(at.saturating_add(1)).copied()
    }

    fn contains(&self, node: usize, list: List, entity: usize) -> bool {
        self.listed(node, list == List::Triggers).contains(&entity)
    }
}

/// The drawing nodes of the world, which an entity's leaves are found in (WORLD-6.3).
#[derive(Clone, Debug, PartialEq)]
pub struct LeafTree {
    planes: Vec<(Plane, u8)>,
    /// Each node's plane and children: a node when 0 or more, else leaf `-1 - c`.
    nodes: Vec<(usize, [i32; 2])>,
    leaf_contents: Vec<i32>,
}

impl LeafTree {
    /// The tree of `bsp`.
    #[must_use]
    pub fn new(bsp: &qs_bsp::Bsp<'_>) -> Self {
        Self {
            planes: bsp
                .planes
                .iter()
                .map(|p| {
                    let plane = Plane {
                        normal: p.normal,
                        dist: p.dist,
                        kind: p.plane_type,
                    };
                    (plane, p.signbits)
                })
                .collect(),
            nodes: bsp.nodes.iter().map(|n| (n.planenum, n.children)).collect(),
            leaf_contents: bsp.leafs.iter().map(|l| l.contents).collect(),
        }
    }

    /// `SV_FindTouchedLeafs` from the world's node 0: the leaves, each its number less one,
    /// the box of `absmin` to `absmax` is in (WORLD-6.3).
    #[must_use]
    pub fn touched(&self, absmin: [f32; 3], absmax: [f32; 3]) -> Vec<i32> {
        let mut leafs = Vec::new();
        self.find(0, (absmin, absmax), &mut leafs, self.nodes.len());
        leafs
    }

    fn find(&self, child: i32, bounds: ([f32; 3], [f32; 3]), out: &mut Vec<i32>, depth: usize) {
        let Ok(node) = usize::try_from(child) else {
            // A leaf, `-1 - child`.
            let leaf = child
                .checked_neg()
                .and_then(|n| n.checked_sub(1))
                .unwrap_or(0);
            let contents = usize::try_from(leaf)
                .ok()
                .and_then(|l| self.leaf_contents.get(l))
                .copied()
                .unwrap_or(CONTENTS_SOLID);
            if contents != CONTENTS_SOLID && out.len() < MAX_ENT_LEAFS {
                out.push(leaf.saturating_sub(1));
            }
            return;
        };
        // A tree is never deeper than its node count; a cycle in a damaged one ends there.
        let (Some(&(plane, [front, back])), Some(depth)) =
            (self.nodes.get(node), depth.checked_sub(1))
        else {
            return;
        };
        let Some(&(plane, signbits)) = self.planes.get(plane) else {
            return;
        };
        let sides = box_on_plane_side(bounds, &plane, signbits);
        if sides & 1 != 0 {
            self.find(front, bounds, out, depth);
        }
        if sides & 2 != 0 {
            self.find(back, bounds, out, depth);
        }
    }
}

/// `BOX_ON_PLANE_SIDE`: 1 when the box is in front of the plane, 2 when behind it, 3 when it
/// crosses it (WORLD-6.4).
#[must_use]
pub fn box_on_plane_side((emins, emaxs): ([f32; 3], [f32; 3]), p: &Plane, signbits: u8) -> u8 {
    if let Some(axis) = usize::try_from(p.kind).ok().filter(|&k| k < 3) {
        let (lo, hi) = (emins.get(axis).copied(), emaxs.get(axis).copied());
        if lo.is_some_and(|lo| p.dist <= lo) {
            return 1;
        }
        if hi.is_some_and(|hi| p.dist >= hi) {
            return 2;
        }
        return 3;
    }
    // BoxOnPlaneSide: the corner furthest along the normal, and the opposite one.
    let mut near = [0.0_f32; 3];
    let mut far = [0.0_f32; 3];
    for (axis, (n, f)) in near.iter_mut().zip(&mut far).enumerate() {
        let negative = signbits & (1 << axis) != 0;
        let (lo, hi) = (
            emins.get(axis).copied().unwrap_or(0.0),
            emaxs.get(axis).copied().unwrap_or(0.0),
        );
        (*f, *n) = if negative { (lo, hi) } else { (hi, lo) };
    }
    let dot = |c: [f32; 3]| p.normal[0] * c[0] + p.normal[1] * c[1] + p.normal[2] * c[2];
    let mut sides = 0;
    if dot(far) >= p.dist {
        sides = 1;
    }
    if dot(near) < p.dist {
        sides |= 2;
    }
    sides
}

/// `SV_LinkEdict`: puts `ent` in the area node its box crosses, after finding its box and its
/// leaves, and when `touch_triggers` is set runs the touch of every trigger it is in
/// (WORLD-6.1 to WORLD-6.7).
///
/// # Errors
///
/// Whatever a trigger's touch raises.
pub fn link_edict<H: Host + ?Sized>(
    host: &mut H,
    ent: usize,
    touch_triggers: bool,
) -> Result<(), CollisionError> {
    host.collision_mut().area.unlink(ent);
    if ent == 0 {
        return Ok(());
    }
    let Some(body) = host.body(ent) else {
        return Ok(());
    };
    if body.free {
        return Ok(());
    }
    // WORLD-6.2
    let mut absmin = add(body.origin, body.mins);
    let mut absmax = add(body.origin, body.maxs);
    if body.flags & FL_ITEM != 0 {
        let [x, y, z] = absmin;
        absmin = [x - ITEM_REACH, y - ITEM_REACH, z];
        let [x, y, z] = absmax;
        absmax = [x + ITEM_REACH, y + ITEM_REACH, z];
    } else {
        absmin = absmin.map(|v| v - EDGE_REACH);
        absmax = absmax.map(|v| v + EDGE_REACH);
    }
    host.set_abs(ent, absmin, absmax);
    // WORLD-6.3
    // `if (ent->v.modelindex)`
    let modelled = body.modelindex != 0.0;
    let leafs = if modelled {
        host.collision().leafs.touched(absmin, absmax)
    } else {
        Vec::new()
    };
    host.collision_mut().set_leafs(ent, leafs);
    // WORLD-6.5
    if body.solid == Solid::Not {
        return Ok(());
    }
    let area = &host.collision().area;
    let mut node = 0;
    while let Some((axis, dist, [front, back])) = area.nodes.get(node).and_then(|n| n.split) {
        if axis.of(absmin) > dist {
            node = front;
        } else if axis.of(absmax) < dist {
            node = back;
        } else {
            break;
        }
    }
    let list = if body.solid == Solid::Trigger {
        List::Triggers
    } else {
        List::Solids
    };
    host.collision_mut().area.insert(ent, node, list);
    if touch_triggers {
        touch_links(host, ent, 0)?;
    }
    Ok(())
}

/// `SV_TouchLinks`: runs the touch of every trigger in `node` and the nodes below it whose box
/// overlaps `ent`'s (WORLD-6.6, WORLD-6.7).
fn touch_links<H: Host + ?Sized>(
    host: &mut H,
    ent: usize,
    node: usize,
) -> Result<(), CollisionError> {
    let mut current = host.collision().area.listed(node, true).first().copied();
    while let Some(touch) = current {
        let next = host.collision().area.after(node, List::Triggers, touch);
        if touch != ent
            && let (Some(trigger), Some(body)) = (host.body(touch), host.body(ent))
            && trigger.touch != 0
            && trigger.solid == Solid::Trigger
            && overlaps((body.absmin, body.absmax), (trigger.absmin, trigger.absmax))
        {
            host.touch(touch, ent)?;
        }
        // WORLD-6.7: the next entity, when it is still in this list.
        current = next.filter(|&n| host.collision().area.contains(node, List::Triggers, n));
    }
    let Some((axis, dist, [front, back])) =
        host.collision().area.nodes.get(node).and_then(|n| n.split)
    else {
        return Ok(());
    };
    let Some(body) = host.body(ent) else {
        return Ok(());
    };
    if axis.of(body.absmax) > dist {
        touch_links(host, ent, front)?;
    }
    if axis.of(body.absmin) < dist {
        touch_links(host, ent, back)?;
    }
    Ok(())
}

/// Whether two boxes overlap: neither's minimum is above the other's maximum on any axis.
fn overlaps((amin, amax): ([f32; 3], [f32; 3]), (bmin, bmax): ([f32; 3], [f32; 3])) -> bool {
    let above = |a: [f32; 3], b: [f32; 3]| a.iter().zip(b).any(|(a, b)| *a > b);
    !(above(amin, bmax) || above(bmin, amax))
}

/// The kind of move `SV_Move` makes (WORLD-7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveKind {
    /// `MOVE_NORMAL`.
    Normal,
    /// `MOVE_NOMONSTERS`: only brush models stop it.
    NoMonsters,
    /// `MOVE_MISSILE`: monsters are hit with a ±15 box.
    Missile,
}

impl MoveKind {
    /// The kind a progs passes: 1 is `MOVE_NOMONSTERS`, 2 `MOVE_MISSILE`, anything else
    /// `MOVE_NORMAL`, as id compares the int.
    #[must_use]
    pub const fn from_int(value: i32) -> Self {
        match value {
            1 => Self::NoMonsters,
            2 => Self::Missile,
            _ => Self::Normal,
        }
    }
}

/// One move being clipped: id's `moveclip_t`.
struct MoveClip {
    boxmins: [f32; 3],
    boxmaxs: [f32; 3],
    mins: [f32; 3],
    maxs: [f32; 3],
    mins2: [f32; 3],
    maxs2: [f32; 3],
    start: [f32; 3],
    end: [f32; 3],
    trace: Trace,
    kind: MoveKind,
    pass: Option<usize>,
}

/// `SV_Move`: the trace of a box of `mins` to `maxs` from `start` to `end`, ignoring `pass`
/// (WORLD-7.1 to WORLD-7.5).
///
/// # Errors
///
/// The errors of [`clip_move_to_entity`], and [`CollisionError::TriggerInClipList`].
pub fn sv_move<H: Host + ?Sized>(
    host: &H,
    (start, mins, maxs, end): ([f32; 3], [f32; 3], [f32; 3], [f32; 3]),
    kind: MoveKind,
    pass: Option<usize>,
) -> Result<Trace, CollisionError> {
    let world = host.body(0).ok_or(CollisionError::PushWithoutBsp)?;
    let collision = host.collision();
    let trace = clip_move_to_entity(
        (0, &world),
        collision.model(world.modelindex),
        &collision.sets,
        (start, mins, maxs, end),
    )?;
    let (mins2, maxs2) = if kind == MoveKind::Missile {
        ([-MISSILE_BOX; 3], [MISSILE_BOX; 3])
    } else {
        (mins, maxs)
    };
    let mut boxmins = [0.0; 3];
    let mut boxmaxs = [0.0; 3];
    let axes = boxmins
        .iter_mut()
        .zip(&mut boxmaxs)
        .zip(start.iter().zip(end));
    for (((lo, hi), (&s, e)), (min, max)) in axes.zip(mins2.iter().zip(maxs2)) {
        // SV_MoveBounds
        (*lo, *hi) = if e > s {
            (s + min - 1.0, e + max + 1.0)
        } else {
            (e + min - 1.0, s + max + 1.0)
        };
    }
    let mut clip = MoveClip {
        boxmins,
        boxmaxs,
        mins,
        maxs,
        mins2,
        maxs2,
        start,
        end,
        trace,
        kind,
        pass,
    };
    clip_to_links(host, 0, &mut clip)?;
    Ok(clip.trace)
}

/// `SV_ClipToLinks`: clips the move to every solid in `node` and the nodes below it that its
/// bounds reach (WORLD-7.2 to WORLD-7.5).
fn clip_to_links<H: Host + ?Sized>(
    host: &H,
    node: usize,
    clip: &mut MoveClip,
) -> Result<(), CollisionError> {
    let collision = host.collision();
    let pass = clip.pass.and_then(|p| host.body(p).map(|b| (p, b)));
    for &number in collision.area.listed(node, false) {
        let Some(touch) = host.body(number) else {
            continue;
        };
        if touch.solid == Solid::Not || Some(number) == clip.pass {
            continue;
        }
        if touch.solid == Solid::Trigger {
            return Err(CollisionError::TriggerInClipList);
        }
        if clip.kind == MoveKind::NoMonsters && touch.solid != Solid::Bsp {
            continue;
        }
        if !overlaps((clip.boxmins, clip.boxmaxs), (touch.absmin, touch.absmax)) {
            continue;
        }
        if let Some((_, p)) = &pass
            && p.size[0] != 0.0
            && touch.size[0] == 0.0
        {
            continue;
        }
        // WORLD-7.3
        if clip.trace.allsolid {
            return Ok(());
        }
        if let Some((p, body)) = &pass
            && (touch.owner == Some(*p) || body.owner == Some(number))
        {
            continue;
        }
        let (mins, maxs) = if touch.flags & FL_MONSTER != 0 {
            (clip.mins2, clip.maxs2)
        } else {
            (clip.mins, clip.maxs)
        };
        let trace = clip_move_to_entity(
            (number, &touch),
            collision.model(touch.modelindex),
            &collision.sets,
            (clip.start, mins, maxs, clip.end),
        )?;
        // WORLD-7.4
        if trace.allsolid || trace.startsolid || trace.fraction < clip.trace.fraction {
            let startsolid = clip.trace.startsolid;
            clip.trace = Trace {
                ent: Some(number),
                ..trace
            };
            if startsolid {
                clip.trace.startsolid = true;
            }
        }
    }
    let Some((axis, dist, [front, back])) = collision.area.nodes.get(node).and_then(|n| n.split)
    else {
        return Ok(());
    };
    if axis.of(clip.boxmaxs) > dist {
        clip_to_links(host, front, clip)?;
    }
    if axis.of(clip.boxmins) < dist {
        clip_to_links(host, back, clip)?;
    }
    Ok(())
}

/// `SV_TestEntityPosition`: whether `ent` starts in solid where it is (WORLD-7.6).
///
/// # Errors
///
/// The errors of [`sv_move`].
pub fn test_entity_position<H: Host + ?Sized>(
    host: &H,
    ent: usize,
) -> Result<bool, CollisionError> {
    let Some(body) = host.body(ent) else {
        return Ok(false);
    };
    let trace = sv_move(
        host,
        (body.origin, body.mins, body.maxs, body.origin),
        MoveKind::Normal,
        Some(ent),
    )?;
    Ok(trace.startsolid)
}
