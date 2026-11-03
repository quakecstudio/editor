// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/view.c, V_CalcRoll, V_CalcBob, V_StartPitchDrift, V_StopPitchDrift, V_DriftPitch, V_ParseDamage, V_BonusFlash_f, V_SetContentsColor, V_CalcPowerupCshift, V_CalcBlend, V_UpdatePalette, BuildGammaTable, angledelta, CalcGunAngle, V_BoundOffsets, V_AddIdle, V_CalcViewRoll, V_CalcIntermissionRefdef, V_CalcRefdef, V_RenderView

//! The view (`docs/spec/engine/view.md`, VIEW-1 to VIEW-4): the roll and bob of movement,
//! the pitch drift toward the server's ideal pitch, the eye and the weapon each frame, and
//! the colour shifts blended over the view.
//!
//! Implements VIEW-1 to VIEW-4.

use qc_builtins::math::{anglemod, makevectors, vector_normalize};
use qc_vm::convert::{double_to_float, double_to_int, float_to_int, int_to_float};

/// `cl_rollspeed` (VIEW-1.1).
pub const VIEW_ROLLSPEED: f32 = 200.0;
/// `cl_rollangle` (VIEW-1.1).
pub const VIEW_ROLLANGLE: f32 = 2.0;
/// `cl_bob` (VIEW-1.2).
pub const VIEW_BOB: f32 = 0.02;
/// `cl_bobcycle` (VIEW-1.2).
pub const VIEW_BOBCYCLE: f32 = 0.6;
/// `cl_bobup` (VIEW-1.2).
pub const VIEW_BOBUP: f32 = 0.5;
/// `v_centermove` (VIEW-2.2).
pub const VIEW_CENTERMOVE: f32 = 0.15;
/// `v_centerspeed` (VIEW-2.1).
pub const VIEW_CENTERSPEED: f32 = 500.0;
/// `cl_forwardspeed` (VIEW-2.2).
pub const VIEW_FORWARDSPEED: f32 = 200.0;
/// `v_kicktime` (VIEW-3.2).
pub const VIEW_KICKTIME: f32 = 0.5;
/// `v_kickroll` (VIEW-4.1).
pub const VIEW_KICKROLL: f32 = 0.6;
/// `v_kickpitch` (VIEW-4.1).
pub const VIEW_KICKPITCH: f32 = 0.6;
/// `v_idlescale` (VIEW-3.3).
pub const VIEW_IDLESCALE: f32 = 0.0;
/// `v_iyaw_cycle`, `v_iroll_cycle`, `v_ipitch_cycle` (VIEW-3.3).
pub const VIEW_IDLE_CYCLES: [f32; 3] = [1.0, 2.0, 0.5];
/// `v_ipitch_level`, `v_iyaw_level`, `v_iroll_level` (VIEW-3.3), in pitch, yaw, roll order.
pub const VIEW_IDLE_LEVELS: [f32; 3] = [0.3, 0.3, 0.1];
/// `gl_cshiftpercent` (VIEW-4.6).
pub const VIEW_CSHIFTPERCENT: f32 = 100.0;
/// `scr_viewsize`'s default, which lifts the weapon (VIEW-3.6).
pub const VIEW_VIEWSIZE: f32 = 100.0;

/// `CSHIFT_CONTENTS` to `CSHIFT_POWERUP` (VIEW-4).
pub const VIEW_NUM_CSHIFTS: usize = 4;
const CSHIFT_CONTENTS: usize = 0;
const CSHIFT_DAMAGE: usize = 1;
const CSHIFT_BONUS: usize = 2;
const CSHIFT_POWERUP: usize = 3;

const IT_INVISIBILITY: i32 = 524_288;
const IT_INVULNERABILITY: i32 = 1_048_576;
const IT_SUIT: i32 = 2_097_152;
const IT_QUAD: i32 = 4_194_304;

/// A colour shift, `cshift_t`: a colour and a percent (VIEW-4).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shift {
    /// `destcolor`.
    pub color: [i32; 3],
    /// `percent`, an `int` (VIEW-4.7).
    pub percent: i32,
}

impl Shift {
    const fn new(color: [i32; 3], percent: i32) -> Self {
        Self { color, percent }
    }
}

