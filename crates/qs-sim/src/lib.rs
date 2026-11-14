// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview host's server simulation (P3-24): a map's entities spawned through the progs
//! (`spawn`, id's `ED_LoadFromFile`, EDICT-8), and the world they live in.
//!
//! Implements `docs/spec/edicts.md` EDICT-4.1, EDICT-8 and EDICT-11.4.

mod builtins;
mod client;
pub mod collision;
mod console;
pub mod debug;
#[cfg(test)]
mod fixtures;
mod host_commands;
pub mod input;
pub mod keys;
mod messages;
mod movement;
mod offsets;
mod physics;
pub mod rerelease;
pub mod scenario;
mod settings;
pub mod spawn;
mod world;

/// A point's extent: a trace with no mins or maxs (WORLD-4).
const POINT: [f32; 3] = [0.0; 3];
/// A vector of zeros: no velocity, or a vector not found.
const ZERO: [f32; 3] = [0.0; 3];
/// Half: an edict's centre is its origin plus half its mins and maxs (BUILTIN-12.3, WORLD-8.8).
const HALF: f32 = 0.5;

pub use builtins::{ModelFiles, SimBuiltins};
pub use client::UserCmd;
pub use messages::{
    ClientMessages, GameKind, Message, NET_FAT_PVS_DISTANCE, NET_GAME_COOP, NET_GAME_DEATHMATCH,
    NET_MAX_DATAGRAM, NET_MAX_FORWARD, NET_MAX_MSGLEN, NET_PACKET_SLACK, NET_PROTOCOL_VERSION,
    NET_SIGNON_SIZE, NET_VERSION, Overflow,
};
pub use offsets::{FieldOffset, GlobalOffset};
pub use physics::Sound;
pub use qc_builtins::localization;
pub use qc_edict::EdictNumber;
pub use world::{
    CLIENT_EDICT, Change, ChangeKind, ClientCommand, EdictChange, EditError, Entity, EvalError,
    HostCommand, HostState, KeyFocus, LineHit, LoadOptions, MapPlace, Migration, NamedValue,
    QcValue, RawState, Reason, SAVEGAME_VERSION, SaveError, SaveGame, Signon, SpawnParms, State,
    Tracking, World, WorldError, command_words, read_state,
};
