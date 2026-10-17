// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, PF_precache_model, PF_precache_sound, PF_lightstyle, PF_cvar, PF_cvar_set

//! The deterministic mock engine of `differential.md` (DIFF-6, DIFF-7).
//!
//! Most of this is a design of this project, not a port: the fixture and the world builtins'
//! answers are decisions made so two independent runners produce the same trace (DIFF-6
//! Source). Where the mock does what id's server does, the method cites the function.
//!
//! The fixture holds the engine state a class `state` builtin reads or writes as plain data
//! (BUILTIN-4.2): the precache lists and model bounds, the lightstyles, the cvars, the
//! `changelevel` flag, the command buffer and the sizes of the three message buffers
//! (DIFF-6.5), and the DIFF-6.2 random source.

use crate::engine::Engine;

/// The settings the mock keeps, with the texts of DIFF-2.4.
///
/// `skill`, `deathmatch`, `coop` and `teamplay` start at `1`, `0`, `0`, `0`; a scenario's
/// `cvar` line replaces a text. Any other name reads as 0 and `cvar_set` of it changes nothing
/// (BUILTIN-11.8, DIFF-2.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cvars {
    entries: Vec<(String, String)>,
}

impl Default for Cvars {
    fn default() -> Self {
        Self {
            entries: vec![
                ("skill".to_string(), "1".to_string()),
                ("deathmatch".to_string(), "0".to_string()),
                ("coop".to_string(), "0".to_string()),
                ("teamplay".to_string(), "0".to_string()),
            ],
        }
    }
}

impl Cvars {
    /// The text of the setting named `name`, or `None` when there is none (BUILTIN-11.7).
    #[must_use]
    pub fn text(&self, name: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Sets the text of `name` when it is a known setting, else does nothing (BUILTIN-11.8,
    /// DIFF-2.4). Returns whether the setting existed.
    pub fn set(&mut self, name: &str, value: &str) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|(key, _)| key == name) {
            entry.1 = value.to_string();
            true
        } else {
            false
        }
    }
}

/// The bounds of a model list entry, for `setmodel` (BUILTIN-9.4, DIFF-6.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelBounds {
    /// The model's mins.
    pub mins: [f32; 3],
    /// The model's maxs.
    pub maxs: [f32; 3],
}

/// The deterministic random source of DIFF-6.2: a 32-bit linear congruential state.
///
/// `state = state * 1103515245 + 12345` modulo 2^32; each draw returns `(state >> 16) & 0x7fff`.
/// `random` (BUILTIN-6.6) is the only mock builtin that draws from it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng {
    state: u32,
}

/// The multiplier of DIFF-6.2, `DIFF_RAND_MULTIPLIER`.
pub const DIFF_RAND_MULTIPLIER: u32 = 1_103_515_245;
/// The increment of DIFF-6.2, `DIFF_RAND_INCREMENT`.
pub const DIFF_RAND_INCREMENT: u32 = 12345;

impl Rng {
    /// A source seeded with the scenario's `seed` (DIFF-3.2, DIFF-2.2).
    #[must_use]
    pub const fn new(seed: u32) -> Self {
        Self { state: seed }
    }

    /// One draw: advances the state and returns `(state >> 16) & 0x7fff` (DIFF-6.2).
    pub fn next_rand(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(DIFF_RAND_MULTIPLIER)
            .wrapping_add(DIFF_RAND_INCREMENT);
        (self.state >> 16) & 0x7fff
    }
}

/// The three output buffers whose sizes the mock keeps (DIFF-6.5).
///
/// The mock keeps only the byte count of each: the unreliable datagram and the reliable
/// datagram (`LIMIT_MAX_DATAGRAM`, 1024) and the signon buffer (`BUILTIN_SIGNON_SIZE`, 8192).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buffers {
    /// Bytes in the unreliable datagram (`MSG_BROADCAST`).
    pub unreliable: usize,
    /// Bytes in the reliable datagram (`MSG_ALL`).
    pub reliable: usize,
    /// Bytes in the signon buffer (`MSG_INIT`).
    pub signon: usize,
}

/// `LIMIT_MAX_DATAGRAM`, the datagram size (BUILTIN-13.3).
pub const LIMIT_MAX_DATAGRAM: usize = 1024;
/// `BUILTIN_SIGNON_SIZE`, the signon buffer size (BUILTIN-13.3).
pub const BUILTIN_SIGNON_SIZE: usize = 8192;
/// `BUILTIN_CMD_TEXT_SIZE`, the command buffer size (BUILTIN-11.5).
pub const BUILTIN_CMD_TEXT_SIZE: usize = 8192;