/// The contents the eye is in, for the contents shift (VIEW-4.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contents {
    /// `CONTENTS_EMPTY` or `CONTENTS_SOLID`.
    Empty,
    /// `CONTENTS_LAVA`.
    Lava,
    /// `CONTENTS_SLIME`.
    Slime,
    /// Any other: water.
    Water,
}

/// The view's settings, each a cvar with its default.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewSettings {
    /// `v_idlescale` (VIEW-3.3).
    pub idlescale: f32,
    /// `scr_ofsx`, `scr_ofsy`, `scr_ofsz` (VIEW-3.4).
    pub offsets: [f32; 3],
    /// `scr_viewsize` (VIEW-3.6).
    pub viewsize: f32,
    /// `gl_cshiftpercent` (VIEW-4.6).
    pub cshiftpercent: f32,
    /// `v_gamma` (VIEW-4.7).
    pub gamma: f32,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            idlescale: VIEW_IDLESCALE,
            offsets: [0.0; 3],
            viewsize: VIEW_VIEWSIZE,
            cshiftpercent: VIEW_CSHIFTPERCENT,
            gamma: 1.0,
        }
    }
}

/// The view state the client keeps between frames (VIEW-2, VIEW-3, VIEW-4).
#[derive(Clone, Debug, PartialEq)]
pub struct ViewState {
    /// `cl.cshifts`.
    pub shifts: [Shift; VIEW_NUM_CSHIFTS],
    /// `v_dmg_time`, `v_dmg_roll`, `v_dmg_pitch` (VIEW-3.2).
    pub dmg_time: f32,
    dmg_roll: f32,
    dmg_pitch: f32,
    /// `cl.faceanimtime` (VIEW-4.1).
    pub faceanimtime: f32,
    /// `cl.nodrift`, `cl.pitchvel`, `cl.driftmove`, `cl.laststop` (VIEW-2).
    pub nodrift: bool,
    pitchvel: f32,
    driftmove: f32,
    laststop: f32,
    /// `CalcGunAngle`'s `oldyaw` and `oldpitch` (VIEW-3.5).
    gun_lag: (f32, f32),
    /// `V_CalcRefdef`'s `oldz` (VIEW-3.7).
    oldz: f32,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            shifts: [Shift::default(); VIEW_NUM_CSHIFTS],
            dmg_time: 0.0,
            dmg_roll: 0.0,
            dmg_pitch: 0.0,
            faceanimtime: 0.0,
            nodrift: true,
            pitchvel: 0.0,
            driftmove: 0.0,
            laststop: 0.0,
            gun_lag: (0.0, 0.0),
            oldz: 0.0,
        }
    }
}

/// What a frame's view is built from (VIEW-3.1): the view entity and the client's data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewInput {
    /// The client's clock, `cl.time`.
    pub time: f32,
    /// `cl.time - cl.oldtime`.
    pub frametime: f32,
    /// The view entity's origin.
    pub origin: [f32; 3],
    /// The view entity's angles, which VIEW-3.1 turns to the view.
    pub entity_angles: [f32; 3],
    /// `cl.viewangles`.
    pub viewangles: [f32; 3],
    /// `cl.velocity` (CLIENT-5.3).
    pub velocity: [f32; 3],
    /// `cl.viewheight`.
    pub viewheight: f32,
    /// `cl.punchangle`.
    pub punchangle: [f32; 3],
    /// `cl.onground`.
    pub onground: bool,
    /// `STAT_HEALTH`.
    pub health: i32,
    /// `cl.intermission` is on.
    pub intermission: bool,
    /// The model index of `STAT_WEAPON` and `STAT_WEAPONFRAME`.
    pub weapon: (usize, i32),
}

/// Where the eye is and what the weapon shows (VIEW-3).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Refdef {
    /// `r_refdef.vieworg`.
    pub origin: [f32; 3],
    /// `r_refdef.viewangles`, pitch, yaw, roll.
    pub angles: [f32; 3],
    /// The view entity's angles as VIEW-3.1 sets them.
    pub entity_angles: [f32; 3],
    /// The weapon: its model index (0 for none), origin, angles and frame.
    pub weapon: Weapon,
}

/// `cl.viewent`: the weapon in the player's hands (VIEW-3.5, VIEW-3.6).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Weapon {
    /// Its model index, 0 for none.
    pub model: usize,
    /// Its origin.
    pub origin: [f32; 3],
    /// Its angles.
    pub angles: [f32; 3],
    /// Its frame.
    pub frame: i32,
}

