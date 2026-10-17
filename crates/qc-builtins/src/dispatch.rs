// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, pr_builtin, PF_Fixme, PF_makevectors, PF_random, PF_normalize, PF_vlen, PF_vectoyaw, PF_vectoangles, PF_ftos, PF_vtos, PF_rint, PF_floor, PF_ceil, PF_fabs, PF_traceon, PF_traceoff, PF_break

//! [`MockBuiltins`]: the builtin dispatch that runs a progs over a [`qc_vm::Machine`].
//!
//! Implements the dispatch of BUILTIN-1: the VM has checked the number is 1 to
//! [`qc_vm::LIMIT_NUM_BUILTINS`] - 1 and set the argument count (VM-14.1, VM-14.4); this reads
//! each builtin's arguments from the parameter slots and writes its result to `OFS_RETURN` or
//! the system globals (BUILTIN-1.3, BUILTIN-1.4, PROGS-18). A number with no builtin fails with
//! the `PF_Fixme` error (BUILTIN-1.2).

use qc_edict::system::global_by_name;
use qc_types::{PROGS_OFS_RETURN, Slot, ofs_parm};
use qc_vm::{Builtins, Context, ErrorKind, RunError};

use crate::engine::{BuiltinError, Engine};
use crate::math;
use crate::mock::MockEngine;
use crate::table::{self, BUILTIN_FIXME_MESSAGE};

/// The builtin dispatch over a mock engine (DIFF-6, DIFF-7).
///
/// Holds the mock engine fixture; it borrows it for the life of the run. A different [`Engine`]
/// can back it through [`MockBuiltins::with_engine`].
#[derive(Clone, Debug)]
pub struct MockBuiltins<E: Engine = MockEngine> {
    engine: E,
}

impl Default for MockBuiltins<MockEngine> {
    fn default() -> Self {
        Self {
            engine: MockEngine::new("test", 0, 1),
        }
    }
}

impl<E: Engine> MockBuiltins<E> {
    /// A dispatch over the given engine fixture.
    pub const fn with_engine(engine: E) -> Self {
        Self { engine }
    }

    /// The engine fixture, for reading what a builtin wrote.
    pub const fn engine(&self) -> &E {
        &self.engine
    }

    /// The engine fixture, for a test to set up state.
    pub const fn engine_mut(&mut self) -> &mut E {
        &mut self.engine
    }
}

impl<E: Engine> Builtins for MockBuiltins<E> {
    fn call(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), RunError> {
        match self.dispatch(number, context) {
            Ok(()) => Ok(()),
            Err(error) => Err(to_run_error(context, error)),
        }
    }
}

impl<E: Engine> MockBuiltins<E> {
    /// Dispatches one builtin, returning a [`BuiltinError`] the caller maps to a [`RunError`].
    fn dispatch(&mut self, number: u32, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        // A number with no builtin is a PF_Fixme gap (BUILTIN-1.2).
        if table::is_fixme(number) || table::lookup(number).is_none() {
            return Err(BuiltinError::Runtime(BUILTIN_FIXME_MESSAGE.to_string()));
        }
        match number {
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
            6 => Err(BuiltinError::Fatal("break statement".to_string())),
            29 | 30 => Ok(()),
            _ => Err(self.engine_only(number)),
        }
    }

    /// The typed engine-only error for a builtin not yet given a mock body (BUILTIN-4.4 for the
    /// class `world` builtins, and the remaining class state and output builtins pending a mock
    /// body). It names the number and name so the acceptance criterion holds: every builtin is
    /// present, implemented by the mock or marked with a typed error.
    fn engine_only(&self, number: u32) -> BuiltinError {
        let name = table::lookup(number).map_or("", |entry| entry.name);
        BuiltinError::EngineOnly { number, name }
    }

    // ----- pure math and string builtins (BUILTIN-6, BUILTIN-7) -----

    fn b_makevectors(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let ang = read_parm_vector(context, 0)?;
        let (forward, right, up) = math::makevectors(ang);
        write_system_vector(context, "v_forward", forward)?;
        write_system_vector(context, "v_right", right)?;
        write_system_vector(context, "v_up", up)?;
        Ok(())
    }

