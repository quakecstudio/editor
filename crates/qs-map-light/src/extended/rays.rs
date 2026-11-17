// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2016 Eric Wasylishen
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: light/trace_embree.cc, Embree_FilterFuncN, PerRay_FilterFuncN, AddGlassToRay, AddDynamicOccluderToRay

//! What a ray of light meets (LIGHT-11): the triangles of the faces that cast shadows, in a
//! tree of boxes, with what each stands for; and the two questions lighting asks of them,
//! whether anything stands between a sample and a light, and what a ray toward the sky reaches
//! first.
//!
//! ericw-tools asks Embree; the tree and the triangle test here are our own, so a ray that
//! grazes the edge of a triangle may be answered differently (LIGHT-11.9).

use crate::extended::keys::CHANNEL_DEFAULT;
use crate::extended::math::{add, dot, scale, sub};
use crate::{AXES, Vec3};

/// The most triangles a leaf of the tree holds.
const LEAF_TRIANGLES: usize = 4;
/// The corners of a triangle, as what their sum is divided by for its middle.
const CORNERS: f32 = 3.0;
/// A texel that is not see-through.
const OPAQUE: u8 = 255;

/// What a triangle is to light (LIGHT-11.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Surface {
    /// Sky: sunlight comes from it, and it stops every other light.
    Sky,
    /// Solid: it stops light.
    Solid,
    /// It stops light for some rays: asked of each (LIGHT-11.4).
    Filtered,
}

/// The model a ray starts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ModelId(pub(crate) usize);

/// What a triangle stands for: its face's model and how it casts shadow (`triinfo`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Caster {
    pub(crate) model: ModelId,
    pub(crate) face: usize,
    pub(crate) surface: Surface,
    /// `_shadowworldonly`: it shadows only the world.
    pub(crate) world_only: bool,
    /// `_shadowself`: it shadows only its own model.
    pub(crate) self_only: bool,
    /// `_switchableshadow`: the style its shadow is switched by.
    pub(crate) switch_style: Option<i32>,
    pub(crate) channels: i32,
    pub(crate) alpha: f32,
    /// A texture whose name starts with `{`.
    pub(crate) fence: bool,
    /// An alpha under 1.
    pub(crate) glass: bool,
}

/// Who asks: the model the sample is on and the channels its light is stopped on
/// (`ray_source_info`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Asker {
    /// The model, none for a point that is on no face.
    pub(crate) model: Option<ModelId>,
    /// Its model's `switchshadstyle`.
    pub(crate) switch_style: i32,
    pub(crate) shadow_mask: i32,
}

impl Asker {
    /// A point on no model, on the default channel.
    pub(crate) const NOBODY: Self = Self {
        model: None,
        switch_style: 0,
        shadow_mask: CHANNEL_DEFAULT,
    };
}

/// A texel: its red, green and blue, and its alpha.
pub(crate) type Texel = [u8; 4];

/// The texels of the faces: what a fence or glass triangle is at a point (`SampleTexture`).
pub(crate) trait Texels: Sync {
    /// The colour and alpha of `face`'s texture at `point`; all 0 for a missing texture.
    fn sample(&self, face: usize, point: Vec3) -> Texel;
}

/// What a ray passed through without being stopped (LIGHT-11.5, LIGHT-11.6).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Passed {
    /// The style of a switchable shadow it crossed, 0 for none.
    pub(crate) dynamic_style: i32,
    /// The colour and opacity of glass it left.
    pub(crate) glass: Option<(Vec3, f32)>,
}

impl Passed {
    /// `getPushedRayColor`: `colour` tinted by the glass, by its opacity.
    pub(crate) fn tinted(&self, colour: Vec3) -> Vec3 {
        match self.glass {
            Some((glass, opacity)) => {
                let tinted = [
                    colour[0] * glass[0],
                    colour[1] * glass[1],
                    colour[2] * glass[2],
                ];
                add(scale(colour, 1.0 - opacity), scale(tinted, opacity))
            }
            None => colour,
        }
    }
}

/// What a ray reaches first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Struck {
    /// What it hit and how far along; none when it hit nothing.
    pub(crate) hit: Option<(Surface, f32)>,
    pub(crate) passed: Passed,
}

struct Triangle {
    v0: Vec3,
    /// `v0 - v1`.
    e1: Vec3,
    /// `v2 - v0`.
    e2: Vec3,
    /// `e2` cross `e1`: the geometric normal, not unit.
    normal: Vec3,
    caster: usize,
}

