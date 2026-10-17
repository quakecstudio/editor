// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, pr_builtin, pr_numbuiltins, PF_Fixme

//! The vanilla builtin table: the number, name, class and prototype of builtins 1 to 78.
//!
//! Implements BUILTIN-1 (the table) and BUILTIN-5 (numbers, names and prototypes). A progs
//! declares a builtin as a function whose `first_statement` is minus its number (PROGS-10.2);
//! the number is the only link, so this table is keyed by number (BUILTIN-1.1). Numbers 5, 33,
//! 39, 42, 50, 60 to 66 and 71 have no builtin and hold `PF_Fixme` (BUILTIN-1.2).

/// How faithfully a builtin can be reproduced without id's engine (BUILTIN-4).
///
/// The class decides what the mock engine of `differential.md` (DIFF-6) can do: `Pure`,
/// `State` and `Output` builtins are exact against a fixture; `World` builtins need the server
/// simulation and are answered by the deterministic rules of DIFF-7.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    /// Reads and writes only VM state and, for `random`, a random source (BUILTIN-4.1).
    Pure,
    /// Also reads or writes engine state a fixture can hold as plain data (BUILTIN-4.2).
    State,
    /// Its effect outside the VM is output only: message bytes, console text, command text
    /// (BUILTIN-4.3).
    Output,
    /// Needs the map's collision hulls, the area tree or the PVS (BUILTIN-4.4): engine-only.
    World,
}

/// One entry of the vanilla builtin table (BUILTIN-5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Builtin {
    /// The builtin number, 1 to 78.
    pub number: u32,
    /// The name id's 1.01 `defs.qc` gives (BUILTIN-5.1); two numbers share a C function under
    /// two names (BUILTIN-5.2), but each entry keeps its own `defs.qc` name.
    pub name: &'static str,
    /// The QuakeC prototype of the 1.01 `defs.qc`.
    pub prototype: &'static str,
    /// The `pr_builtin` C function, as a plain name for documentation (BUILTIN-5.1).
    pub c_function: &'static str,
    /// The mock class (BUILTIN-4).
    pub class: Class,
}

/// The highest vanilla builtin number, id's `pr_numbuiltins - 1` (BUILTIN-1.1, LIMIT-5.1).
pub const BUILTIN_MAX_NUMBER: u32 = 78;

/// The error text id's `PF_Fixme` gives, including id's spelling (BUILTIN-1.2).
///
/// `BUILTIN_FIXME_MESSAGE` for the runtime error of a call to one of the numbers that hold
/// `PF_Fixme`.
pub const BUILTIN_FIXME_MESSAGE: &str = "unimplemented bulitin";

use Class::{Output, Pure, State, World};

