// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2002-2009 John Fitzgibbons and others
// Copyright (C) 2010-2014 QuakeSpasm developers
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/cl_parse.c, CL_EntityNum, CL_ParseBaseline, CL_ParseUpdate
// Ported from sezero/quakespasm@c9c8ee9895961669c4674e8f8ca8c64cc43e38b5: Quake/cl_parse.c, CL_ParseUpdate

//! An entity slot, `entity_t` (CLIENT-1.2, CLIENT-2): its baseline, the two latest
//! positions from the server, where it is drawn, and the interpolation flags of CLIENT-7.

/// An entity's baseline, `entity_state_t` (CLIENT-1.3).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Baseline {
    /// Its origin.
    pub origin: [f32; 3],
    /// Its angles.
    pub angles: [f32; 3],
    /// Its model precache index.
    pub modelindex: usize,
    /// Its frame.
    pub frame: i32,
    /// Its colormap.
    pub colormap: i32,
    /// Its skin.
    pub skin: i32,
    /// Its effects, never set from a message.
    pub effects: i32,
}

/// The interpolation flags the FitzQuake lineage keeps (CLIENT-7).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LerpFlags {
    /// `LERP_MOVESTEP`: a `MOVETYPE_STEP` entity, whose moves are interpolated.
    pub movestep: bool,
    /// `LERP_RESETANIM`: restart the animation lerp.
    pub reset_anim: bool,
    /// `LERP_RESETANIM2`: restart it once more.
    pub reset_anim2: bool,
    /// `LERP_RESETMOVE`: restart the move lerp.
    pub reset_move: bool,
}

/// The animation and move lerp state of an entity (CLIENT-7.3, CLIENT-7.4).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lerp {
    /// `lerpstart`: when the pose lerp started.
    pub start: f64,
    /// `lerptime`: how long the pose lerp takes.
    pub time: f32,
    /// `previouspose`.
    pub previous_pose: usize,
    /// `currentpose`.
    pub current_pose: usize,
    /// `movelerpstart`.
    pub move_start: f64,
    /// `previousorigin`.
    pub previous_origin: [f32; 3],
    /// `currentorigin`.
    pub current_origin: [f32; 3],
    /// `previousangles`.
    pub previous_angles: [f32; 3],
    /// `currentangles`.
    pub current_angles: [f32; 3],
}

/// One entity slot, `entity_t` (CLIENT-1.2).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Entity {
    /// Its baseline (CLIENT-1.3).
    pub baseline: Baseline,
    /// `msgtime`: the time of its last update (CLIENT-2.2).
    pub msgtime: f32,
    /// `msg_origins`: the latest and the previous origin from the server.
    pub msg_origins: [[f32; 3]; 2],
    /// `msg_angles`: the latest and the previous angles.
    pub msg_angles: [[f32; 3]; 2],
    /// Where it is drawn (CLIENT-5.4).
    pub origin: [f32; 3],
    /// How it is turned.
    pub angles: [f32; 3],
    /// Its model precache index, 0 for none.
    pub model: usize,
    /// Its frame.
    pub frame: i32,
    /// The client slot whose colours it wears, 0 for none (CLIENT-1.6).
    pub colormap: i32,
    /// Its skin.
    pub skin: i32,
    /// Its `effects` bits (CLIENT-5.5).
    pub effects: i32,
    /// `syncbase`: where its animation starts (CLIENT-2.4).
    pub syncbase: f32,
    /// `forcelink`: drawn at its latest position without interpolation (CLIENT-2.5).
    pub forcelink: bool,
    /// The interpolation flags (CLIENT-7).
    pub lerpflags: LerpFlags,
    /// The interpolation state (CLIENT-7).
    pub lerp: Lerp,
}

impl Entity {
    /// A slot that has never been updated: a new slot gets `LERP_RESETMOVE | LERP_RESETANIM`
    /// (CLIENT-7.1).
    #[must_use]
    pub fn new() -> Self {
        Self {
            lerpflags: LerpFlags {
                reset_move: true,
                reset_anim: true,
                ..LerpFlags::default()
            },
            ..Self::default()
        }
    }

    /// Whether the slot shows a model.
    #[must_use]
    pub const fn has_model(&self) -> bool {
        self.model != 0
    }
}
