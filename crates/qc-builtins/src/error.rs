// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The errors a builtin raises (BUILTIN-3), each with id's message.

use core::fmt;

use qc_edict::system::{SysField, SysGlobal};

use crate::table::BUILTIN_FIXME_MESSAGE;

/// How a builtin fails, before it is turned into a VM [`RunError`](qc_vm::RunError).
///
/// The kinds mirror BUILTIN-3: a runtime error aborts the run (BUILTIN-3.1), a host error
/// stops the server without resetting the depth (BUILTIN-3.2), and a fatal error ends the run
/// with a typed message (BUILTIN-3.3).
#[derive(Clone, Debug, PartialEq)]
pub enum BuiltinError {
    /// A runtime error, id's `PR_RunError` (BUILTIN-3.1).
    Runtime(RuntimeError),
    /// A host error, id's `Host_Error ("Program error")`; the message is the joined text
    /// QuakeC gave `error` or `objerror` (BUILTIN-3.2, DIFF-8.2).
    Host(String),
    /// A fatal error, id's `Sys_Error` (BUILTIN-3.3).
    Fatal(FatalError),
}

impl BuiltinError {
    /// The message the error carries, byte for byte (DIFF-8.2).
    #[must_use]
    pub fn message(&self) -> String {
        self.to_string()
    }
}

impl From<RuntimeError> for BuiltinError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl From<FatalError> for BuiltinError {
    fn from(error: FatalError) -> Self {
        Self::Fatal(error)
    }
}

impl fmt::Display for BuiltinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Runtime(error) => write!(f, "{error}"),
            Self::Host(message) => f.write_str(message),
            Self::Fatal(error) => write!(f, "{error}"),
        }
    }
}

/// A builtin's runtime error (BUILTIN-3.1), with id's message where id has one.
///
/// Its `Display` is the message byte for byte, as the trace's error record holds it
/// (DIFF-8.2). The last few are this crate's own, for a progs whose globals or fields cannot
/// hold what the builtin reads or writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    /// A call to a number id fills with `PF_Fixme` (BUILTIN-1.2).
    Unimplemented,
    /// `G_EDICT` on a value that does not name an edict (BUILTIN-2.7, DIFF-6.6).
    InvalidEntity,
    /// A string value outside the table and the engine strings (BUILTIN-2.8, DIFF-6.6).
    InvalidString,
    /// A field value outside the edict's fields (BUILTIN-2.8, DIFF-6.6).
    InvalidField,
    /// A lightstyle number outside 0 to 63 (DIFF-6.6).
    InvalidLightstyle,
    /// `setsize` with a minimum above its maximum (BUILTIN-9.2).
    BackwardsBounds,
    /// `setmodel` of a model that is not precached (BUILTIN-9.3).
    NoPrecache {
        /// The model name.
        name: String,
    },
    /// A precache outside the spawn functions (BUILTIN-10.1).
    PrecacheOutsideSpawn,
    /// A precache whose name starts with a space or below (BUILTIN-10.2).
    BadString,
    /// `setspawnparms` of an edict that is not a client (BUILTIN-8.7).
    EntityNotClient,
    /// `sprint`, `centerprint` or `stuffcmd` to an edict that is not a client (BUILTIN-11.4).
    ParmNotClient,
    /// `MSG_ONE` with a `msg_entity` that is not a client (BUILTIN-13.1).
    WriteDestNotClient,
    /// A message destination other than 0 to 3 (BUILTIN-13.1).
    WriteDestBad,
    /// Parameter `number` has no slot, past `OFS_PARM7` (PROGS-18.1).
    NoParameter {
        /// The parameter number.
        number: u16,
    },
    /// A parameter slot past the globals.
    ParameterOutOfRange,
    /// The return slots past the globals.
    ReturnOutOfRange,
    /// A system global the progs' globals cannot hold.
    NoSystemGlobal(SysGlobal),
    /// A scalar system field the edict cannot hold.
    NoField(SysField),
    /// A vector system field the edict cannot hold.
    NoVectorField(SysField),
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unimplemented => f.write_str(BUILTIN_FIXME_MESSAGE),
            Self::InvalidEntity => f.write_str("invalid entity"),
            Self::InvalidString => f.write_str("invalid string"),
            Self::InvalidField => f.write_str("invalid field"),
            Self::InvalidLightstyle => f.write_str("invalid lightstyle"),
            Self::BackwardsBounds => f.write_str("backwards mins/maxs"),
            Self::NoPrecache { name } => writeln!(f, "no precache: {name}"),
            Self::PrecacheOutsideSpawn => {
                f.write_str("PF_Precache_*: Precache can only be done in spawn functions")
            }
            Self::BadString => f.write_str("Bad string"),
            Self::EntityNotClient => f.write_str("Entity is not a client"),
            Self::ParmNotClient => f.write_str("Parm 0 not a client"),
            Self::WriteDestNotClient => f.write_str("WriteDest: not a client"),
            Self::WriteDestBad => f.write_str("WriteDest: bad destination"),
            Self::NoParameter { number } => write!(f, "no parameter {number}"),
            Self::ParameterOutOfRange => f.write_str("builtin parameter out of range"),
            Self::ReturnOutOfRange => f.write_str("return slot out of range"),
            Self::NoSystemGlobal(global) => write!(f, "no system global {}", global.name()),
            Self::NoField(field) => write!(f, "no field {}", field.name()),
            Self::NoVectorField(field) => write!(f, "no vector field {}", field.name()),
        }
    }
}

