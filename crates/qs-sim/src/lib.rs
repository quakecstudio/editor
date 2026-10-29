// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview host's server simulation (P3-24): a map's entities spawned through the progs
//! (`spawn`, id's `ED_LoadFromFile`, EDICT-8), and the world they live in.
//!
//! Implements `docs/spec/edicts.md` EDICT-4.1, EDICT-8 and EDICT-11.4.

pub mod collision;
pub mod spawn;
mod world;

pub use world::{Entity, World, WorldError};