/// The 78 vanilla builtins, numbers 1 to 78, in number order (BUILTIN-5.1).
///
/// The numbers with no builtin (5, 33, 39, 42, 50, 60 to 66, 71) are absent; [`lookup`] returns
/// `None` for them, which the dispatcher turns into the `PF_Fixme` error (BUILTIN-1.2).
pub const BUILTINS: &[Builtin] = &[
    b(1, "makevectors", "void(vector ang)", "PF_makevectors", Pure),
    b(
        2,
        "setorigin",
        "void(entity e, vector o)",
        "PF_setorigin",
        State,
    ),
    b(
        3,
        "setmodel",
        "void(entity e, string m)",
        "PF_setmodel",
        State,
    ),
    b(
        4,
        "setsize",
        "void(entity e, vector min, vector max)",
        "PF_setsize",
        State,
    ),
    b(6, "break", "void()", "PF_break", Pure),
    b(7, "random", "float()", "PF_random", Pure),
    b(
        8,
        "sound",
        "void(entity e, float chan, string samp, float vol, float atten)",
        "PF_sound",
        Output,
    ),
    b(9, "normalize", "vector(vector v)", "PF_normalize", Pure),
    b(10, "error", "void(string e)", "PF_error", Pure),
    b(11, "objerror", "void(string e)", "PF_objerror", Pure),
    b(12, "vlen", "float(vector v)", "PF_vlen", Pure),
    b(13, "vectoyaw", "float(vector v)", "PF_vectoyaw", Pure),
    b(14, "spawn", "entity()", "PF_Spawn", Pure),
    b(15, "remove", "void(entity e)", "PF_Remove", Pure),
    b(
        16,
        "traceline",
        "void(vector v1, vector v2, float nomonsters, entity forent)",
        "PF_traceline",
        World,
    ),
    b(17, "checkclient", "entity()", "PF_checkclient", World),
    b(
        18,
        "find",
        "entity(entity start, .string fld, string match)",
        "PF_Find",
        Pure,
    ),
    b(
        19,
        "precache_sound",
        "string(string s)",
        "PF_precache_sound",
        State,
    ),
    b(
        20,
        "precache_model",
        "string(string s)",
        "PF_precache_model",
        State,
    ),
    b(
        21,
        "stuffcmd",
        "void(entity client, string s)",
        "PF_stuffcmd",
        Output,
    ),
    b(
        22,
        "findradius",
        "entity(vector org, float rad)",
        "PF_findradius",
        Pure,
    ),
    b(23, "bprint", "void(string s)", "PF_bprint", Output),
    b(
        24,
        "sprint",
        "void(entity client, string s)",
        "PF_sprint",
        Output,
    ),
    b(25, "dprint", "void(string s)", "PF_dprint", Output),
    b(26, "ftos", "string(float f)", "PF_ftos", Pure),
    b(27, "vtos", "string(vector v)", "PF_vtos", Pure),
    b(28, "coredump", "void()", "PF_coredump", Output),
    b(29, "traceon", "void()", "PF_traceon", Pure),
    b(30, "traceoff", "void()", "PF_traceoff", Pure),
    b(31, "eprint", "void(entity e)", "PF_eprint", Output),
    b(
        32,
        "walkmove",
        "float(float yaw, float dist)",
        "PF_walkmove",
        World,
    ),
    b(
        34,
        "droptofloor",
        "float(float yaw, float dist)",
        "PF_droptofloor",
        World,
    ),
    b(
        35,
        "lightstyle",
        "void(float style, string value)",
        "PF_lightstyle",
        Output,
    ),
    b(36, "rint", "float(float v)", "PF_rint", Pure),
    b(37, "floor", "float(float v)", "PF_floor", Pure),
    b(38, "ceil", "float(float v)", "PF_ceil", Pure),
    b(
        40,
        "checkbottom",
        "float(entity e)",
        "PF_checkbottom",
        World,
    ),
    b(
        41,
        "pointcontents",
        "float(vector v)",
        "PF_pointcontents",
        World,
    ),
    b(43, "fabs", "float(float f)", "PF_fabs", Pure),
    b(44, "aim", "vector(entity e, float speed)", "PF_aim", World),
    b(45, "cvar", "float(string s)", "PF_cvar", State),
    b(46, "localcmd", "void(string s)", "PF_localcmd", Output),
    b(47, "nextent", "entity(entity e)", "PF_nextent", Pure),
    b(
        48,
        "particle",
        "void(vector o, vector d, float color, float count)",
        "PF_particle",
        Output,
    ),
    b(49, "ChangeYaw", "void()", "PF_changeyaw", Pure),
    b(
        51,
        "vectoangles",
        "vector(vector v)",
        "PF_vectoangles",
        Pure,
    ),
    b(
        52,
        "WriteByte",
        "void(float to, float f)",
        "PF_WriteByte",
        Output,
    ),
    b(
        53,
        "WriteChar",
        "void(float to, float f)",
        "PF_WriteChar",
        Output,
    ),
    b(
        54,
        "WriteShort",
        "void(float to, float f)",
        "PF_WriteShort",
        Output,
    ),
    b(
        55,
        "WriteLong",
        "void(float to, float f)",
        "PF_WriteLong",
        Output,
    ),
    b(
        56,
        "WriteCoord",
        "void(float to, float f)",
        "PF_WriteCoord",
        Output,
    ),
    b(
        57,
        "WriteAngle",
        "void(float to, float f)",
        "PF_WriteAngle",
        Output,
    ),
    b(
        58,
        "WriteString",
        "void(float to, string s)",
        "PF_WriteString",
        Output,
    ),
    b(
        59,
        "WriteEntity",
        "void(float to, entity s)",
        "PF_WriteEntity",
        Output,
    ),
    b(67, "movetogoal", "void(float step)", "SV_MoveToGoal", World),
    b(
        68,
        "precache_file",
        "string(string s)",
        "PF_precache_file",
        Pure,
    ),
    b(69, "makestatic", "void(entity e)", "PF_makestatic", Output),
    b(70, "changelevel", "void(string s)", "PF_changelevel", State),
    b(
        72,
        "cvar_set",
        "void(string var, string val)",
        "PF_cvar_set",
        State,
    ),
    b(
        73,
        "centerprint",
        "void(entity client, string s)",
        "PF_centerprint",
        Output,
    ),
    b(
        74,
        "ambientsound",
        "void(vector pos, string samp, float vol, float atten)",
        "PF_ambientsound",
        Output,
    ),
    b(
        75,
        "precache_model2",
        "string(string s)",
        "PF_precache_model",
        State,
    ),
    b(
        76,
        "precache_sound2",
        "string(string s)",
        "PF_precache_sound",
        State,
    ),
    b(
        77,
        "precache_file2",
        "string(string s)",
        "PF_precache_file",
        Pure,
    ),
    b(
        78,
        "setspawnparms",
        "void(entity e)",
        "PF_setspawnparms",
        State,
    ),
];

/// A `const` constructor so [`BUILTINS`] reads as a table.
const fn b(
    number: u32,
    name: &'static str,
    prototype: &'static str,
    c_function: &'static str,
    class: Class,
) -> Builtin {
    Builtin {
        number,
        name,
        prototype,
        c_function,
        class,
    }
}

/// The table entry for `number`, or `None` for a number with no builtin (BUILTIN-1.2).
///
/// A `None` for a number from 1 to 78 is a `PF_Fixme` gap; a `None` for 0 or a number above 78
/// is outside the table (the VM rejects those before dispatch, VM-14.4, LIMIT-5).
#[must_use]
pub fn lookup(number: u32) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|entry| entry.number == number)
}

/// Whether `number` is a `PF_Fixme` gap in 1 to 78 (BUILTIN-1.2).
#[must_use]
pub fn is_fixme(number: u32) -> bool {
    (1..=BUILTIN_MAX_NUMBER).contains(&number) && lookup(number).is_none()
}
