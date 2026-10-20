// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/progs.h, EDICT_NUM, NUM_FOR_EDICT, EDICT_TO_PROG, PROG_TO_EDICT, NEXT_EDICT
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_edict.c, PR_LoadProgs, ED_Alloc, ED_Free, ED_ClearEdict

//! The server's edict store: the entities the engine and QuakeC share.
//!
//! An edict is a fixed engine header followed by the progs' field slots, and the engine keeps
//! all of them in one array that QuakeC addresses by byte offset ([`EdictLayout`]). This crate
//! lays that array out for a loaded `progs.dat`, stores and clears field slots, allocates and
//! frees edicts by id's reuse rules ([`EdictStore`]), and names the system globals and fields
//! the engine reads and writes at fixed slots ([`system`]).
//!
//! Implements `docs/spec/edicts.md`, EDICT-1 (entity memory and numbering), EDICT-2 (system
//! globals), EDICT-3 (system fields), EDICT-6 (allocation) and EDICT-7 (freeing). The physics
//! that moves entities, the map spawn loop, the engine entry points and which engine code
//! writes each system global and field (EDICT-4, EDICT-5, EDICT-8 to EDICT-13) are the
//! preview host's simulation, built on this store.
//!
//! Nothing here panics: every index, conversion and arithmetic is checked, and id's behaviour
//! on an out-of-range value is written out as the spec fixes it.

mod layout;
mod store;
pub mod system;

#[cfg(test)]
mod tests;

pub use layout::{EdictLayout, LayoutError};
pub use store::{EdictError, EdictStore};

/// Bytes of engine state at the start of every edict, before its field slots.
///
/// `EDICT_HEADER_SIZE` in EDICT-1.1, the size id's `PR_LoadProgs` computes as
/// `sizeof(edict_t) - sizeof(entvars_t)` on the 32-bit x86 build id shipped (EDICT-1.3). The
/// header holds the free flag, the area links, the leaf list, the network baseline and
/// `freetime` (EDICT-1.2); QuakeC cannot address it, so field slot 0 is the first byte after
/// it. This store models the header fields it needs (the free flag and `freetime`) as typed
/// state, not as bytes, but fixes the size at this value so entity and pointer values match
/// the 32-bit engine (EDICT-1.5, EDICT-1.6).
pub const EDICT_HEADER_SIZE: i32 = 96;

/// Field slots id's `progdefs.q1` declares before `end_sys_fields`, the system fields of
/// EDICT-3.1.
///
/// `EDICT_SYS_FIELDS_END` in EDICT-3. A progs with fewer field slots cannot hold them
/// (EDICT-3.3).
pub const EDICT_SYS_FIELDS_END: usize = 105;

/// Global slots id's `progdefs.q1` declares up to and including `end_sys_globals`.
///
/// `EDICT_SYS_GLOBALS_END` in EDICT-2. The system globals are slots 28 to 91 (EDICT-2.1); a
/// progs with fewer than this many global slots cannot hold them (EDICT-2.4).
pub const EDICT_SYS_GLOBALS_END: usize = 92;

/// Most client slots a server has, id's `Host_FindMaxClients` clamp (EDICT-5.1).
///
/// A real server has 1 to 16 clients; [`EdictStore::new`] also accepts 0, which the VM test
/// environment uses for a server with no client slots.
pub const EDICT_MAX_CLIENTS: usize = 16;

/// Most field slots a progs may declare before an entity value cannot name every edict.
///
/// From EDICT-3.5: `(2147483647 / 600 - 96) / 4` with each division rounded down. A progs
/// whose `entityfields` is greater than this is refused ([`LayoutError::FieldsTooMany`]).
pub const EDICT_MAX_FIELDS: usize = 894_760;

/// The reuse delay of EDICT-6.1: an edict freed more than this many seconds ago may be reused.
///
/// `EDICT_REUSE_DELAY` in EDICT-6, from id's `ED_Alloc`: `sv.time - e->freetime > 0.5`.
pub const EDICT_REUSE_DELAY: f64 = 0.5;

/// The reuse grace of EDICT-6.1: an edict whose `freetime` is below this may always be reused.
///
/// `EDICT_REUSE_GRACE` in EDICT-6, from id's `ED_Alloc`: `e->freetime < 2`, relaxing the
/// policy in the first couple of seconds of a map.
pub const EDICT_REUSE_GRACE: f32 = 2.0;

/// The `nextthink` value freeing an edict leaves, id's `ED_Free`: `ed->v.nextthink = -1`.
///
/// `EDICT_FREED_NEXTTHINK` in EDICT-7, the field at slot 46.
pub const EDICT_FREED_NEXTTHINK: f32 = -1.0;
