// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_user.c, SV_ClientThink, SV_AirMove, SV_WaterMove, SV_WaterJump, SV_UserFriction, SV_Accelerate, SV_AirAccelerate, DropPunchAngle, SV_ReadClientMove
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_phys.c, SV_Physics_Client, SV_WalkMove, SV_CheckWater, SV_CheckStuck, SV_WallFriction, SV_TryUnstick
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/view.c, V_CalcRoll
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/mathlib.c, AngleVectors, VectorNormalize, Length

//! The player (`docs/spec/engine/physics.md` PHYS-8 and PHYS-9): what the client asks for each
//! frame, and how the server moves the player for it.

use qc_builtins::math::{f64_to_f32_public, float_to_int};
use qc_types::Slot;
use qc_vm::{ErrorKind, RunError};

use crate::builtins::{FL_ONGROUND, SimCall};
use crate::collision::{CONTENTS_WATER, MoveKind, Solid, Trace, test_entity_position};
use crate::physics::MoveType;

/// `sv_friction` (PHYS-9.3).
const FRICTION: f32 = 4.0;
/// `sv_edgefriction`, "edgefriction" (PHYS-9.3).
const EDGE_FRICTION: f32 = 2.0;
/// `sv_stopspeed` (PHYS-9.3).
const STOP_SPEED: f32 = 100.0;
/// `sv_maxspeed` (PHYS-9.2).
const MAX_SPEED: f32 = 320.0;
/// `sv_accelerate` (PHYS-9.4).
const ACCELERATE: f32 = 10.0;
/// The air speed an airborne player may add toward, `wishspd > 30` (PHYS-9.4).
const AIR_SPEED: f32 = 30.0;
/// `cl_rollangle` (PHYS-9.1).
const ROLL_ANGLE: f32 = 2.0;
/// `cl_rollspeed` (PHYS-9.1).
const ROLL_SPEED: f32 = 200.0;
/// `STEPSIZE` in `SV_WalkMove` (PHYS-8.3).
const STEPSIZE: f32 = 18.0;
/// `FL_WATERJUMP` (PHYS-8.2).
const FL_WATERJUMP: i32 = 2048;
/// How far `SV_UserFriction` looks ahead for an edge (PHYS-9.3).
const EDGE_AHEAD: f32 = 16.0;
/// How far below the feet the edge is looked for (PHYS-9.3).
const EDGE_DEPTH: f32 = 34.0;
/// A swimmer with no input sinks at this (PHYS-9.5).
const SINK: f32 = 60.0;
/// Swimming is this much of the wished speed (PHYS-9.5).
const SWIM_SCALE: f32 = 0.7;
/// A punch fades at 10 degrees a second (PHYS-9.1).
const PUNCH_FADE: f32 = 10.0;

/// What a client asks for in a frame, `usercmd_t` and the move message (PHYS-9).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct UserCmd {
    /// The view's pitch, yaw and roll, which become `v_angle`.
    pub angles: [f32; 3],
    /// Units a second forward, negative back: `cl_forwardspeed` 200 a key.
    pub forward: f32,
    /// Units a second right, negative left: `cl_sidespeed` 350.
    pub side: f32,
    /// Units a second up, for swimming and flying: `cl_upspeed` 200.
    pub up: f32,
    /// Bit 0 attack, bit 1 jump.
    pub buttons: u8,
    /// An impulse, 0 for none.
    pub impulse: u8,
}

/// An angle as `MSG_WriteAngle` sends it and `MSG_ReadAngle` reads it:
/// `((int)f*256/360) & 255` as a signed byte times `360.0/256` (PHYS-9.6).
pub(crate) fn wire_angle(f: f32) -> f32 {
    let step = float_to_int(f).wrapping_mul(256).wrapping_div(360) & 255;
    let signed = i8::from_le_bytes([u8::try_from(step).unwrap_or(0)]);
    f64_to_f32_public(f64::from(signed) * (360.0 / 256.0))
}

/// A move as `MSG_WriteShort` sends it: the float to an int, kept to 16 bits (PHYS-9.6).
fn wire_short(f: f32) -> f32 {
    let bytes = float_to_int(f).to_le_bytes();
    f32::from(i16::from_le_bytes([bytes[0], bytes[1]]))
}