/// `V_CalcRoll (angles, velocity)` (VIEW-1.1).
#[must_use]
pub fn calc_roll(angles: [f32; 3], velocity: [f32; 3]) -> f32 {
    let (_, right, _) = makevectors(angles);
    let side = velocity[0] * right[0] + velocity[1] * right[1] + velocity[2] * right[2];
    let sign = if side < 0.0 { -1.0 } else { 1.0 };
    let side = side.abs();
    let value = if side < VIEW_ROLLSPEED {
        side * VIEW_ROLLANGLE / VIEW_ROLLSPEED
    } else {
        VIEW_ROLLANGLE
    };
    value * sign
}

/// `V_CalcBob` (VIEW-1.2).
#[must_use]
pub fn calc_bob(time: f32, velocity: [f32; 3]) -> f32 {
    let whole = float_to_int(time / VIEW_BOBCYCLE);
    let mut cycle = time - int_to_float(whole) * VIEW_BOBCYCLE;
    cycle /= VIEW_BOBCYCLE;
    let pi = double_to_float(core::f64::consts::PI);
    cycle = if cycle < VIEW_BOBUP {
        pi * cycle / VIEW_BOBUP
    } else {
        pi + pi * (cycle - VIEW_BOBUP) / (1.0 - VIEW_BOBUP)
    };
    let [vx, vy, _] = velocity;
    let speed = double_to_float(f64::from(vx * vx + vy * vy).sqrt());
    let bob = speed * VIEW_BOB;
    let bob = bob * 0.3 + bob * 0.7 * double_to_float(f64::from(cycle).sin());
    bob.clamp(-7.0, 4.0)
}

/// `angledelta (a)`: `a` wrapped into -180 to 180 (VIEW-3.5).
#[must_use]
pub fn angledelta(a: f32) -> f32 {
    let a = anglemod(a);
    if a > 180.0 { a - 360.0 } else { a }
}

/// The gamma table (VIEW-4.7): `(int) (255 * pow ((i + 0.5) / 255.5, gamma) + 0.5)` clamped.
#[must_use]
pub fn gamma_table(gamma: f32) -> [u8; 256] {
    let mut table = [0_u8; 256];
    for (i, entry) in table.iter_mut().enumerate() {
        #[expect(
            clippy::float_cmp,
            reason = "spec VIEW-4.7: gamma 1 is the identity, tested exactly as id does"
        )]
        if gamma == 1.0 {
            *entry = u8::try_from(i).unwrap_or(u8::MAX);
            continue;
        }
        let base = (f64::from(u8::try_from(i).unwrap_or(u8::MAX)) + 0.5) / 255.5;
        let inf = float_to_int(double_to_float(255.0 * base.powf(f64::from(gamma)) + 0.5));
        *entry = u8::try_from(inf.clamp(0, 255)).unwrap_or(u8::MAX);
    }
    table
}

