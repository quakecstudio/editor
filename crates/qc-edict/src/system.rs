// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/progdefs.q1, globalvars_t, entvars_t

//! The system globals and fields the engine and QuakeC share at fixed slots.
//!
//! The engine reads and writes these as members of a C struct laid over the globals and over
//! each edict's fields, so each one is at a fixed slot and is never looked up by name
//! (EDICT-2, EDICT-3). This module names them with their slots and types, and offers lookup
//! by slot and by name over the fixed tables, for tools and the simulation; the engine's own
//! access is by slot (EDICT-2.3, EDICT-3.2).

use qc_types::Etype;

/// One system global or field: its name, its first slot and its type.
///
/// For a vector the slot is the x component; the y and z follow (EDICT-2.1, EDICT-3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SysVar {
    /// The name id's `progdefs.q1` declares, as QuakeC source writes it.
    pub name: &'static str,
    /// The slot the value starts at.
    pub slot: usize,
    /// The value's type.
    pub etype: Etype,
}

/// The 64 system globals, slots 28 to 91, in slot order.
///
/// Implements EDICT-2.1, from `globalvars_t` in `WinQuake/progdefs.q1`: the 64 slots from
/// `PROGS_RESERVED_OFS` (28) up to but not including `end_sys_globals` (92). `end_sys_globals`
/// is not itself a system global (EDICT-2.2).
pub const SYSTEM_GLOBALS: &[SysVar] = &[
    SysVar {
        name: "self",
        slot: 28,
        etype: Etype::Entity,
    },
    SysVar {
        name: "other",
        slot: 29,
        etype: Etype::Entity,
    },
    SysVar {
        name: "world",
        slot: 30,
        etype: Etype::Entity,
    },
    SysVar {
        name: "time",
        slot: 31,
        etype: Etype::Float,
    },
    SysVar {
        name: "frametime",
        slot: 32,
        etype: Etype::Float,
    },
    SysVar {
        name: "force_retouch",
        slot: 33,
        etype: Etype::Float,
    },
    SysVar {
        name: "mapname",
        slot: 34,
        etype: Etype::String,
    },
    SysVar {
        name: "deathmatch",
        slot: 35,
        etype: Etype::Float,
    },
    SysVar {
        name: "coop",
        slot: 36,
        etype: Etype::Float,
    },
    SysVar {
        name: "teamplay",
        slot: 37,
        etype: Etype::Float,
    },
    SysVar {
        name: "serverflags",
        slot: 38,
        etype: Etype::Float,
    },
    SysVar {
        name: "total_secrets",
        slot: 39,
        etype: Etype::Float,
    },
    SysVar {
        name: "total_monsters",
        slot: 40,
        etype: Etype::Float,
    },
    SysVar {
        name: "found_secrets",
        slot: 41,
        etype: Etype::Float,
    },
    SysVar {
        name: "killed_monsters",
        slot: 42,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm1",
        slot: 43,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm2",
        slot: 44,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm3",
        slot: 45,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm4",
        slot: 46,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm5",
        slot: 47,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm6",
        slot: 48,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm7",
        slot: 49,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm8",
        slot: 50,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm9",
        slot: 51,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm10",
        slot: 52,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm11",
        slot: 53,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm12",
        slot: 54,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm13",
        slot: 55,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm14",
        slot: 56,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm15",
        slot: 57,
        etype: Etype::Float,
    },
    SysVar {
        name: "parm16",
        slot: 58,
        etype: Etype::Float,
    },
    SysVar {
        name: "v_forward",
        slot: 59,
        etype: Etype::Vector,
    },
    SysVar {
        name: "v_up",
        slot: 62,
        etype: Etype::Vector,
    },
    SysVar {
        name: "v_right",
        slot: 65,
        etype: Etype::Vector,
    },
    SysVar {
        name: "trace_allsolid",
        slot: 68,
        etype: Etype::Float,
    },
    SysVar {
        name: "trace_startsolid",
        slot: 69,
        etype: Etype::Float,
    },
    SysVar {
        name: "trace_fraction",
        slot: 70,
        etype: Etype::Float,
    },
    SysVar {
        name: "trace_endpos",
        slot: 71,
        etype: Etype::Vector,
    },
    SysVar {
        name: "trace_plane_normal",
        slot: 74,
        etype: Etype::Vector,
    },
    SysVar {
        name: "trace_plane_dist",
        slot: 77,
        etype: Etype::Float,
    },
    SysVar {
        name: "trace_ent",
        slot: 78,
        etype: Etype::Entity,
    },
    SysVar {
        name: "trace_inopen",
        slot: 79,
        etype: Etype::Float,
    },
    SysVar {
        name: "trace_inwater",
        slot: 80,
        etype: Etype::Float,
    },
    SysVar {
        name: "msg_entity",
        slot: 81,
        etype: Etype::Entity,
    },
    SysVar {
        name: "main",
        slot: 82,
        etype: Etype::Function,
    },
    SysVar {
        name: "StartFrame",
        slot: 83,
        etype: Etype::Function,
    },
    SysVar {
        name: "PlayerPreThink",
        slot: 84,
        etype: Etype::Function,
    },
    SysVar {
        name: "PlayerPostThink",
        slot: 85,
        etype: Etype::Function,
    },
    SysVar {
        name: "ClientKill",
        slot: 86,
        etype: Etype::Function,
    },
    SysVar {
        name: "ClientConnect",
        slot: 87,
        etype: Etype::Function,
    },
    SysVar {
        name: "PutClientInServer",
        slot: 88,
        etype: Etype::Function,
    },
    SysVar {
        name: "ClientDisconnect",
        slot: 89,
        etype: Etype::Function,
    },
    SysVar {
        name: "SetNewParms",
        slot: 90,
        etype: Etype::Function,
    },
    SysVar {
        name: "SetChangeParms",
        slot: 91,
        etype: Etype::Function,
    },
];