/// The mock engine fixture (DIFF-6).
///
/// It implements [`Engine`] with [`Engine::has_world`] returning `true`, because the mock
/// answers the class `world` builtins by the deterministic rules of DIFF-7. A real engine
/// (`qs-sim`, P3-24) will implement [`Engine`] with the true simulation.
#[derive(Clone, Debug, PartialEq)]
pub struct MockEngine {
    /// The sound precache list, entry 0 the empty string (DIFF-3.3, BUILTIN-10.3).
    pub sound_precache: Vec<String>,
    /// The model precache list: entry 0 empty, 1 the world, 2.. the submodels (DIFF-3.3).
    pub model_precache: Vec<String>,
    /// The bounds of each named model for `setmodel` (DIFF-6.3), when a `model` directive gave
    /// them; a name not here uses the DIFF-6.3 default rule.
    pub model_bounds: Vec<(String, ModelBounds)>,
    /// The 64 lightstyles, each a string or none (BUILTIN-12.6, DIFF-3.2).
    pub lightstyles: Vec<Option<String>>,
    /// The settings (DIFF-2.4).
    pub cvars: Cvars,
    /// Whether a `changelevel` was issued since the map started (BUILTIN-11.6, DIFF-6.4).
    pub changelevel_issued: bool,
    /// The command buffer `localcmd` and `changelevel` append to; never run (DIFF-6.4). Kept
    /// as bytes so an overflow is detected (BUILTIN-11.5).
    pub command_buffer: Vec<u8>,
    /// The three message buffer sizes (DIFF-6.5).
    pub buffers: Buffers,
    /// The random source (DIFF-6.2).
    pub rng: Rng,
}

/// The lightstyle count id does not range-check against, `LIMIT_MAX_LIGHTSTYLES` (BUILTIN-12.6).
pub const LIMIT_MAX_LIGHTSTYLES: usize = 64;

impl MockEngine {
    /// A fresh fixture for a map, before any spawn function runs (DIFF-3.2, DIFF-3.3).
    ///
    /// The sound list holds entry 0; the model list holds entry 0, `maps/<map>.bsp` and the
    /// `submodels` names `*1`..`*n`. The lightstyles are empty, the buffers empty, the
    /// `changelevel` flag clear, and the random source is seeded.
    #[must_use]
    pub fn new(map: &str, submodels: usize, seed: u32) -> Self {
        let mut model_precache = vec![String::new(), format!("maps/{map}.bsp")];
        for n in 1..=submodels {
            model_precache.push(format!("*{n}"));
        }
        Self {
            sound_precache: vec![String::new()],
            model_precache,
            model_bounds: Vec::new(),
            lightstyles: vec![None; LIMIT_MAX_LIGHTSTYLES],
            cvars: Cvars::default(),
            changelevel_issued: false,
            command_buffer: Vec::new(),
            buffers: Buffers::default(),
            rng: Rng::new(seed),
        }
    }

    /// The bounds for a model list entry's name, by the DIFF-6.3 rule.
    ///
    /// Entry 0 is `'0 0 0'`/`'0 0 0'`; a name a `model` directive gave uses those bounds; any
    /// other name ending in `.mdl` is `'-16 -16 -16'`/`'16 16 16'`; every other name (the
    /// world, submodels and sprites among them) is `'0 0 0'`/`'0 0 0'`.
    #[must_use]
    pub fn model_bounds_for(&self, name: &str) -> ModelBounds {
        if name.is_empty() {
            return ModelBounds {
                mins: [0.0; 3],
                maxs: [0.0; 3],
            };
        }
        if let Some((_, bounds)) = self.model_bounds.iter().find(|(key, _)| key == name) {
            return *bounds;
        }
        if name.as_bytes().ends_with(b".mdl") {
            return ModelBounds {
                mins: [-16.0; 3],
                maxs: [16.0; 3],
            };
        }
        ModelBounds {
            mins: [0.0; 3],
            maxs: [0.0; 3],
        }
    }
}

impl Engine for MockEngine {
    fn has_world(&self) -> bool {
        true
    }

    fn next_rand(&mut self) -> u32 {
        self.rng.next_rand()
    }
}
