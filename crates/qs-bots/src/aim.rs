// SPDX-License-Identifier: GPL-3.0-or-later

//! How well a bot aims at each skill (BOT-5): its error, how fast it turns, and how long it
//! takes to react.

use qs_sim::BotSkill;

use crate::rng::BotRng;

/// Degrees in a half turn and a whole one.
const HALF_TURN: f32 = 180.0;
const TURN: f32 = 360.0;
/// How far the view may be from the aim for a bot to fire, in degrees (BOT-5.3).
pub(crate) const FIRE_CONE: f32 = 15.0;

/// What a skill gives a bot's aim (BOT-5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aim {
    /// The most its aim is off, side to side and up and down, in degrees.
    pub error: f32,
    /// How fast it turns, in degrees a second.
    pub turn: f32,
    /// How long it sees a foe before it fires, in seconds.
    pub reaction: f32,
}

/// The aim of each skill, 0 to 3 (BOT-5.1, BOT-5.2, BOT-5.3).
const EASY: Aim = Aim {
    error: 12.0,
    turn: 180.0,
    reaction: 0.6,
};
const NORMAL: Aim = Aim {
    error: 7.0,
    turn: 360.0,
    reaction: 0.4,
};
const HARD: Aim = Aim {
    error: 4.0,
    turn: 540.0,
    reaction: 0.25,
};
const NIGHTMARE: Aim = Aim {
    error: 2.0,
    turn: 900.0,
    reaction: 0.15,
};

impl Aim {
    /// The aim of `skill` (BOT-5.1, BOT-5.2, BOT-5.3).
    #[must_use]
    pub const fn of(skill: BotSkill) -> Self {
        match skill {
            BotSkill::Easy => EASY,
            BotSkill::Normal => NORMAL,
            BotSkill::Hard => HARD,
            BotSkill::Nightmare => NIGHTMARE,
        }
    }

    /// A fresh error, pitch and yaw, each uniformly within plus or minus [`Aim::error`]
    /// (BOT-5.1).
    pub fn draw_error(&self, rng: &mut BotRng) -> [f32; 2] {
        [rng.signed() * self.error, rng.signed() * self.error]
    }
}

/// `to - from` in degrees, from -180 up to but not including 180.
#[must_use]
pub(crate) fn angle_between(from: f32, to: f32) -> f32 {
    (to - from + HALF_TURN).rem_euclid(TURN) - HALF_TURN
}

/// The angles, pitch and yaw in degrees as `v_angle` holds them, of the line from `from` to
/// `to`: pitch up negative, as the engine's view pitch is.
#[must_use]
pub(crate) fn angles_to(from: [f32; 3], to: [f32; 3]) -> [f32; 2] {
    let [dx, dy, dz] = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
    let yaw = dy.atan2(dx).to_degrees();
    let pitch = -dz.atan2(dx.hypot(dy)).to_degrees();
    [pitch, yaw]
}

/// `view` turned toward `want` by at most `step` degrees on each axis.
#[must_use]
pub(crate) fn turned(view: [f32; 2], want: [f32; 2], step: f32) -> [f32; 2] {
    let toward = |from: f32, to: f32| {
        let d = angle_between(from, to);
        from + d.clamp(-step, step)
    };
    [toward(view[0], want[0]), toward(view[1], want[1])]
}
