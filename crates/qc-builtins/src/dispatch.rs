// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, pr_builtin, PF_Fixme, PF_makevectors, PF_random, PF_normalize, PF_vlen, PF_vectoyaw, PF_vectoangles, PF_ftos, PF_vtos, PF_rint, PF_floor, PF_ceil, PF_fabs, PF_traceon, PF_traceoff, PF_break
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, PF_Spawn, PF_Remove, PF_Find, PF_nextent, PF_findradius, PF_setspawnparms, PF_setorigin, PF_setsize, PF_setmodel, PF_precache_sound, PF_precache_model, PF_precache_file, PF_bprint, PF_sprint, PF_centerprint, PF_dprint, PF_stuffcmd, PF_localcmd, PF_changelevel, PF_cvar, PF_cvar_set, PF_sound, PF_ambientsound, PF_particle, PF_lightstyle, PF_makestatic, PF_WriteByte, PF_WriteChar, PF_WriteShort, PF_WriteLong, PF_WriteCoord, PF_WriteAngle, PF_WriteString, PF_WriteEntity, PF_error, PF_objerror, PF_coredump, PF_eprint
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/sv_main.c, SV_StartSound, SV_StartParticle

//! [`MockBuiltins`]: the builtin dispatch that runs a progs over a [`qc_vm::Machine`].
//!
//! Implements the dispatch of BUILTIN-1: the VM has checked the number is 1 to
//! [`qc_vm::LIMIT_NUM_BUILTINS`] - 1 and set the argument count (VM-14.1, VM-14.4); this reads
//! each builtin's arguments from the parameter slots and writes its result to `OFS_RETURN` or
//! the system globals (BUILTIN-1.3, BUILTIN-1.4, PROGS-18). A number with no builtin fails with
//! the `PF_Fixme` error (BUILTIN-1.2).
//!
//! The class pure builtins (BUILTIN-4.1) need no engine; the class state, output and world
//! builtins (BUILTIN-4.2 to BUILTIN-4.4) read and write the [`MockEngine`] fixture by the
//! deterministic rules of `differential.md` (DIFF-6, DIFF-7).

use qc_edict::EdictNumber;
use qc_edict::system::{SysField, SysGlobal};
use qc_types::{EntityRef, PROGS_OFS_RETURN, Slot, ofs_parm};
use qc_vm::convert::{double_to_float, float_to_int, int_to_float};
use qc_vm::{Builtins, Context, ErrorKind, RunError};

use crate::error::{BuiltinError, FatalError, RuntimeError};
use crate::math;
use crate::mock::{
    BUILTIN_CHECKCLIENT_INTERVAL, BUILTIN_FL_FLY, BUILTIN_FL_NOTARGET, BUILTIN_FL_ONGROUND,
    BUILTIN_FL_SWIM, LIMIT_MAX_DATAGRAM, LIMIT_MAX_LIGHTSTYLES, MessageDest, MockEngine, Overflow,
    Precache,
};

/// The builtin dispatch over a mock engine (DIFF-6, DIFF-7).
///
/// Holds the mock engine fixture the class state, output and world builtins read and write
/// (DIFF-6, DIFF-7).
#[derive(Clone, Debug)]
pub struct MockBuiltins {
    engine: MockEngine,
}

impl Default for MockBuiltins {
    fn default() -> Self {
        Self {
            engine: MockEngine::new("test", 0, 1),
        }
    }
}

impl MockBuiltins {
    /// A dispatch over the given engine fixture.
    pub const fn with_engine(engine: MockEngine) -> Self {
        Self { engine }
    }

    /// The engine fixture, for reading what a builtin wrote.
    pub const fn engine(&self) -> &MockEngine {
        &self.engine
    }

    /// The engine fixture, for a test to set up state.
    pub const fn engine_mut(&mut self) -> &mut MockEngine {
        &mut self.engine
    }
}

impl Builtins for MockBuiltins {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        match self.dispatch(number, context) {
            Ok(()) => Ok(()),
            Err(error) => Err(to_run_error(context, error)),
        }
    }

    /// The text of the engine-allocated string `value` the mock holds, or `None` (EDICT-13.1,
    /// EDICT-13.3). The mock keeps engine strings by their negative value (`mapname`,
    /// `netname`, the world's `model`, the `ftos`/`vtos` buffer); the VM's string opcodes
    /// resolve a value outside the string table through this (VM-7.3, PROGS-11.6).
    fn engine_string(&self, value: i32) -> Option<&[u8]> {
        self.engine.strings.text(value)
    }
}

impl MockBuiltins {
    /// Dispatches one builtin, returning a [`BuiltinError`] the caller maps to a [`RunError`].
    fn dispatch(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        match number {
            // --- class pure: math and strings (BUILTIN-6, BUILTIN-7) ---
            1 => self.b_makevectors(context),
            7 => self.b_random(context),
            9 => self.b_normalize(context),
            12 => self.b_vlen(context),
            13 => self.b_vectoyaw(context),
            51 => self.b_vectoangles(context),
            36 => self.b_rint(context),
            37 => self.b_floor(context),
            38 => self.b_ceil(context),
            43 => self.b_fabs(context),
            26 => self.b_ftos(context),
            27 => self.b_vtos(context),
            // --- class pure: entities (BUILTIN-8) ---
            14 => self.b_spawn(context),
            15 => self.b_remove(context),
            18 => self.b_find(context),
            22 => self.b_findradius(context),
            47 => self.b_nextent(context),
            78 => self.b_setspawnparms(context),
            // --- class state: placing (BUILTIN-9) ---
            2 => self.b_setorigin(context),
            3 => self.b_setmodel(context),
            4 => self.b_setsize(context),
            // --- class state: precaching (BUILTIN-10) ---
            19 | 76 => self.b_precache(context, Precache::Sound),
            20 | 75 => self.b_precache(context, Precache::Model),
            68 | 77 => self.b_precache_file(context),
            // --- class output: printing, commands (BUILTIN-11) ---
            23 => self.b_bprint(context),
            24 => self.b_sprint(context),
            73 => self.b_centerprint(context),
            25 => self.b_dprint(context),
            21 => self.b_stuffcmd(context),
            46 => self.b_localcmd(context),
            70 => self.b_changelevel(context),
            45 => self.b_cvar(context),
            72 => self.b_cvar_set(context),
            // --- class output: sounds, particles, lightstyles, statics (BUILTIN-12) ---
            8 => self.b_sound(context),
            74 => self.b_ambientsound(context),
            48 => self.b_particle(context),
            35 => self.b_lightstyle(context),
            69 => self.b_makestatic(context),
            // --- class output: network messages (BUILTIN-13) ---
            // WriteByte, WriteChar, WriteShort, WriteLong, WriteCoord, WriteAngle.
            52 | 53 | 57 => self.b_write(context, 1),
            54 | 56 => self.b_write(context, 2),
            55 => self.b_write(context, 4),
            58 => self.b_write_string(context),
            59 => self.b_write_entity(context),
            // --- class world: answered by DIFF-7 ---
            16 => self.b_traceline(context),
            17 => self.b_checkclient(context),
            41 => self.b_pointcontents(context),
            40 => self.b_checkbottom(context),
            44 => self.b_aim(context),
            34 => self.b_droptofloor(context),
            32 => self.b_walkmove(context),
            67 => self.b_movetogoal(context),
            // --- class pure: errors and debugging (BUILTIN-16) ---
            6 => Err(BuiltinError::from(FatalError::BreakStatement)),
            10 => self.b_error(context),
            11 => self.b_objerror(context),
            28 | 31 => Ok(()), // coredump, eprint: console only (BUILTIN-16.5)
            29 => self.b_traceon(context),
            30 => self.b_traceoff(context),
            // 49 ChangeYaw is a pure movement builtin handled with movement.
            49 => self.b_changeyaw(context),
            // A number with no builtin is a PF_Fixme gap (BUILTIN-1.2).
            _ => Err(BuiltinError::from(RuntimeError::Unimplemented)),
        }
    }
}

// ===== class pure: math and strings (BUILTIN-6, BUILTIN-7) =====

