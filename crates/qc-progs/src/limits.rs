// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_comp.h, dstatement_t, ddef_t
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_exec.c, MAX_STACK_DEPTH, LOCALSTACK_SIZE, PR_ExecuteProgram
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/pr_cmds.c, pr_numbuiltins
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/quakedef.h, MAX_EDICTS, MAX_MODELS, MAX_SOUNDS, MAX_LIGHTSTYLES, MAX_QPATH, MAX_MSGLEN, MAX_DATAGRAM
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/progs.h, MAX_ENT_LEAFS
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/qcc.h, MAX_STRINGS, MAX_STATEMENTS, MAX_FUNCTIONS, MAX_GLOBALS, MAX_FIELDS, MAX_REGS, MAX_NAME, MAX_SOUNDS, MAX_MODELS, MAX_FILES, MAX_DATA_PATH, MAX_ERRORS
// Ported from id-Software/Quake-Tools@c0d1b91c74eb654365ac7755bc837e497caaca73: qcc/pr_lex.c, MAX_FRAMES

//! Every limit in `docs/spec/limits.md` as a named constant.
//!
//! The format limits (LIMIT-1) are the ones this crate enforces. The VM, server and qcc limits
//! (LIMIT-2 to LIMIT-7) are named here, at the bottom layer, so the VM, the edict store, the
//! builtins and the compiler share one definition; those crates implement the behaviour.

pub use qc_types::LIMIT_MAX_PARMS;

/// Highest global slot a version 6 statement operand can name: operands are `short`.
///
/// `LIMIT_MAX_OPERAND_SLOT` in LIMIT-1, from `dstatement_t` in `WinQuake/pr_comp.h`.
pub const LIMIT_MAX_OPERAND_SLOT: i16 = i16::MAX;

/// Highest global or field slot a version 6 definition can name: `ofs` is `unsigned short`.
///
/// `LIMIT_MAX_DEF_SLOT` in LIMIT-1, from `ddef_t` in `WinQuake/pr_comp.h`.
pub const LIMIT_MAX_DEF_SLOT: u16 = u16::MAX;

/// Highest type number a version 6 definition can carry once `DEF_SAVEGLOBAL` is removed.
///
/// `LIMIT_MAX_DEF_TYPE` in LIMIT-1, from `ddef_t` in `WinQuake/pr_comp.h`.
pub const LIMIT_MAX_DEF_TYPE: u16 = 0x7FFF;

/// QC call depth at which entering a function fails with `stack overflow`.
///
/// `LIMIT_MAX_STACK_DEPTH` in LIMIT-2, from `MAX_STACK_DEPTH` in `WinQuake/pr_exec.c`.
pub const LIMIT_MAX_STACK_DEPTH: usize = 32;

/// Slots of the local stack shared by every active QC function.
///
/// `LIMIT_LOCALSTACK_SIZE` in LIMIT-3, from `LOCALSTACK_SIZE` in `WinQuake/pr_exec.c`.
pub const LIMIT_LOCALSTACK_SIZE: usize = 2048;

/// Starting value of the runaway loop counter of one `PR_ExecuteProgram` call.
///
/// `LIMIT_RUNAWAY` in LIMIT-4, from `PR_ExecuteProgram` in `WinQuake/pr_exec.c`.
pub const LIMIT_RUNAWAY: u32 = 100_000;

/// Entries in id's builtin table, numbers 0 to 78.
///
/// `LIMIT_NUM_BUILTINS` in LIMIT-5, from `pr_numbuiltins` in `WinQuake/pr_cmds.c`.
pub const LIMIT_NUM_BUILTINS: u32 = 79;

/// Edicts a server has, the world and the clients included.
///
/// `LIMIT_MAX_EDICTS` in LIMIT-6, from `MAX_EDICTS` in `WinQuake/quakedef.h`.
pub const LIMIT_MAX_EDICTS: usize = 600;

/// Entries in the model precache list, entry 0 included.
///
/// `LIMIT_MAX_MODELS` in LIMIT-6, from `MAX_MODELS` in `WinQuake/quakedef.h`.
pub const LIMIT_MAX_MODELS: usize = 256;

