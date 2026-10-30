// SPDX-License-Identifier: GPL-3.0-or-later

//! The QuakeC Studio command engine (PRIN-3, PRIN-7).
//!
//! Every menu item, keystroke, palette entry, CLI subcommand, agent tool and MCP tool is a
//! [`Command`] in one [`Registry`]: a stable [`CommandId`], typed parameters whose JSON Schema
//! the registry exports, and an [`UndoStep`] for what it changes. A [`Session`] owns the state
//! commands work on, runs them by id with JSON arguments, keeps the undo history, and runs
//! every command behind a guard that turns a panic into a [`CommandError`] instead of ending
//! the session.
//!
//! [`Registry::markdown`] renders the command reference, `docs/commands.md`, which
//! `cargo xtask commands` writes and CI checks.

#[cfg(not(panic = "unwind"))]
compile_error!(
    "qs-commands needs panic = \"unwind\": its guard catches a panicking command (PRIN-7, CONV-6)"
);

mod builtin;
mod command;
mod error;
mod id;
pub mod palette;
pub mod params;
mod registry;
mod session;

#[cfg(test)]
mod tests;

pub use builtin::BUILTIN_IDS;
pub use command::{BoxError, Command, NoParams, Outcome, UndoStep};
pub use error::{CommandError, RegisterError};
pub use id::{CommandId, IdError, MAX_ID_LEN};
pub use registry::{CommandInfo, Registry};
pub use session::Session;