impl MockBuiltins {
    fn b_makevectors(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let ang = read_parm_vector(context, 0)?;
        let (forward, right, up) = math::makevectors(ang);
        write_system_vector(context, SysGlobal::VForward, forward)?;
        write_system_vector(context, SysGlobal::VRight, right)?;
        write_system_vector(context, SysGlobal::VUp, up)?;
        Ok(())
    }

    fn b_random(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let rand_value = self.engine.rng.next_rand();
        write_return_float(context, math::random(rand_value))
    }

    fn b_normalize(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let v = read_parm_vector(context, 0)?;
        write_return_vector(context, math::normalize(v))
    }

    fn b_vlen(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let v = read_parm_vector(context, 0)?;
        write_return_float(context, math::vlen(v))
    }

    fn b_vectoyaw(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let v = read_parm_vector(context, 0)?;
        write_return_float(context, math::vectoyaw(v))
    }

    fn b_vectoangles(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let v = read_parm_vector(context, 0)?;
        write_return_vector(context, math::vectoangles(v))
    }

    fn b_rint(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let f = read_parm_float(context, 0)?;
        write_return_float(context, math::rint(f))
    }

    fn b_floor(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let f = read_parm_float(context, 0)?;
        write_return_float(context, math::floor(f))
    }

    fn b_ceil(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let f = read_parm_float(context, 0)?;
        write_return_float(context, math::ceil(f))
    }

    fn b_fabs(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let f = read_parm_float(context, 0)?;
        write_return_float(context, math::fabs(f))
    }

    /// `ftos(f)` (BUILTIN-7.1): the text goes in the one engine buffer, whose value is returned.
    fn b_ftos(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let f = read_parm_float(context, 0)?;
        let value = self.engine.strings.set_temp(math::ftos(f).into_bytes());
        write_return_string(context, value)
    }

    /// `vtos(v)` (BUILTIN-7.2): the text goes in the one engine buffer, whose value is returned.
    fn b_vtos(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let v = read_parm_vector(context, 0)?;
        let value = self.engine.strings.set_temp(math::vtos(v).into_bytes());
        write_return_string(context, value)
    }

    /// `traceon()` (BUILTIN-16.4): sets the VM's trace flag, which the trace does not show
    /// (DIFF-6.7).
    fn b_traceon(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        context.machine().set_tracing(true);
        Ok(())
    }

    /// `traceoff()` (BUILTIN-16.4): clears the VM's trace flag.
    fn b_traceoff(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        context.machine().set_tracing(false);
        Ok(())
    }
}

// ===== argument reading, checks and result writing (BUILTIN-2, PROGS-18) =====

/// Reads parameter `n`'s slot (`OFS_PARM0` + 3n), bounds-checked (VM-1.4).
fn read_parm(context: &mut Context<'_, '_>, n: u16) -> Result<Slot, BuiltinError> {
    let slot = parm_slot(n)?;
    context
        .machine()
        .global(slot)
        .map_err(|_| BuiltinError::from(RuntimeError::ParameterOutOfRange))
}

/// Reads parameter `n` as a float.
fn read_parm_float(context: &mut Context<'_, '_>, n: u16) -> Result<f32, BuiltinError> {
    Ok(read_parm(context, n)?.as_f32())
}

/// Reads parameter `n` as an integer, id's `G_FLOAT` then `(int)` (BUILTIN-2.1, BUILTIN-2.2).
fn read_parm_int(context: &mut Context<'_, '_>, n: u16) -> Result<i32, BuiltinError> {
    Ok(float_to_int(read_parm_float(context, n)?))
}

/// Reads parameter `n` as a vector from its three slots, bounds-checked (VM-1.4).
fn read_parm_vector(context: &mut Context<'_, '_>, n: u16) -> Result<[f32; 3], BuiltinError> {
    let slot = parm_slot(n)?;
    let slots = context
        .machine()
        .global3(slot)
        .map_err(|_| BuiltinError::from(RuntimeError::ParameterOutOfRange))?;
    Ok(slots.map(Slot::as_f32))
}

/// Reads the raw bits of parameter `n`, for an entity, string or field value.
fn read_parm_raw(context: &mut Context<'_, '_>, n: u16) -> Result<i32, BuiltinError> {
    Ok(read_parm(context, n)?.as_i32())
}

/// The runtime error for an entity value that names no edict (BUILTIN-2.7, DIFF-6.6).
fn invalid_entity() -> BuiltinError {
    BuiltinError::from(RuntimeError::InvalidEntity)
}

/// The runtime error for a string value that is neither in the table nor an engine string
/// (BUILTIN-2.5, DIFF-6.6).
fn invalid_string() -> BuiltinError {
    BuiltinError::from(RuntimeError::InvalidString)
}

/// The runtime error for a field index outside the progs' fields (BUILTIN-8.3, DIFF-6.6).
fn invalid_field() -> BuiltinError {
    BuiltinError::from(RuntimeError::InvalidField)
}

/// The fatal error `NUM_FOR_EDICT: bad pointer` for an entity at or above `num_edicts`
/// (BUILTIN-2.4).
fn num_for_edict_bad() -> BuiltinError {
    BuiltinError::from(FatalError::BadPointer)
}

/// Checks an entity value as `G_EDICT`/`PROG_TO_EDICT` do (BUILTIN-2.7): a value that names no
/// edict (EDICT-1.9) is `invalid entity`; a value that names one is returned as its number.
///
/// Unlike [`entity_num_for`] this accepts an edict at or above `num_edicts` and a free one
/// (BUILTIN-2.7).
fn checked_edict(context: &mut Context<'_, '_>, value: i32) -> Result<EdictNumber, BuiltinError> {
    let edict_size = context.machine().edicts().layout().edict_size();
    EntityRef::from_bits(value)
        .number(edict_size)
        .and_then(EdictNumber::from_i32)
        .ok_or_else(invalid_entity)
}

/// Reads entity parameter `n` with the `G_EDICT` check (BUILTIN-2.7).
fn read_parm_edict(context: &mut Context<'_, '_>, n: u16) -> Result<EdictNumber, BuiltinError> {
    let value = read_parm_raw(context, n)?;
    checked_edict(context, value)
}

/// Turns an entity value into its number as `G_EDICTNUM`/`NUM_FOR_EDICT` do (BUILTIN-2.4,
/// BUILTIN-2.9): the `G_EDICT` check first (`invalid entity`), then `NUM_FOR_EDICT` (the fatal
/// error when the number is at or above `num_edicts`).
fn entity_num_for(context: &mut Context<'_, '_>, value: i32) -> Result<EdictNumber, BuiltinError> {
    let number = checked_edict(context, value)?;
    if number.index() >= context.machine().edicts().num_edicts() {
        return Err(num_for_edict_bad());
    }
    Ok(number)
}

/// Reads entity parameter `n` with the `G_EDICTNUM` check (BUILTIN-2.4).
fn read_parm_edictnum(context: &mut Context<'_, '_>, n: u16) -> Result<EdictNumber, BuiltinError> {
    let value = read_parm_raw(context, n)?;
    entity_num_for(context, value)
}

/// Resolves a string value to its text bytes, with the string check (BUILTIN-2.5, BUILTIN-2.8).
///
/// Valid when it is 0 to `numstrings - 1` (a string-table offset, PROGS-11.3) or exactly an
/// engine-made string's value (EDICT-13.1); every other value is `invalid string`.
fn resolve_string(
    context: &Context<'_, '_>,
    engine: &MockEngine,
    value: i32,
) -> Result<Vec<u8>, BuiltinError> {
    if value >= 0 {
        // A string-table offset (BUILTIN-2.8): read up to its NUL (PROGS-11.1).
        return context
            .progs()
            .string(value)
            .map(<[u8]>::to_vec)
            .ok_or_else(invalid_string);
    }
    // A negative value is an engine-made string in the mock (EDICT-13.3).
    engine
        .strings
        .text(value)
        .map(<[u8]>::to_vec)
        .ok_or_else(invalid_string)
}

