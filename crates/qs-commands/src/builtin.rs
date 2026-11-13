// SPDX-License-Identifier: GPL-3.0-or-later

//! The commands every registry has, which work on the session itself.

use serde::Serialize;

use crate::command::NoParams;
use crate::id::CommandId;
use crate::registry::{CommandInfo, schema_of};

/// The ids of the built-in commands, in the order [`crate::Registry::commands`] lists them.
#[cfg(test)]
pub(crate) const BUILTIN_IDS: [&str; 4] = [
    Builtin::List.id(),
    Builtin::Redo.id(),
    Builtin::Repeat.id(),
    Builtin::Undo.id(),
];

/// A built-in command. These read or change the session's registry and undo history, which
/// no [`crate::Command`] can reach, so the session carries them out itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    /// `commands.list`: every command with its parameter schema.
    List,
    /// `edit.redo`: makes the last undone change again.
    Redo,
    /// `edit.repeat`: runs the command of the last map edit again with its parameters.
    Repeat,
    /// `edit.undo`: takes the last change back.
    Undo,
}

impl Builtin {
    pub(crate) const ALL: [Self; 4] = [Self::List, Self::Redo, Self::Repeat, Self::Undo];

    const fn id(self) -> &'static str {
        match self {
            Self::List => "commands.list",
            Self::Redo => "edit.redo",
            Self::Repeat => "edit.repeat",
            Self::Undo => "edit.undo",
        }
    }

    const fn summary(self) -> &'static str {
        match self {
            Self::List => "Lists every command with its id, summary and parameter schema.",
            Self::Redo => "Makes the most recently undone change again.",
            Self::Repeat => {
                "Makes the most recent map edit once more: same command, same arguments."
            }
            Self::Undo => "Takes back the most recent change that has not been undone.",
        }
    }

    pub(crate) fn info(self) -> CommandInfo {
        CommandInfo::new(
            CommandId::builtin(self.id()),
            self.summary(),
            schema_of::<NoParams>(),
        )
    }
}

/// What `edit.undo` and `edit.redo` return: the command whose change was undone or redone.
#[derive(Serialize)]
pub(crate) struct HistoryOutput<'a> {
    pub(crate) command: &'a CommandId,
}

/// What `edit.repeat` returns: the command run again and what it returned.
#[derive(Serialize)]
pub(crate) struct RepeatOutput<'a> {
    pub(crate) command: &'a CommandId,
    pub(crate) output: serde_json::Value,
}