/// The 105 system fields, slots 0 to 104, in slot order.
///
/// Implements EDICT-3.1, from `entvars_t` in `WinQuake/progdefs.q1`: every field slot up to
/// but not including `end_sys_fields` (105). A mod's own fields follow from slot 105
/// (EDICT-3.2).
pub const SYSTEM_FIELDS: &[SysVar] = &[
    SysVar {
        name: "modelindex",
        slot: 0,
        etype: Etype::Float,
    },
    SysVar {
        name: "absmin",
        slot: 1,
        etype: Etype::Vector,
    },
    SysVar {
        name: "absmax",
        slot: 4,
        etype: Etype::Vector,
    },
    SysVar {
        name: "ltime",
        slot: 7,
        etype: Etype::Float,
    },
    SysVar {
        name: "movetype",
        slot: 8,
        etype: Etype::Float,
    },
    SysVar {
        name: "solid",
        slot: 9,
        etype: Etype::Float,
    },
    SysVar {
        name: "origin",
        slot: 10,
        etype: Etype::Vector,
    },
    SysVar {
        name: "oldorigin",
        slot: 13,
        etype: Etype::Vector,
    },
    SysVar {
        name: "velocity",
        slot: 16,
        etype: Etype::Vector,
    },
    SysVar {
        name: "angles",
        slot: 19,
        etype: Etype::Vector,
    },
    SysVar {
        name: "avelocity",
        slot: 22,
        etype: Etype::Vector,
    },
    SysVar {
        name: "punchangle",
        slot: 25,
        etype: Etype::Vector,
    },
    SysVar {
        name: "classname",
        slot: 28,
        etype: Etype::String,
    },
    SysVar {
        name: "model",
        slot: 29,
        etype: Etype::String,
    },
    SysVar {
        name: "frame",
        slot: 30,
        etype: Etype::Float,
    },
    SysVar {
        name: "skin",
        slot: 31,
        etype: Etype::Float,
    },
    SysVar {
        name: "effects",
        slot: 32,
        etype: Etype::Float,
    },
    SysVar {
        name: "mins",
        slot: 33,
        etype: Etype::Vector,
    },
    SysVar {
        name: "maxs",
        slot: 36,
        etype: Etype::Vector,
    },
    SysVar {
        name: "size",
        slot: 39,
        etype: Etype::Vector,
    },
    SysVar {
        name: "touch",
        slot: 42,
        etype: Etype::Function,
    },
    SysVar {
        name: "use",
        slot: 43,
        etype: Etype::Function,
    },
    SysVar {
        name: "think",
        slot: 44,
        etype: Etype::Function,
    },
    SysVar {
        name: "blocked",
        slot: 45,
        etype: Etype::Function,
    },
    SysVar {
        name: "nextthink",
        slot: 46,
        etype: Etype::Float,
    },
    SysVar {
        name: "groundentity",
        slot: 47,
        etype: Etype::Entity,
    },
    SysVar {
        name: "health",
        slot: 48,
        etype: Etype::Float,
    },
    SysVar {
        name: "frags",
        slot: 49,
        etype: Etype::Float,
    },
    SysVar {
        name: "weapon",
        slot: 50,
        etype: Etype::Float,
    },
    SysVar {
        name: "weaponmodel",
        slot: 51,
        etype: Etype::String,
    },
    SysVar {
        name: "weaponframe",
        slot: 52,
        etype: Etype::Float,
    },
    SysVar {
        name: "currentammo",
        slot: 53,
        etype: Etype::Float,
    },
    SysVar {
        name: "ammo_shells",
        slot: 54,
        etype: Etype::Float,
    },
    SysVar {
        name: "ammo_nails",
        slot: 55,
        etype: Etype::Float,
    },
    SysVar {
        name: "ammo_rockets",
        slot: 56,
        etype: Etype::Float,
    },
    SysVar {
        name: "ammo_cells",
        slot: 57,
        etype: Etype::Float,
    },
    SysVar {
        name: "items",
        slot: 58,
        etype: Etype::Float,
    },
    SysVar {
        name: "takedamage",
        slot: 59,
        etype: Etype::Float,
    },
    SysVar {
        name: "chain",
        slot: 60,
        etype: Etype::Entity,
    },
    SysVar {
        name: "deadflag",
        slot: 61,
        etype: Etype::Float,
    },
    SysVar {
        name: "view_ofs",
        slot: 62,
        etype: Etype::Vector,
    },
    SysVar {
        name: "button0",
        slot: 65,
        etype: Etype::Float,
    },
    SysVar {
        name: "button1",
        slot: 66,
        etype: Etype::Float,
    },
    SysVar {
        name: "button2",
        slot: 67,
        etype: Etype::Float,
    },
    SysVar {
        name: "impulse",
        slot: 68,
        etype: Etype::Float,
    },
    SysVar {
        name: "fixangle",
        slot: 69,
        etype: Etype::Float,
    },
    SysVar {
        name: "v_angle",
        slot: 70,
        etype: Etype::Vector,
    },
    SysVar {
        name: "idealpitch",
        slot: 73,
        etype: Etype::Float,
    },
    SysVar {
        name: "netname",
        slot: 74,
        etype: Etype::String,
    },
    SysVar {
        name: "enemy",
        slot: 75,
        etype: Etype::Entity,
    },
    SysVar {
        name: "flags",
        slot: 76,
        etype: Etype::Float,
    },
    SysVar {
        name: "colormap",
        slot: 77,
        etype: Etype::Float,
    },
    SysVar {
        name: "team",
        slot: 78,
        etype: Etype::Float,
    },
    SysVar {
        name: "max_health",
        slot: 79,
        etype: Etype::Float,
    },
    SysVar {
        name: "teleport_time",
        slot: 80,
        etype: Etype::Float,
    },
    SysVar {
        name: "armortype",
        slot: 81,
        etype: Etype::Float,
    },
    SysVar {
        name: "armorvalue",
        slot: 82,
        etype: Etype::Float,
    },
    SysVar {
        name: "waterlevel",
        slot: 83,
        etype: Etype::Float,
    },
    SysVar {
        name: "watertype",
        slot: 84,
        etype: Etype::Float,
    },
    SysVar {
        name: "ideal_yaw",
        slot: 85,
        etype: Etype::Float,
    },
    SysVar {
        name: "yaw_speed",
        slot: 86,
        etype: Etype::Float,
    },
    SysVar {
        name: "aiment",
        slot: 87,
        etype: Etype::Entity,
    },
    SysVar {
        name: "goalentity",
        slot: 88,
        etype: Etype::Entity,
    },
    SysVar {
        name: "spawnflags",
        slot: 89,
        etype: Etype::Float,
    },
    SysVar {
        name: "target",
        slot: 90,
        etype: Etype::String,
    },
    SysVar {
        name: "targetname",
        slot: 91,
        etype: Etype::String,
    },
    SysVar {
        name: "dmg_take",
        slot: 92,
        etype: Etype::Float,
    },
    SysVar {
        name: "dmg_save",
        slot: 93,
        etype: Etype::Float,
    },
    SysVar {
        name: "dmg_inflictor",
        slot: 94,
        etype: Etype::Entity,
    },
    SysVar {
        name: "owner",
        slot: 95,
        etype: Etype::Entity,
    },
    SysVar {
        name: "movedir",
        slot: 96,
        etype: Etype::Vector,
    },
    SysVar {
        name: "message",
        slot: 99,
        etype: Etype::String,
    },
    SysVar {
        name: "sounds",
        slot: 100,
        etype: Etype::Float,
    },
    SysVar {
        name: "noise",
        slot: 101,
        etype: Etype::String,
    },
    SysVar {
        name: "noise1",
        slot: 102,
        etype: Etype::String,
    },
    SysVar {
        name: "noise2",
        slot: 103,
        etype: Etype::String,
    },
    SysVar {
        name: "noise3",
        slot: 104,
        etype: Etype::String,
    },
];

