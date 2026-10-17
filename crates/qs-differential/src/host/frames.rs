// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/host.c, Host_ServerFrame
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_phys.c, SV_Physics, SV_Physics_Client, SV_Physics_Pusher, SV_PushMove, SV_RunThink

//! The server frame and the physics frame (DIFF-5), without the movement (DIFF-5.8).
//!
//! A server frame (DIFF-5.1) sets `frametime`, applies the scenario's input (DIFF-4.3), runs a
//! physics frame (DIFF-5.3) and clears the muzzle-flash bit (DIFF-5.1). A physics frame runs
//! `StartFrame`, then gives each edict its turn: a client by DIFF-5.4, a pusher by DIFF-5.5,
//! and every other thinking movetype by DIFF-5.6. Nothing moves or collides (DIFF-5.8).

use qc_progs::Progs;
use qc_types::Slot;
use qc_vm::{ErrorKind, RunError};

use crate::scenario::Input;

use super::{
    EF_MUZZLEFLASH, Host, MOVETYPE_BOUNCE, MOVETYPE_FLY, MOVETYPE_FLYMISSILE, MOVETYPE_NOCLIP,
    MOVETYPE_NONE, MOVETYPE_PUSH, MOVETYPE_STEP, MOVETYPE_TOSS, MOVETYPE_WALK, ftoi, slot_of_field,
};

/// The fatal error the physics frame raises on a bad movetype (DIFF-5.3, DIFF-5.4).
///
/// id's `Sys_Error`, raised by the host outside any run, so its error record has function 0
/// and statement -1 (DIFF-8.1); the trace records it here. The returned [`RunError`] carries
/// the same message so the host's error path ends the scenario; a fatal error runs no shutdown
/// (DIFF-8.6), but for the host loop of P1-61 a bad movetype only appears in error tests, and
/// the trace record is already written.
fn bad_movetype(host: &mut Host<'_>, which: &str, movetype: i32) -> RunError {
    let message = format!("SV_Physics{which}: bad movetype {movetype}");
    host.trace.host_error(0, -1, &message);
    RunError {
        kind: ErrorKind::Builtin { message },
        function: 0,
        statement: -1,
        trace: Vec::new(),
    }
}