/// A builtin's fatal error, id's `Sys_Error` (BUILTIN-3.3), with id's message.
///
/// Its `Display` is id's message as `Sys_Error` formats it, byte for byte (DIFF-8.2).
#[derive(Clone, Debug, PartialEq)]
pub enum FatalError {
    /// The `break` builtin (BUILTIN-16.3).
    BreakStatement,
    /// `NUM_FOR_EDICT` of a number at or above `num_edicts` (BUILTIN-2.4, EDICT-1.7).
    BadPointer,
    /// `spawn` with every edict in use (EDICT-6.2).
    NoFreeEdicts,
    /// A `precache_sound` of a new name into a full list.
    SoundIndexOverflow,
    /// A `precache_model` of a new name into a full list.
    ModelIndexOverflow,
    /// `makestatic` of a model that is not precached (BUILTIN-12.8).
    ModelNotPrecached {
        /// The model name.
        name: String,
    },
    /// A `sound` volume outside 0 to 255 (BUILTIN-12.1).
    SoundVolume {
        /// The volume, `vol * 255` as an integer.
        volume: i32,
    },
    /// A `sound` attenuation outside 0 to 4 (BUILTIN-12.1).
    SoundAttenuation {
        /// The attenuation.
        atten: f32,
    },
    /// A `sound` channel outside 0 to 7 (BUILTIN-12.1).
    SoundChannel {
        /// The channel.
        channel: i32,
    },
    /// A message write past the buffer, id's `SZ_GetSpace` (BUILTIN-13.3, DIFF-6.5).
    MessageOverflow,
}

impl fmt::Display for FatalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BreakStatement => f.write_str("break statement"),
            Self::BadPointer => f.write_str("NUM_FOR_EDICT: bad pointer"),
            Self::NoFreeEdicts => f.write_str("ED_Alloc: no free edicts"),
            Self::SoundIndexOverflow => f.write_str("SV_SoundIndex: overflow"),
            Self::ModelIndexOverflow => f.write_str("SV_ModelIndex: overflow"),
            Self::ModelNotPrecached { name } => {
                write!(f, "SV_ModelIndex: model {name} not precached")
            }
            Self::SoundVolume { volume } => write!(f, "SV_StartSound: volume = {volume}"),
            Self::SoundAttenuation { atten } => write!(f, "SV_StartSound: attenuation = {atten}"),
            Self::SoundChannel { channel } => write!(f, "SV_StartSound: channel = {channel}"),
            Self::MessageOverflow => f.write_str("SZ_GetSpace: overflow without allowoverflow set"),
        }
    }
}

impl core::error::Error for BuiltinError {}
