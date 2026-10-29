// SPDX-License-Identifier: GPL-3.0-or-later

//! What can go wrong registering and running commands.

use std::error::Error;
use std::fmt;

use crate::command::BoxError;
use crate::id::{CommandId, IdError};

/// Why a command could not be added to a registry.
#[derive(Debug)]
pub enum RegisterError {
    /// [`crate::Command::ID`] is not a valid [`CommandId`].
    InvalidId {
        /// The text of the id.
        id: &'static str,
        /// What is wrong with it.
        error: IdError,
    },
    /// The registry already has a command with this id.
    Duplicate(CommandId),
    /// The parameters' schema does not describe a JSON object, which MCP requires of a tool's
    /// input (MCP 2026-07-28, Tools, Tool, `inputSchema`).
    ParamsNotObject(CommandId),
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId { id, error } => write!(f, "invalid command id {id:?}: {error}"),
            Self::Duplicate(id) => write!(f, "command {id} is already registered"),
            Self::ParamsNotObject(id) => {
                write!(f, "the parameters of command {id} are not a JSON object")
            }
        }
    }
}

impl Error for RegisterError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidId { error, .. } => Some(error),
            Self::Duplicate(_) | Self::ParamsNotObject(_) => None,
        }
    }
}

/// Why a command, an undo or a redo did not complete. The session carries on after every one
/// of them (PRIN-7).
#[derive(Debug)]
pub enum CommandError {
    /// No command has this id.
    Unknown {
        /// The id as the caller sent it.
        id: String,
    },
    /// The arguments are not a JSON object, which every command's parameters are.
    ParamsNotObject {
        /// The command.
        command: CommandId,
    },
    /// The arguments do not match the command's parameter schema.
    InvalidParams {
        /// The command.
        command: CommandId,
        /// Where and how they differ.
        error: serde_json::Error,
    },
    /// The command ran and failed; the context is as it was.
    Failed {
        /// The command.
        command: CommandId,
        /// The command's own error.
        error: BoxError,
    },
    /// The command's output could not be serialised as JSON. Its change, if any, stands and
    /// can be undone.
    Output {
        /// The command.
        command: CommandId,
        /// What serde reported.
        error: serde_json::Error,
    },
    /// The command, or the undo step it left, panicked. The guard caught the panic; the
    /// context may hold part of the change, so the undo history was cleared.
    Panicked {
        /// The command.
        command: CommandId,
        /// The panic message. It can hold anything, user content included, so it is shown to
        /// the person and never logged (QA-7).
        message: String,
    },
    /// Undo was asked for with nothing left to undo.
    NothingToUndo,
    /// Redo was asked for with nothing left to redo.
    NothingToRedo,
    /// The undo step of this command failed; the context is as it was and the step can be
    /// tried again.
    UndoFailed {
        /// The command whose change could not be taken back.
        command: CommandId,
        /// The step's error.
        error: BoxError,
    },
    /// The redo step of this command failed; the context is as it was and the step can be
    /// tried again.
    RedoFailed {
        /// The command whose change could not be made again.
        command: CommandId,
        /// The step's error.
        error: BoxError,
    },
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { id } => write!(f, "no command has the id {id:?}"),
            Self::ParamsNotObject { command } => {
                write!(f, "the arguments to {command} are not a JSON object")
            }
            Self::InvalidParams { command, error } => {
                write!(f, "invalid arguments to {command}: {error}")
            }
            Self::Failed { command, error } => write!(f, "{command} failed: {error}"),
            Self::Output { command, error } => {
                write!(f, "the output of {command} is not JSON: {error}")
            }
            Self::Panicked { command, message } => {
                write!(f, "{command} hit an internal error: {message}")
            }
            Self::NothingToUndo => f.write_str("nothing to undo"),
            Self::NothingToRedo => f.write_str("nothing to redo"),
            Self::UndoFailed { command, error } => {
                write!(f, "cannot undo {command}: {error}")
            }
            Self::RedoFailed { command, error } => {
                write!(f, "cannot redo {command}: {error}")
            }
        }
    }
}

impl Error for CommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidParams { error, .. } | Self::Output { error, .. } => Some(error),
            Self::Failed { error, .. }
            | Self::UndoFailed { error, .. }
            | Self::RedoFailed { error, .. } => Some(error.as_ref()),
            Self::Unknown { .. }
            | Self::ParamsNotObject { .. }
            | Self::Panicked { .. }
            | Self::NothingToUndo
            | Self::NothingToRedo => None,
        }
    }
}
