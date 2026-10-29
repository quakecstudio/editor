// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_move.c, SV_CheckBottom, SV_movestep, SV_StepDirection, SV_FixCheckBottom, SV_NewChaseDir, SV_CloseEnough, SV_MoveToGoal
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, PF_walkmove

//! Monster movement (`docs/spec/engine/move.md`): whether the ground is under a box, one
//! step, a step in a direction, choosing a new one, and moving toward a goal.

use qc_builtins::math::{anglemod, f64_to_f32_public, float_to_int, int_to_f32_public};
use qc_vm::{Builtins as _, RunError};

use crate::builtins::{FL_ONGROUND, SimCall, parm_float};
use crate::collision::{CONTENTS_EMPTY, CONTENTS_SOLID, MoveKind, Trace};

/// `STEPSIZE` (MOVE-1.3, MOVE-2.2).
const STEPSIZE: f32 = 18.0;
/// How far a flying monster's step bends toward its enemy (MOVE-2.1).
const FLY_ADJUST: f32 = 8.0;
/// `dz > 40` lowers a flying monster's step (MOVE-2.1).
const FLY_HIGH: f32 = 40.0;
/// `dz < 30` raises it (MOVE-2.1).
const FLY_LOW: f32 = 30.0;
/// `FL_FLY`.
const FL_FLY: i32 = 1;
/// `FL_SWIM`.
const FL_SWIM: i32 = 2;
/// `FL_PARTIALGROUND`.
const FL_PARTIALGROUND: i32 = 1024;
/// `ChangeYaw`, builtin 49 (MOVE-3.1).
const CHANGEYAW: u32 = 49;
/// How far an enemy must be off an axis for the chase to head along it (MOVE-4.1).
const CHASE_SLACK: f32 = 10.0;
/// The turn a step must be within, `delta > 45 && delta < 315` (MOVE-3.2).
const TURN_TOLERANCE: (f32, f32) = (45.0, 315.0);