impl Triangle {
    /// Where along the ray from `origin` along `dir` it is hit, past 0 and at most `far`: the
    /// Moller-Trumbore test, a hit on an edge counting (LIGHT-11.9).
    fn hit(&self, origin: Vec3, dir: Vec3, far: f32) -> Option<f32> {
        let (cross, dot) = (fused_cross, fused_dot);
        let to_corner = sub(self.v0, origin);
        let around = cross(to_corner, dir);
        let den = dot(self.normal, dir);
        if den == 0.0 {
            return None;
        }
        let (size, sign) = (den.abs(), den.signum());
        let u = dot(around, self.e2) * sign;
        let v = dot(around, self.e1) * sign;
        if u < 0.0 || v < 0.0 || u + v > size {
            return None;
        }
        let t = dot(self.normal, to_corner) * sign;
        if !(t > 0.0 && t <= size * far) {
            return None;
        }
        Some(t / size)
    }
}

/// The even cuts of an axis a node's split is chosen from.
const SPLIT_BINS: u8 = 16;

/// The bin a triangle's middle falls in, `along` its place in bin widths from the first.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a place from 0 to the number of bins, cut to a whole bin"
)]
fn bin_of(along: f32) -> usize {
    (along.max(0.0) as usize).min(usize::from(SPLIT_BINS).saturating_sub(1))
}

/// A count of triangles as a float.
#[expect(
    clippy::cast_precision_loss,
    reason = "a count of triangles weighs a box's surface: its last digits do not matter"
)]
const fn count_of(count: u32) -> f32 {
    count as f32
}

/// A dot product with each product but the last fused into the sum, as Embree works it
/// where the processor fuses a multiply and an add (LIGHT-11.9).
fn fused_dot(a: Vec3, b: Vec3) -> f32 {
    a[0].mul_add(b[0], a[1].mul_add(b[1], a[2] * b[2]))
}

/// A cross product with the first product of each difference fused into it.
fn fused_cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1].mul_add(b[2], -(a[2] * b[1])),
        a[2].mul_add(b[0], -(a[0] * b[2])),
        a[0].mul_add(b[1], -(a[1] * b[0])),
    ]
}

/// A side of a plane across an axis.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    /// The triangle is wholly below a ray that does not move along the axis.
    Low,
    /// It is wholly above one.
    High,
    /// The ray moves, by next to nothing, toward the triangle's side.
    Ahead,
    /// It moves away from it.
    Behind,
}

/// A ray that moves less than this along an axis for each unit it goes runs along the
/// planes across that axis.
const NEARLY_IN_PLANE: f32 = 1e-6;

/// How near two hits are for them to be of one surface.
const SAME_DISTANCE: f32 = 0.01;