/// `AngleVectors`: forward, right and up for `angles` in degrees.
pub(crate) fn angle_vectors([pitch, yaw, roll]: [f32; 3]) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let radians = |a: f32| f64_to_f32_public(f64::from(a) * (core::f64::consts::PI * 2.0 / 360.0));
    let trig = |a: f32| {
        let a = f64::from(radians(a));
        (f64_to_f32_public(a.sin()), f64_to_f32_public(a.cos()))
    };
    let (sy, cy) = trig(yaw);
    let (sp, cp) = trig(pitch);
    let (sr, cr) = trig(roll);
    let forward = [cp * cy, cp * sy, -sp];
    let right = [
        -sr * sp * cy + -cr * -sy,
        -sr * sp * sy + -cr * cy,
        -sr * cp,
    ];
    let up = [cr * sp * cy + -sr * -sy, cr * sp * sy + -sr * cy, cr * cp];
    (forward, right, up)
}

fn dot([a, b, c]: [f32; 3], [x, y, z]: [f32; 3]) -> f32 {
    a * x + b * y + c * z
}

/// `Length`.
fn length(v: [f32; 3]) -> f32 {
    f64_to_f32_public(f64::from(dot(v, v)).sqrt())
}

/// `VectorNormalize`: the vector made one long, when it is not 0, and its length.
fn normalize(v: [f32; 3]) -> ([f32; 3], f32) {
    let len = length(v);
    if len == 0.0 {
        return (v, len);
    }
    let inverse = 1.0 / len;
    (v.map(|c| c * inverse), len)
}

/// `V_CalcRoll`: the roll a sideways velocity leans the view by.
fn calc_roll(angles: [f32; 3], velocity: [f32; 3]) -> f32 {
    let (_, right, _) = angle_vectors(angles);
    let side = dot(velocity, right);
    let sign = if side < 0.0 { -1.0 } else { 1.0 };
    let side = side.abs();
    let lean = if side < ROLL_SPEED {
        side * ROLL_ANGLE / ROLL_SPEED
    } else {
        ROLL_ANGLE
    };
    lean * sign
}

