// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_phys.c, SV_Physics, SV_RunThink, SV_Physics_None, SV_Physics_Noclip, SV_Physics_Pusher, SV_PushMove, SV_PushEntity, SV_Physics_Toss, SV_CheckWaterTransition, SV_Physics_Step, SV_Impact, ClipVelocity, SV_CheckVelocity, SV_AddGravity, SV_FlyMove, SV_Physics_Client

//! The server frame (`docs/spec/engine/physics.md`): thinking, and how each kind of entity
//! moves.

use qc_builtins::math::{f64_to_f32_public, float_to_int};
use qc_types::Slot;
use qc_vm::{ErrorKind, RunError};

use crate::builtins::{FL_ONGROUND, SimCall};
use crate::collision::{
    CONTENTS_EMPTY, CONTENTS_WATER, MoveKind, Solid, Trace, test_entity_position,
};

/// `sv_gravity` (PHYS-6.4).
const GRAVITY: f32 = 800.0;
/// `sv_maxvelocity` (PHYS-6.3).
const MAXVELOCITY: f32 = 2000.0;
/// `STOP_EPSILON` (PHYS-6.2).
const STOP_EPSILON: f32 = 0.1;
/// `MAX_CLIP_PLANES` (PHYS-7.3).
const MAX_CLIP_PLANES: usize = 5;
/// `numbumps` (PHYS-7.1).
const BUMPS: usize = 4;
/// A plane is a floor when its normal's z is above this (PHYS-4.3, PHYS-7.2).
const FLOOR_NORMAL: f32 = 0.7;
/// `backoff = 1.5` for `MOVETYPE_BOUNCE` (PHYS-4.3).
const BOUNCE: f32 = 1.5;
/// A bouncer lands below this upward speed (PHYS-4.3).
const LAND_SPEED: f32 = 60.0;
/// `sv_gravity*-0.1`: a fall faster than this lands with a sound (PHYS-5.1).
const HARD_LANDING: f32 = -0.1;

/// An entity's `movetype` (PHYS-1.2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MoveType {
    /// `MOVETYPE_NONE`.
    None,
    /// `MOVETYPE_WALK`, the player's.
    Walk,
    /// `MOVETYPE_STEP`, a walking monster's.
    Step,
    /// `MOVETYPE_FLY`.
    Fly,
    /// `MOVETYPE_TOSS`.
    Toss,
    /// `MOVETYPE_PUSH`, a door's or a platform's.
    Push,
    /// `MOVETYPE_NOCLIP`.
    Noclip,
    /// `MOVETYPE_FLYMISSILE`.
    FlyMissile,
    /// `MOVETYPE_BOUNCE`.
    Bounce,
    /// Any other value.
    Other(f32),
}

impl MoveType {
    pub(crate) fn from_float(value: f32) -> Self {
        let whole = |n: u8| value.to_bits() == f32::from(n).to_bits();
        match () {
            () if whole(0) || value.to_bits() == (-0.0_f32).to_bits() => Self::None,
            () if whole(3) => Self::Walk,
            () if whole(4) => Self::Step,
            () if whole(5) => Self::Fly,
            () if whole(6) => Self::Toss,
            () if whole(7) => Self::Push,
            () if whole(8) => Self::Noclip,
            () if whole(9) => Self::FlyMissile,
            () if whole(10) => Self::Bounce,
            () => Self::Other(value),
        }
    }
}

/// A sound the server started, `SV_StartSound`: the entity and the sample's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sound {
    /// The entity it comes from.
    pub entity: usize,
    /// The sample, such as `misc/h2ohit1.wav`.
    pub sample: &'static str,
}

