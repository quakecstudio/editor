// SPDX-License-Identifier: GPL-3.0-or-later

//! What the client needs to know of each model the server names (CLIENT-1.1): its kind, its
//! flags, whether its animation is synchronised or random (CLIENT-2.4) and, for an alias
//! model, each frame's poses and interval (CLIENT-7.3). The host reads the files; the client
//! asks it through [`Models`].

use qs_mdl::mdl::{Mdl, MdlFrameEntry};
use qs_mdl::spr::Sprite;

/// `ST_RAND`: a model whose animation starts at a random time (CLIENT-2.4).
pub const SYNCTYPE_RAND: i32 = 1;

/// A model's `flags`, `EF_ROCKET` to `EF_TRACER3` (CLIENT-5.5).
pub mod flags {
    /// `EF_ROCKET`: a rocket trail and a light.
    pub const ROCKET: i32 = 1;
    /// `EF_GRENADE`: a smoke trail.
    pub const GRENADE: i32 = 2;
    /// `EF_GIB`: a blood trail.
    pub const GIB: i32 = 4;
    /// `EF_ROTATE`: the model turns, as a bonus item does.
    pub const ROTATE: i32 = 8;
    /// `EF_TRACER`: a green split trail.
    pub const TRACER: i32 = 16;
    /// `EF_ZOMGIB`: a small blood trail.
    pub const ZOMGIB: i32 = 32;
    /// `EF_TRACER2`: an orange split trail.
    pub const TRACER2: i32 = 64;
    /// `EF_TRACER3`: a purple trail.
    pub const TRACER3: i32 = 128;
}

/// One frame of an alias model: its first pose, how many poses it cycles through and the
/// interval between them (CLIENT-7.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameInfo {
    /// The frame's first pose.
    pub first_pose: usize,
    /// How many poses the frame has: 1 for a single frame.
    pub poses: usize,
    /// The seconds between a group's poses, 0 for a single frame.
    pub interval: f32,
}

/// What kind of model a precache entry is.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelKind {
    /// An `.mdl`: its frames for the animation lerp.
    Alias {
        /// Each frame's poses.
        frames: Vec<FrameInfo>,
    },
    /// A `.bsp`, the world or a brush model.
    Brush,
    /// A `.spr`.
    Sprite,
}

/// What the client knows of one model.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelInfo {
    /// Its kind.
    pub kind: ModelKind,
    /// Its `flags` (an alias model's `EF_` bits, else 0).
    pub flags: i32,
    /// Whether its animation starts at a random time, `ST_RAND`.
    pub random_sync: bool,
    /// Whether the animation lerp is off for it (`MOD_NOLERP`), as the FitzQuake lineage
    /// sets for a few models (CLIENT-7.3); never set by the files themselves.
    pub nolerp: bool,
}

impl ModelInfo {
    /// A brush model.
    #[must_use]
    pub const fn brush() -> Self {
        Self {
            kind: ModelKind::Brush,
            flags: 0,
            random_sync: false,
            nolerp: false,
        }
    }

    /// An alias model's info from its file: the frames' poses and intervals, as
    /// `Mod_LoadAliasModel` keeps them (a group's interval is its last pose's).
    #[must_use]
    pub fn alias(mdl: &Mdl<'_>) -> Self {
        let mut frames = Vec::new();
        let mut first_pose = 0;
        for entry in &mdl.frames {
            let (poses, interval) = match entry {
                MdlFrameEntry::Single(_) => (1, 0.0),
                MdlFrameEntry::Group {
                    frames, intervals, ..
                } => (
                    frames.len().max(1),
                    intervals.last().copied().unwrap_or(0.0),
                ),
            };
            frames.push(FrameInfo {
                first_pose,
                poses,
                interval,
            });
            first_pose = first_pose.saturating_add(poses);
        }
        Self {
            kind: ModelKind::Alias { frames },
            flags: mdl.header.flags,
            random_sync: mdl.header.synctype == SYNCTYPE_RAND,
            nolerp: false,
        }
    }

    /// A sprite's info from its file.
    #[must_use]
    pub fn sprite(sprite: &Sprite<'_>) -> Self {
        Self {
            kind: ModelKind::Sprite,
            flags: 0,
            random_sync: sprite.header.sync_type == SYNCTYPE_RAND,
            nolerp: false,
        }
    }

    /// Frame `frame`'s info for an alias model, frame 0 for one the model does not have
    /// (CLIENT-7.3); `None` for another kind.
    #[must_use]
    pub fn frame(&self, frame: usize) -> Option<FrameInfo> {
        match &self.kind {
            ModelKind::Alias { frames } => frames.get(frame).or_else(|| frames.first()).copied(),
            ModelKind::Brush | ModelKind::Sprite => None,
        }
    }
}

/// How the client learns of the models the server names (CLIENT-1.1): the host loads the
/// file and describes it, or says it is missing.
pub trait Models {
    /// The model named `name` (`progs/soldier.mdl`, `maps/e1m1.bsp`, `*3`), or `None` when
    /// it cannot be loaded, which stops the connection.
    fn describe(&mut self, name: &str) -> Option<ModelInfo>;
}

/// A source of `rand ()` for the client: the sync base, the lights' radii, the particle
/// effects and the beams' rolls.
pub trait Random {
    /// The next value, as C's `rand ()`: non-negative, at most `RAND_MAX`.
    fn rand(&mut self) -> i32;
}
