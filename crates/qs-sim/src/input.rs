// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/cl_input.c, KeyDown, KeyUp, IN_MLookUp, IN_Impulse, CL_KeyState, CL_AdjustAngles, CL_BaseMove, CL_SendMove
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/in_win.c, IN_MouseMove

//! The client's buttons and the move they make (`docs/spec/engine/input.md` INPUT-2): the
//! `+` and `-` commands hold and release a button, and each frame the buttons, the mouse and
//! the settings make the command sent to the server.

use crate::client::UserCmd;

/// A button the `+` and `-` commands hold (INPUT-2.1), `cl_input.c`'s `kbutton_t`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// `in_mlook`, `+mlook`.
    MLook,
    /// `in_klook`, `+klook`.
    KLook,
    /// `in_left`, `+left`.
    Left,
    /// `in_right`, `+right`.
    Right,
    /// `in_forward`, `+forward`.
    Forward,
    /// `in_back`, `+back`.
    Back,
    /// `in_lookup`, `+lookup`.
    LookUp,
    /// `in_lookdown`, `+lookdown`.
    LookDown,
    /// `in_moveleft`, `+moveleft`.
    MoveLeft,
    /// `in_moveright`, `+moveright`.
    MoveRight,
    /// `in_strafe`, `+strafe`.
    Strafe,
    /// `in_speed`, `+speed`.
    Speed,
    /// `in_use`, `+use`.
    Use,
    /// `in_jump`, `+jump`.
    Jump,
    /// `in_attack`, `+attack`.
    Attack,
    /// `in_up`, `+moveup`.
    Up,
    /// `in_down`, `+movedown`.
    Down,
}

impl Button {
    /// Every button with the name its commands take after `+` and `-`, `CL_InitInput`'s.
    pub const ALL: [(&'static str, Self); 17] = [
        ("moveup", Self::Up),
        ("movedown", Self::Down),
        ("left", Self::Left),
        ("right", Self::Right),
        ("forward", Self::Forward),
        ("back", Self::Back),
        ("lookup", Self::LookUp),
        ("lookdown", Self::LookDown),
        ("strafe", Self::Strafe),
        ("moveleft", Self::MoveLeft),
        ("moveright", Self::MoveRight),
        ("speed", Self::Speed),
        ("attack", Self::Attack),
        ("use", Self::Use),
        ("jump", Self::Jump),
        ("klook", Self::KLook),
        ("mlook", Self::MLook),
    ];

    /// The button a command's name after its `+` or `-` names.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.iter().find(|(n, _)| *n == name).map(|(_, b)| *b)
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// `kbutton_t`: the two keys holding it and its state, bit 0 down, bit 1 pressed and bit 2
/// released since the last frame took them (INPUT-2.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct KButton {
    down: [i32; 2],
    state: u8,
}

/// What a frame's input asks of the view's pitch drift, `V_StartPitchDrift` and
/// `V_StopPitchDrift`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Drift {
    /// `V_StartPitchDrift`.
    pub start: bool,
    /// `V_StopPitchDrift`.
    pub stop: bool,
}

/// The settings the move reads, each a cvar of id's (CON-7.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoveSettings {
    /// `cl_upspeed`.
    pub upspeed: f32,
    /// `cl_forwardspeed`.
    pub forwardspeed: f32,
    /// `cl_backspeed`.
    pub backspeed: f32,
    /// `cl_sidespeed`.
    pub sidespeed: f32,
    /// `cl_movespeedkey`.
    pub movespeedkey: f32,
    /// `cl_yawspeed`.
    pub yawspeed: f32,
    /// `cl_pitchspeed`.
    pub pitchspeed: f32,
    /// `cl_anglespeedkey`.
    pub anglespeedkey: f32,
    /// `sensitivity`.
    pub sensitivity: f32,
    /// `m_pitch`.
    pub m_pitch: f32,
    /// `m_yaw`.
    pub m_yaw: f32,
    /// `m_forward`.
    pub m_forward: f32,
    /// `m_side`.
    pub m_side: f32,
    /// `lookspring`.
    pub lookspring: f32,
    /// `lookstrafe`.
    pub lookstrafe: f32,
}

