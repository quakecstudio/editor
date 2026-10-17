// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

//! The engine a builtin asks for its side effects, and the error kinds a builtin raises.
//!
//! Most builtins read or write engine state outside the VM: the precache lists, the
//! lightstyles, the cvars, the message buffers, the console, the command buffer (BUILTIN-4).
//! An [`Engine`] provides that state. The mock engine ([`crate::mock`]) implements it as the
//! deterministic fixture of `differential.md` (DIFF-6); a real engine (`qs-sim`, P3-24) will
//! implement it too, and the class `world` builtins it serves that the mock cannot
//! (BUILTIN-4.4).

/// How a builtin fails, before it is turned into a VM [`RunError`](qc_vm::RunError).
///
/// The kinds mirror BUILTIN-3: a runtime error aborts the run (BUILTIN-3.1), a host error
/// stops the server without resetting the depth (BUILTIN-3.2), and a fatal error ends the run
/// with a typed message (BUILTIN-3.3). The class `world` builtins a mock cannot reproduce
/// fail with [`BuiltinError::EngineOnly`] (BUILTIN-4.4) unless the engine implements them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuiltinError {
    /// A runtime error with id's message, id's `PR_RunError` (BUILTIN-3.1).
    Runtime(String),
    /// A host error, id's `Host_Error ("Program error")`; the message is the joined text
    /// (BUILTIN-3.2, DIFF-8.2).
    Host(String),
    /// A fatal error, id's `Sys_Error`; the message is the one id gives (BUILTIN-3.3).
    Fatal(String),
    /// A class `world` builtin (BUILTIN-4.4) that the engine does not implement: the mock's
    /// own world builtins answer deterministically (DIFF-7) rather than raise this, but a real
    /// engine that lacks one, or a caller using [`NoEngine`], gets it.
    EngineOnly {
        /// The builtin number (BUILTIN-5.1).
        number: u32,
        /// The builtin name (BUILTIN-5.1).
        name: &'static str,
    },
}

impl BuiltinError {
    /// The message the error carries, byte for byte (DIFF-8.2).
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Runtime(message) | Self::Host(message) | Self::Fatal(message) => message.clone(),
            Self::EngineOnly { number, name } => {
                format!("builtin {number} ({name}) needs an engine")
            }
        }
    }
}

impl core::fmt::Display for BuiltinError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl core::error::Error for BuiltinError {}

/// The engine state the builtins read and write (BUILTIN-4.2, BUILTIN-4.3).
///
/// A real engine (`qs-sim`, P3-24) and the mock ([`crate::mock::MockEngine`]) both implement
/// this. The class `world` builtins (BUILTIN-4.4) are the methods that return
/// [`BuiltinError::EngineOnly`] in [`NoEngine`]; an engine that cannot do one answers with that
/// error and the dispatcher turns it into a runtime error.
///
/// This trait names only the engine-side effects; the VM state (globals, edicts) is reached
/// through the [`Context`](qc_vm::Context) the dispatcher holds, not here.
pub trait Engine {
    /// Whether the class `world` builtins are available (BUILTIN-4.4).
    ///
    /// The mock returns `true` and answers them by DIFF-7; [`NoEngine`] returns `false`, so a
    /// class `world` builtin fails with [`BuiltinError::EngineOnly`].
    fn has_world(&self) -> bool;

    /// One draw from the engine's random source, id's `rand() & 0x7fff` (BUILTIN-6.6).
    ///
    /// The mock's source is the deterministic one of DIFF-6.2; `random` is the only builtin
    /// that draws from it. [`NoEngine`] returns 0.
    fn next_rand(&mut self) -> u32;
}

/// An [`Engine`] with no engine-side state: every class `world` builtin is engine-only.
///
/// Useful for a caller that runs a progs making no engine calls, or to prove a class `world`
/// builtin is marked engine-only (the acceptance criterion of P1-16).
#[derive(Clone, Copy, Debug, Default)]
pub struct NoEngine;

impl Engine for NoEngine {
    fn has_world(&self) -> bool {
        false
    }

    fn next_rand(&mut self) -> u32 {
        0
    }
}