/// Reads string parameter `n` as its text, with the string check (BUILTIN-2.5, BUILTIN-2.8).
fn read_parm_string(
    context: &mut Context<'_, '_>,
    engine: &MockEngine,
    n: u16,
) -> Result<Vec<u8>, BuiltinError> {
    let value = read_parm_raw(context, n)?;
    resolve_string(context, engine, value)
}

/// The joined text of every string parameter from `first` to the last the call supplied
/// (BUILTIN-7.3); each is checked (BUILTIN-2.5). Argument count decides how many (VM-14.1).
fn joined_string(
    context: &mut Context<'_, '_>,
    engine: &MockEngine,
    first: u16,
) -> Result<Vec<u8>, BuiltinError> {
    let argc = context.machine().argc();
    let mut text = Vec::new();
    let last = u16::try_from(argc).unwrap_or(u16::MAX);
    for n in first..last {
        let value = read_parm_raw(context, n)?;
        let part = resolve_string(context, engine, value)?;
        text.extend_from_slice(&part);
    }
    Ok(text)
}

/// A field value's slot, checked to be a field slot of the progs (BUILTIN-8.3): `invalid
/// field` outside 0 to `entityfields - 1`.
fn checked_field(context: &mut Context<'_, '_>, value: i32) -> Result<usize, BuiltinError> {
    let fields = context.machine().edicts().layout().entityfields();
    match usize::try_from(value) {
        Ok(slot) if slot < fields => Ok(slot),
        _ => Err(invalid_field()),
    }
}

/// Writes a result to `OFS_RETURN` (BUILTIN-1.4, PROGS-18).
fn write_return(context: &mut Context<'_, '_>, value: Slot) -> Result<(), BuiltinError> {
    context
        .machine()
        .set_global(i32::from(PROGS_OFS_RETURN), value)
        .map_err(|_| BuiltinError::from(RuntimeError::ReturnOutOfRange))
}

/// Writes a float result to `OFS_RETURN`.
fn write_return_float(context: &mut Context<'_, '_>, value: f32) -> Result<(), BuiltinError> {
    write_return(context, Slot::from_f32(value))
}

/// Writes an integer-valued result (an entity, string or field value) to `OFS_RETURN`.
fn write_return_int(context: &mut Context<'_, '_>, value: i32) -> Result<(), BuiltinError> {
    write_return(context, Slot::from_i32(value))
}

/// Writes a string value to `OFS_RETURN` (BUILTIN-7.4, BUILTIN-10.2).
fn write_return_string(context: &mut Context<'_, '_>, value: i32) -> Result<(), BuiltinError> {
    write_return_int(context, value)
}

/// Writes an entity value (the edict's value) to `OFS_RETURN`.
fn write_return_edict(context: &mut Context<'_, '_>, number: i32) -> Result<(), BuiltinError> {
    let value = context
        .machine()
        .edicts()
        .edict_num(number)
        .map_err(|_| invalid_entity())?;
    write_return(context, Slot::from(value))
}

/// Writes a vector result to `OFS_RETURN` (BUILTIN-1.4, PROGS-18).
fn write_return_vector(context: &mut Context<'_, '_>, value: [f32; 3]) -> Result<(), BuiltinError> {
    context
        .machine()
        .set_global3(i32::from(PROGS_OFS_RETURN), value.map(Slot::from_f32))
        .map_err(|_| BuiltinError::from(RuntimeError::ReturnOutOfRange))
}

/// Reads system global `global`.
fn read_system_global(
    context: &mut Context<'_, '_>,
    global: SysGlobal,
) -> Result<Slot, BuiltinError> {
    context
        .machine()
        .global(global.ofs())
        .map_err(|_| BuiltinError::from(RuntimeError::NoSystemGlobal(global)))
}

/// Writes system global `global`.
fn write_system_global(
    context: &mut Context<'_, '_>,
    global: SysGlobal,
    value: Slot,
) -> Result<(), BuiltinError> {
    context
        .machine()
        .set_global(global.ofs(), value)
        .map_err(|_| BuiltinError::from(RuntimeError::NoSystemGlobal(global)))
}

/// Writes a float to system global `global`.
fn write_system_global_float(
    context: &mut Context<'_, '_>,
    global: SysGlobal,
    value: f32,
) -> Result<(), BuiltinError> {
    write_system_global(context, global, Slot::from_f32(value))
}

/// The entity value in system global `global` (`self`, `msg_entity`).
fn read_system_global_entity(
    context: &mut Context<'_, '_>,
    global: SysGlobal,
) -> Result<i32, BuiltinError> {
    Ok(read_system_global(context, global)?.as_i32())
}

/// Reads vector system global `global` (`v_forward`).
fn read_system_vector(
    context: &mut Context<'_, '_>,
    global: SysGlobal,
) -> Result<[f32; 3], BuiltinError> {
    let slots = context
        .machine()
        .global3(global.ofs())
        .map_err(|_| BuiltinError::from(RuntimeError::NoSystemGlobal(global)))?;
    Ok(slots.map(Slot::as_f32))
}

/// Writes vector system global `global` (`v_forward`, `v_right`, `v_up`) (BUILTIN-6.1).
fn write_system_vector(
    context: &mut Context<'_, '_>,
    global: SysGlobal,
    value: [f32; 3],
) -> Result<(), BuiltinError> {
    context
        .machine()
        .set_global3(global.ofs(), value.map(Slot::from_f32))
        .map_err(|_| BuiltinError::from(RuntimeError::NoSystemGlobal(global)))
}

/// Reads scalar field `field` of edict `number`.
fn read_field(
    context: &mut Context<'_, '_>,
    number: EdictNumber,
    field: SysField,
) -> Result<Slot, BuiltinError> {
    context
        .machine()
        .edicts()
        .field(number, field.slot())
        .ok_or_else(|| BuiltinError::from(RuntimeError::NoField(field)))
}

/// Reads float field `field` of edict `number`.
fn read_field_float(
    context: &mut Context<'_, '_>,
    number: EdictNumber,
    field: SysField,
) -> Result<f32, BuiltinError> {
    Ok(read_field(context, number, field)?.as_f32())
}

/// Reads an integer-valued field (an entity, string or function value) of edict `number`.
fn read_field_int(
    context: &mut Context<'_, '_>,
    number: EdictNumber,
    field: SysField,
) -> Result<i32, BuiltinError> {
    Ok(read_field(context, number, field)?.as_i32())
}

/// Reads vector field `field` of edict `number`: all three slots, or the error.
fn read_field_vector(
    context: &mut Context<'_, '_>,
    number: EdictNumber,
    field: SysField,
) -> Result<[f32; 3], BuiltinError> {
    let edicts = context.machine().edicts();
    let slot = field.slot();
    let mut value = [0.0; 3];
    for (offset, cell) in value.iter_mut().enumerate() {
        *cell = slot
            .checked_add(offset)
            .and_then(|s| edicts.field(number, s))
            .ok_or_else(|| BuiltinError::from(RuntimeError::NoVectorField(field)))?
            .as_f32();
    }
    Ok(value)
}

/// Writes scalar field `field` of edict `number`.
fn write_field(
    context: &mut Context<'_, '_>,
    number: EdictNumber,
    field: SysField,
    value: Slot,
) -> Result<(), BuiltinError> {
    context
        .machine()
        .edicts_mut()
        .set_field(number, field.slot(), value)
        .map_err(|_| BuiltinError::from(RuntimeError::NoField(field)))
}

/// Writes float field `field` of edict `number`.
fn write_field_float(
    context: &mut Context<'_, '_>,
    number: EdictNumber,
    field: SysField,
    value: f32,
) -> Result<(), BuiltinError> {
    write_field(context, number, field, Slot::from_f32(value))
}

/// Writes an integer-valued field (a string or entity value) of edict `number`.
fn write_field_int(
    context: &mut Context<'_, '_>,
    number: EdictNumber,
    field: SysField,
    value: i32,
) -> Result<(), BuiltinError> {
    write_field(context, number, field, Slot::from_i32(value))
}

