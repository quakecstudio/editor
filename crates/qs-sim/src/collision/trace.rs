// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/world.c, SV_RecursiveHullCheck, SV_ClipMoveToEntity

//! A line through a hull (WORLD-3), and a move clipped to one entity (WORLD-4).

use super::hull::{
    BrushModel, CONTENTS_EMPTY, CONTENTS_SOLID, Hull, HullSet, add, hull_for_entity, sub,
};
use super::{Body, CollisionError};

/// `DIST_EPSILON`, "1/32 epsilon to keep floating point happy" (WORLD-3.4).
pub const DIST_EPSILON: f64 = 0.03125;
/// `frac -= 0.1`, the step a crossing backs up by while it is in solid (WORLD-3.6).
const BACKUP: f64 = 0.1;
/// The name of `SV_RecursiveHullCheck`'s error (WORLD-3.2).
const BAD_NODE: &str = "SV_RecursiveHullCheck: bad node number";

/// The plane a trace stopped on.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TracePlane {
    /// Its normal, facing the side the move came from.
    pub normal: [f32; 3],
    /// Its distance.
    pub dist: f32,
}

/// What a move hit: id's `trace_t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    /// The whole move was in solid.
    pub allsolid: bool,
    /// The move started in solid.
    pub startsolid: bool,
    /// Some of the move was in open space.
    pub inopen: bool,
    /// Some of the move was in liquid.
    pub inwater: bool,
    /// How much of the move was made, 0 to 1.
    pub fraction: f32,
    /// Where the move ended.
    pub endpos: [f32; 3],
    /// The plane it stopped on.
    pub plane: TracePlane,
    /// The entity it hit.
    pub ent: Option<usize>,
}

impl Trace {
    /// The trace a clip starts from: all zero but for `fraction` 1, `allsolid` and `endpos`
    /// `end` (WORLD-4.1).
    #[must_use]
    pub fn new(end: [f32; 3]) -> Self {
        Self {
            allsolid: true,
            startsolid: false,
            inopen: false,
            inwater: false,
            fraction: 1.0,
            endpos: end,
            plane: TracePlane::default(),
            ent: None,
        }
    }
}

/// One stretch of a line through a hull: from `p1` to `p2`, over `p1f` to `p2f` of the move.
#[derive(Clone, Copy, Debug)]
struct Stretch {
    p1f: f32,
    p2f: f32,
    p1: [f32; 3],
    p2: [f32; 3],
}

impl Stretch {
    /// The point `frac` of the way along, and its share of the move (WORLD-3.4).
    fn at(&self, frac: f32) -> (f32, [f32; 3]) {
        let midf = self.p1f + (self.p2f - self.p1f) * frac;
        let mut mid = self.p1;
        for ((m, a), b) in mid.iter_mut().zip(self.p1).zip(self.p2) {
            *m = a + frac * (b - a);
        }
        (midf, mid)
    }
}

/// `SV_RecursiveHullCheck` from clipnode `num` along the whole move from `p1` to `p2`
/// (WORLD-3): false once it found the impact.
///
/// # Errors
///
/// [`CollisionError::BadNode`] at a clipnode outside the hull, and when the check goes deeper
/// than the hull has clipnodes, which only a cycle can do.
pub fn recursive_hull_check(
    hull: &Hull<'_>,
    num: i32,
    (p1, p2): ([f32; 3], [f32; 3]),
    trace: &mut Trace,
) -> Result<bool, CollisionError> {
    let stretch = Stretch {
        p1f: 0.0,
        p2f: 1.0,
        p1,
        p2,
    };
    check(hull, num, stretch, trace, hull.size())
}