/// Entries in the sound precache list, entry 0 included.
///
/// `LIMIT_MAX_SOUNDS` in LIMIT-6, from `MAX_SOUNDS` in `WinQuake/quakedef.h`.
pub const LIMIT_MAX_SOUNDS: usize = 256;

/// Light styles.
///
/// `LIMIT_MAX_LIGHTSTYLES` in LIMIT-6, from `MAX_LIGHTSTYLES` in `WinQuake/quakedef.h`.
pub const LIMIT_MAX_LIGHTSTYLES: usize = 64;

/// Longest game path, NUL included.
///
/// `LIMIT_MAX_QPATH` in LIMIT-6, from `MAX_QPATH` in `WinQuake/quakedef.h`.
pub const LIMIT_MAX_QPATH: usize = 64;

/// Longest reliable message.
///
/// `LIMIT_MAX_MSGLEN` in LIMIT-6, from `MAX_MSGLEN` in `WinQuake/quakedef.h`.
pub const LIMIT_MAX_MSGLEN: usize = 8000;

/// Longest unreliable message.
///
/// `LIMIT_MAX_DATAGRAM` in LIMIT-6, from `MAX_DATAGRAM` in `WinQuake/quakedef.h`.
pub const LIMIT_MAX_DATAGRAM: usize = 1024;

/// BSP leaves an edict is linked into.
///
/// `LIMIT_MAX_ENT_LEAFS` in LIMIT-6, from `MAX_ENT_LEAFS` in `WinQuake/progs.h`.
pub const LIMIT_MAX_ENT_LEAFS: usize = 16;

/// Bytes in id qcc's string table.
///
/// `LIMIT_QCC_MAX_STRINGS` in LIMIT-7, from `MAX_STRINGS` in `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_STRINGS: usize = 500_000;

/// Statements id qcc can hold.
///
/// `LIMIT_QCC_MAX_STATEMENTS` in LIMIT-7, from `MAX_STATEMENTS` in `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_STATEMENTS: usize = 65536;

/// Functions id qcc can hold.
///
/// `LIMIT_QCC_MAX_FUNCTIONS` in LIMIT-7, from `MAX_FUNCTIONS` in `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_FUNCTIONS: usize = 8192;

/// Global definitions id qcc can hold.
///
/// `LIMIT_QCC_MAX_GLOBALS` in LIMIT-7, from `MAX_GLOBALS` in `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_GLOBALS: usize = 16384;

/// Field definitions id qcc can hold.
///
/// `LIMIT_QCC_MAX_FIELDS` in LIMIT-7, from `MAX_FIELDS` in `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_FIELDS: usize = 1024;

/// Global slots id qcc can hold.
///
/// `LIMIT_QCC_MAX_REGS` in LIMIT-7, from `MAX_REGS` in `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_REGS: usize = 16384;

/// id qcc's name buffer; a name of `LIMIT_QCC_MAX_NAME - 1` characters or more is too long.
///
/// `LIMIT_QCC_MAX_NAME` in LIMIT-7, from `MAX_NAME` in `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_NAME: usize = 64;

/// Sounds, models and files id qcc records from precache calls, each.
///
/// `LIMIT_QCC_MAX_PRECACHE` in LIMIT-7, from `MAX_SOUNDS`, `MAX_MODELS` and `MAX_FILES` in
/// `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_PRECACHE: usize = 1024;

/// Length of a precache name id qcc records, unchecked.
///
/// `LIMIT_QCC_MAX_DATA_PATH` in LIMIT-7, from `MAX_DATA_PATH` in `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_DATA_PATH: usize = 64;

/// `$frame` names id qcc can hold, unchecked.
///
/// `LIMIT_QCC_MAX_FRAMES` in LIMIT-7, from `MAX_FRAMES` in `qcc/pr_lex.c`.
pub const LIMIT_QCC_MAX_FRAMES: usize = 256;

/// Errors after which id qcc stops compiling.
///
/// `LIMIT_QCC_MAX_ERRORS` in LIMIT-7, from `MAX_ERRORS` in `qcc/qcc.h`.
pub const LIMIT_QCC_MAX_ERRORS: usize = 10;