impl MoveSettings {
    /// The settings, each read by its name from `setting`.
    #[must_use]
    pub fn read(setting: &dyn Fn(&str) -> f32) -> Self {
        Self {
            upspeed: setting("cl_upspeed"),
            forwardspeed: setting("cl_forwardspeed"),
            backspeed: setting("cl_backspeed"),
            sidespeed: setting("cl_sidespeed"),
            movespeedkey: setting("cl_movespeedkey"),
            yawspeed: setting("cl_yawspeed"),
            pitchspeed: setting("cl_pitchspeed"),
            anglespeedkey: setting("cl_anglespeedkey"),
            sensitivity: setting("sensitivity"),
            m_pitch: setting("m_pitch"),
            m_yaw: setting("m_yaw"),
            m_forward: setting("m_forward"),
            m_side: setting("m_side"),
            lookspring: setting("lookspring"),
            lookstrafe: setting("lookstrafe"),
        }
    }
}

/// The buttons and the impulse waiting to be sent (INPUT-2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Input {
    buttons: [KButton; 17],
    /// `in_impulse`.
    impulse: u8,
}

impl Input {
    /// No button held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether `button` is down, its state's bit 0.
    #[must_use]
    pub fn held(&self, button: Button) -> bool {
        self.state(button) & 1 != 0
    }

    fn state(&self, button: Button) -> u8 {
        self.buttons.get(button.index()).map_or(0, |b| b.state)
    }

    fn button_mut(&mut self, button: Button) -> Option<&mut KButton> {
        self.buttons.get_mut(button.index())
    }