fn check(
    hull: &Hull<'_>,
    num: i32,
    s: Stretch,
    trace: &mut Trace,
    depth: usize,
) -> Result<bool, CollisionError> {
    // WORLD-3.1: a contents value.
    if num < 0 {
        if num == CONTENTS_SOLID {
            trace.startsolid = true;
        } else {
            trace.allsolid = false;
            if num == CONTENTS_EMPTY {
                trace.inopen = true;
            } else {
                trace.inwater = true;
            }
        }
        return Ok(true);
    }
    let depth = depth
        .checked_sub(1)
        .ok_or(CollisionError::BadNode(BAD_NODE))?;
    let (node, plane) = hull.node(num, BAD_NODE)?;
    let [front, back] = node.children;
    // WORLD-3.3
    let t1 = plane.distance(s.p1);
    let t2 = plane.distance(s.p2);
    if t1 >= 0.0 && t2 >= 0.0 {
        return check(hull, front, s, trace, depth);
    }
    if t1 < 0.0 && t2 < 0.0 {
        return check(hull, back, s, trace, depth);
    }
    // WORLD-3.4: the crossing, DIST_EPSILON on the near side.
    let epsilon = if t1 < 0.0 {
        DIST_EPSILON
    } else {
        -DIST_EPSILON
    };
    let mut frac = narrow((f64::from(t1) + epsilon) / f64::from(t1 - t2)).clamp(0.0, 1.0);
    let (mut midf, mut mid) = s.at(frac);
    let (near, far) = if t1 < 0.0 {
        (back, front)
    } else {
        (front, back)
    };
    // WORLD-3.5: up to the node, then past it when the far side is not solid there.
    let to_mid = Stretch {
        p2f: midf,
        p2: mid,
        ..s
    };
    if !check(hull, near, to_mid, trace, depth)? {
        return Ok(false);
    }
    if hull.point_contents(far, mid)? != CONTENTS_SOLID {
        let past = Stretch {
            p1f: midf,
            p1: mid,
            ..s
        };
        return check(hull, far, past, trace, depth);
    }
    // WORLD-3.6: the impact.
    if trace.allsolid {
        return Ok(false);
    }
    trace.plane = if t1 < 0.0 {
        let [a, b, c] = plane.normal;
        TracePlane {
            normal: [-a, -b, -c],
            dist: -plane.dist,
        }
    } else {
        TracePlane {
            normal: plane.normal,
            dist: plane.dist,
        }
    };
    while hull.point_contents(hull.first, mid)? == CONTENTS_SOLID {
        frac = narrow(f64::from(frac) - BACKUP);
        if frac < 0.0 {
            // "backup past 0"
            trace.fraction = midf;
            trace.endpos = mid;
            return Ok(false);
        }
        (midf, mid) = s.at(frac);
    }
    trace.fraction = midf;
    trace.endpos = mid;
    Ok(false)
}

/// A `double` stored in a `float`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec WORLD-3.4: the double quotient is stored in the float frac"
)]
const fn narrow(value: f64) -> f32 {
    value as f32
}

/// `SV_ClipMoveToEntity`: the move of a box of `mins` to `maxs` from `start` to `end`
/// clipped to `ent`, entity number `number` (WORLD-4.1).
///
/// # Errors
///
/// The errors of [`hull_for_entity`] and [`recursive_hull_check`].
pub fn clip_move_to_entity(
    (number, ent): (usize, &Body),
    model: Option<&BrushModel>,
    sets: &[HullSet],
    (start, mins, maxs, end): ([f32; 3], [f32; 3], [f32; 3], [f32; 3]),
) -> Result<Trace, CollisionError> {
    let mut trace = Trace::new(end);
    let (hull, offset) = hull_for_entity(ent, model, sets, (mins, maxs))?;
    let hull = hull.hull();
    recursive_hull_check(
        &hull,
        hull.first,
        (sub(start, offset), sub(end, offset)),
        &mut trace,
    )?;
    #[expect(clippy::float_cmp, reason = "spec WORLD-4.1: id tests fraction != 1")]
    let moved = trace.fraction != 1.0;
    if moved {
        trace.endpos = add(trace.endpos, offset);
    }
    if trace.fraction < 1.0 || trace.startsolid {
        trace.ent = Some(number);
    }
    Ok(trace)
}