impl Triangle {
    /// For each axis `dir` does not move along, the side of the plane through `origin`
    /// across that axis the triangle lies wholly on; none where it spans it.
    fn sides_of(&self, origin: Vec3, dir: Vec3) -> [Option<Side>; AXES] {
        let (v1, v2) = (sub(self.v0, self.e1), add(self.v0, self.e2));
        let mut out = [None; AXES];
        for (axis, slot) in out.iter_mut().enumerate() {
            let (Some(&d), Some(&o), Some(&a), Some(&b), Some(&c)) = (
                dir.get(axis),
                origin.get(axis),
                self.v0.get(axis),
                v1.get(axis),
                v2.get(axis),
            ) else {
                continue;
            };
            if d.abs() >= NEARLY_IN_PLANE {
                continue;
            }
            let (above, below) = (o <= a.min(b).min(c), o >= a.max(b).max(c));
            *slot = match (above, below) {
                (true, _) if d == 0.0 => Some(Side::High),
                (_, true) if d == 0.0 => Some(Side::Low),
                (true, _) => Some(if d > 0.0 { Side::Ahead } else { Side::Behind }),
                (_, true) => Some(if d < 0.0 { Side::Ahead } else { Side::Behind }),
                _ => None,
            };
        }
        out
    }
}

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    const EMPTY: Self = Self {
        min: [f32::INFINITY; AXES],
        max: [f32::NEG_INFINITY; AXES],
    };

    fn with(self, point: Vec3) -> Self {
        Self {
            min: [
                self.min[0].min(point[0]),
                self.min[1].min(point[1]),
                self.min[2].min(point[2]),
            ],
            max: [
                self.max[0].max(point[0]),
                self.max[1].max(point[1]),
                self.max[2].max(point[2]),
            ],
        }
    }

    /// The box widened to hold `triangle`.
    fn with_triangle(self, triangle: &Triangle) -> Self {
        self.with(triangle.v0)
            .with(sub(triangle.v0, triangle.e1))
            .with(add(triangle.v0, triangle.e2))
    }

    /// The box's surface, halved; nothing for a box that holds nothing.
    fn half_surface(self) -> f32 {
        let size = sub(self.max, self.min);
        if size.iter().any(|c| *c < 0.0) {
            return 0.0;
        }
        size[0] * size[1] + size[1] * size[2] + size[2] * size[0]
    }

    /// Where to part `pending`, whose middles this box holds: the axis and the coordinate
    /// that leave the least surface, each side's box weighed by its triangles, of the
    /// [`SPLIT_BINS`] even cuts of each axis; none when no cut parts them.
    fn best_split(self, pending: &[(Vec3, Triangle)]) -> Option<(usize, f32)> {
        let mut best: Option<(f32, usize, f32)> = None;
        for axis in 0..AXES {
            let (Some(&low), Some(&high)) = (self.min.get(axis), self.max.get(axis)) else {
                continue;
            };
            let width = (high - low) / f32::from(SPLIT_BINS);
            if width <= 0.0 {
                continue;
            }
            let mut bins = [(Self::EMPTY, 0_u32); SPLIT_BINS as usize];
            for (middle, triangle) in pending {
                let along = middle.get(axis).copied().unwrap_or_default();
                let bin = bin_of((along - low) / width);
                if let Some((bounds, count)) = bins.get_mut(bin) {
                    *bounds = bounds.with_triangle(triangle);
                    *count = count.saturating_add(1);
                }
            }
            // The boxes and counts of the bins after each cut, then those before it.
            let mut after = [(Self::EMPTY, 0_u32); SPLIT_BINS as usize];
            let mut running = (Self::EMPTY, 0_u32);
            for (slot, (bounds, count)) in after.iter_mut().zip(&bins).rev() {
                running = (running.0.union(*bounds), running.1.saturating_add(*count));
                *slot = running;
            }
            let mut before = (Self::EMPTY, 0_u32);
            for (cut, ((bounds, count), rest)) in bins.iter().zip(after.iter().skip(1)).enumerate()
            {
                before = (before.0.union(*bounds), before.1.saturating_add(*count));
                if before.1 == 0 || rest.1 == 0 {
                    continue;
                }
                let cost = before.0.half_surface() * count_of(before.1)
                    + rest.0.half_surface() * count_of(rest.1);
                if best.is_none_or(|(least, ..)| cost < least) {
                    let cuts = u8::try_from(cut.saturating_add(1)).unwrap_or(SPLIT_BINS);
                    best = Some((cost, axis, low + width * f32::from(cuts)));
                }
            }
        }
        best.map(|(_, axis, coordinate)| (axis, coordinate))
    }

    /// The box that holds both.
    fn union(self, other: Self) -> Self {
        Self {
            min: [
                self.min[0].min(other.min[0]),
                self.min[1].min(other.min[1]),
                self.min[2].min(other.min[2]),
            ],
            max: [
                self.max[0].max(other.max[0]),
                self.max[1].max(other.max[1]),
                self.max[2].max(other.max[2]),
            ],
        }
    }

    /// How far along the ray from `origin`, `inverse` one over its direction, the box is
    /// entered; none when the ray misses it or enters it past `far`.
    fn entered(self, origin: Vec3, inverse: Vec3, far: f32) -> Option<f32> {
        let mut near = 0.0_f32;
        let mut end = far;
        for axis in 0..AXES {
            let (Some(o), Some(i), Some(low), Some(high)) = (
                origin.get(axis),
                inverse.get(axis),
                self.min.get(axis),
                self.max.get(axis),
            ) else {
                continue;
            };
            if i.is_infinite() {
                // A ray along this axis crosses the box only from between its two sides.
                if o < low || o > high {
                    return None;
                }
                continue;
            }
            let (a, b) = ((low - o) * i, (high - o) * i);
            near = near.max(a.min(b));
            end = end.min(a.max(b));
        }
        (near <= end).then_some(near)
    }
}

enum Node {
    /// The triangles from `first`, `count` of them.
    Leaf { first: usize, count: usize },
    /// Two boxes: the left is the next node, the right at `right`.
    Split { right: usize },
}

/// Triangles and the tree of boxes over them.
#[derive(Default)]
struct Tree {
    triangles: Vec<Triangle>,
    nodes: Vec<(Bounds, Node)>,
}

/// The triangles that cast shadow, in a tree; and those of them that are sky, in another, so
/// that what a ray toward a sun meets is asked of the sky first.
pub(crate) struct Rays {
    tree: Tree,
    sky: Tree,
    casters: Vec<Caster>,
}

/// A triangle to be put in the tree.
pub(crate) struct Corners {
    pub(crate) points: [Vec3; 3],
    pub(crate) caster: Caster,
}

impl Rays {
    /// The tree over `corners`' triangles.
    pub(crate) fn new(corners: Vec<Corners>) -> Self {
        let mut casters = Vec::with_capacity(corners.len());
        let mut pending: Vec<(Vec3, Triangle)> = Vec::with_capacity(corners.len());
        let mut sky: Vec<(Vec3, Triangle)> = Vec::new();
        for (index, corner) in corners.into_iter().enumerate() {
            let [v0, v1, v2] = corner.points;
            let (e1, e2) = (sub(v0, v1), sub(v2, v0));
            let middle = scale(add(add(v0, v1), v2), 1.0 / CORNERS);
            let triangle = || Triangle {
                v0,
                e1,
                e2,
                normal: fused_cross(e2, e1),
                caster: index,
            };
            if corner.caster.surface == Surface::Sky {
                sky.push((middle, triangle()));
            }
            pending.push((middle, triangle()));
            casters.push(corner.caster);
        }
        let mut rays = Self {
            tree: Tree::default(),
            sky: Tree::default(),
            casters,
        };
        if !pending.is_empty() {
            rays.tree.build(pending);
        }
        if !sky.is_empty() {
            rays.sky.build(sky);
        }
        rays
    }
}