impl ViewState {
    /// `V_StartPitchDrift` (VIEW-2.1).
    pub fn start_pitch_drift(&mut self, time: f32) {
        #[expect(
            clippy::float_cmp,
            reason = "spec VIEW-2.1: nothing happens when laststop is exactly the current time"
        )]
        if self.laststop == time {
            return;
        }
        if self.nodrift || self.pitchvel == 0.0 {
            self.pitchvel = VIEW_CENTERSPEED;
            self.nodrift = false;
            self.driftmove = 0.0;
        }
    }

    /// `V_StopPitchDrift` (VIEW-2.1).
    pub fn stop_pitch_drift(&mut self, time: f32) {
        self.laststop = time;
        self.nodrift = true;
        self.pitchvel = 0.0;
    }

    /// `V_DriftPitch` (VIEW-2.2, VIEW-2.3): moves `viewangles[PITCH]` toward `idealpitch`;
    /// `forwardmove` is the command's, `onground` the client's.
    pub fn drift_pitch(
        &mut self,
        time: f32,
        frametime: f32,
        onground: bool,
        forwardmove: f32,
        idealpitch: f32,
        viewangles: &mut [f32; 3],
    ) {
        if !onground {
            self.driftmove = 0.0;
            self.pitchvel = 0.0;
            return;
        }
        if self.nodrift {
            if forwardmove.abs() < VIEW_FORWARDSPEED {
                self.driftmove = 0.0;
            } else {
                self.driftmove += frametime;
            }
            if self.driftmove > VIEW_CENTERMOVE {
                self.start_pitch_drift(time);
            }
            return;
        }
        let delta = idealpitch - viewangles[0];
        if delta == 0.0 {
            self.pitchvel = 0.0;
            return;
        }
        let mut mv = frametime * self.pitchvel;
        self.pitchvel += frametime * VIEW_CENTERSPEED;
        if delta > 0.0 {
            if mv > delta {
                self.pitchvel = 0.0;
                mv = delta;
            }
            viewangles[0] += mv;
        } else {
            if mv > -delta {
                self.pitchvel = 0.0;
                mv = -delta;
            }
            viewangles[0] -= mv;
        }
    }

    /// `V_ParseDamage`'s effect (VIEW-4.1): `armor` and `blood` bytes and the `from` point,
    /// against the view entity's origin and angles.
    pub fn damage(
        &mut self,
        time: f32,
        armor: i32,
        blood: i32,
        from: [f32; 3],
        origin: [f32; 3],
        angles: [f32; 3],
    ) {
        let count = int_to_float(blood) * 0.5 + int_to_float(armor) * 0.5;
        let count = count.max(10.0);
        self.faceanimtime = time + 0.2;
        let damage = &mut self.shifts[CSHIFT_DAMAGE];
        // `percent += 3*count`: the int plus the float, truncated back into the int (VIEW-4.7).
        damage.percent = float_to_int(int_to_float(damage.percent) + 3.0 * count).clamp(0, 150);
        damage.color = if armor > blood {
            [200, 100, 100]
        } else if armor != 0 {
            [220, 50, 50]
        } else {
            [255, 0, 0]
        };
        let [fx, fy, fz] = from;
        let [ox, oy, oz] = origin;
        let (dir, _) = vector_normalize([fx - ox, fy - oy, fz - oz]);
        let (forward, right, _) = makevectors(angles);
        let side = dir[0] * right[0] + dir[1] * right[1] + dir[2] * right[2];
        self.dmg_roll = count * side * VIEW_KICKROLL;
        let side = dir[0] * forward[0] + dir[1] * forward[1] + dir[2] * forward[2];
        self.dmg_pitch = count * side * VIEW_KICKPITCH;
        self.dmg_time = VIEW_KICKTIME;
    }

    /// `V_BonusFlash_f` (VIEW-4.2).
    pub fn bonus_flash(&mut self) {
        self.shifts[CSHIFT_BONUS] = Shift::new([215, 186, 69], 50);
    }

    /// `V_SetContentsColor` (VIEW-4.3).
    pub fn set_contents(&mut self, contents: Contents) {
        self.shifts[CSHIFT_CONTENTS] = match contents {
            Contents::Empty => Shift::new([130, 80, 50], 0),
            Contents::Lava => Shift::new([255, 80, 0], 150),
            Contents::Slime => Shift::new([0, 25, 5], 150),
            Contents::Water => Shift::new([130, 80, 50], 128),
        };
    }

    /// `V_CalcPowerupCshift` (VIEW-4.4).
    pub fn powerup_shift(&mut self, items: i32) {
        let shift = &mut self.shifts[CSHIFT_POWERUP];
        if items & IT_QUAD != 0 {
            *shift = Shift::new([0, 0, 255], 30);
        } else if items & IT_SUIT != 0 {
            *shift = Shift::new([0, 255, 0], 20);
        } else if items & IT_INVISIBILITY != 0 {
            *shift = Shift::new([100, 100, 100], 100);
        } else if items & IT_INVULNERABILITY != 0 {
            *shift = Shift::new([255, 255, 0], 30);
        } else {
            shift.percent = 0;
        }
    }

    /// `V_UpdatePalette`'s fades (VIEW-4.5): the damage and bonus percents lose their rates,
    /// each `percent -= host_frametime*rate` truncating the float result back into the int
    /// (VIEW-4.7).
    pub fn fade(&mut self, frametime: f32) {
        let fade = |percent: i32, rate: f64| {
            let faded = double_to_int(f64::from(percent) - f64::from(frametime) * rate);
            if faded <= 0 { 0 } else { faded }
        };
        let damage = &mut self.shifts[CSHIFT_DAMAGE];
        damage.percent = fade(damage.percent, 150.0);
        let bonus = &mut self.shifts[CSHIFT_BONUS];
        bonus.percent = fade(bonus.percent, 100.0);
    }

    /// `V_CalcBlend` (VIEW-4.6): the shifts folded into one RGBA, colours over 255.
    #[must_use]
    pub fn blend(&self, cshiftpercent: f32) -> [f32; 4] {
        let (mut r, mut g, mut b, mut a) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        for shift in &self.shifts {
            if cshiftpercent == 0.0 {
                continue;
            }
            let mut a2 = ((int_to_float(shift.percent) * cshiftpercent) / 100.0) / 255.0;
            if a2 == 0.0 {
                continue;
            }
            a += a2 * (1.0 - a);
            a2 /= a;
            let [cr, cg, cb] = shift.color.map(int_to_float);
            r = r * (1.0 - a2) + cr * a2;
            g = g * (1.0 - a2) + cg * a2;
            b = b * (1.0 - a2) + cb * a2;
        }
        [r / 255.0, g / 255.0, b / 255.0, a.clamp(0.0, 1.0)]
    }

    /// `V_RenderView` (VIEW-3): the eye and the weapon for a frame, either the intermission's
    /// (VIEW-3.8) or `V_CalcRefdef`'s (VIEW-3.1 to VIEW-3.7).
    pub fn render(&mut self, input: &ViewInput, settings: &ViewSettings) -> Refdef {
        if input.intermission {
            return self.intermission(input, settings);
        }
        self.calc_refdef(input, settings)
    }

    /// `V_CalcIntermissionRefdef` (VIEW-3.8).
    fn intermission(&self, input: &ViewInput, settings: &ViewSettings) -> Refdef {
        let mut angles = input.entity_angles;
        add_idle(&mut angles, input.time, 1.0);
        let _ = settings;
        Refdef {
            origin: input.origin,
            angles,
            entity_angles: input.entity_angles,
            weapon: Weapon::default(),
        }
    }

    /// `V_CalcRefdef` (VIEW-3.1 to VIEW-3.7).
    fn calc_refdef(&mut self, input: &ViewInput, settings: &ViewSettings) -> Refdef {
        let [pitch, yaw, _] = input.viewangles;
        let entity_angles = [-pitch, yaw, input.entity_angles[2]];
        let bob = calc_bob(input.time, input.velocity);
        let [ox, oy, oz] = input.origin;
        let nudge = 1.0 / 32.0;
        let mut vieworg = [ox + nudge, oy + nudge, oz + input.viewheight + bob + nudge];
        let mut viewangles = input.viewangles;
        // V_CalcViewRoll (VIEW-3.2).
        viewangles[2] += calc_roll(entity_angles, input.velocity);
        if self.dmg_time > 0.0 {
            viewangles[2] += self.dmg_time / VIEW_KICKTIME * self.dmg_roll;
            viewangles[0] += self.dmg_time / VIEW_KICKTIME * self.dmg_pitch;
            self.dmg_time -= input.frametime;
        }
        if input.health <= 0 {
            viewangles[2] = 80.0;
        }
        add_idle(&mut viewangles, input.time, settings.idlescale);
        // VIEW-3.4: the offsets along the entity's axes with its pitch negated.
        let angles = [-entity_angles[0], entity_angles[1], entity_angles[2]];
        let (forward, right, up) = makevectors(angles);
        let [sx, sy, sz] = settings.offsets;
        for i in 0..3 {
            if let (Some(v), Some(f), Some(r), Some(u)) =
                (vieworg.get_mut(i), forward.get(i), right.get(i), up.get(i))
            {
                *v += sx * f + sy * r + sz * u;
            }
        }
        // V_BoundOffsets.
        vieworg[0] = vieworg[0].clamp(ox - 14.0, ox + 14.0);
        vieworg[1] = vieworg[1].clamp(oy - 14.0, oy + 14.0);
        vieworg[2] = vieworg[2].clamp(oz - 22.0, oz + 30.0);
        // VIEW-3.5: the weapon's angles, from `r_refdef.viewangles` as the roll, the kick
        // and the idle have left them.
        let mut weapon_angles = input.viewangles;
        self.gun_angle(input, viewangles, &mut weapon_angles, settings.idlescale);
        // VIEW-3.6: the weapon's origin.
        let mut weapon_origin = [ox, oy, oz + input.viewheight];
        for (o, f) in weapon_origin.iter_mut().zip(forward) {
            *o += f * bob * 0.4;
        }
        weapon_origin[2] += bob;
        weapon_origin[2] += viewsize_lift(settings.viewsize);
        let (weapon_model, weapon_frame) = input.weapon;
        for (v, p) in viewangles.iter_mut().zip(input.punchangle) {
            *v += p;
        }
        // VIEW-3.7: smoothed stepping.
        if input.onground && oz - self.oldz > 0.0 {
            let steptime = input.frametime.max(0.0);
            self.oldz += steptime * 80.0;
            if self.oldz > oz {
                self.oldz = oz;
            }
            if oz - self.oldz > 12.0 {
                self.oldz = oz - 12.0;
            }
            vieworg[2] += self.oldz - oz;
            weapon_origin[2] += self.oldz - oz;
        } else {
            self.oldz = oz;
        }
        Refdef {
            origin: vieworg,
            angles: viewangles,
            entity_angles,
            weapon: Weapon {
                model: weapon_model,
                origin: weapon_origin,
                angles: weapon_angles,
                frame: weapon_frame,
            },
        }
    }

    /// `CalcGunAngle` (VIEW-3.5): the weapon's yaw and pitch from the view angles with the
    /// smoothed lag, less the idle terms.
    fn gun_angle(
        &mut self,
        input: &ViewInput,
        refdef_angles: [f32; 3],
        angles: &mut [f32; 3],
        idlescale: f32,
    ) {
        let [view_pitch, view_yaw, _] = refdef_angles;
        let yaw = view_yaw;
        let pitch = -view_pitch;
        let mut yaw = (angledelta(yaw - view_yaw) * 0.4).clamp(-10.0, 10.0);
        let mut pitch = (angledelta(-pitch - view_pitch) * 0.4).clamp(-10.0, 10.0);
        let mv = input.frametime * 20.0;
        let (oldyaw, oldpitch) = self.gun_lag;
        if yaw > oldyaw {
            if oldyaw + mv < yaw {
                yaw = oldyaw + mv;
            }
        } else if oldyaw - mv > yaw {
            yaw = oldyaw - mv;
        }
        if pitch > oldpitch {
            if oldpitch + mv < pitch {
                pitch = oldpitch + mv;
            }
        } else if oldpitch - mv > pitch {
            pitch = oldpitch - mv;
        }
        self.gun_lag = (yaw, pitch);
        angles[1] = view_yaw + yaw;
        angles[0] = -(view_pitch + pitch);
        let [cy, cr, cp] = VIEW_IDLE_CYCLES;
        let [lp, ly, lr] = VIEW_IDLE_LEVELS;
        let sin = |cycle: f32| double_to_float(f64::from(input.time * cycle).sin());
        angles[2] -= idlescale * sin(cr) * lr;
        angles[0] -= idlescale * sin(cp) * lp;
        angles[1] -= idlescale * sin(cy) * ly;
    }
}

/// `V_AddIdle` (VIEW-3.3): the idle sway added to roll, pitch and yaw.
fn add_idle(angles: &mut [f32; 3], time: f32, idlescale: f32) {
    let [cy, cr, cp] = VIEW_IDLE_CYCLES;
    let [lp, ly, lr] = VIEW_IDLE_LEVELS;
    let sin = |cycle: f32| double_to_float(f64::from(time * cycle).sin());
    angles[2] += idlescale * sin(cr) * lr;
    angles[0] += idlescale * sin(cp) * lp;
    angles[1] += idlescale * sin(cy) * ly;
}

/// How much `scr_viewsize` lifts the weapon (VIEW-3.6).
fn viewsize_lift(viewsize: f32) -> f32 {
    #[expect(
        clippy::float_cmp,
        reason = "spec VIEW-3.6: the lift is chosen by the view size's exact value"
    )]
    if viewsize == 110.0 {
        1.0
    } else if viewsize == 100.0 {
        2.0
    } else if viewsize == 90.0 {
        1.0
    } else if viewsize == 80.0 {
        0.5
    } else {
        0.0
    }
}