/// Writes vector field `field` of edict `number`: each slot that exists is written, and a
/// missing one is the error once all three are tried.
fn write_field_vector(
    context: &mut Context<'_, '_>,
    number: EdictNumber,
    field: SysField,
    value: [f32; 3],
) -> Result<(), BuiltinError> {
    let slot = field.slot();
    let edicts = context.machine().edicts_mut();
    let mut written = true;
    for (offset, component) in value.into_iter().enumerate() {
        written &= slot.checked_add(offset).is_some_and(|s| {
            edicts
                .set_field(number, s, Slot::from_f32(component))
                .is_ok()
        });
    }
    if written {
        Ok(())
    } else {
        Err(BuiltinError::from(RuntimeError::NoVectorField(field)))
    }
}

/// The global slot of parameter `n`, id's `OFS_PARM0 + 3n` (PROGS-18.1).
fn parm_slot(n: u16) -> Result<i32, BuiltinError> {
    let offset =
        ofs_parm(n).ok_or_else(|| BuiltinError::from(RuntimeError::NoParameter { number: n }))?;
    Ok(i32::from(offset))
}

/// Turns a [`BuiltinError`] into the [`RunError`] the VM carries (BUILTIN-3, DIFF-8.2).
///
/// Each kind keeps its class, with its verbatim message (the trace compares the bytes, DIFF-6.6,
/// DIFF-8.2): a runtime error, and an engine-only builtin (BUILTIN-4.4), become
/// [`ErrorKind::Builtin`], which resets the call depth (BUILTIN-3.1, VM-19.1); a host error
/// becomes [`ErrorKind::Host`], which keeps it (BUILTIN-3.2, DIFF-8.4); a fatal error becomes
/// [`ErrorKind::Fatal`], after which a host runs no shutdown (BUILTIN-3.3, DIFF-8.6).
fn to_run_error(context: &Context<'_, '_>, error: BuiltinError) -> RunError {
    let kind = match error {
        BuiltinError::Host(message) => ErrorKind::Host { message },
        BuiltinError::Fatal(fatal) => ErrorKind::Fatal {
            message: fatal.to_string(),
        },
        BuiltinError::Runtime(runtime) => ErrorKind::Builtin {
            message: runtime.to_string(),
        },
    };
    context.run_error(kind)
}

// ===== class pure: entities (BUILTIN-8) =====

impl MockBuiltins {
    /// `spawn()` (BUILTIN-8.1): id's `ED_Alloc`, failing fatally when none is left (EDICT-6.2).
    fn b_spawn(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let now = self.engine.clock;
        let number = context
            .machine()
            .edicts_mut()
            .alloc(now)
            .map_err(|_| BuiltinError::from(FatalError::NoFreeEdicts))?;
        let value = number.to_i32().ok_or_else(invalid_entity)?;
        write_return_edict(context, value)
    }

    /// `remove(e)` (BUILTIN-8.2): id's `ED_Free`; it refuses nothing (EDICT-7.4). `e` is read
    /// with `G_EDICT` (BUILTIN-2.7), not `G_EDICTNUM`, so a free edict passes.
    fn b_remove(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let number = read_parm_edict(context, 0)?;
        let now = self.engine.clock;
        context.machine().edicts_mut().free(number, now);
        Ok(())
    }

    /// `find(start, fld, match)` (BUILTIN-8.3, BUILTIN-8.8): the first edict after `start`
    /// whose string field `fld` equals `match`, or the world.
    fn b_find(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        // BUILTIN-8.8: read start (G_EDICTNUM), then match, then, per examined edict, the field
        // (checking fld and the value). An examined-nothing find checks neither.
        let start = read_parm_edictnum(context, 0)?;
        let wanted = read_parm_string(context, &self.engine, 2)?;
        // The raw field value is read now, but checked only when an edict is examined.
        let field_value = read_parm_raw(context, 1)?;
        let num_edicts = context.machine().edicts().num_edicts();
        for number in (start.index().saturating_add(1)..num_edicts).map(EdictNumber::new) {
            let is_free = context.machine().edicts().is_free(number).unwrap_or(true);
            if !is_free {
                // BUILTIN-8.8: fld is checked (invalid field) at the first examined edict.
                let field = checked_field(context, field_value)?;
                // E_STRING: the field's value is read and checked (BUILTIN-8.8, BUILTIN-2.8).
                let value = context
                    .machine()
                    .edicts()
                    .field(number, field)
                    .map(Slot::as_i32)
                    .ok_or_else(invalid_field)?;
                let text = resolve_string(context, &self.engine, value)?;
                if text == wanted {
                    let found = number.to_i32().ok_or_else(invalid_entity)?;
                    return write_return_edict(context, found);
                }
            }
        }
        write_return_edict(context, 0)
    }

    /// `nextent(e)` (BUILTIN-8.4): the first non-free edict after `e`, or the world. `e` is
    /// read with `G_EDICTNUM` (BUILTIN-2.4).
    fn b_nextent(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let e = read_parm_edictnum(context, 0)?;
        let num_edicts = context.machine().edicts().num_edicts();
        for number in (e.index().saturating_add(1)..num_edicts).map(EdictNumber::new) {
            let is_free = context.machine().edicts().is_free(number).unwrap_or(true);
            if !is_free {
                let next = number.to_i32().ok_or_else(invalid_entity)?;
                return write_return_edict(context, next);
            }
        }
        write_return_edict(context, 0)
    }

    /// `findradius(org, rad)` (BUILTIN-8.5, BUILTIN-8.6): links the solid edicts within `rad`
    /// through their `chain` field, newest first, and returns the head.
    fn b_findradius(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let org = read_parm_vector(context, 0)?;
        let rad = read_parm_float(context, 1)?;
        let num_edicts = context.machine().edicts().num_edicts();
        let mut chain = 0_i32; // the world
        for number in (1..num_edicts).map(EdictNumber::new) {
            let is_free = context.machine().edicts().is_free(number).unwrap_or(true);
            let solid = read_field_float(context, number, SysField::Solid).unwrap_or(0.0);
            // BUILTIN-8.5: skip free edicts and those whose solid is SOLID_NOT (0).
            if !is_free && solid != 0.0 {
                let origin = read_field_vector(context, number, SysField::Origin)?;
                let mins = read_field_vector(context, number, SysField::Mins)?;
                let maxs = read_field_vector(context, number, SysField::Maxs)?;
                let d = radius_delta(org, origin, mins, maxs);
                // BUILTIN-8.5: included unless the length is greater than rad (NaN excludes).
                if math::vlen(d) <= rad {
                    let this = number.to_i32().ok_or_else(invalid_entity)?;
                    let value = context
                        .machine()
                        .edicts()
                        .edict_num(this)
                        .map_err(|_| invalid_entity())?;
                    write_field_int(context, number, SysField::Chain, chain)?;
                    chain = value.bits();
                }
            }
        }
        write_return_int(context, chain)
    }

    /// `setspawnparms(e)` (BUILTIN-8.7): copies the client slot's 16 saved spawn parms to the
    /// globals `parm1` to `parm16`. In the mock the slots hold no saved parms, so it copies the
    /// globals to themselves: the check is what QuakeC observes, and that is the client-slot
    /// range error.
    fn b_setspawnparms(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let number = read_parm_edictnum(context, 0)?;
        if !context.machine().edicts().is_client(number) {
            return Err(BuiltinError::from(RuntimeError::EntityNotClient));
        }
        // The mock keeps no per-slot spawn parms, so parm1..parm16 are left unchanged
        // (EDICT-11.9): nothing QuakeC can observe differs from id with empty slots.
        Ok(())
    }
}

/// The per-axis delta `findradius` measures (BUILTIN-8.5): `org - (origin + (mins+maxs)*0.5)`,
/// with `mins + maxs` in f32 and the rest in double, rounded to f32.
fn radius_delta(org: [f32; 3], origin: [f32; 3], mins: [f32; 3], maxs: [f32; 3]) -> [f32; 3] {
    let delta = |o: f32, orig: f32, mn: f32, mx: f32| {
        let half = mn + mx; // f32 sum (BUILTIN-8.5)
        double_to_float(f64::from(o) - (f64::from(orig) + f64::from(half) * 0.5))
    };
    [
        delta(org[0], origin[0], mins[0], maxs[0]),
        delta(org[1], origin[1], mins[1], maxs[1]),
        delta(org[2], origin[2], mins[2], maxs[2]),
    ]
}