impl Tree {
    /// The node over `pending`, and those under it, after the nodes there are; its triangles
    /// go after those there are, in the order of the leaves. A node is split where the two
    /// halves' boxes, each weighed by its triangles, have the least surface: a ray meets a
    /// box about as often as it has surface.
    fn build(&mut self, mut pending: Vec<(Vec3, Triangle)>) {
        let mut bounds = Bounds::EMPTY;
        let mut middles = Bounds::EMPTY;
        for (middle, triangle) in &pending {
            bounds = bounds.with_triangle(triangle);
            middles = middles.with(*middle);
        }
        let split = if pending.len() <= LEAF_TRIANGLES {
            None
        } else {
            middles.best_split(&pending)
        };
        let Some((axis, at_coordinate)) = split else {
            let first = self.triangles.len();
            let count = pending.len();
            self.triangles.extend(pending.into_iter().map(|(_, t)| t));
            self.nodes.push((bounds, Node::Leaf { first, count }));
            return;
        };
        let coordinate = |middle: &Vec3| middle.get(axis).copied().unwrap_or_default();
        let (lower, upper): (Vec<_>, Vec<_>) = pending
            .drain(..)
            .partition(|(middle, _)| coordinate(middle) < at_coordinate);
        let at = self.nodes.len();
        self.nodes.push((bounds, Node::Split { right: 0 }));
        self.build(lower);
        let right = self.nodes.len();
        if let Some((_, Node::Split { right: slot })) = self.nodes.get_mut(at) {
            *slot = right;
        }
        self.build(upper);
    }

    /// The triangles the ray from `origin` along `dir` hits before `far`, nearest box first
    /// where it can tell, each given to `visit` with how far along; `visit` gives the new
    /// `far`, and stops the walk with none.
    fn walk(
        &self,
        (origin, dir): (Vec3, Vec3),
        mut far: f32,
        stack: &mut Vec<usize>,
        mut visit: impl FnMut(&Triangle, f32) -> Option<f32>,
    ) {
        let inverse = dir.map(|c| 1.0 / c);
        stack.clear();
        if self
            .nodes
            .first()
            .is_some_and(|(bounds, _)| bounds.entered(origin, inverse, far).is_some())
        {
            stack.push(0);
        }
        while let Some(at) = stack.pop() {
            let Some((_, node)) = self.nodes.get(at) else {
                continue;
            };
            match node {
                Node::Split { right } => {
                    // The nearer of the two boxes first: what stops the ray is likelier there,
                    // and a nearer hit lets the farther box be passed over.
                    let left = at.saturating_add(1);
                    let entry = |index: usize| {
                        self.nodes
                            .get(index)
                            .and_then(|(bounds, _)| bounds.entered(origin, inverse, far))
                    };
                    match (entry(left), entry(*right)) {
                        (Some(a), Some(b)) if a <= b => {
                            stack.push(*right);
                            stack.push(left);
                        }
                        (Some(_), Some(_)) => {
                            stack.push(left);
                            stack.push(*right);
                        }
                        (Some(_), None) => stack.push(left),
                        (None, Some(_)) => stack.push(*right),
                        (None, None) => {}
                    }
                }
                Node::Leaf { first, count } => {
                    let triangles = self
                        .triangles
                        .get(*first..first.saturating_add(*count))
                        .unwrap_or_default();
                    for triangle in triangles {
                        if let Some(t) = triangle.hit(origin, dir, far) {
                            match visit(triangle, t) {
                                Some(nearer) => far = nearer,
                                None => return,
                            }
                        }
                    }
                }
            }
        }
    }
}

impl Rays {
    /// Whether `caster` stops the ray `asker` sent, hit at `point`; what the ray passes
    /// through instead is added to `passed` (`Embree_FilterFuncN`, LIGHT-11.4 to LIGHT-11.7).
    fn stops(
        caster: &Caster,
        asker: &Asker,
        texels: &dyn Texels,
        (point, dir, normal): (Vec3, Vec3, Vec3),
        passed: &mut Passed,
    ) -> bool {
        // `PerRay_FilterFuncN`: a ray not on the default channel asks every triangle.
        if asker.shadow_mask != CHANNEL_DEFAULT && caster.channels & asker.shadow_mask == 0 {
            return false;
        }
        if caster.surface != Surface::Filtered {
            return true;
        }
        if caster.channels & asker.shadow_mask == 0 {
            return false;
        }
        let from_world = asker.model == Some(ModelId(0));
        if caster.world_only && !from_world {
            return false;
        }
        if caster.self_only && asker.model != Some(caster.model) {
            return false;
        }
        if let Some(style) = caster.switch_style
            && (asker.model.is_none() || asker.switch_style != style)
        {
            passed.dynamic_style = style;
            return false;
        }
        if caster.fence || caster.glass {
            let [r, g, b, a] = texels.sample(caster.face, point);
            if caster.glass {
                let alpha = if a < OPAQUE {
                    f32::from(a) / f32::from(OPAQUE)
                } else {
                    caster.alpha
                };
                // Only the side the ray leaves the glass by gives its colour.
                if dot(dir, normal) < 0.0 {
                    let colour = [r, g, b].map(|c| f32::from(c) / f32::from(OPAQUE));
                    passed.glass = Some((colour, alpha.clamp(0.0, 1.0)));
                }
                return false;
            }
            if a < OPAQUE {
                return false;
            }
        }
        true
    }