impl SimCall<'_, '_, '_> {
    fn point(&self, (from, to): ([f32; 3], [f32; 3]), ent: usize) -> Result<Trace, RunError> {
        self.trace((from, [0.0; 3], [0.0; 3], to), MoveKind::NoMonsters, ent)
    }

    /// `SV_CheckBottom` (MOVE-1).
    pub(crate) fn bottom(&self, ent: usize) -> Result<bool, RunError> {
        let origin = self.vector(ent, "origin");
        let mins = add(origin, self.vector(ent, "mins"));
        let maxs = add(origin, self.vector(ent, "maxs"));
        let corners = [
            (mins[0], mins[1]),
            (mins[0], maxs[1]),
            (maxs[0], mins[1]),
            (maxs[0], maxs[1]),
        ];
        // MOVE-1.1
        let mut all_solid = true;
        for (x, y) in corners {
            let contents = self
                .sim
                .collision
                .point_contents([x, y, mins[2] - 1.0])
                .map_err(|e| self.error(e))?;
            if contents != CONTENTS_SOLID {
                all_solid = false;
                break;
            }
        }
        if all_solid {
            return Ok(true);
        }
        // MOVE-1.2
        let middle = [(mins[0] + maxs[0]) * 0.5, (mins[1] + maxs[1]) * 0.5];
        let top = mins[2];
        let floor = top - 2.0 * STEPSIZE;
        let centre = self.point(
            ([middle[0], middle[1], top], [middle[0], middle[1], floor]),
            ent,
        )?;
        if is_whole(centre.fraction) {
            return Ok(false);
        }
        let mid = centre.endpos[2];
        // MOVE-1.3
        for (x, y) in corners {
            let trace = self.point(([x, y, top], [x, y, floor]), ent)?;
            if is_whole(trace.fraction) || mid - trace.endpos[2] > STEPSIZE {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// `SV_movestep` (MOVE-2).
    fn movestep(&mut self, ent: usize, step: [f32; 3], relink: bool) -> Result<bool, RunError> {
        let oldorg = self.vector(ent, "origin");
        let mins = self.vector(ent, "mins");
        let maxs = self.vector(ent, "maxs");
        let flags = self.flags(ent);
        // MOVE-2.1: flying and swimming monsters do not step up.
        if flags & (FL_SWIM | FL_FLY) != 0 {
            let enemy = self.entity(ent, "enemy");
            for attempt in 0..2 {
                let origin = self.vector(ent, "origin");
                let mut neworg = add(origin, step);
                if attempt == 0 && enemy != 0 {
                    let dz = origin[2] - self.vector(enemy, "origin")[2];
                    if dz > FLY_HIGH {
                        neworg[2] -= FLY_ADJUST;
                    }
                    if dz < FLY_LOW {
                        neworg[2] += FLY_ADJUST;
                    }
                }
                let trace = self.trace((origin, mins, maxs, neworg), MoveKind::Normal, ent)?;
                if is_whole(trace.fraction) {
                    let contents = self
                        .sim
                        .collision
                        .point_contents(trace.endpos)
                        .map_err(|e| self.error(e))?;
                    if flags & FL_SWIM != 0 && contents == CONTENTS_EMPTY {
                        // "swim monster left water"
                        return Ok(false);
                    }
                    self.set_vector(ent, "origin", trace.endpos);
                    if relink {
                        self.link(ent, true)?;
                    }
                    return Ok(true);
                }
                if enemy == 0 {
                    break;
                }
            }
            return Ok(false);
        }
        // MOVE-2.2: push down from a step height above the wished position.
        let mut neworg = add(oldorg, step);
        neworg[2] += STEPSIZE;
        let mut end = neworg;
        end[2] -= STEPSIZE * 2.0;
        let mut trace = self.trace((neworg, mins, maxs, end), MoveKind::Normal, ent)?;
        if trace.allsolid {
            return Ok(false);
        }
        if trace.startsolid {
            neworg[2] -= STEPSIZE;
            trace = self.trace((neworg, mins, maxs, end), MoveKind::Normal, ent)?;
            if trace.allsolid || trace.startsolid {
                return Ok(false);
            }
        }
        // MOVE-2.3
        if is_whole(trace.fraction) {
            if flags & FL_PARTIALGROUND != 0 {
                // "if monster had the ground pulled out, go ahead and fall"
                self.set_vector(ent, "origin", add(oldorg, step));
                if relink {
                    self.link(ent, true)?;
                }
                let flags = self.flags(ent) & !FL_ONGROUND;
                self.set_flags(ent, flags);
                return Ok(true);
            }
            // "walked off an edge"
            return Ok(false);
        }
        // MOVE-2.4: check point traces down for dangling corners.
        self.set_vector(ent, "origin", trace.endpos);
        if !self.bottom(ent)? {
            if flags & FL_PARTIALGROUND != 0 {
                if relink {
                    self.link(ent, true)?;
                }
                return Ok(true);
            }
            self.set_vector(ent, "origin", oldorg);
            return Ok(false);
        }
        // MOVE-2.5: only when it is set, so a `flags` that is not a whole number is kept.
        if flags & FL_PARTIALGROUND != 0 {
            let flags = self.flags(ent) & !FL_PARTIALGROUND;
            self.set_flags(ent, flags);
        }
        self.set_entity(ent, "groundentity", trace.ent.unwrap_or(0));
        if relink {
            self.link(ent, true)?;
        }
        Ok(true)
    }

    /// `SV_StepDirection` (MOVE-3).
    fn step_direction(&mut self, ent: usize, yaw: f32, dist: f32) -> Result<bool, RunError> {
        self.set_float(ent, "ideal_yaw", yaw);
        self.sim.mock.call(CHANGEYAW, self.context)?;
        let step = step(yaw, dist);
        let oldorigin = self.vector(ent, "origin");
        if self.movestep(ent, step, false)? {
            let delta = self.vector(ent, "angles")[1] - self.float(ent, "ideal_yaw");
            if delta > TURN_TOLERANCE.0 && delta < TURN_TOLERANCE.1 {
                // "not turned far enough, so don't take the step"
                self.set_vector(ent, "origin", oldorigin);
            }
            self.link(ent, true)?;
            return Ok(true);
        }
        self.link(ent, true)?;
        Ok(false)
    }

    /// `SV_NewChaseDir` (MOVE-4).
    #[expect(
        clippy::float_cmp,
        reason = "spec MOVE-4.2: id compares the headings as floats"
    )]
    fn new_chase_dir(&mut self, actor: usize, enemy: usize, dist: f32) -> Result<(), RunError> {
        let ideal = self.float(actor, "ideal_yaw");
        let olddir = anglemod(int_to_f32_public(
            float_to_int(ideal / 45.0).wrapping_mul(45),
        ));
        let turnaround = anglemod(olddir - 180.0);
        let [ex, ey, _] = self.vector(enemy, "origin");
        let [ax, ay, _] = self.vector(actor, "origin");
        let (deltax, deltay) = (ex - ax, ey - ay);
        // MOVE-4.1: `None` is id's DI_NODIR.
        let mut d1 = if deltax > CHASE_SLACK {
            Some(0.0)
        } else if deltax < -CHASE_SLACK {
            Some(180.0)
        } else {
            None
        };
        let mut d2 = if deltay < -CHASE_SLACK {
            Some(270.0)
        } else if deltay > CHASE_SLACK {
            Some(90.0)
        } else {
            None
        };
        // MOVE-4.2: the direct route.
        if let (Some(x), Some(y)) = (d1, d2) {
            let tdir = if x == 0.0 {
                if y == 90.0 { 45.0 } else { 315.0 }
            } else if y == 90.0 {
                135.0
            } else {
                215.0
            };
            if tdir != turnaround && self.step_direction(actor, tdir, dist)? {
                return Ok(());
            }
        }
        // MOVE-4.3: the other directions, in an order one rand() decides.
        let swap = (self.rand() & 3) & 1 != 0
            || float_to_int(deltay).unsigned_abs() > float_to_int(deltax).unsigned_abs();
        if swap {
            (d1, d2) = (d2, d1);
        }
        for dir in [d1, d2].into_iter().flatten() {
            if dir != turnaround && self.step_direction(actor, dir, dist)? {
                return Ok(());
            }
        }
        // MOVE-4.4: no direct path, so another direction. id tests `olddir != DI_NODIR` and
        // `turnaround != DI_NODIR`, which always hold: anglemod is never -1.
        if self.step_direction(actor, olddir, dist)? {
            return Ok(());
        }
        let headings: [f32; 8] = [0.0, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
        let order: Vec<f32> = if self.rand() & 1 != 0 {
            headings.to_vec()
        } else {
            headings.iter().rev().copied().collect()
        };
        for tdir in order {
            if tdir != turnaround && self.step_direction(actor, tdir, dist)? {
                return Ok(());
            }
        }
        if self.step_direction(actor, turnaround, dist)? {
            return Ok(());
        }
        // MOVE-4.5: "can't move".
        self.set_float(actor, "ideal_yaw", olddir);
        if !self.bottom(actor)? {
            let flags = self.flags(actor) | FL_PARTIALGROUND;
            self.set_flags(actor, flags);
        }
        Ok(())
    }

    /// `SV_CloseEnough` (MOVE-5.2).
    fn close_enough(&self, ent: usize, goal: usize, dist: f32) -> bool {
        let (gmin, gmax) = (self.vector(goal, "absmin"), self.vector(goal, "absmax"));
        let (emin, emax) = (self.vector(ent, "absmin"), self.vector(ent, "absmax"));
        gmin.iter()
            .zip(gmax)
            .zip(emin.iter().zip(emax))
            .all(|((&gn, gx), (&en, ex))| gn <= ex + dist && gx >= en - dist)
    }

    /// `SV_MoveToGoal`, builtin `movetogoal` (MOVE-5).
    pub(crate) fn move_to_goal(&mut self) -> Result<(), RunError> {
        let ent = self.self_edict();
        let goal = self.entity(ent, "goalentity");
        let dist = parm_float(self.context, 0);
        if self.flags(ent) & (FL_ONGROUND | FL_FLY | FL_SWIM) == 0 {
            return self.set_return(0.0);
        }
        // "if the next step hits the enemy, return immediately"
        if self.entity(ent, "enemy") != 0 && self.close_enough(ent, goal, dist) {
            return Ok(());
        }
        // "bump around..."
        let ideal = self.float(ent, "ideal_yaw");
        if self.rand() & 3 == 1 || !self.step_direction(ent, ideal, dist)? {
            self.new_chase_dir(ent, goal, dist)?;
        }
        Ok(())
    }

    /// `PF_walkmove` (WORLD-8.6, BUILTIN-15.1, BUILTIN-15.2).
    pub(crate) fn walkmove(&mut self) -> Result<(), RunError> {
        let ent = self.self_edict();
        let yaw = parm_float(self.context, 0);
        let dist = parm_float(self.context, 1);
        if self.flags(ent) & (FL_ONGROUND | FL_FLY | FL_SWIM) == 0 {
            return self.set_return(0.0);
        }
        let old_self = self.global("self");
        let moved = self.movestep(ent, step(yaw, dist), true)?;
        self.set_return(if moved { 1.0 } else { 0.0 })?;
        self.set_global("self", old_self);
        Ok(())
    }
}

/// `(cos(y)*dist, sin(y)*dist, 0)` with `y = yaw*M_PI*2 / 360` in `double` stored in `float`
/// (MOVE-3.1).
fn step(yaw: f32, dist: f32) -> [f32; 3] {
    let y = f64::from(f64_to_f32_public(
        f64::from(yaw) * core::f64::consts::PI * 2.0 / 360.0,
    ));
    let d = f64::from(dist);
    [
        f64_to_f32_public(y.cos() * d),
        f64_to_f32_public(y.sin() * d),
        0.0,
    ]
}

/// `fraction == 1`.
#[expect(
    clippy::float_cmp,
    reason = "spec MOVE-1.2: id tests trace.fraction == 1.0"
)]
fn is_whole(fraction: f32) -> bool {
    fraction == 1.0
}

fn add([a, b, c]: [f32; 3], [x, y, z]: [f32; 3]) -> [f32; 3] {
    [a + x, b + y, c + z]
}