// ===== class state: placing entities (BUILTIN-9) =====

/// `FL_ITEM`, which widens the link box by 15 on x and y (EDICT-12.2).
const BUILTIN_FL_ITEM: i32 = 256;

impl MockBuiltins {
    /// Links `number` into the area tree without touching triggers, writing `absmin`/`absmax`
    /// (EDICT-12.2, DIFF-5.7). Not for the world (0) or a free edict.
    fn link_edict(
        &mut self,
        context: &mut Context<'_, '_>,
        number: EdictNumber,
    ) -> Result<(), BuiltinError> {
        if number == EdictNumber::WORLD {
            return Ok(());
        }
        if context.machine().edicts().is_free(number).unwrap_or(true) {
            return Ok(());
        }
        let origin = read_field_vector(context, number, SysField::Origin)?;
        let mins = read_field_vector(context, number, SysField::Mins)?;
        let maxs = read_field_vector(context, number, SysField::Maxs)?;
        let flags = float_to_int(read_field_float(context, number, SysField::Flags).unwrap_or(0.0));
        let (wx, wz) = if flags & BUILTIN_FL_ITEM != 0 {
            (15.0_f32, 0.0_f32)
        } else {
            (1.0, 1.0)
        };
        let widen = [wx, wx, wz];
        let absmin = [
            origin[0] + mins[0] - widen[0],
            origin[1] + mins[1] - widen[1],
            origin[2] + mins[2] - widen[2],
        ];
        let absmax = [
            origin[0] + maxs[0] + widen[0],
            origin[1] + maxs[1] + widen[1],
            origin[2] + maxs[2] + widen[2],
        ];
        write_field_vector(context, number, SysField::Absmin, absmin)?;
        write_field_vector(context, number, SysField::Absmax, absmax)
    }

    /// `setorigin(e, o)` (BUILTIN-9.1): set `e`'s `origin` to `o` and link it.
    fn b_setorigin(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let number = read_parm_edict(context, 0)?;
        let origin = read_parm_vector(context, 1)?;
        write_field_vector(context, number, SysField::Origin, origin)?;
        self.link_edict(context, number)
    }

    /// `setsize(e, min, max)` (BUILTIN-9.2): checks `min <= max` per axis, then sets `mins`,
    /// `maxs`, `size` and links.
    fn b_setsize(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let number = read_parm_edict(context, 0)?;
        let min = read_parm_vector(context, 1)?;
        let max = read_parm_vector(context, 2)?;
        // BUILTIN-9.2: fail before writing anything when min > max on any axis; NaN passes.
        if min.iter().zip(&max).any(|(a, b)| a > b) {
            return Err(BuiltinError::from(RuntimeError::BackwardsBounds));
        }
        set_size(context, number, min, max)?;
        self.link_edict(context, number)
    }

    /// `setmodel(e, m)` (BUILTIN-9.3, BUILTIN-9.4): fail when `m` is not precached; else set
    /// `model`, `modelindex` and the size from the model's bounds.
    fn b_setmodel(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        // BUILTIN-2.9: e is read before m.
        let number = read_parm_edict(context, 0)?;
        let model_value = read_parm_raw(context, 1)?;
        let model_text = resolve_string(context, &self.engine, model_value)?;
        let name = String::from_utf8_lossy(&model_text).into_owned();
        let Some(index) = self.engine.precache_index(Precache::Model, &name) else {
            return Err(BuiltinError::from(RuntimeError::NoPrecache { name }));
        };
        // BUILTIN-9.4: model is the string value itself (BUILTIN-2.6), not the list's copy.
        write_field_int(context, number, SysField::Model, model_value)?;
        let index_f = int_to_float(i32::try_from(index).unwrap_or(0));
        write_field_float(context, number, SysField::Modelindex, index_f)?;
        let bounds = self.engine.model_bounds_for(&name);
        // Set the size as setsize does, from the model's bounds (these never go backwards).
        set_size(context, number, bounds.mins, bounds.maxs)?;
        self.link_edict(context, number)
    }
}

// ===== class state: precaching (BUILTIN-10) =====

impl MockBuiltins {
    /// Whether the first byte of `text`, read as a signed char, is a space or below (BUILTIN-10.2).
    fn bad_precache_string(text: &[u8]) -> bool {
        match text.first() {
            None => true,
            Some(&byte) => byte >= 0x80 || byte <= b' ',
        }
    }

    /// `precache_sound(s)` and `precache_model(s)` (BUILTIN-10.1 to BUILTIN-10.4):
    /// loading-only, returns `s`, adds it to the list; the mock never fails to load a model
    /// (DIFF-6.3).
    fn b_precache(
        &mut self,
        context: &mut Context<'_, '_>,
        kind: Precache,
    ) -> Result<(), BuiltinError> {
        if !matches!(context.server(), qc_vm::ServerState::Loading) {
            return Err(BuiltinError::from(RuntimeError::PrecacheOutsideSpawn));
        }
        let value = read_parm_raw(context, 0)?;
        let text = resolve_string(context, &self.engine, value)?;
        write_return_string(context, value)?;
        if Self::bad_precache_string(&text) {
            return Err(BuiltinError::from(RuntimeError::BadString));
        }
        let name = String::from_utf8_lossy(&text).into_owned();
        self.engine.precache(kind, &name).map_err(|Overflow| {
            BuiltinError::from(match kind {
                Precache::Sound => FatalError::SoundIndexOverflow,
                Precache::Model => FatalError::ModelIndexOverflow,
            })
        })
    }

    /// `precache_file(s)` (BUILTIN-10.5): returns `s` and does nothing else, at any time.
    fn b_precache_file(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        // BUILTIN-2.9: precache_file reads no string, so a bad value returns without an error.
        let value = read_parm_raw(context, 0)?;
        write_return_string(context, value)
    }
}

// ===== class output: printing, commands and settings (BUILTIN-11) =====

impl MockBuiltins {
    /// `bprint(s)` (BUILTIN-11.1): `svc_print` and the joined text with its NUL to every active
    /// spawned client's reliable message, which the mock does not keep (DIFF-6.5).
    fn b_bprint(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        // The joined text is read (and checked) even though the reliable message is not kept.
        let _text = joined_string(context, &self.engine, 0)?;
        Ok(())
    }

    /// `sprint(client, s)` (BUILTIN-11.2): to one client's reliable message, not kept; a
    /// non-client prints a console message and returns.
    fn b_sprint(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        self.print_to_client(context)
    }

    /// `centerprint(client, s)` (BUILTIN-11.2): as `sprint` with `svc_centerprint`.
    fn b_centerprint(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        self.print_to_client(context)
    }

    /// The shared body of `sprint` and `centerprint` (BUILTIN-11.2).
    fn print_to_client(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let client = read_parm_edictnum(context, 0)?;
        if !context.machine().edicts().is_client(client) {
            // BUILTIN-11.2: a console message, then return; not an error.
            return Ok(());
        }
        // The joined text from PARM1 is read and checked; the reliable message is not kept.
        let _text = joined_string(context, &self.engine, 1)?;
        Ok(())
    }

    /// `dprint(s)` (BUILTIN-11.3): console only when `developer` is set; the mock has no
    /// `developer` setting, so it only reads and checks the text.
    fn b_dprint(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let _text = joined_string(context, &self.engine, 0)?;
        Ok(())
    }

    /// `stuffcmd(client, s)` (BUILTIN-11.4): a non-client is the runtime error `Parm 0 not a
    /// client`; otherwise the reliable message gets `svc_stufftext` and `s`, not kept.
    fn b_stuffcmd(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let client = read_parm_edictnum(context, 0)?;
        if !context.machine().edicts().is_client(client) {
            return Err(BuiltinError::from(RuntimeError::ParmNotClient));
        }
        let _text = read_parm_string(context, &self.engine, 1)?;
        Ok(())
    }