    /// Whether anything stops the ray from `origin` along the unit `dir` within `far`; when
    /// nothing does, what it passed through (`rtcOccluded1`, LIGHT-11.3).
    pub(crate) fn occluded(
        &self,
        (origin, dir, far): (Vec3, Vec3, f32),
        asker: &Asker,
        texels: &dyn Texels,
        stack: &mut Vec<usize>,
    ) -> Option<Passed> {
        if dir.iter().any(|c| c.abs() < NEARLY_IN_PLANE) {
            let struck = self.along_axis_plane((origin, dir, far), asker, texels, stack);
            return struck.hit.is_none().then_some(struck.passed);
        }
        let mut passed = Passed::default();
        let mut blocked = false;
        self.tree.walk((origin, dir), far, stack, |triangle, t| {
            let Some(caster) = self.casters.get(triangle.caster) else {
                return Some(far);
            };
            let point = add(origin, scale(dir, t));
            if Self::stops(
                caster,
                asker,
                texels,
                (point, dir, triangle.normal),
                &mut passed,
            ) {
                blocked = true;
                None
            } else {
                Some(far)
            }
        });
        (!blocked).then_some(passed)
    }

    /// What the ray from `origin` along the unit `dir` reaches first within `far`, and what
    /// it passed through before that (`rtcIntersect1`, LIGHT-11.3).
    pub(crate) fn struck(
        &self,
        (origin, dir, far): (Vec3, Vec3, f32),
        asker: &Asker,
        texels: &dyn Texels,
        stack: &mut Vec<usize>,
    ) -> Struck {
        if dir.iter().any(|c| c.abs() < NEARLY_IN_PLANE) {
            return self.along_axis_plane((origin, dir, far), asker, texels, stack);
        }
        let mut hit: Option<(Surface, f32)> = None;
        // What each triangle that did not stop the ray would add, with how far along it is:
        // only those before the hit count.
        let mut through: Vec<(f32, Passed)> = Vec::new();
        self.tree.walk((origin, dir), far, stack, |triangle, t| {
            let nearest = hit.map_or(far, |(_, at)| at);
            let Some(caster) = self.casters.get(triangle.caster) else {
                return Some(nearest);
            };
            let point = add(origin, scale(dir, t));
            let mut passed = Passed::default();
            if Self::stops(
                caster,
                asker,
                texels,
                (point, dir, triangle.normal),
                &mut passed,
            ) {
                hit = Some((caster.surface, t));
                Some(t)
            } else {
                through.push((t, passed));
                Some(nearest)
            }
        });
        Struck {
            hit,
            passed: passed_before(through, hit.map_or(far, |(_, at)| at)),
        }
    }

    /// Whether the ray from `origin` along the unit `dir` reaches sky before anything else
    /// stops it, within `far`, and what it passed through on the way (LIGHT-11.3). The sky
    /// is asked first, since most rays reach none: only one that does is asked of the rest.
    pub(crate) fn sees_sky(
        &self,
        (origin, dir, far): (Vec3, Vec3, f32),
        asker: &Asker,
        texels: &dyn Texels,
        stack: &mut Vec<usize>,
    ) -> Option<Passed> {
        if self.sky.nodes.is_empty() {
            return None;
        }
        // A ray along a plane of the grid, or one that sky may not stop, is asked whole.
        if asker.shadow_mask != CHANNEL_DEFAULT || dir.iter().any(|c| c.abs() < NEARLY_IN_PLANE) {
            let struck = self.struck((origin, dir, far), asker, texels, stack);
            return matches!(struck.hit, Some((Surface::Sky, _))).then_some(struck.passed);
        }
        let mut nearest: Option<f32> = None;
        self.sky.walk((origin, dir), far, stack, |_, t| {
            nearest = Some(t);
            Some(t)
        });
        let to_sky = nearest?;
        let mut passed = Passed::default();
        let mut blocked = false;
        self.tree.walk((origin, dir), to_sky, stack, |triangle, t| {
            let Some(caster) = self.casters.get(triangle.caster) else {
                return Some(to_sky);
            };
            if caster.surface == Surface::Sky {
                return Some(to_sky);
            }
            let point = add(origin, scale(dir, t));
            if Self::stops(
                caster,
                asker,
                texels,
                (point, dir, triangle.normal),
                &mut passed,
            ) {
                blocked = true;
                None
            } else {
                Some(to_sky)
            }
        });
        (!blocked).then_some(passed)
    }