/// The system global at slot `slot`, if one starts there.
///
/// Implements EDICT-2.1: lookup over [`SYSTEM_GLOBALS`] by the slot a value starts at. A slot
/// inside a vector global (`v_forward_y`, slot 60) names none, since no system global starts
/// there.
#[must_use]
pub fn global_at_slot(slot: usize) -> Option<&'static SysVar> {
    SYSTEM_GLOBALS.iter().find(|var| var.slot == slot)
}

/// The system field at slot `slot`, if one starts there.
///
/// Implements EDICT-3.1: lookup over [`SYSTEM_FIELDS`] by the slot a value starts at. A slot
/// inside a vector field (`origin_y`, slot 11) names none.
#[must_use]
pub fn field_at_slot(slot: usize) -> Option<&'static SysVar> {
    SYSTEM_FIELDS.iter().find(|var| var.slot == slot)
}

/// The system global named `name`, if one is.
///
/// Implements EDICT-2.1. The engine never does this (EDICT-2.3); it is for tools over the
/// fixed table.
#[must_use]
pub fn global_by_name(name: &str) -> Option<&'static SysVar> {
    SYSTEM_GLOBALS.iter().find(|var| var.name == name)
}

/// The system field named `name`, if one is.
///
/// Implements EDICT-3.1. The engine never does this (EDICT-3.2); it is for tools over the
/// fixed table.
#[must_use]
pub fn field_by_name(name: &str) -> Option<&'static SysVar> {
    SYSTEM_FIELDS.iter().find(|var| var.name == name)
}