    /// `localcmd(s)` (BUILTIN-11.5): append `s` (no NUL) to the command buffer; an overflow is
    /// a console message and `s` is dropped, no error.
    fn b_localcmd(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let text = read_parm_string(context, &self.engine, 0)?;
        // The overflow is silent (BUILTIN-11.5): the text is dropped and the call succeeds.
        self.engine.append_command(&text).or(Ok(()))
    }

    /// `changelevel(s)` (BUILTIN-11.6): once per map; records it and appends `changelevel <s>`
    /// and a newline to the command buffer.
    fn b_changelevel(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        // BUILTIN-2.9: once a changelevel was issued it reads no string, so no string check.
        if self.engine.changelevel_issued {
            return Ok(());
        }
        let text = read_parm_string(context, &self.engine, 0)?;
        self.engine.changelevel_issued = true;
        let mut command = b"changelevel ".to_vec();
        command.extend_from_slice(&text);
        command.push(b'\n');
        // As localcmd, an overflow drops the command silently (BUILTIN-11.5).
        self.engine.append_command(&command).or(Ok(()))
    }

    /// `cvar(s)` (BUILTIN-11.7): the value of setting `s`, or 0 when there is none.
    fn b_cvar(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let name_bytes = read_parm_string(context, &self.engine, 0)?;
        let name = String::from_utf8_lossy(&name_bytes).into_owned();
        let value = self.engine.cvars.text(&name).map_or(0.0, math::q_atof);
        write_return_float(context, value)
    }

    /// `cvar_set(var, val)` (BUILTIN-11.8): set the setting; an unknown name is a console
    /// message and changes nothing.
    fn b_cvar_set(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let var_bytes = read_parm_string(context, &self.engine, 0)?;
        let val_bytes = read_parm_string(context, &self.engine, 1)?;
        let var = String::from_utf8_lossy(&var_bytes).into_owned();
        let val = String::from_utf8_lossy(&val_bytes).into_owned();
        // An unknown name is a console message, not an error (BUILTIN-11.8).
        let _existed = self.engine.cvars.set(&var, &val);
        Ok(())
    }
}

// ===== class output: sounds, particles, lightstyles, statics (BUILTIN-12) =====

/// `MAX_DATAGRAM - 16`, the slack a sound or particle needs free (BUILTIN-12.2, BUILTIN-12.5).
const BUILTIN_DATAGRAM_SLACK: usize = 16;
/// Bytes a `sound` message occupies with both flags set (BUILTIN-12.3): svc, flags, volume,
/// attenuation, a short channel, the sound byte and three coordinates.
const BUILTIN_SOUND_MAX_BYTES: usize = 1 + 1 + 1 + 1 + 2 + 1 + 6;
/// Bytes a `particle` message occupies (BUILTIN-12.5): svc, three coords, three direction
/// chars, count and color.
const BUILTIN_PARTICLE_BYTES: usize = 1 + 6 + 3 + 1 + 1;
/// Bytes an `ambientsound` message occupies (BUILTIN-12.4): svc, three coords, index, volume,
/// attenuation.
const BUILTIN_AMBIENTSOUND_BYTES: usize = 1 + 6 + 1 + 1 + 1;
/// Bytes a `makestatic` message occupies (BUILTIN-12.8): svc, modelindex, frame, colormap,
/// skin and three (coord, angle) pairs.
const BUILTIN_MAKESTATIC_BYTES: usize = 1 + 1 + 1 + 1 + 1 + 3 * (2 + 1);
/// `DEFAULT_SOUND_PACKET_VOLUME` (BUILTIN-12.3).
const BUILTIN_DEFAULT_SOUND_VOLUME: i32 = 255;

impl MockBuiltins {
    /// `sound(e, chan, samp, vol, atten)` (BUILTIN-12.1 to BUILTIN-12.3).
    fn b_sound(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let chan = read_parm_int(context, 1)?;
        let vol_f = read_parm_float(context, 3)?;
        let atten = read_parm_float(context, 4)?;
        let volume = float_to_int(vol_f * 255.0);
        // BUILTIN-12.1: volume, attenuation, channel checked in that order; NaN atten passes.
        if !(0..=255).contains(&volume) {
            return Err(BuiltinError::from(FatalError::SoundVolume { volume }));
        }
        if atten < 0.0 || atten > 4.0 {
            return Err(BuiltinError::from(FatalError::SoundAttenuation { atten }));
        }
        if !(0..=7).contains(&chan) {
            return Err(BuiltinError::from(FatalError::SoundChannel {
                channel: chan,
            }));
        }
        // BUILTIN-12.2: drop silently when the datagram is nearly full.
        if self.engine.buffers.unreliable
            > LIMIT_MAX_DATAGRAM.saturating_sub(BUILTIN_DATAGRAM_SLACK)
        {
            return Ok(());
        }
        let samp = read_parm_string(context, &self.engine, 2)?;
        let name = String::from_utf8_lossy(&samp).into_owned();
        // BUILTIN-12.2: not precached (from entry 1) is a console message, then return.
        if self
            .engine
            .precache_index(Precache::Sound, &name)
            .filter(|&i| i >= 1)
            .is_none()
        {
            return Ok(());
        }
        // BUILTIN-12.2: e goes through NUM_FOR_EDICT after these checks.
        let _e = read_parm_edictnum(context, 0)?;
        // BUILTIN-12.3: the message size varies with the flags; use the maximum, since a flag
        // is set only when the value differs from the default, which does not change the
        // overflow behaviour QuakeC observes for a buffer this far from full.
        let bytes = BUILTIN_SOUND_MAX_BYTES.saturating_sub(sound_flag_savings(volume, atten));
        self.write_bytes(MessageDest::Broadcast, bytes)
    }

    /// `ambientsound(pos, samp, vol, atten)` (BUILTIN-12.4): to the signon buffer; a name not
    /// precached is a console message.
    fn b_ambientsound(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let _pos = read_parm_vector(context, 0)?;
        let samp = read_parm_string(context, &self.engine, 1)?;
        let _vol = read_parm_float(context, 2)?;
        let _atten = read_parm_float(context, 3)?;
        let name = String::from_utf8_lossy(&samp).into_owned();
        if self.engine.precache_index(Precache::Sound, &name).is_none() {
            return Ok(());
        }
        self.write_bytes(MessageDest::Init, BUILTIN_AMBIENTSOUND_BYTES)
    }

    /// `particle(o, d, color, count)` (BUILTIN-12.5): to the unreliable datagram; dropped when
    /// nearly full.
    fn b_particle(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let _o = read_parm_vector(context, 0)?;
        let _d = read_parm_vector(context, 1)?;
        let _color = read_parm_int(context, 2)?;
        let _count = read_parm_int(context, 3)?;
        if self.engine.buffers.unreliable
            > LIMIT_MAX_DATAGRAM.saturating_sub(BUILTIN_DATAGRAM_SLACK)
        {
            return Ok(());
        }
        self.write_bytes(MessageDest::Broadcast, BUILTIN_PARTICLE_BYTES)
    }

    /// `lightstyle(style, value)` (BUILTIN-12.6, BUILTIN-12.7): store the style; a number
    /// outside 0 to 63 is `invalid lightstyle`.
    fn b_lightstyle(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let style = read_parm_int(context, 0)?;
        let value = read_parm_string(context, &self.engine, 1)?;
        // BUILTIN-12.6: fail before storing anything for a style outside the array.
        let index = usize::try_from(style)
            .ok()
            .filter(|&n| n < LIMIT_MAX_LIGHTSTYLES)
            .ok_or_else(|| BuiltinError::from(RuntimeError::InvalidLightstyle))?;
        // BUILTIN-12.7: when active, also send svc_lightstyle to clients, not kept (DIFF-6.5).
        self.engine.set_lightstyle(index, value);
        Ok(())
    }