impl SimCall<'_, '_, '_> {
    /// `SV_ReadClientMove`: the command as it arrives, its angles through `MSG_WriteAngle`
    /// and its moves through `MSG_WriteShort`, then `v_angle`, the buttons and the impulse
    /// (PHYS-9.6).
    pub(crate) fn read_move(&mut self, ent: usize, cmd: &UserCmd) -> UserCmd {
        let cmd = UserCmd {
            angles: cmd.angles.map(wire_angle),
            forward: wire_short(cmd.forward),
            side: wire_short(cmd.side),
            up: wire_short(cmd.up),
            ..*cmd
        };
        self.set_vector(ent, "v_angle", cmd.angles);
        self.set_float(ent, "button0", f32::from(cmd.buttons & 1));
        self.set_float(ent, "button2", f32::from((cmd.buttons & 2) >> 1));
        if cmd.impulse != 0 {
            self.set_float(ent, "impulse", f32::from(cmd.impulse));
        }
        cmd
    }

    /// `SV_ClientThink` (PHYS-9.1, PHYS-9.2).
    pub(crate) fn client_think(
        &mut self,
        ent: usize,
        cmd: &UserCmd,
        frametime: f32,
    ) -> Result<(), RunError> {
        if self.movetype(ent) == MoveType::None {
            return Ok(());
        }
        let onground = self.flags(ent) & FL_ONGROUND != 0;
        // DropPunchAngle
        let (punch, len) = normalize(self.vector(ent, "punchangle"));
        let len = (len - PUNCH_FADE * frametime).max(0.0);
        self.set_vector(ent, "punchangle", punch.map(|c| c * len));
        if self.float(ent, "health") <= 0.0 {
            return Ok(());
        }
        let v_angle = add(self.vector(ent, "v_angle"), self.vector(ent, "punchangle"));
        let mut angles = self.vector(ent, "angles");
        angles[2] = calc_roll(angles, self.vector(ent, "velocity")) * 4.0;
        if self.float(ent, "fixangle") == 0.0 {
            angles[0] = -v_angle[0] / 3.0;
            angles[1] = v_angle[1];
        }
        self.set_vector(ent, "angles", angles);
        if self.flags(ent) & FL_WATERJUMP != 0 {
            self.water_jump(ent);
            return Ok(());
        }
        if self.float(ent, "waterlevel") >= 2.0 && self.movetype(ent) != MoveType::Noclip {
            self.water_move(ent, cmd, frametime);
            return Ok(());
        }
        self.air_move(ent, cmd, onground, frametime)
    }

    /// `SV_WaterJump` (PHYS-9.5).
    fn water_jump(&mut self, ent: usize) {
        let teleport = self.float(ent, "teleport_time");
        if self.clock() > f64::from(teleport) || self.float(ent, "waterlevel") == 0.0 {
            let flags = self.flags(ent) & !FL_WATERJUMP;
            self.set_flags(ent, flags);
            self.set_float(ent, "teleport_time", 0.0);
        }
        let movedir = self.vector(ent, "movedir");
        let [_, _, z] = self.vector(ent, "velocity");
        self.set_vector(ent, "velocity", [movedir[0], movedir[1], z]);
    }

    /// `SV_AirMove` (PHYS-9.2).
    fn air_move(
        &mut self,
        ent: usize,
        cmd: &UserCmd,
        onground: bool,
        frametime: f32,
    ) -> Result<(), RunError> {
        let (forward, right, _) = angle_vectors(self.vector(ent, "angles"));
        let mut fmove = cmd.forward;
        let smove = cmd.side;
        if self.clock() < f64::from(self.float(ent, "teleport_time")) && fmove < 0.0 {
            fmove = 0.0;
        }
        let movetype = self.movetype(ent);
        let mut wishvel = [0.0; 3];
        for ((w, f), r) in wishvel.iter_mut().zip(forward).zip(right) {
            *w = f * fmove + r * smove;
        }
        wishvel[2] = if movetype == MoveType::Walk {
            0.0
        } else {
            cmd.up
        };
        let (wishdir, mut wishspeed) = normalize(wishvel);
        if wishspeed > MAX_SPEED {
            wishvel = wishvel.map(|c| c * (MAX_SPEED / wishspeed));
            wishspeed = MAX_SPEED;
        }
        if movetype == MoveType::Noclip {
            self.set_vector(ent, "velocity", wishvel);
        } else if onground {
            self.user_friction(ent, frametime)?;
            self.accelerate(ent, wishdir, wishspeed, frametime);
        } else {
            self.air_accelerate(ent, wishvel, wishspeed, frametime);
        }
        Ok(())
    }

    /// `SV_UserFriction` (PHYS-9.3).
    fn user_friction(&mut self, ent: usize, frametime: f32) -> Result<(), RunError> {
        let vel = self.vector(ent, "velocity");
        let speed = f64_to_f32_public(f64::from(vel[0] * vel[0] + vel[1] * vel[1]).sqrt());
        if speed == 0.0 {
            return Ok(());
        }
        let origin = self.vector(ent, "origin");
        let x = origin[0] + vel[0] / speed * EDGE_AHEAD;
        let y = origin[1] + vel[1] / speed * EDGE_AHEAD;
        let z = origin[2] + self.vector(ent, "mins")[2];
        let trace = self.trace(
            ([x, y, z], [0.0; 3], [0.0; 3], [x, y, z - EDGE_DEPTH]),
            MoveKind::NoMonsters,
            ent,
        )?;
        #[expect(
            clippy::float_cmp,
            reason = "spec PHYS-9.3: id tests trace.fraction == 1.0"
        )]
        let edge = trace.fraction == 1.0;
        let friction = if edge {
            FRICTION * EDGE_FRICTION
        } else {
            FRICTION
        };
        let control = if speed < STOP_SPEED {
            STOP_SPEED
        } else {
            speed
        };
        let newspeed = (speed - frametime * control * friction).max(0.0) / speed;
        self.set_vector(ent, "velocity", vel.map(|c| c * newspeed));
        Ok(())
    }

    /// `SV_Accelerate` (PHYS-9.4).
    fn accelerate(&mut self, ent: usize, wishdir: [f32; 3], wishspeed: f32, frametime: f32) {
        let velocity = self.vector(ent, "velocity");
        let addspeed = wishspeed - dot(velocity, wishdir);
        if addspeed <= 0.0 {
            return;
        }
        let accelspeed = (ACCELERATE * frametime * wishspeed).min(addspeed);
        self.set_vector(
            ent,
            "velocity",
            add(velocity, wishdir.map(|c| c * accelspeed)),
        );
    }

    /// `SV_AirAccelerate` (PHYS-9.4).
    fn air_accelerate(&mut self, ent: usize, wishvel: [f32; 3], wishspeed: f32, frametime: f32) {
        let velocity = self.vector(ent, "velocity");
        let (wishveloc, wishspd) = normalize(wishvel);
        let wishspd = wishspd.min(AIR_SPEED);
        let addspeed = wishspd - dot(velocity, wishveloc);
        if addspeed <= 0.0 {
            return;
        }
        let accelspeed = (ACCELERATE * wishspeed * frametime).min(addspeed);
        self.set_vector(
            ent,
            "velocity",
            add(velocity, wishveloc.map(|c| c * accelspeed)),
        );
    }

    /// `SV_WaterMove` (PHYS-9.5).
    fn water_move(&mut self, ent: usize, cmd: &UserCmd, frametime: f32) {
        let (forward, right, _) = angle_vectors(self.vector(ent, "v_angle"));
        let mut wishvel = [0.0; 3];
        for ((w, f), r) in wishvel.iter_mut().zip(forward).zip(right) {
            *w = f * cmd.forward + r * cmd.side;
        }
        if cmd.forward == 0.0 && cmd.side == 0.0 && cmd.up == 0.0 {
            // "drift towards bottom"
            wishvel[2] -= SINK;
        } else {
            wishvel[2] += cmd.up;
        }
        let mut wishspeed = length(wishvel);
        if wishspeed > MAX_SPEED {
            wishvel = wishvel.map(|c| c * (MAX_SPEED / wishspeed));
            wishspeed = MAX_SPEED;
        }
        wishspeed *= SWIM_SCALE;
        let velocity = self.vector(ent, "velocity");
        let speed = length(velocity);
        let newspeed = if speed == 0.0 {
            0.0
        } else {
            let newspeed = (speed - frametime * speed * FRICTION).max(0.0);
            self.set_vector(ent, "velocity", velocity.map(|c| c * (newspeed / speed)));
            newspeed
        };
        if wishspeed == 0.0 {
            return;
        }
        let addspeed = wishspeed - newspeed;
        if addspeed <= 0.0 {
            return;
        }
        let (wishvel, _) = normalize(wishvel);
        let accelspeed = (ACCELERATE * wishspeed * frametime).min(addspeed);
        let velocity = self.vector(ent, "velocity");
        self.set_vector(
            ent,
            "velocity",
            add(velocity, wishvel.map(|c| c * accelspeed)),
        );
    }

    /// `SV_Physics_Client` for a connected client (PHYS-8.1, PHYS-8.2).
    pub(crate) fn physics_client(&mut self, ent: usize, frametime: f64) -> Result<(), RunError> {
        let value = self.value_of(ent);
        self.set_time(self.clock());
        self.set_global("self", Slot::from_i32(value));
        let pre = self.global("PlayerPreThink").as_i32();
        self.run(pre)?;
        self.check_velocity(ent);
        let movetype = float_to_int(self.float(ent, "movetype"));
        let t = f64_to_f32_public(frametime);
        match movetype {
            0 => {
                if !self.run_think(ent, frametime)? {
                    return Ok(());
                }
            }
            3 => {
                if !self.run_think(ent, frametime)? {
                    return Ok(());
                }
                if !self.check_water(ent)? && self.flags(ent) & FL_WATERJUMP == 0 {
                    self.add_gravity(ent, frametime);
                }
                self.check_stuck(ent)?;
                self.walk_move(ent, t)?;
            }
            6 | 10 => self.physics_toss(ent, frametime)?,
            5 => {
                if !self.run_think(ent, frametime)? {
                    return Ok(());
                }
                self.fly_move(ent, t, &mut None)?;
            }
            8 => {
                if !self.run_think(ent, frametime)? {
                    return Ok(());
                }
                let velocity = self.vector(ent, "velocity");
                let origin = add(self.vector(ent, "origin"), velocity.map(|c| c * t));
                self.set_vector(ent, "origin", origin);
            }
            other => {
                return Err(self.context.run_error(ErrorKind::Fatal {
                    message: format!("SV_Physics_client: bad movetype {other}"),
                }));
            }
        }
        self.link(ent, true)?;
        self.set_time(self.clock());
        self.set_global("self", Slot::from_i32(value));
        let post = self.global("PlayerPostThink").as_i32();
        self.run(post)
    }

    /// `SV_CheckWater`: whether the player is in liquid to the waist (PHYS-8.4).
    fn check_water(&mut self, ent: usize) -> Result<bool, RunError> {
        let [x, y, z] = self.vector(ent, "origin");
        let mins = self.vector(ent, "mins");
        let maxs = self.vector(ent, "maxs");
        self.set_float(ent, "waterlevel", 0.0);
        self.set_float(ent, "watertype", -1.0);
        let contents = |call: &Self, h: f32| {
            call.sim
                .collision
                .point_contents([x, y, z + h])
                .map_err(|e| call.error(e))
        };
        let feet = contents(self, mins[2] + 1.0)?;
        if feet <= CONTENTS_WATER {
            self.set_float(
                ent,
                "watertype",
                f32::from(i16::try_from(feet).unwrap_or(-3)),
            );
            self.set_float(ent, "waterlevel", 1.0);
            if contents(self, (mins[2] + maxs[2]) * 0.5)? <= CONTENTS_WATER {
                self.set_float(ent, "waterlevel", 2.0);
                let eyes = self.vector(ent, "view_ofs")[2];
                if contents(self, eyes)? <= CONTENTS_WATER {
                    self.set_float(ent, "waterlevel", 3.0);
                }
            }
        }
        Ok(self.float(ent, "waterlevel") > 1.0)
    }

    /// `SV_CheckStuck` (PHYS-8.7).
    fn check_stuck(&mut self, ent: usize) -> Result<(), RunError> {
        let stuck = |call: &Self| test_entity_position(call, ent).map_err(|e| call.error(e));
        if !stuck(self)? {
            let origin = self.vector(ent, "origin");
            self.set_vector(ent, "oldorigin", origin);
            return Ok(());
        }
        let org = self.vector(ent, "origin");
        let old = self.vector(ent, "oldorigin");
        self.set_vector(ent, "origin", old);
        if !stuck(self)? {
            // "Unstuck."
            return self.link(ent, true);
        }
        for z in 0..18_u8 {
            for i in [-1.0, 0.0, 1.0] {
                for j in [-1.0, 0.0, 1.0] {
                    self.set_vector(
                        ent,
                        "origin",
                        [org[0] + i, org[1] + j, org[2] + f32::from(z)],
                    );
                    if !stuck(self)? {
                        return self.link(ent, true);
                    }
                }
            }
        }
        // "player is stuck."
        self.set_vector(ent, "origin", org);
        Ok(())
    }

    /// `SV_WallFriction` (PHYS-8.5).
    fn wall_friction(&mut self, ent: usize, trace: &Trace) {
        let (forward, _, _) = angle_vectors(self.vector(ent, "v_angle"));
        let d = dot(trace.plane.normal, forward) + 0.5;
        if d >= 0.0 {
            return;
        }
        let velocity = self.vector(ent, "velocity");
        let i = dot(trace.plane.normal, velocity);
        let into = trace.plane.normal.map(|c| c * i);
        let side = [velocity[0] - into[0], velocity[1] - into[1]];
        self.set_vector(
            ent,
            "velocity",
            [side[0] * (1.0 + d), side[1] * (1.0 + d), velocity[2]],
        );
    }

    /// `SV_TryUnstick` (PHYS-8.6).
    fn try_unstick(&mut self, ent: usize, oldvel: [f32; 3]) -> Result<i32, RunError> {
        let oldorg = self.vector(ent, "origin");
        let pushes = [
            [2.0, 0.0],
            [0.0, 2.0],
            [-2.0, 0.0],
            [0.0, -2.0],
            [2.0, 2.0],
            [-2.0, 2.0],
            [2.0, -2.0],
            [-2.0, -2.0],
        ];
        for [x, y] in pushes {
            self.push_entity(ent, [x, y, 0.0])?;
            self.set_vector(ent, "velocity", [oldvel[0], oldvel[1], 0.0]);
            let mut steptrace = None;
            let clip = self.fly_move(ent, 0.1, &mut steptrace)?;
            let origin = self.vector(ent, "origin");
            if (oldorg[1] - origin[1]).abs() > 4.0 || (oldorg[0] - origin[0]).abs() > 4.0 {
                return Ok(clip);
            }
            self.set_vector(ent, "origin", oldorg);
        }
        self.set_vector(ent, "velocity", [0.0; 3]);
        // "still not moving"
        Ok(7)
    }

    /// `SV_WalkMove` (PHYS-8.3).
    fn walk_move(&mut self, ent: usize, frametime: f32) -> Result<(), RunError> {
        let oldonground = self.flags(ent) & FL_ONGROUND;
        let flags = self.flags(ent) & !FL_ONGROUND;
        self.set_flags(ent, flags);
        let oldorg = self.vector(ent, "origin");
        let oldvel = self.vector(ent, "velocity");
        let mut steptrace = None;
        let clip = self.fly_move(ent, frametime, &mut steptrace)?;
        // "move didn't block on a step"
        if clip & 2 == 0 {
            return Ok(());
        }
        // "don't stair up while jumping"
        if oldonground == 0 && self.float(ent, "waterlevel") == 0.0 {
            return Ok(());
        }
        // "gibbed by a trigger"
        if self.movetype(ent) != MoveType::Walk {
            return Ok(());
        }
        if self.flags(ent) & FL_WATERJUMP != 0 {
            return Ok(());
        }
        let nosteporg = self.vector(ent, "origin");
        let nostepvel = self.vector(ent, "velocity");
        // "try moving up and forward to go up a step"
        self.set_vector(ent, "origin", oldorg);
        let upmove = [0.0, 0.0, STEPSIZE];
        let downmove = [0.0, 0.0, -STEPSIZE + oldvel[2] * frametime];
        self.push_entity(ent, upmove)?;
        self.set_vector(ent, "velocity", [oldvel[0], oldvel[1], 0.0]);
        let mut clip = self.fly_move(ent, frametime, &mut steptrace)?;
        if clip != 0 {
            let origin = self.vector(ent, "origin");
            if (oldorg[1] - origin[1]).abs() < 0.031_25 && (oldorg[0] - origin[0]).abs() < 0.031_25
            {
                // "stepping up didn't make any progress"
                clip = self.try_unstick(ent, oldvel)?;
            }
        }
        if clip & 2 != 0
            && let Some(trace) = steptrace
        {
            self.wall_friction(ent, &trace);
        }
        let downtrace = self.push_entity(ent, downmove)?;
        if downtrace.plane.normal[2] > 0.7 {
            // id tests the player's own solid here, which is never SOLID_BSP.
            if self.solid(ent) == Solid::Bsp {
                let flags = self.flags(ent) | FL_ONGROUND;
                self.set_flags(ent, flags);
                self.set_entity(ent, "groundentity", downtrace.ent.unwrap_or(0));
            }
        } else {
            // "use the move without the step up"
            self.set_vector(ent, "origin", nosteporg);
            self.set_vector(ent, "velocity", nostepvel);
        }
        Ok(())
    }
}

