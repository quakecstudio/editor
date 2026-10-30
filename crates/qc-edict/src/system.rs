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

/// Declares a table of system variables and an enum with one variant each, from one list, so
/// the two cannot disagree: `$table` is the `[SysVar]` in slot order, `$enum` names each entry,
/// and `$enum::var` is its row.
macro_rules! system_vars {
    (
        $(#[$table_meta:meta])*
        $table:ident;
        $(#[$enum_meta:meta])*
        $enum:ident;
        $($variant:ident = ($name:literal, $slot:literal, $etype:ident),)*
    ) => {
        $(#[$table_meta])*
        pub const $table: &[SysVar] = &[$(SysVar {
            name: $name,
            slot: $slot,
            etype: Etype::$etype,
        }),*];

        $(#[$enum_meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $enum {
            $(
                #[doc = concat!("`", $name, "`, slot ", stringify!($slot), ".")]
                $variant,
            )*
        }

        impl $enum {
            /// Every one, in slot order.
            pub const ALL: &[Self] = &[$(Self::$variant),*];

            /// Its row of the table: name, first slot and type.
            #[must_use]
            pub const fn var(self) -> SysVar {
                match self {
                    $(Self::$variant => SysVar {
                        name: $name,
                        slot: $slot,
                        etype: Etype::$etype,
                    },)*
                }
            }

            /// The slot its value starts at.
            #[must_use]
            pub const fn slot(self) -> usize {
                self.var().slot
            }

            /// The slot its value starts at, as the VM addresses globals and fields.
            #[must_use]
            pub const fn ofs(self) -> i32 {
                match self {
                    $(Self::$variant => $slot,)*
                }
            }

            /// Its name, as QuakeC source writes it.
            #[must_use]
            pub const fn name(self) -> &'static str {
                self.var().name
            }
        }
    };
}

system_vars! {
    /// The 64 system globals, slots 28 to 91, in slot order.
    ///
    /// Implements EDICT-2.1, from `globalvars_t` in `WinQuake/progdefs.q1`: the 64 slots from
    /// `PROGS_RESERVED_OFS` (28) up to but not including `end_sys_globals` (92). `end_sys_globals`
    /// is not itself a system global (EDICT-2.2).
    SYSTEM_GLOBALS;
    /// One system global, by its name in `progdefs.q1`; its row of [`SYSTEM_GLOBALS`] is [`SysGlobal::var`].
    SysGlobal;
    SelfEntity = ("self", 28, Entity),
    Other = ("other", 29, Entity),
    World = ("world", 30, Entity),
    Time = ("time", 31, Float),
    Frametime = ("frametime", 32, Float),
    ForceRetouch = ("force_retouch", 33, Float),
    Mapname = ("mapname", 34, String),
    Deathmatch = ("deathmatch", 35, Float),
    Coop = ("coop", 36, Float),
    Teamplay = ("teamplay", 37, Float),
    Serverflags = ("serverflags", 38, Float),
    TotalSecrets = ("total_secrets", 39, Float),
    TotalMonsters = ("total_monsters", 40, Float),
    FoundSecrets = ("found_secrets", 41, Float),
    KilledMonsters = ("killed_monsters", 42, Float),
    Parm1 = ("parm1", 43, Float),
    Parm2 = ("parm2", 44, Float),
    Parm3 = ("parm3", 45, Float),
    Parm4 = ("parm4", 46, Float),
    Parm5 = ("parm5", 47, Float),
    Parm6 = ("parm6", 48, Float),
    Parm7 = ("parm7", 49, Float),
    Parm8 = ("parm8", 50, Float),
    Parm9 = ("parm9", 51, Float),
    Parm10 = ("parm10", 52, Float),
    Parm11 = ("parm11", 53, Float),
    Parm12 = ("parm12", 54, Float),
    Parm13 = ("parm13", 55, Float),
    Parm14 = ("parm14", 56, Float),
    Parm15 = ("parm15", 57, Float),
    Parm16 = ("parm16", 58, Float),
    VForward = ("v_forward", 59, Vector),
    VUp = ("v_up", 62, Vector),
    VRight = ("v_right", 65, Vector),
    TraceAllsolid = ("trace_allsolid", 68, Float),
    TraceStartsolid = ("trace_startsolid", 69, Float),
    TraceFraction = ("trace_fraction", 70, Float),
    TraceEndpos = ("trace_endpos", 71, Vector),
    TracePlaneNormal = ("trace_plane_normal", 74, Vector),
    TracePlaneDist = ("trace_plane_dist", 77, Float),
    TraceEnt = ("trace_ent", 78, Entity),
    TraceInopen = ("trace_inopen", 79, Float),
    TraceInwater = ("trace_inwater", 80, Float),
    MsgEntity = ("msg_entity", 81, Entity),
    Main = ("main", 82, Function),
    Startframe = ("StartFrame", 83, Function),
    Playerprethink = ("PlayerPreThink", 84, Function),
    Playerpostthink = ("PlayerPostThink", 85, Function),
    Clientkill = ("ClientKill", 86, Function),
    Clientconnect = ("ClientConnect", 87, Function),
    Putclientinserver = ("PutClientInServer", 88, Function),
    Clientdisconnect = ("ClientDisconnect", 89, Function),
    Setnewparms = ("SetNewParms", 90, Function),
    Setchangeparms = ("SetChangeParms", 91, Function),
}

system_vars! {
    /// The 105 system fields, slots 0 to 104, in slot order.
    ///
    /// Implements EDICT-3.1, from `entvars_t` in `WinQuake/progdefs.q1`: every field slot up to
    /// but not including `end_sys_fields` (105). A mod's own fields follow from slot 105
    /// (EDICT-3.2).
    SYSTEM_FIELDS;
    /// One system field, by its name in `progdefs.q1`; its row of [`SYSTEM_FIELDS`] is [`SysField::var`].
    SysField;
    Modelindex = ("modelindex", 0, Float),
    Absmin = ("absmin", 1, Vector),
    Absmax = ("absmax", 4, Vector),
    Ltime = ("ltime", 7, Float),
    Movetype = ("movetype", 8, Float),
    Solid = ("solid", 9, Float),
    Origin = ("origin", 10, Vector),
    Oldorigin = ("oldorigin", 13, Vector),
    Velocity = ("velocity", 16, Vector),
    Angles = ("angles", 19, Vector),
    Avelocity = ("avelocity", 22, Vector),
    Punchangle = ("punchangle", 25, Vector),
    Classname = ("classname", 28, String),
    Model = ("model", 29, String),
    Frame = ("frame", 30, Float),
    Skin = ("skin", 31, Float),
    Effects = ("effects", 32, Float),
    Mins = ("mins", 33, Vector),
    Maxs = ("maxs", 36, Vector),
    Size = ("size", 39, Vector),
    Touch = ("touch", 42, Function),
    Use = ("use", 43, Function),
    Think = ("think", 44, Function),
    Blocked = ("blocked", 45, Function),
    Nextthink = ("nextthink", 46, Float),
    Groundentity = ("groundentity", 47, Entity),
    Health = ("health", 48, Float),
    Frags = ("frags", 49, Float),
    Weapon = ("weapon", 50, Float),
    Weaponmodel = ("weaponmodel", 51, String),
    Weaponframe = ("weaponframe", 52, Float),
    Currentammo = ("currentammo", 53, Float),
    AmmoShells = ("ammo_shells", 54, Float),
    AmmoNails = ("ammo_nails", 55, Float),
    AmmoRockets = ("ammo_rockets", 56, Float),
    AmmoCells = ("ammo_cells", 57, Float),
    Items = ("items", 58, Float),
    Takedamage = ("takedamage", 59, Float),
    Chain = ("chain", 60, Entity),
    Deadflag = ("deadflag", 61, Float),
    ViewOfs = ("view_ofs", 62, Vector),
    Button0 = ("button0", 65, Float),
    Button1 = ("button1", 66, Float),
    Button2 = ("button2", 67, Float),
    Impulse = ("impulse", 68, Float),
    Fixangle = ("fixangle", 69, Float),
    VAngle = ("v_angle", 70, Vector),
    Idealpitch = ("idealpitch", 73, Float),
    Netname = ("netname", 74, String),
    Enemy = ("enemy", 75, Entity),
    Flags = ("flags", 76, Float),
    Colormap = ("colormap", 77, Float),
    Team = ("team", 78, Float),
    MaxHealth = ("max_health", 79, Float),
    TeleportTime = ("teleport_time", 80, Float),
    Armortype = ("armortype", 81, Float),
    Armorvalue = ("armorvalue", 82, Float),
    Waterlevel = ("waterlevel", 83, Float),
    Watertype = ("watertype", 84, Float),
    IdealYaw = ("ideal_yaw", 85, Float),
    YawSpeed = ("yaw_speed", 86, Float),
    Aiment = ("aiment", 87, Entity),
    Goalentity = ("goalentity", 88, Entity),
    Spawnflags = ("spawnflags", 89, Float),
    Target = ("target", 90, String),
    Targetname = ("targetname", 91, String),
    DmgTake = ("dmg_take", 92, Float),
    DmgSave = ("dmg_save", 93, Float),
    DmgInflictor = ("dmg_inflictor", 94, Entity),
    Owner = ("owner", 95, Entity),
    Movedir = ("movedir", 96, Vector),
    Message = ("message", 99, String),
    Sounds = ("sounds", 100, Float),
    Noise = ("noise", 101, String),
    Noise1 = ("noise1", 102, String),
    Noise2 = ("noise2", 103, String),
    Noise3 = ("noise3", 104, String),
}

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