    /// `makestatic(e)` (BUILTIN-12.8): append to the signon buffer, then free `e`.
    fn b_makestatic(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let number = read_parm_edictnum(context, 0)?;
        // BUILTIN-12.8, BUILTIN-2.9: the model text is read here (invalid string if bad);
        // 0 for an empty model, the fatal error when not precached.
        let model_value = read_field_int(context, number, SysField::Model)?;
        let model_text = resolve_string(context, &self.engine, model_value)?;
        let name = String::from_utf8_lossy(&model_text).into_owned();
        let _index = if name.is_empty() {
            0
        } else {
            self.engine
                .precache_index(Precache::Model, &name)
                .ok_or_else(|| BuiltinError::from(FatalError::ModelNotPrecached { name }))?
        };
        self.write_bytes(MessageDest::Init, BUILTIN_MAKESTATIC_BYTES)?;
        // BUILTIN-12.8: then frees e (EDICT-7.5).
        let now = self.engine.clock;
        context.machine().edicts_mut().free(number, now);
        Ok(())
    }
}

/// Bytes saved from a `sound` message by the flags id omits (BUILTIN-12.3): the volume byte
/// when the volume is the default, and the attenuation byte when `atten` is 1.0.
fn sound_flag_savings(volume: i32, atten: f32) -> usize {
    let mut saved: usize = 0;
    if volume == BUILTIN_DEFAULT_SOUND_VOLUME {
        saved = saved.saturating_add(1);
    }
    #[expect(
        clippy::float_cmp,
        reason = "spec BUILTIN-12.3: flag 2 when atten is not 1.0, an exact IEEE comparison"
    )]
    if atten == 1.0 {
        saved = saved.saturating_add(1);
    }
    saved
}

// ===== class output: network messages (BUILTIN-13) =====

impl MockBuiltins {
    /// Resolves the `Write*` destination `to` (BUILTIN-13.1): a bad destination or a non-client
    /// `MSG_ONE` is a runtime error.
    fn write_dest(&mut self, context: &mut Context<'_, '_>) -> Result<MessageDest, BuiltinError> {
        let to = read_parm_int(context, 0)?;
        match to {
            0 => Ok(MessageDest::Broadcast),
            1 => {
                let value = read_system_global_entity(context, SysGlobal::MsgEntity)?;
                let number = entity_num_for(context, value)?;
                if !context.machine().edicts().is_client(number) {
                    return Err(BuiltinError::from(RuntimeError::WriteDestNotClient));
                }
                Ok(MessageDest::One)
            }
            2 => Ok(MessageDest::All),
            3 => Ok(MessageDest::Init),
            _ => Err(BuiltinError::from(RuntimeError::WriteDestBad)),
        }
    }

    /// Appends `len` bytes of a `Write*` message to the destination, failing on overflow.
    fn write_bytes(&mut self, dest: MessageDest, len: usize) -> Result<(), BuiltinError> {
        self.engine
            .append_message(dest, len)
            .map_err(|Overflow| BuiltinError::from(FatalError::MessageOverflow))
    }

    /// `WriteByte`, `WriteChar`, `WriteShort`, `WriteLong`, `WriteCoord` and `WriteAngle`
    /// (BUILTIN-13.1, BUILTIN-13.2): the destination, then the value, then `len` bytes.
    fn b_write(&mut self, context: &mut Context<'_, '_>, len: usize) -> Result<(), BuiltinError> {
        let dest = self.write_dest(context)?;
        let _value = read_parm(context, 1)?;
        self.write_bytes(dest, len)
    }

    /// `WriteString(to, s)` (BUILTIN-13.2, BUILTIN-2.10): the destination is read before the
    /// string, so a bad destination and a bad string give the destination's error.
    fn b_write_string(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let dest = self.write_dest(context)?;
        let text = read_parm_string(context, &self.engine, 1)?;
        self.write_bytes(dest, text.len().saturating_add(1))
    }

    /// `WriteEntity(to, e)` (BUILTIN-13.2, BUILTIN-2.10): the destination first, then `e`
    /// through `NUM_FOR_EDICT`.
    fn b_write_entity(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let dest = self.write_dest(context)?;
        let value = read_parm_raw(context, 1)?;
        let _number = entity_num_for(context, value)?;
        self.write_bytes(dest, 2)
    }
}

// ===== class world: answered by the deterministic rules of DIFF-7 =====

impl MockBuiltins {
    /// `traceline(v1, v2, nomonsters, forent)` (DIFF-7.1): hits nothing; writes the nine trace
    /// globals with `trace_endpos` a copy of PARM1.
    fn b_traceline(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let _v1 = read_parm_vector(context, 0)?;
        let v2 = read_parm_vector(context, 1)?;
        let _nomonsters = read_parm_float(context, 2)?;
        // forent is read with the G_EDICT check (BUILTIN-2.9), though the mock ignores it.
        let _forent = read_parm_edict(context, 3)?;
        write_system_global_float(context, SysGlobal::TraceAllsolid, 0.0)?;
        write_system_global_float(context, SysGlobal::TraceStartsolid, 0.0)?;
        write_system_global_float(context, SysGlobal::TraceFraction, 1.0)?;
        write_system_vector(context, SysGlobal::TraceEndpos, v2)?;
        write_system_vector(context, SysGlobal::TracePlaneNormal, [0.0, 0.0, 0.0])?;
        write_system_global_float(context, SysGlobal::TracePlaneDist, 0.0)?;
        let world = context
            .machine()
            .edicts()
            .edict_num(0)
            .map_err(|_| invalid_entity())?;
        write_system_global(context, SysGlobal::TraceEnt, Slot::from(world))?;
        write_system_global_float(context, SysGlobal::TraceInopen, 1.0)?;
        write_system_global_float(context, SysGlobal::TraceInwater, 0.0)
    }

    /// `pointcontents(v)` (DIFF-7.2): returns -1 (`CONTENTS_EMPTY`).
    fn b_pointcontents(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let _v = read_parm_vector(context, 0)?;
        write_return_float(context, -1.0)
    }

    /// `checkbottom(e)` (DIFF-7.4): returns 1.
    fn b_checkbottom(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let _e = read_parm_edictnum(context, 0)?;
        write_return_float(context, 1.0)
    }

    /// `aim(e, speed)` (DIFF-7.4): returns a copy of `v_forward`.
    fn b_aim(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let _e = read_parm_edictnum(context, 0)?;
        let _speed = read_parm_float(context, 1)?;
        let v_forward = read_system_vector(context, SysGlobal::VForward)?;
        write_return_vector(context, v_forward)
    }

    /// `checkclient()` (DIFF-7.3): chooses the next qualifying client, with no PVS; does not
    /// read `self`.
    fn b_checkclient(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let maxclients = context.machine().edicts().maxclients();
        let clock = self.engine.clock;
        // DIFF-7.3: reconsider when at least 0.1 seconds have passed since the last choice.
        if clock - self.engine.checkclient.last_check_time >= BUILTIN_CHECKCLIENT_INTERVAL {
            let chosen = self.pick_client(context, maxclients)?;
            self.engine.checkclient.last_check = chosen;
            self.engine.checkclient.last_check_time = clock;
        }
        let chosen = self.engine.checkclient.last_check;
        // DIFF-7.3: the world when the chosen edict is free or its health is 0 or less.
        let chosen_number = EdictNumber::from_i32(chosen).unwrap_or(EdictNumber::WORLD);
        let free = context
            .machine()
            .edicts()
            .is_free(chosen_number)
            .unwrap_or(true);
        let health = read_field_float(context, chosen_number, SysField::Health).unwrap_or(0.0);
        if free || health <= 0.0 {
            return write_return_edict(context, 0);
        }
        write_return_edict(context, chosen)
    }