impl SimCall<'_, '_, '_> {
    pub(crate) fn movetype(&self, n: usize) -> MoveType {
        MoveType::from_float(self.float(n, "movetype"))
    }

    pub(crate) fn solid(&self, n: usize) -> Solid {
        Solid::from_float(self.float(n, "solid"))
    }

    pub(crate) fn is_free(&self, n: usize) -> bool {
        self.context
            .machine_view()
            .edicts()
            .is_free(n)
            .unwrap_or(true)
    }

    pub(crate) fn run(&mut self, function: i32) -> Result<(), RunError> {
        self.context.execute_program(self.sim, function)
    }

    pub(crate) fn function(&self, n: usize, name: &str) -> i32 {
        self.entity_raw(n, name)
    }

    pub(crate) fn set_time(&mut self, time: f64) {
        self.set_global("time", Slot::from_f32(f64_to_f32_public(time)));
    }

    pub(crate) fn clock(&self) -> f64 {
        self.sim.mock.engine().clock
    }

    /// `SV_Physics` (PHYS-1).
    pub(crate) fn physics(&mut self, frametime: f64, clients: &[bool]) -> Result<(), RunError> {
        let world = self.value_of(0);
        self.set_global("self", Slot::from_i32(world));
        self.set_global("other", Slot::from_i32(world));
        self.set_time(self.clock());
        let start = self.global("StartFrame").as_i32();
        self.run(start)?;
        let mut n = 0;
        while n < self.context.machine_view().edicts().num_edicts() {
            if !self.is_free(n) {
                if self.global("force_retouch").as_f32() != 0.0 {
                    self.link(n, true)?;
                }
                let client = n.checked_sub(1).filter(|&c| c < clients.len());
                match (client, self.movetype(n)) {
                    // PHYS-8.1: an unconnected slot does nothing.
                    (Some(slot), _) => {
                        if clients.get(slot).copied().unwrap_or(false) {
                            self.physics_client(n, frametime)?;
                        }
                    }
                    (None, MoveType::Push) => self.physics_pusher(n, frametime)?,
                    (None, MoveType::None) => {
                        self.run_think(n, frametime)?;
                    }
                    (None, MoveType::Noclip) => self.physics_noclip(n, frametime)?,
                    (None, MoveType::Step) => self.physics_step(n, frametime)?,
                    (
                        None,
                        MoveType::Toss | MoveType::Bounce | MoveType::Fly | MoveType::FlyMissile,
                    ) => self.physics_toss(n, frametime)?,
                    (None, other) => {
                        let value = match other {
                            MoveType::Walk => 3,
                            MoveType::Other(v) => float_to_int(v),
                            _ => 0,
                        };
                        return Err(self.context.run_error(ErrorKind::Fatal {
                            message: format!("SV_Physics: bad movetype {value}"),
                        }));
                    }
                }
            }
            n = n.saturating_add(1);
        }
        let retouch = self.global("force_retouch").as_f32();
        if retouch != 0.0 {
            self.set_global("force_retouch", Slot::from_f32(retouch - 1.0));
        }
        let clock = self.clock() + frametime;
        self.sim.mock.engine_mut().clock = clock;
        Ok(())
    }

    /// `SV_RunThink`: whether the entity is still in use (PHYS-2.1).
    pub(crate) fn run_think(&mut self, ent: usize, frametime: f64) -> Result<bool, RunError> {
        let nextthink = self.float(ent, "nextthink");
        let now = self.clock();
        if nextthink <= 0.0 || f64::from(nextthink) > now + frametime {
            return Ok(true);
        }
        // "don't let things stay in the past"
        let time = if f64::from(nextthink) < now {
            f64_to_f32_public(now)
        } else {
            nextthink
        };
        self.set_float(ent, "nextthink", 0.0);
        self.set_global("time", Slot::from_f32(time));
        let value = self.value_of(ent);
        let world = self.value_of(0);
        self.set_global("self", Slot::from_i32(value));
        self.set_global("other", Slot::from_i32(world));
        let think = self.function(ent, "think");
        self.run(think)?;
        Ok(!self.is_free(ent))
    }

    /// `SV_Physics_Noclip` (PHYS-2.3).
    fn physics_noclip(&mut self, ent: usize, frametime: f64) -> Result<(), RunError> {
        if !self.run_think(ent, frametime)? {
            return Ok(());
        }
        let t = f64_to_f32_public(frametime);
        let angles = ma(self.vector(ent, "angles"), t, self.vector(ent, "avelocity"));
        self.set_vector(ent, "angles", angles);
        let origin = ma(self.vector(ent, "origin"), t, self.vector(ent, "velocity"));
        self.set_vector(ent, "origin", origin);
        self.link(ent, false)
    }

    /// `SV_Physics_Pusher` (PHYS-3.1).
    fn physics_pusher(&mut self, ent: usize, frametime: f64) -> Result<(), RunError> {
        let frametime = f64_to_f32_public(frametime);
        let oldltime = self.float(ent, "ltime");
        let thinktime = self.float(ent, "nextthink");
        let movetime = if thinktime < oldltime + frametime {
            (thinktime - oldltime).max(0.0)
        } else {
            frametime
        };
        if movetime != 0.0 {
            self.push_move(ent, movetime)?;
        }
        if thinktime > oldltime && thinktime <= self.float(ent, "ltime") {
            self.set_float(ent, "nextthink", 0.0);
            self.set_time(self.clock());
            let value = self.value_of(ent);
            let world = self.value_of(0);
            self.set_global("self", Slot::from_i32(value));
            self.set_global("other", Slot::from_i32(world));
            let think = self.function(ent, "think");
            self.run(think)?;
        }
        Ok(())
    }

    /// `SV_PushMove` (PHYS-3.2 to PHYS-3.5).
    fn push_move(&mut self, pusher: usize, movetime: f32) -> Result<(), RunError> {
        let velocity = self.vector(pusher, "velocity");
        let ltime = self.float(pusher, "ltime");
        if is_zero(velocity) {
            self.set_float(pusher, "ltime", ltime + movetime);
            return Ok(());
        }
        let step = velocity.map(|v| v * movetime);
        let mins = add(self.vector(pusher, "absmin"), step);
        let maxs = add(self.vector(pusher, "absmax"), step);
        let pushorig = self.vector(pusher, "origin");
        self.set_vector(pusher, "origin", add(pushorig, step));
        self.set_float(pusher, "ltime", ltime + movetime);
        self.link(pusher, false)?;
        let mut moved: Vec<(usize, [f32; 3])> = Vec::new();
        let mut check = 1;
        while check < self.context.machine_view().edicts().num_edicts() {
            let n = check;
            check = check.saturating_add(1);
            if self.is_free(n)
                || matches!(
                    self.movetype(n),
                    MoveType::Push | MoveType::None | MoveType::Noclip
                )
            {
                continue;
            }
            let riding =
                self.flags(n) & FL_ONGROUND != 0 && self.entity(n, "groundentity") == pusher;
            if !riding {
                let (absmin, absmax) = (self.vector(n, "absmin"), self.vector(n, "absmax"));
                let apart = absmin.iter().zip(maxs).any(|(a, m)| *a >= m)
                    || absmax.iter().zip(mins).any(|(a, m)| *a <= m);
                if apart || !test_entity_position(self, n).map_err(|e| self.error(e))? {
                    continue;
                }
            }
            if self.movetype(n) != MoveType::Walk {
                let flags = self.flags(n) & !FL_ONGROUND;
                self.set_flags(n, flags);
            }
            let entorig = self.vector(n, "origin");
            moved.push((n, entorig));
            self.set_float(pusher, "solid", 0.0);
            self.push_entity(n, step)?;
            self.set_float(pusher, "solid", 4.0);
            if test_entity_position(self, n).map_err(|e| self.error(e))? {
                let (mins_n, maxs_n) = (self.vector(n, "mins"), self.vector(n, "maxs"));
                if mins_n[0].to_bits() == maxs_n[0].to_bits() {
                    continue;
                }
                if matches!(self.solid(n), Solid::Not | Solid::Trigger) {
                    // "corpse"
                    let flat = [0.0, 0.0, mins_n[2]];
                    self.set_vector(n, "mins", flat);
                    self.set_vector(n, "maxs", flat);
                    continue;
                }
                self.set_vector(n, "origin", entorig);
                self.link(n, true)?;
                self.set_vector(pusher, "origin", pushorig);
                self.link(pusher, false)?;
                let ltime = self.float(pusher, "ltime");
                self.set_float(pusher, "ltime", ltime - movetime);
                let blocked = self.function(pusher, "blocked");
                if blocked != 0 {
                    let (p, o) = (self.value_of(pusher), self.value_of(n));
                    self.set_global("self", Slot::from_i32(p));
                    self.set_global("other", Slot::from_i32(o));
                    self.run(blocked)?;
                }
                for (m, from) in moved {
                    self.set_vector(m, "origin", from);
                    self.link(m, false)?;
                }
                return Ok(());
            }
        }
        Ok(())
    }

    /// `SV_PushEntity` (PHYS-3.6).
    pub(crate) fn push_entity(&mut self, ent: usize, push: [f32; 3]) -> Result<Trace, RunError> {
        let origin = self.vector(ent, "origin");
        let end = add(origin, push);
        let kind = if self.movetype(ent) == MoveType::FlyMissile {
            MoveKind::Missile
        } else if matches!(self.solid(ent), Solid::Trigger | Solid::Not) {
            MoveKind::NoMonsters
        } else {
            MoveKind::Normal
        };
        let line = (
            origin,
            self.vector(ent, "mins"),
            self.vector(ent, "maxs"),
            end,
        );
        let trace = self.trace(line, kind, ent)?;
        self.set_vector(ent, "origin", trace.endpos);
        self.link(ent, true)?;
        if let Some(hit) = trace.ent {
            self.impact(ent, hit)?;
        }
        Ok(trace)
    }

    /// `SV_Impact` (PHYS-6.1).
    fn impact(&mut self, e1: usize, e2: usize) -> Result<(), RunError> {
        let (old_self, old_other) = (self.global("self"), self.global("other"));
        self.set_time(self.clock());
        for (a, b) in [(e1, e2), (e2, e1)] {
            let touch = self.function(a, "touch");
            if touch != 0 && self.solid(a) != Solid::Not {
                let (va, vb) = (self.value_of(a), self.value_of(b));
                self.set_global("self", Slot::from_i32(va));
                self.set_global("other", Slot::from_i32(vb));
                self.run(touch)?;
            }
        }
        self.set_global("self", old_self);
        self.set_global("other", old_other);
        Ok(())
    }

    /// `SV_Physics_Toss` (PHYS-4).
    pub(crate) fn physics_toss(&mut self, ent: usize, frametime: f64) -> Result<(), RunError> {
        if !self.run_think(ent, frametime)? {
            return Ok(());
        }
        if self.flags(ent) & FL_ONGROUND != 0 {
            return Ok(());
        }
        self.check_velocity(ent);
        let movetype = self.movetype(ent);
        if !matches!(movetype, MoveType::Fly | MoveType::FlyMissile) {
            self.add_gravity(ent, frametime);
        }
        let t = f64_to_f32_public(frametime);
        let angles = ma(self.vector(ent, "angles"), t, self.vector(ent, "avelocity"));
        self.set_vector(ent, "angles", angles);
        let push = self.vector(ent, "velocity").map(|v| v * t);
        let trace = self.push_entity(ent, push)?;
        #[expect(
            clippy::float_cmp,
            reason = "spec PHYS-4.3: id tests trace.fraction == 1"
        )]
        let whole = trace.fraction == 1.0;
        if whole || self.is_free(ent) {
            return Ok(());
        }
        let overbounce = if movetype == MoveType::Bounce {
            BOUNCE
        } else {
            1.0
        };
        let (velocity, _) =
            clip_velocity(self.vector(ent, "velocity"), trace.plane.normal, overbounce);
        self.set_vector(ent, "velocity", velocity);
        if trace.plane.normal[2] > FLOOR_NORMAL
            && (velocity[2] < LAND_SPEED || movetype != MoveType::Bounce)
        {
            let flags = self.flags(ent) | FL_ONGROUND;
            self.set_flags(ent, flags);
            self.set_entity(ent, "groundentity", trace.ent.unwrap_or(0));
            self.set_vector(ent, "velocity", [0.0; 3]);
            self.set_vector(ent, "avelocity", [0.0; 3]);
        }
        self.check_water_transition(ent)
    }

    /// `SV_CheckWaterTransition` (PHYS-4.4).
    pub(crate) fn check_water_transition(&mut self, ent: usize) -> Result<(), RunError> {
        let cont = self
            .sim
            .collision
            .point_contents(self.vector(ent, "origin"))
            .map_err(|e| self.error(e))?;
        let cont_f = f32::from(i16::try_from(cont).unwrap_or(i16::MIN));
        let watertype = self.float(ent, "watertype");
        if watertype == 0.0 {
            // "just spawned here"
            self.set_float(ent, "watertype", cont_f);
            self.set_float(ent, "waterlevel", 1.0);
            return Ok(());
        }
        let empty = f32::from(i16::try_from(CONTENTS_EMPTY).unwrap_or(-1));
        #[expect(
            clippy::float_cmp,
            reason = "spec PHYS-4.4: id compares watertype as a float"
        )]
        let was_empty = watertype == empty;
        if cont <= CONTENTS_WATER {
            if was_empty {
                self.sound(ent, "misc/h2ohit1.wav");
            }
            self.set_float(ent, "watertype", cont_f);
            self.set_float(ent, "waterlevel", 1.0);
        } else {
            if !was_empty {
                self.sound(ent, "misc/h2ohit1.wav");
            }
            self.set_float(ent, "watertype", empty);
            self.set_float(ent, "waterlevel", cont_f);
        }
        Ok(())
    }

    fn sound(&mut self, entity: usize, sample: &'static str) {
        self.sim.sounds.push(Sound { entity, sample });
    }

    /// `SV_Physics_Step` (PHYS-5).
    fn physics_step(&mut self, ent: usize, frametime: f64) -> Result<(), RunError> {
        const SUPPORTED: i32 = FL_ONGROUND | 1 | 2;
        if self.flags(ent) & SUPPORTED == 0 {
            let hitsound = self.vector(ent, "velocity")[2] < GRAVITY * HARD_LANDING;
            self.add_gravity(ent, frametime);
            self.check_velocity(ent);
            self.fly_move(ent, f64_to_f32_public(frametime), &mut None)?;
            self.link(ent, true)?;
            if self.flags(ent) & FL_ONGROUND != 0 && hitsound {
                self.sound(ent, "demon/dland2.wav");
            }
        }
        self.run_think(ent, frametime)?;
        self.check_water_transition(ent)
    }

    /// `SV_CheckVelocity` (PHYS-6.3).
    pub(crate) fn check_velocity(&mut self, ent: usize) {
        let velocity = self.vector(ent, "velocity");
        let origin = self.vector(ent, "origin");
        let velocity = velocity.map(|v| {
            if v.is_nan() {
                0.0
            } else {
                v.clamp(-MAXVELOCITY, MAXVELOCITY)
            }
        });
        let origin = origin.map(|v| if v.is_nan() { 0.0 } else { v });
        self.set_vector(ent, "velocity", velocity);
        self.set_vector(ent, "origin", origin);
    }

    /// `SV_AddGravity` (PHYS-6.4).
    pub(crate) fn add_gravity(&mut self, ent: usize, frametime: f64) {
        let gravity = self
            .progs_field(ent, "gravity")
            .map(Slot::as_f32)
            .filter(|g| *g != 0.0)
            .unwrap_or(1.0);
        let [x, y, z] = self.vector(ent, "velocity");
        let fall = f64_to_f32_public(f64::from(gravity) * f64::from(GRAVITY) * frametime);
        self.set_vector(ent, "velocity", [x, y, z - fall]);
    }

    /// `SV_FlyMove` (PHYS-7): what blocked the move.
    pub(crate) fn fly_move(
        &mut self,
        ent: usize,
        time: f32,
        steptrace: &mut Option<Trace>,
    ) -> Result<i32, RunError> {
        let mut blocked = 0;
        let mut original = self.vector(ent, "velocity");
        let primal = original;
        let mut planes: Vec<[f32; 3]> = Vec::new();
        let mut time_left = time;
        for _ in 0..BUMPS {
            let velocity = self.vector(ent, "velocity");
            if is_zero(velocity) {
                break;
            }
            let origin = self.vector(ent, "origin");
            let end = ma(origin, time_left, velocity);
            let line = (
                origin,
                self.vector(ent, "mins"),
                self.vector(ent, "maxs"),
                end,
            );
            let trace = self.trace(line, MoveKind::Normal, ent)?;
            if trace.allsolid {
                // "entity is trapped in another solid"
                self.set_vector(ent, "velocity", [0.0; 3]);
                return Ok(3);
            }
            if trace.fraction > 0.0 {
                self.set_vector(ent, "origin", trace.endpos);
                original = self.vector(ent, "velocity");
                planes.clear();
            }
            #[expect(
                clippy::float_cmp,
                reason = "spec PHYS-7.1: id tests trace.fraction == 1"
            )]
            let whole = trace.fraction == 1.0;
            if whole {
                break;
            }
            let Some(hit) = trace.ent else {
                return Err(self.context.run_error(ErrorKind::Fatal {
                    message: "SV_FlyMove: !trace.ent".to_owned(),
                }));
            };
            let normal = trace.plane.normal;
            if normal[2] > FLOOR_NORMAL {
                blocked |= 1;
                if self.solid(hit) == Solid::Bsp {
                    let flags = self.flags(ent) | FL_ONGROUND;
                    self.set_flags(ent, flags);
                    self.set_entity(ent, "groundentity", hit);
                }
            }
            if normal[2] == 0.0 {
                blocked |= 2;
                // "save for player extrafriction"
                *steptrace = Some(trace);
            }
            self.impact(ent, hit)?;
            if self.is_free(ent) {
                break;
            }
            time_left -= time_left * trace.fraction;
            if planes.len() >= MAX_CLIP_PLANES {
                // "this shouldn't really happen"
                self.set_vector(ent, "velocity", [0.0; 3]);
                return Ok(3);
            }
            planes.push(normal);
            // Parallel to every plane, if one clip is.
            let along = planes.iter().enumerate().find_map(|(i, plane)| {
                let (clipped, _) = clip_velocity(original, *plane, 1.0);
                let fits = planes
                    .iter()
                    .enumerate()
                    .all(|(j, other)| j == i || dot(clipped, *other) >= 0.0);
                fits.then_some(clipped)
            });
            let velocity = match (along, planes.as_slice()) {
                (Some(v), _) => v,
                (None, [a, b]) => {
                    // "go along the crease"
                    let dir = cross(*a, *b);
                    let d = dot(dir, self.vector(ent, "velocity"));
                    dir.map(|c| c * d)
                }
                (None, _) => {
                    self.set_vector(ent, "velocity", [0.0; 3]);
                    return Ok(7);
                }
            };
            self.set_vector(ent, "velocity", velocity);
            // "stop dead to avoid tiny occilations in sloping corners"
            if dot(velocity, primal) <= 0.0 {
                self.set_vector(ent, "velocity", [0.0; 3]);
                return Ok(blocked);
            }
        }
        Ok(blocked)
    }
}

