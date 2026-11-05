// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview host's server simulation (P3-24): a map's entities spawned through the progs
//! (`spawn`, id's `ED_LoadFromFile`, EDICT-8), and the world they live in.
//!
//! Implements `docs/spec/edicts.md` EDICT-4.1, EDICT-8 and EDICT-11.4.

mod builtins;
mod client;
pub mod collision;
#[cfg(test)]
mod fixtures;
mod messages;
mod movement;
mod physics;
pub mod rerelease;
pub mod spawn;
mod world;

pub use builtins::{ModelFiles, SimBuiltins};
pub use client::UserCmd;
pub use messages::{
    ClientMessages, Message, NET_FAT_PVS_DISTANCE, NET_GAME_COOP, NET_GAME_DEATHMATCH,
    NET_IDEALPITCH_SCALE, NET_MAX_DATAGRAM, NET_MAX_FORWARD, NET_MAX_MSGLEN, NET_PACKET_SLACK,
    NET_PROTOCOL_VERSION, NET_SIGNON_SIZE, NET_VERSION,
};
pub use physics::Sound;
pub use qc_builtins::localization;
pub use world::{Entity, LoadOptions, SpawnParms, World, WorldError};