fn add([a, b, c]: [f32; 3], [x, y, z]: [f32; 3]) -> [f32; 3] {
    [a + x, b + y, c + z]
}

#[cfg(test)]
mod tests {
    use super::{angle_vectors, calc_roll, wire_angle, wire_short};

    fn near(got: [f32; 3], want: [f32; 3]) -> bool {
        got.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-5)
    }

    // spec PHYS-9.2: forward and right come from the angles as AngleVectors gives them
    #[test]
    fn angle_vectors_of_pitch_yaw_and_roll() {
        let (forward, right, up) = angle_vectors([30.0, 45.0, 60.0]);
        assert!(
            near(forward, [0.612_372_4, 0.612_372_4, -0.5]),
            "{forward:?}"
        );
        assert!(
            near(right, [0.047_367_17, -0.659_739_6, -0.75]),
            "{right:?}"
        );
        assert!(near(up, [0.789_149_1, -0.435_595_7, 0.433_012_7]), "{up:?}");
    }

    // spec PHYS-9.6: angles go as (int)f*256/360 in a byte, read back signed; moves as the
    // int kept to 16 bits
    #[test]
    fn the_wire_quantises_the_command() {
        assert_eq!(wire_angle(90.0), 90.0);
        assert_eq!(wire_angle(100.7), 99.843_75);
        assert_eq!(wire_angle(-10.0), -9.843_75);
        assert_eq!(wire_angle(180.0), -180.0);
        assert_eq!(wire_angle(720.0), 0.0);
        assert_eq!(wire_short(200.9), 200.0);
        assert_eq!(wire_short(-200.9), -200.0);
        assert_eq!(wire_short(70_000.0), 4464.0);
        assert_eq!(wire_short(-70_000.0), -4464.0);
    }

    // spec PHYS-9.1: the roll is the sideways speed times 2/200 below 200 and 2 from there,
    // its sign kept
    #[test]
    fn the_roll_leans_with_the_sideways_speed() {
        assert_eq!(calc_roll([0.0; 3], [0.0, -100.0, 0.0]), 1.0);
        assert_eq!(calc_roll([0.0; 3], [0.0, 100.0, 0.0]), -1.0);
        assert_eq!(calc_roll([0.0; 3], [0.0, -200.0, 0.0]), 2.0);
        assert_eq!(calc_roll([0.0; 3], [0.0, -900.0, 0.0]), 2.0);
        assert_eq!(calc_roll([0.0; 3], [500.0, 0.0, 0.0]), 0.0);
    }
}