    /// What a ray with no movement along one axis or two reaches first (LIGHT-11.9). Such a
    /// ray lies in a plane of the grid maps are built on, and so runs along the edges of
    /// triangles that end on that plane. Embree's boxes lose it where the box of a leaf
    /// ends on the plane, its slab test multiplying a distance of 0 by an infinite
    /// reciprocal, so which of those triangles stop it is its tree's; here one does only
    /// where another across the plane from it is hit at the same distance: the ray is
    /// stopped by a surface it crosses, and passes the edge of one it only touches.
    fn along_axis_plane(
        &self,
        (origin, dir, far): (Vec3, Vec3, f32),
        asker: &Asker,
        texels: &dyn Texels,
        stack: &mut Vec<usize>,
    ) -> Struck {
        struct Met {
            t: f32,
            surface: Surface,
            /// For each axis the ray does not move along, the side of the ray's plane the
            /// triangle lies wholly on; none where it spans the plane.
            sides: [Option<Side>; AXES],
        }
        let mut stopping: Vec<Met> = Vec::new();
        let mut through: Vec<(f32, Passed)> = Vec::new();
        self.tree.walk((origin, dir), far, stack, |triangle, t| {
            let Some(caster) = self.casters.get(triangle.caster) else {
                return Some(far);
            };
            let point = add(origin, scale(dir, t));
            let mut passed = Passed::default();
            if Self::stops(
                caster,
                asker,
                texels,
                (point, dir, triangle.normal),
                &mut passed,
            ) {
                stopping.push(Met {
                    t,
                    surface: caster.surface,
                    sides: triangle.sides_of(origin, dir),
                });
            } else {
                through.push((t, passed));
            }
            Some(far)
        });
        let crossed = |met: &Met| {
            met.sides.iter().enumerate().all(|(axis, side)| match side {
                None | Some(Side::Ahead) => true,
                Some(Side::Behind) => false,
                Some(side @ (Side::Low | Side::High)) => stopping.iter().any(|other| {
                    (other.t - met.t).abs() <= SAME_DISTANCE
                        && other
                            .sides
                            .get(axis)
                            .is_some_and(|across| *across != Some(*side))
                }),
            })
        };
        let hit = stopping
            .iter()
            .filter(|met| crossed(met))
            .min_by(|a, b| a.t.total_cmp(&b.t))
            .map(|met| (met.surface, met.t));
        Struck {
            hit,
            passed: passed_before(through, hit.map_or(far, |(_, at)| at)),
        }
    }
}