    fn b_random(&mut self, context: &mut Context<'_, '_>) -> Result<(), BuiltinError> {
        let rand_value = self.engine.next_rand();
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
}

// ----- argument reading and result writing (PROGS-18, BUILTIN-1.3, BUILTIN-1.4) -----

/// Reads parameter `n` as a float from its slot (`OFS_PARM0` + 3n), bounds-checked (VM-1.4).
fn read_parm_float(context: &mut Context<'_, '_>, n: u16) -> Result<f32, BuiltinError> {
    let slot = parm_slot(n)?;
    let value = context
        .machine()
        .global(slot)
        .map_err(|_| BuiltinError::Runtime("builtin parameter out of range".to_string()))?;
    Ok(value.as_f32())
}

/// Reads parameter `n` as a vector from its three slots, bounds-checked (VM-1.4).
fn read_parm_vector(context: &mut Context<'_, '_>, n: u16) -> Result<[f32; 3], BuiltinError> {
    let slot = parm_slot(n)?;
    let slots = context
        .machine()
        .global3(slot)
        .map_err(|_| BuiltinError::Runtime("builtin parameter out of range".to_string()))?;
    Ok([slots[0].as_f32(), slots[1].as_f32(), slots[2].as_f32()])
}

/// Writes a float result to `OFS_RETURN` (BUILTIN-1.4, PROGS-18).
fn write_return_float(context: &mut Context<'_, '_>, value: f32) -> Result<(), BuiltinError> {
    context
        .machine()
        .set_global(i32::from(PROGS_OFS_RETURN), Slot::from_f32(value))
        .map_err(|_| BuiltinError::Runtime("return slot out of range".to_string()))
}

/// Writes a vector result to `OFS_RETURN` (BUILTIN-1.4, PROGS-18).
fn write_return_vector(context: &mut Context<'_, '_>, value: [f32; 3]) -> Result<(), BuiltinError> {
    let slots = [
        Slot::from_f32(value[0]),
        Slot::from_f32(value[1]),
        Slot::from_f32(value[2]),
    ];
    context
        .machine()
        .set_global3(i32::from(PROGS_OFS_RETURN), slots)
        .map_err(|_| BuiltinError::Runtime("return slot out of range".to_string()))
}

/// Writes a vector to a named system global (`v_forward`, `v_right`, `v_up`) (BUILTIN-6.1).
fn write_system_vector(
    context: &mut Context<'_, '_>,
    name: &str,
    value: [f32; 3],
) -> Result<(), BuiltinError> {
    let sysvar = global_by_name(name)
        .ok_or_else(|| BuiltinError::Runtime(format!("no system global {name}")))?;
    let slot = i32::try_from(sysvar.slot)
        .map_err(|_| BuiltinError::Runtime("system global slot out of range".to_string()))?;
    let slots = [
        Slot::from_f32(value[0]),
        Slot::from_f32(value[1]),
        Slot::from_f32(value[2]),
    ];
    context
        .machine()
        .set_global3(slot, slots)
        .map_err(|_| BuiltinError::Runtime("system global slot out of range".to_string()))
}

/// The global slot of parameter `n`, id's `OFS_PARM0 + 3n` (PROGS-18.1).
fn parm_slot(n: u16) -> Result<i32, BuiltinError> {
    let offset = ofs_parm(n).ok_or_else(|| BuiltinError::Runtime(format!("no parameter {n}")))?;
    Ok(i32::from(offset))
}

/// Turns a [`BuiltinError`] into the [`RunError`] the VM carries (BUILTIN-3, DIFF-8.2).
///
/// Every kind becomes a runtime error with its verbatim message (the trace compares the bytes,
/// DIFF-6.6, DIFF-8.2); the host and fatal distinctions (BUILTIN-3.2, BUILTIN-3.3) are kept in
/// [`BuiltinError`] for a caller that needs them and collapse to the message here.
fn to_run_error(context: &Context<'_, '_>, error: BuiltinError) -> RunError {
    context.run_error(ErrorKind::Builtin {
        message: error.message(),
    })
}
