// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The vanilla QuakeC builtins and a deterministic mock engine for them.
//!
//! id's engine exposes 78 builtins to QuakeC, numbers 1 to 78, dispatched by number from
//! `pr_builtins` (BUILTIN-1). This crate holds the table of their numbers, names, prototypes
//! and mock classes ([`table`]), the pure math and string builtins that need no engine
//! ([`math`]), a deterministic [`mock`] engine implementing `differential.md`'s fixture
//! (DIFF-6, DIFF-7) for unit and differential tests, and an [`Engine`] trait a real engine
//! (`qs-sim`, P3-24) implements.
//!
//! [`MockBuiltins`] implements [`qc_vm::Builtins`], so a [`qc_vm::Machine`] can run a progs
//! that calls builtins: it reads each builtin's arguments from the parameter slots, writes the
//! result to `OFS_RETURN` or the system globals, and updates the mock engine (PROGS-18,
//! BUILTIN-1.3, BUILTIN-1.4). A class `world` builtin (BUILTIN-4.4) is answered by DIFF-7 when
//! the engine has a world, and otherwise fails with a typed [`BuiltinError::EngineOnly`].
//!
//! Implements `docs/spec/builtins.md` (BUILTIN-1 to BUILTIN-17) and the mock engine of
//! `docs/spec/differential.md` (DIFF-6, DIFF-7). The builtin numbers, value encodings and the
//! VM call are in [`qc_progs`], [`qc_types`] and [`qc_vm`]; the edicts are [`qc_edict`].
//!
//! # Layer
//!
//! L1 VM. It depends on [`qc_vm`], [`qc_edict`] (both L1) and [`qc_progs`], [`qc_types`] (both
//! L0), and nothing higher.

mod dispatch;
mod engine;
pub mod math;
pub mod mock;
pub mod table;

#[cfg(test)]
mod tests;

pub use dispatch::MockBuiltins;
pub use engine::{BuiltinError, Engine, NoEngine};
pub use mock::MockEngine;
pub use table::{BUILTIN_MAX_NUMBER, Builtin, Class, lookup};