/// What the triangles that did not stop a ray add to it, of those no farther than `nearest`,
/// the farther over the nearer.
fn passed_before(mut through: Vec<(f32, Passed)>, nearest: f32) -> Passed {
    through.retain(|(t, _)| *t <= nearest);
    through.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut passed = Passed::default();
    for (_, one) in through {
        if one.dynamic_style != 0 {
            passed.dynamic_style = one.dynamic_style;
        }
        if one.glass.is_some() {
            passed.glass = one.glass;
        }
    }
    passed
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::extended::math::normalize;

    struct NoTexels;

    impl Texels for NoTexels {
        fn sample(&self, _face: usize, _point: Vec3) -> Texel {
            [0; 4]
        }
    }

    struct Grey;

    impl Texels for Grey {
        fn sample(&self, _face: usize, _point: Vec3) -> Texel {
            [255, 0, 0, 255]
        }
    }

    const fn caster(surface: Surface) -> Caster {
        Caster {
            model: ModelId(0),
            face: 0,
            surface,
            world_only: false,
            self_only: false,
            switch_style: None,
            channels: CHANNEL_DEFAULT,
            alpha: 1.0,
            fence: false,
            glass: false,
        }
    }

    /// A square of side 8 at height `z`, as two triangles.
    fn square(z: f32, caster: Caster) -> Vec<Corners> {
        let corner = |x: f32, y: f32| [x, y, z];
        vec![
            Corners {
                points: [corner(0.0, 0.0), corner(8.0, 0.0), corner(8.0, 8.0)],
                caster,
            },
            Corners {
                points: [corner(0.0, 0.0), corner(8.0, 8.0), corner(0.0, 8.0)],
                caster,
            },
        ]
    }

    const UP: Vec3 = [0.0, 0.0, 1.0];
    const WORLD: Asker = Asker {
        model: Some(ModelId(0)),
        switch_style: 0,
        shadow_mask: CHANNEL_DEFAULT,
    };

    fn clear(rays: &Rays, from: Vec3, far: f32, asker: &Asker, texels: &dyn Texels) -> bool {
        rays.occluded((from, UP, far), asker, texels, &mut Vec::new())
            .is_some()
    }

    // spec LIGHT-11.3: a solid triangle between a sample and a light stops the light; one
    // past the light, or beside the ray, does not.
    #[test]
    fn a_solid_triangle_stops_a_ray_that_reaches_it() {
        let rays = Rays::new(square(10.0, caster(Surface::Solid)));
        assert!(!clear(&rays, [4.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels));
        assert!(clear(&rays, [4.0, 4.0, 0.0], 9.0, &WORLD, &NoTexels));
        assert!(clear(&rays, [20.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels));
        assert!(
            clear(&rays, [4.0, 4.0, 20.0], 20.0, &WORLD, &NoTexels),
            "a ray starting past it"
        );
        let down = rays.occluded(
            ([4.0, 4.0, 20.0], [0.0, 0.0, -1.0], 20.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert!(down.is_none(), "either side of a triangle stops a ray");
    }

    // spec LIGHT-11.9: a ray that slants through a triangle's edge hits it; one that lies in
    // a plane of the grid is stopped by a surface it crosses, and passes the edge of one that
    // ends on the plane.
    #[test]
    fn a_ray_in_a_grid_plane_passes_an_edge_and_is_stopped_by_a_surface() {
        let slant = normalize([1.0, 0.0, 10.0]);
        let rays = Rays::new(square(10.0, caster(Surface::Solid)));
        let through_edge = rays.occluded(
            ([-1.0, 4.0, 0.0], slant, 40.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert!(through_edge.is_none(), "a slanting ray through the edge");
        assert!(
            clear(&rays, [0.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels),
            "a ray straight up the plane the square ends on only touches it"
        );
        assert!(clear(&rays, [8.0, 8.0, 0.0], 20.0, &WORLD, &NoTexels));
        assert!(
            !clear(&rays, [4.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels),
            "the seam of its two triangles is within it"
        );
        // A second square across the plane: the ray crosses a surface.
        let mut both = square(10.0, caster(Surface::Solid));
        let mut beyond = square(10.0, caster(Surface::Solid));
        for corner in &mut beyond {
            for point in &mut corner.points {
                point[0] -= 8.0;
            }
        }
        both.extend(beyond);
        let rays = Rays::new(both);
        assert!(!clear(&rays, [0.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels));
        let struck = rays.struck(
            ([0.0, 4.0, 0.0], UP, 20.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert_eq!(struck.hit, Some((Surface::Solid, 10.0)));
        let along = rays.occluded(
            ([-12.0, 4.0, 10.0], [1.0, 0.0, 0.0], 40.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert!(along.is_some(), "a ray in the triangles' plane misses them");
        // A ray that barely moves across the plane is stopped by what it moves toward only.
        let toward = rays_of_one_square().occluded(
            ([0.0, 4.0, 0.0], normalize([1e-8, 0.0, 1.0]), 20.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert!(toward.is_none());
        let away = rays_of_one_square().occluded(
            ([0.0, 4.0, 0.0], normalize([-1e-8, 0.0, 1.0]), 20.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert!(away.is_some());
    }

    fn rays_of_one_square() -> Rays {
        Rays::new(square(10.0, caster(Surface::Solid)))
    }

    // spec LIGHT-11.3: the first thing a ray reaches is the nearest that stops it, sky or
    // solid.
    #[test]
    fn the_nearest_triangle_is_what_a_ray_reaches() {
        let mut corners = square(30.0, caster(Surface::Sky));
        corners.extend(square(10.0, caster(Surface::Solid)));
        for z in 0..40_u8 {
            let mut far = square(100.0 + f32::from(z), caster(Surface::Sky));
            for corner in &mut far {
                for point in &mut corner.points {
                    point[0] += 100.0;
                }
            }
            corners.extend(far);
        }
        let rays = Rays::new(corners);
        let from_below = rays.struck(
            ([4.0, 4.0, 0.0], UP, 1000.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert_eq!(from_below.hit, Some((Surface::Solid, 10.0)));
        let between = rays.struck(
            ([4.0, 4.0, 20.0], UP, 1000.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert_eq!(between.hit, Some((Surface::Sky, 10.0)));
        let aside = rays.struck(
            ([104.0, 4.0, 0.0], UP, 1000.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert_eq!(aside.hit, Some((Surface::Sky, 100.0)));
        let nothing = rays.struck(
            ([50.0, 4.0, 0.0], UP, 1000.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert_eq!(nothing.hit, None);
    }

    // spec LIGHT-11.4: a model that shadows only the world, or only itself, stops only the
    // rays of the world's samples, or of its own.
    #[test]
    fn world_only_and_self_only_shadows_stop_only_their_own_rays() {
        let other = Asker {
            model: Some(ModelId(2)),
            ..WORLD
        };
        let mut world_only = caster(Surface::Filtered);
        world_only.world_only = true;
        world_only.model = ModelId(1);
        let rays = Rays::new(square(10.0, world_only));
        assert!(!clear(&rays, [4.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels));
        assert!(clear(&rays, [4.0, 4.0, 0.0], 20.0, &other, &NoTexels));
        assert!(clear(
            &rays,
            [4.0, 4.0, 0.0],
            20.0,
            &Asker::NOBODY,
            &NoTexels
        ));
        let mut self_only = caster(Surface::Filtered);
        self_only.self_only = true;
        self_only.model = ModelId(2);
        let rays = Rays::new(square(10.0, self_only));
        assert!(clear(&rays, [4.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels));
        assert!(!clear(&rays, [4.0, 4.0, 0.0], 20.0, &other, &NoTexels));
    }

    // spec LIGHT-11.5: a switchable shadow does not stop a ray; the ray carries its style,
    // unless it starts on a model of that style.
    #[test]
    fn a_switchable_shadow_gives_the_ray_its_style() {
        let mut switchable = caster(Surface::Filtered);
        switchable.switch_style = Some(33);
        switchable.model = ModelId(1);
        let rays = Rays::new(square(10.0, switchable));
        let through = rays.occluded(
            ([4.0, 4.0, 0.0], UP, 20.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert_eq!(through.map(|p| p.dynamic_style), Some(33));
        let own = Asker {
            model: Some(ModelId(1)),
            switch_style: 33,
            shadow_mask: CHANNEL_DEFAULT,
        };
        assert!(!clear(&rays, [4.0, 4.0, 0.0], 20.0, &own, &NoTexels));
        let struck = rays.struck(
            ([4.0, 4.0, 0.0], UP, 20.0),
            &WORLD,
            &NoTexels,
            &mut Vec::new(),
        );
        assert_eq!(struck.hit, None);
        assert_eq!(struck.passed.dynamic_style, 33);
    }

    // spec LIGHT-11.6: glass never stops a ray; it tints the ray that leaves it, by its
    // texel's colour and its alpha.
    // spec LIGHT-11.7: a fence stops a ray only at an opaque texel.
    #[test]
    fn glass_tints_and_a_fence_stops_at_opaque_texels() {
        let mut glass = caster(Surface::Filtered);
        glass.glass = true;
        glass.alpha = 0.5;
        // The normal of these triangles is down, so a ray going up enters by their back.
        let rays = Rays::new(square(10.0, glass));
        let up = rays.occluded(([4.0, 4.0, 0.0], UP, 20.0), &WORLD, &Grey, &mut Vec::new());
        let down = rays.occluded(
            ([4.0, 4.0, 20.0], [0.0, 0.0, -1.0], 20.0),
            &WORLD,
            &Grey,
            &mut Vec::new(),
        );
        let tinted: Vec<Passed> = [up, down]
            .into_iter()
            .flatten()
            .filter(|p| p.glass.is_some())
            .collect();
        assert_eq!(tinted.len(), 1, "only the side the ray leaves by tints");
        let passed = tinted.first().copied().unwrap_or_default();
        assert_eq!(passed.glass, Some(([1.0, 0.0, 0.0], 0.5)));
        assert_eq!(passed.tinted([100.0, 100.0, 100.0]), [100.0, 50.0, 50.0]);
        assert_eq!(Passed::default().tinted([1.0, 2.0, 3.0]), [1.0, 2.0, 3.0]);

        let mut fence = caster(Surface::Filtered);
        fence.fence = true;
        let rays = Rays::new(square(10.0, fence));
        assert!(!clear(&rays, [4.0, 4.0, 0.0], 20.0, &WORLD, &Grey));
        assert!(
            clear(&rays, [4.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels),
            "a missing texture is see-through"
        );
    }

    // spec LIGHT-11.8: a ray stopped only on channels its light is not on passes.
    #[test]
    fn a_ray_passes_triangles_on_other_channels() {
        let mut second = caster(Surface::Solid);
        second.channels = 2;
        let rays = Rays::new(square(10.0, second));
        let on_two = Asker {
            shadow_mask: 2,
            ..WORLD
        };
        let on_four = Asker {
            shadow_mask: 4,
            ..WORLD
        };
        assert!(!clear(&rays, [4.0, 4.0, 0.0], 20.0, &on_two, &NoTexels));
        assert!(clear(&rays, [4.0, 4.0, 0.0], 20.0, &on_four, &NoTexels));
        assert!(
            !clear(&rays, [4.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels),
            "a ray on the default channel asks only filtered triangles"
        );
        let mut filtered = caster(Surface::Filtered);
        filtered.channels = 2;
        let rays = Rays::new(square(10.0, filtered));
        assert!(clear(&rays, [4.0, 4.0, 0.0], 20.0, &WORLD, &NoTexels));
        assert!(!clear(&rays, [4.0, 4.0, 0.0], 20.0, &on_two, &NoTexels));
    }
}