    /// `KeyDown` (INPUT-2.1) for the key `key`, `None` when typed at the console; what it
    /// prints.
    pub fn press(&mut self, button: Button, key: Option<i32>) -> Option<&'static str> {
        let k = key.unwrap_or(-1);
        let b = self.button_mut(button)?;
        if k == b.down[0] || k == b.down[1] {
            return None;
        }
        if b.down[0] == 0 {
            b.down[0] = k;
        } else if b.down[1] == 0 {
            b.down[1] = k;
        } else {
            return Some("Three keys down for a button!\n");
        }
        if b.state & 1 == 0 {
            b.state |= 1 | 2;
        }
        None
    }

    /// `KeyUp` (INPUT-2.1) for the key `key`, `None` when typed at the console, which lets the
    /// button go whatever holds it; `IN_MLookUp` asks for the pitch drift after `-mlook` with
    /// `lookspring` set.
    pub fn release(&mut self, button: Button, key: Option<i32>, lookspring: f32) -> Drift {
        let Some(b) = self.button_mut(button) else {
            return Drift::default();
        };
        match key {
            None => {
                b.down = [0, 0];
                b.state = 4;
            }
            Some(k) => {
                if b.down[0] == k {
                    b.down[0] = 0;
                } else if b.down[1] == k {
                    b.down[1] = 0;
                } else {
                    return Drift::default();
                }
                if b.down != [0, 0] || b.state & 1 == 0 {
                    return Drift::default();
                }
                b.state &= !1;
                b.state |= 4;
            }
        }
        let up = b.state & 1 == 0;
        Drift {
            start: button == Button::MLook && up && lookspring != 0.0,
            stop: false,
        }
    }

    /// `IN_Impulse`: `impulse <n>`, `Q_atoi` of the word as a byte.
    pub const fn set_impulse(&mut self, impulse: u8) {
        self.impulse = impulse;
    }

    /// `CL_KeyState` (INPUT-2.2): 0.25 for a button pressed and released this frame, 0.5 for
    /// pressed and held, 0.75 for released and pressed again, 1 for held throughout, else 0;
    /// the edges are cleared.
    fn key_state(&mut self, button: Button) -> f32 {
        let Some(b) = self.button_mut(button) else {
            return 0.0;
        };
        let pressed = b.state & 2 != 0;
        let released = b.state & 4 != 0;
        let down = b.state & 1 != 0;
        let val = match (pressed, released, down) {
            (true, false, true) => 0.5,
            (false, false, true) => 1.0,
            (true, true, true) => 0.75,
            (true, true, false) => 0.25,
            (true | false, _, _) => 0.0,
        };
        b.state &= 1;
        val
    }

    /// `CL_AdjustAngles`, `CL_BaseMove`, `IN_MouseMove` and `CL_SendMove`'s buttons for a frame
    /// of `frametime` seconds (INPUT-2.2 to INPUT-2.5): `angles` turned by the keys and the
    /// mouse's `motion`, and the command made; what it asks of the pitch drift.
    pub fn frame(
        &mut self,
        settings: &MoveSettings,
        frametime: f32,
        angles: &mut [f32; 3],
        motion: [f32; 2],
    ) -> (UserCmd, Drift) {
        let mut drift = Drift::default();
        self.adjust_angles(settings, frametime, angles, &mut drift);
        let mut cmd = UserCmd::default();
        if self.held(Button::Strafe) {
            cmd.side += settings.sidespeed * self.key_state(Button::Right);
            cmd.side -= settings.sidespeed * self.key_state(Button::Left);
        }
        cmd.side += settings.sidespeed * self.key_state(Button::MoveRight);
        cmd.side -= settings.sidespeed * self.key_state(Button::MoveLeft);
        cmd.up += settings.upspeed * self.key_state(Button::Up);
        cmd.up -= settings.upspeed * self.key_state(Button::Down);
        if !self.held(Button::KLook) {
            cmd.forward += settings.forwardspeed * self.key_state(Button::Forward);
            cmd.forward -= settings.backspeed * self.key_state(Button::Back);
        }
        if self.held(Button::Speed) {
            cmd.forward *= settings.movespeedkey;
            cmd.side *= settings.movespeedkey;
            cmd.up *= settings.movespeedkey;
        }
        self.mouse_move(settings, motion, angles, &mut cmd, &mut drift);
        cmd.angles = *angles;
        // `CL_SendMove`: "`if ( in_attack.state & 3 ) bits |= 1;`", then the press is cleared.
        let mut bits = 0;
        for (button, bit) in [(Button::Attack, 1), (Button::Jump, 2)] {
            if self.state(button) & 3 != 0 {
                bits |= bit;
            }
            if let Some(b) = self.button_mut(button) {
                b.state &= !2;
            }
        }
        cmd.buttons = bits;
        cmd.impulse = core::mem::take(&mut self.impulse);
        (cmd, drift)
    }

    /// `CL_AdjustAngles` (INPUT-2.3).
    fn adjust_angles(
        &mut self,
        settings: &MoveSettings,
        frametime: f32,
        angles: &mut [f32; 3],
        drift: &mut Drift,
    ) {
        let speed = if self.held(Button::Speed) {
            frametime * settings.anglespeedkey
        } else {
            frametime
        };
        if !self.held(Button::Strafe) {
            angles[1] -= speed * settings.yawspeed * self.key_state(Button::Right);
            angles[1] += speed * settings.yawspeed * self.key_state(Button::Left);
            angles[1] = anglemod(angles[1]);
        }
        if self.held(Button::KLook) {
            drift.stop = true;
            angles[0] -= speed * settings.pitchspeed * self.key_state(Button::Forward);
            angles[0] += speed * settings.pitchspeed * self.key_state(Button::Back);
        }
        let up = self.key_state(Button::LookUp);
        let down = self.key_state(Button::LookDown);
        angles[0] -= speed * settings.pitchspeed * up;
        angles[0] += speed * settings.pitchspeed * down;
        if up != 0.0 || down != 0.0 {
            drift.stop = true;
        }
        angles[0] = angles[0].clamp(-70.0, 80.0);
        angles[2] = angles[2].clamp(-50.0, 50.0);
    }

    /// `IN_MouseMove`'s use of the motion (INPUT-2.4): `mouse_x *= sensitivity.value`, then
    /// strafing or turning, and pitch with mouse look, else moving.
    fn mouse_move(
        &self,
        settings: &MoveSettings,
        motion: [f32; 2],
        angles: &mut [f32; 3],
        cmd: &mut UserCmd,
        drift: &mut Drift,
    ) {
        let x = motion[0] * settings.sensitivity;
        let y = motion[1] * settings.sensitivity;
        let mlook = self.held(Button::MLook);
        let strafe = self.held(Button::Strafe);
        if strafe || (settings.lookstrafe != 0.0 && mlook) {
            cmd.side += settings.m_side * x;
        } else {
            angles[1] -= settings.m_yaw * x;
        }
        if mlook {
            drift.stop = true;
        }
        if mlook && !strafe {
            angles[0] = (angles[0] + settings.m_pitch * y).clamp(-70.0, 80.0);
        } else {
            // `noclip_anglehack` is a server's view of its own client and is off here.
            cmd.forward -= settings.m_forward * y;
        }
    }
}

/// `anglemod`: `(360.0/65536) * ((int)(a*(65536/360.0)) & 65535)`.
fn anglemod(a: f32) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec INPUT-2.3: anglemod's `(int)`, toward zero"
    )]
    let steps = (f64::from(a) * (65536.0 / 360.0)) as i32 & 65535;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec INPUT-2.3: the double product stored in the float angle"
    )]
    let angle = ((360.0 / 65536.0) * f64::from(steps)) as f32;
    angle
}

#[cfg(test)]
mod tests;