    /// DIFF-7.3's choice: from `check + 1` (clamped), wrapping, the first client that is not
    /// free, has health above 0 and no `FL_NOTARGET`; `check` when none qualifies.
    fn pick_client(
        &self,
        context: &mut Context<'_, '_>,
        maxclients: usize,
    ) -> Result<i32, BuiltinError> {
        if maxclients == 0 {
            return Ok(0);
        }
        let max_i32 = i32::try_from(maxclients).unwrap_or(1);
        // check: the last choice clamped to 1 to maxclients.
        let check = self.engine.checkclient.last_check.clamp(1, max_i32);
        let mut n = check;
        for _ in 0..maxclients {
            n = if n == max_i32 { 1 } else { n.saturating_add(1) };
            let number = EdictNumber::from_i32(n).unwrap_or(EdictNumber::WORLD);
            let free = context.machine().edicts().is_free(number).unwrap_or(true);
            if free {
                continue;
            }
            let health = read_field_float(context, number, SysField::Health).unwrap_or(0.0);
            let flags =
                float_to_int(read_field_float(context, number, SysField::Flags).unwrap_or(0.0));
            if health > 0.0 && flags & BUILTIN_FL_NOTARGET == 0 {
                return Ok(n);
            }
        }
        Ok(check)
    }

    /// `droptofloor()` (DIFF-7.5): leaves origin, links `self`, sets `FL_ONGROUND`, sets
    /// `groundentity` to the world, returns 1.
    fn b_droptofloor(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let self_num = checked_self(context)?;
        self.link_edict(context, self_num)?;
        let flags = float_to_int(read_field_float(context, self_num, SysField::Flags)?);
        let new_flags = int_to_float(flags | BUILTIN_FL_ONGROUND);
        write_field_float(context, self_num, SysField::Flags, new_flags)?;
        write_field_int(context, self_num, SysField::Groundentity, 0)?;
        write_return_float(context, 1.0)
    }

    /// `walkmove(yaw, dist)` (DIFF-7.6, DIFF-7.7).
    fn b_walkmove(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let yaw = read_parm_float(context, 0)?;
        let dist = read_parm_float(context, 1)?;
        let self_num = checked_self(context)?;
        if !can_move(context, self_num)? {
            return write_return_float(context, 0.0);
        }
        self.step_self(context, self_num, yaw, dist)?;
        self.link_edict(context, self_num)?;
        write_return_float(context, 1.0)
    }

    /// `ChangeYaw()` (BUILTIN-15.4): turns `self`'s yaw toward `ideal_yaw` by at most
    /// `yaw_speed`.
    fn b_changeyaw(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let self_num = checked_self(context)?;
        self.change_yaw(context, self_num)
    }

    /// `movetogoal(step)` (DIFF-7.8): turns `self` as `ChangeYaw`, steps when within 45 degrees
    /// of `ideal_yaw`, links; writes 0 to RETURN only when `self` cannot move.
    fn b_movetogoal(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let step = read_parm_float(context, 0)?;
        let self_num = checked_self(context)?;
        if !can_move(context, self_num)? {
            return write_return_float(context, 0.0);
        }
        self.change_yaw(context, self_num)?;
        let yaw = read_field_float(context, self_num, SysField::IdealYaw)?;
        let angles_y = read_field_vector(context, self_num, SysField::Angles)?[1];
        // DIFF-7.8: step only when angles_y - ideal_yaw is not strictly between 45 and 315.
        let delta = angles_y - yaw;
        if !(delta > 45.0 && delta < 315.0) {
            self.step_self(context, self_num, yaw, step)?;
        }
        self.link_edict(context, self_num)
        // DIFF-7.8: writes nothing to RETURN.
    }

    /// The step of DIFF-7.6 along yaw `a` by distance `d`, writing `origin_x` and `origin_y`.
    ///
    /// id's `PF_walkmove`/`movetogoal` keeps the yaw in a `float` (`yaw = yaw*M_PI*2 / 360`),
    /// so the double product is rounded to f32 before `cos`/`sin` (DIFF-7.6). `double_to_float`
    /// does that `(f32)(...)` round; the promotion back to double for the library call is exact.
    fn step_self(
        &mut self,
        context: &mut Context<'_, '_>,
        number: EdictNumber,
        a: f32,
        d: f32,
    ) -> Result<(), BuiltinError> {
        let y = f64::from(double_to_float(
            f64::from(a) * core::f64::consts::PI * 2.0 / 360.0,
        ));
        let move_x = double_to_float(y.cos() * f64::from(d));
        let move_y = double_to_float(y.sin() * f64::from(d));
        let origin = read_field_vector(context, number, SysField::Origin)?;
        let new = [origin[0] + move_x, origin[1] + move_y, origin[2]];
        write_field_vector(context, number, SysField::Origin, new)
    }

    /// The `ChangeYaw` turn of BUILTIN-15.4 on edict `number`.
    fn change_yaw(
        &mut self,
        context: &mut Context<'_, '_>,
        number: EdictNumber,
    ) -> Result<(), BuiltinError> {
        let angles_y = read_field_vector(context, number, SysField::Angles)?[1];
        let ideal = read_field_float(context, number, SysField::IdealYaw)?;
        let yaw_speed = read_field_float(context, number, SysField::YawSpeed)?;
        let current = math::anglemod(angles_y);
        #[expect(
            clippy::float_cmp,
            reason = "spec BUILTIN-15.4: nothing changes if current == ideal_yaw, an exact IEEE comparison"
        )]
        if current == ideal {
            return Ok(());
        }
        let mut mv = ideal - current;
        if ideal > current {
            if mv >= 180.0 {
                mv -= 360.0;
            }
        } else if mv <= -180.0 {
            mv += 360.0;
        }
        if mv > 0.0 {
            if mv > yaw_speed {
                mv = yaw_speed;
            }
        } else if mv < -yaw_speed {
            mv = -yaw_speed;
        }
        let new_y = math::anglemod(current + mv);
        let mut angles = read_field_vector(context, number, SysField::Angles)?;
        if let Some(cell) = angles.get_mut(1) {
            *cell = new_y;
        }
        write_field_vector(context, number, SysField::Angles, angles)
    }
}

/// Writes `mins`, `maxs` and `size` of edict `number`, the writes `setsize` and `setmodel`
/// share (BUILTIN-9.2, BUILTIN-9.4).
fn set_size(
    context: &mut Context<'_, '_>,
    number: EdictNumber,
    min: [f32; 3],
    max: [f32; 3],
) -> Result<(), BuiltinError> {
    write_field_vector(context, number, SysField::Mins, min)?;
    write_field_vector(context, number, SysField::Maxs, max)?;
    let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    write_field_vector(context, number, SysField::Size, size)
}

/// Whether edict `number` is on the ground, flying or swimming, the monsters that may move
/// (DIFF-7.6, DIFF-7.8).
fn can_move(context: &mut Context<'_, '_>, number: EdictNumber) -> Result<bool, BuiltinError> {
    let flags = float_to_int(read_field_float(context, number, SysField::Flags)?);
    Ok([BUILTIN_FL_ONGROUND, BUILTIN_FL_FLY, BUILTIN_FL_SWIM]
        .iter()
        .any(|flag| flags & flag != 0))
}

/// Reads the global `self` with the `G_EDICT` check (BUILTIN-2.9): a `self` that names no edict
/// is `invalid entity`.
fn checked_self(context: &mut Context<'_, '_>) -> Result<EdictNumber, BuiltinError> {
    let value = read_system_global_entity(context, SysGlobal::SelfEntity)?;
    checked_edict(context, value)
}

// ===== class pure: errors and debugging (BUILTIN-16) =====

impl MockBuiltins {
    /// `error(s)` (BUILTIN-16.1): a host error carrying the joined text (DIFF-8.2).
    fn b_error(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let text = joined_string(context, &self.engine, 0)?;
        Err(BuiltinError::Host(
            String::from_utf8_lossy(&text).into_owned(),
        ))
    }

    /// `objerror(s)` (BUILTIN-16.2): frees `self`, then a host error with the joined text.
    fn b_objerror(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let text = joined_string(context, &self.engine, 0)?;
        if let Ok(self_num) = checked_self(context) {
            let now = self.engine.clock;
            context.machine().edicts_mut().free(self_num, now);
        }
        Err(BuiltinError::Host(
            String::from_utf8_lossy(&text).into_owned(),
        ))
    }
}