/// `ClipVelocity` (PHYS-6.2): the velocity slid off the plane, and the blocked bits.
fn clip_velocity(velocity: [f32; 3], normal: [f32; 3], overbounce: f32) -> ([f32; 3], i32) {
    let mut blocked = 0;
    if normal[2] > 0.0 {
        blocked |= 1;
    }
    if normal[2] == 0.0 {
        blocked |= 2;
    }
    let backoff = dot(velocity, normal) * overbounce;
    let mut out = [0.0; 3];
    for ((o, v), n) in out.iter_mut().zip(velocity).zip(normal) {
        let value = v - n * backoff;
        *o = if value > -STOP_EPSILON && value < STOP_EPSILON {
            0.0
        } else {
            value
        };
    }
    (out, blocked)
}

/// `!v[0] && !v[1] && !v[2]`.
fn is_zero(v: [f32; 3]) -> bool {
    v.iter().all(|c| *c == 0.0)
}

fn dot([a, b, c]: [f32; 3], [x, y, z]: [f32; 3]) -> f32 {
    a * x + b * y + c * z
}

fn cross([a, b, c]: [f32; 3], [x, y, z]: [f32; 3]) -> [f32; 3] {
    [b * z - c * y, c * x - a * z, a * y - b * x]
}

fn add([a, b, c]: [f32; 3], [x, y, z]: [f32; 3]) -> [f32; 3] {
    [a + x, b + y, c + z]
}

/// `VectorMA (a, scale, b)`.
fn ma([a, b, c]: [f32; 3], scale: f32, [x, y, z]: [f32; 3]) -> [f32; 3] {
    [a + scale * x, b + scale * y, c + scale * z]
}