impl Host<'_> {
    /// DIFF-5.1: one server frame of length `t`, applying the input of frame `frame`.
    pub(crate) fn server_frame(
        &mut self,
        progs: &Progs<'_>,
        t: f64,
        frame: u32,
    ) -> Result<(), RunError> {
        // DIFF-5.1: set frametime to (f32)t.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec DIFF-5.1: frametime is the f32 nearest the frame length"
        )]
        let frametime = t as f32;
        self.set_global("frametime", Slot::from_f32(frametime));

        // DIFF-5.1: empty the unreliable datagram, apply the frame's input, run the physics
        // frame, empty the reliable datagram, clear the muzzle flashes.
        self.builtins.engine_mut().buffers.unreliable = 0;
        self.apply_inputs(frame);
        self.physics(progs, t)?;
        self.builtins.engine_mut().buffers.reliable = 0;
        self.clear_muzzle_flashes();
        Ok(())
    }

    /// DIFF-4.3: apply the scenario's input lines for frame `frame`, in file order.
    fn apply_inputs(&mut self, frame: u32) {
        let inputs = self.scenario.inputs.clone();
        for input in &inputs {
            if input.frame() != frame {
                continue;
            }
            let edict = usize::try_from(input.edict()).unwrap_or(0);
            match *input {
                Input::Button0 { value, .. } => {
                    self.set_field(edict, slot_of_field("button0"), Slot::from_f32(value));
                }
                Input::Button2 { value, .. } => {
                    self.set_field(edict, slot_of_field("button2"), Slot::from_f32(value));
                }
                Input::Impulse { value, .. } => {
                    self.set_field(edict, slot_of_field("impulse"), Slot::from_f32(value));
                }
                Input::VAngle { values, .. } => {
                    let base = slot_of_field("v_angle");
                    self.set_field(edict, base, Slot::from_f32(values[0]));
                    self.set_field(edict, base.saturating_add(1), Slot::from_f32(values[1]));
                    self.set_field(edict, base.saturating_add(2), Slot::from_f32(values[2]));
                }
            }
        }
    }

    /// DIFF-5.3: a physics frame of length `t`, id's `SV_Physics` without the movement.
    pub(crate) fn physics(&mut self, progs: &Progs<'_>, t: f64) -> Result<(), RunError> {
        // DIFF-5.3: self and other to the world, time to (f32)clock, run StartFrame.
        self.set_self(0);
        self.set_other(0);
        let clock = self.builtins.engine().clock;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec DIFF-5.3: time is the f32 nearest the clock"
        )]
        let time = clock as f32;
        self.set_global("time", Slot::from_f32(time));
        self.run_named(progs, "StartFrame")?;

        // DIFF-5.3: for i from 0 while i < num_edicts, reading num_edicts again each time.
        let mut i = 0;
        while i < self.machine.edicts().num_edicts() {
            // skip a free edict
            if self.machine.edicts().is_free(i) != Some(false) {
                i = i.saturating_add(1);
                continue;
            }
            // DIFF-5.3: link if force_retouch is non-zero (the mock link writes absmin/absmax,
            // DIFF-5.7; with no movement it changes nothing here, so it is omitted as a no-op).
            let maxclients = self.machine.edicts().maxclients();
            let movetype = ftoi(self.field(i, slot_of_field("movetype")).as_f32());
            if i >= 1 && i <= maxclients {
                self.physics_client(progs, i)?;
            } else if movetype == MOVETYPE_PUSH {
                self.physics_pusher(progs, i, t)?;
            } else if matches!(
                movetype,
                MOVETYPE_NONE
                    | MOVETYPE_NOCLIP
                    | MOVETYPE_STEP
                    | MOVETYPE_TOSS
                    | MOVETYPE_BOUNCE
                    | MOVETYPE_FLY
                    | MOVETYPE_FLYMISSILE
            ) {
                self.run_think(progs, i, t)?;
            } else {
                return Err(bad_movetype(self, "", movetype));
            }
            i = i.saturating_add(1);
        }

        // DIFF-5.3: the clock advances by t, in double.
        self.builtins.engine_mut().clock += t;
        Ok(())
    }

    /// DIFF-5.4: a client edict's turn, id's `SV_Physics_Client` without the move.
    fn physics_client(&mut self, progs: &Progs<'_>, edict: usize) -> Result<(), RunError> {
        // The slot has a player only when it entered (DIFF-4.2); otherwise the turn is empty.
        let slot = edict.saturating_sub(1);
        if !self.builtins.engine().client_active(edict) {
            let _ = slot;
            return Ok(());
        }
        let clock = self.builtins.engine().clock;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec DIFF-5.4: time is the f32 nearest the clock"
        )]
        let time = clock as f32;
        self.set_global("time", Slot::from_f32(time));
        self.set_self(edict);
        self.run_named(progs, "PlayerPreThink")?;

        // DIFF-5.4: think by the movetype; 0/3/5/8 end the turn if the edict is then free,
        // 6/10 go on; any other value is the fatal SV_Physics_client error.
        let movetype = ftoi(self.field(edict, slot_of_field("movetype")).as_f32());
        match movetype {
            MOVETYPE_NONE | MOVETYPE_WALK | MOVETYPE_FLY | MOVETYPE_NOCLIP => {
                self.run_think(progs, edict, self.scenario.frametime)?;
                if self.machine.edicts().is_free(edict) == Some(true) {
                    return Ok(());
                }
            }
            MOVETYPE_TOSS | MOVETYPE_BOUNCE => {
                self.run_think(progs, edict, self.scenario.frametime)?;
            }
            other => return Err(bad_movetype(self, "_client", other)),
        }

        // DIFF-5.4: link the edict (a no-op with no movement), then PlayerPostThink.
        let clock = self.builtins.engine().clock;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec DIFF-5.4: time is the f32 nearest the clock"
        )]
        let time = clock as f32;
        self.set_global("time", Slot::from_f32(time));
        self.set_self(edict);
        self.run_named(progs, "PlayerPostThink")
    }

    /// DIFF-5.6: thinking, id's `SV_RunThink` (EDICT-10.3).
    fn run_think(&mut self, progs: &Progs<'_>, edict: usize, t: f64) -> Result<(), RunError> {
        let nextthink = self.field(edict, slot_of_field("nextthink")).as_f32();
        let clock = self.builtins.engine().clock;
        // DIFF-5.6: runs when nextthink > 0 and <= clock + t (in double).
        if !(nextthink > 0.0 && f64::from(nextthink) <= clock + t) {
            return Ok(());
        }
        // time = nextthink, or (f32)clock if nextthink is less than the clock.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "spec DIFF-5.6: (f32)clock when nextthink is before the clock"
        )]
        let clock_f32 = clock as f32;
        let time = if f64::from(nextthink) < clock {
            clock_f32
        } else {
            nextthink
        };
        self.set_global("time", Slot::from_f32(time));
        // nextthink = 0, self = the edict, other = the world, run think.
        self.set_field(edict, slot_of_field("nextthink"), Slot::from_f32(0.0));
        self.set_self(edict);
        self.set_other(0);
        let think = self.field(edict, slot_of_field("think")).as_i32();
        self.run_function(progs, think)
    }

    /// DIFF-5.5: a `MOVETYPE_PUSH` edict's turn, id's `SV_Physics_Pusher` with the zero-velocity
    /// branch of `SV_PushMove`; nothing moves.
    fn physics_pusher(&mut self, progs: &Progs<'_>, edict: usize, t: f64) -> Result<(), RunError> {
        let ltime = self.field(edict, slot_of_field("ltime")).as_f32();
        let thinktime = self.field(edict, slot_of_field("nextthink")).as_f32();
        let old_ltime = ltime;
        // movetime: if thinktime < ltime + t (in double), thinktime - ltime (0 if negative),
        // else (f32)t.
        let movetime = if f64::from(thinktime) < f64::from(ltime) + t {
            let diff = thinktime - ltime;
            if diff < 0.0 { 0.0 } else { diff }
        } else {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "spec DIFF-5.5: movetime is the f32 nearest the frame length"
            )]
            let whole = t as f32;
            whole
        };
        let mut new_ltime = ltime;
        if movetime != 0.0 {
            new_ltime = ltime + movetime;
            self.set_field(edict, slot_of_field("ltime"), Slot::from_f32(new_ltime));
        }
        // DIFF-5.5: if thinktime > old_ltime and <= ltime, think with time = (f32)clock,
        // self = the edict, other = the world.
        if thinktime > old_ltime && thinktime <= new_ltime {
            let clock = self.builtins.engine().clock;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "spec DIFF-5.5: time is the f32 nearest the clock"
            )]
            let time = clock as f32;
            self.set_global("time", Slot::from_f32(time));
            self.set_field(edict, slot_of_field("nextthink"), Slot::from_f32(0.0));
            self.set_self(edict);
            self.set_other(0);
            let think = self.field(edict, slot_of_field("think")).as_i32();
            self.run_function(progs, think)?;
        }
        Ok(())
    }

    /// DIFF-5.1: clear `EF_MUZZLEFLASH` in `effects` on edicts 1 to num_edicts - 1.
    fn clear_muzzle_flashes(&mut self) {
        let num = self.machine.edicts().num_edicts();
        let effects_slot = slot_of_field("effects");
        for edict in 1..num {
            let effects = ftoi(self.field(edict, effects_slot).as_f32());
            let cleared = effects & !EF_MUZZLEFLASH;
            if cleared != effects {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "spec DIFF-5.1: effects is converted to an integer and back (BUILTIN-2.2)"
                )]
                let value = cleared as f32;
                self.set_field(edict, effects_slot, Slot::from_f32(value));
            }
        }
    }
}
