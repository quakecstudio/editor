// SPDX-License-Identifier: GPL-3.0-or-later

//! The session: a context, the registry that works on it, the undo history and the guard.

use std::panic::{self, AssertUnwindSafe};

use serde_json::Value;

use crate::builtin::{Builtin, HistoryOutput};
use crate::command::{Effect, NoParams, UndoStep};
use crate::error::CommandError;
use crate::id::CommandId;
use crate::registry::{Handler, Invoked, Registry};

/// One change in the undo history: the command that made it and the step that undoes it.
struct Change<C> {
    command: CommandId,
    step: Box<dyn UndoStep<C>>,
}

/// Which way through the undo history a step goes.
#[derive(Clone, Copy)]
enum Direction {
    Undo,
    Redo,
}

/// A context with the registry that works on it and its undo history.
///
/// Every change to the context goes through [`Session::execute`], so the context is reachable
/// only for reading ([`Session::context`]) and every change is in the history.
///
/// **The guard (PRIN-7).** Every command, undo and redo runs inside
/// [`std::panic::catch_unwind`]. A panic becomes [`CommandError::Panicked`] and the session
/// carries on. The guard cannot know how much of its change the panicking code made, so it
/// clears the undo history, whose steps may no longer match the context. This relies on the
/// workspace's `panic = "unwind"` in every profile (docs/conventions.md, CONV-6); the crate
/// does not compile with `panic = "abort"`. Panics stay denied by lint: the guard is the last
/// line of defence, not a way to report errors.
pub struct Session<C> {
    context: C,
    registry: Registry<C>,
    undo: Vec<Change<C>>,
    redo: Vec<Change<C>>,
}

impl<C: 'static> Session<C> {
    /// A session over `context` running the commands of `registry`, with an empty history.
    #[must_use]
    pub const fn new(context: C, registry: Registry<C>) -> Self {
        Self {
            context,
            registry,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    /// The context, for reading.
    #[must_use]
    pub const fn context(&self) -> &C {
        &self.context
    }

    /// The commands this session runs.
    #[must_use]
    pub const fn registry(&self) -> &Registry<C> {
        &self.registry
    }

    /// How many changes [`Session::undo`] can take back.
    #[must_use]
    pub const fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// How many undone changes [`Session::redo`] can make again.
    #[must_use]
    pub const fn redo_depth(&self) -> usize {
        self.redo.len()
    }

    /// Runs the command `id` with the JSON object `params`, inside the guard, and returns its
    /// output as JSON.
    ///
    /// A command that changes the context goes on the undo history and empties the redo
    /// history.
    ///
    /// # Errors
    ///
    /// [`CommandError`] for an unknown id, arguments that do not match the schema, the
    /// command's own failure, output that is not JSON, a panic, and for the built-ins
    /// `edit.undo` and `edit.redo` the errors of [`Session::undo`] and [`Session::redo`].
    pub fn execute(&mut self, id: &str, params: Value) -> Result<Value, CommandError> {
        let Some(entry) = self.registry.entry(id) else {
            return Err(CommandError::Unknown { id: id.to_owned() });
        };
        let command = entry.info.id().clone();
        let builtin = match &entry.handler {
            Handler::Builtin(builtin) => *builtin,
            Handler::Command(handler) => {
                let context = &mut self.context;
                let invoked = guard(|| handler.invoke(&command, context, params));
                return self.record(command, invoked);
            }
        };
        let NoParams {} =
            serde_json::from_value(params).map_err(|error| CommandError::InvalidParams {
                command: command.clone(),
                error,
            })?;
        let output = match builtin {
            Builtin::List => serde_json::to_value(self.registry.commands().collect::<Vec<_>>()),
            Builtin::Undo => serde_json::to_value(HistoryOutput {
                command: &self.undo()?,
            }),
            Builtin::Redo => serde_json::to_value(HistoryOutput {
                command: &self.redo()?,
            }),
        };
        output.map_err(|error| CommandError::Output { command, error })
    }

    /// Takes back the most recent change that has not been undone, inside the guard, and
    /// returns the command that made it.
    ///
    /// # Errors
    ///
    /// [`CommandError::NothingToUndo`], [`CommandError::UndoFailed`] (the change stays next to
    /// undo) or [`CommandError::Panicked`] (the history is cleared).
    pub fn undo(&mut self) -> Result<CommandId, CommandError> {
        self.step(Direction::Undo)
    }

    /// Makes the most recently undone change again, inside the guard, and returns the command
    /// that made it.
    ///
    /// # Errors
    ///
    /// [`CommandError::NothingToRedo`], [`CommandError::RedoFailed`] (the change stays next to
    /// redo) or [`CommandError::Panicked`] (the history is cleared).
    pub fn redo(&mut self) -> Result<CommandId, CommandError> {
        self.step(Direction::Redo)
    }

    /// Moves one change from one end of the history to the other, running its step.
    fn step(&mut self, direction: Direction) -> Result<CommandId, CommandError> {
        let (from, to) = match direction {
            Direction::Undo => (&mut self.undo, &mut self.redo),
            Direction::Redo => (&mut self.redo, &mut self.undo),
        };
        let Some(mut change) = from.pop() else {
            return Err(match direction {
                Direction::Undo => CommandError::NothingToUndo,
                Direction::Redo => CommandError::NothingToRedo,
            });
        };
        let context = &mut self.context;
        let step = &mut change.step;
        let ran = guard(|| match direction {
            Direction::Undo => step.undo(context),
            Direction::Redo => step.redo(context),
        });
        let command = change.command.clone();
        match ran {
            Ok(Ok(())) => {
                to.push(change);
                Ok(command)
            }
            Ok(Err(error)) => {
                from.push(change);
                Err(match direction {
                    Direction::Undo => CommandError::UndoFailed { command, error },
                    Direction::Redo => CommandError::RedoFailed { command, error },
                })
            }
            Err(message) => {
                from.clear();
                to.clear();
                Err(panicked(command, message))
            }
        }
    }

    /// Turns what a guarded command did into its result, recording its change.
    fn record(
        &mut self,
        command: CommandId,
        invoked: Result<Result<Invoked<C>, CommandError>, String>,
    ) -> Result<Value, CommandError> {
        match invoked {
            Ok(Ok(Invoked { output, effect })) => {
                match effect {
                    Effect::Unchanged => {}
                    Effect::Undoable(step) => {
                        self.redo.clear();
                        self.undo.push(Change {
                            command: command.clone(),
                            step,
                        });
                    }
                }
                output.map_err(|error| CommandError::Output { command, error })
            }
            Ok(Err(error)) => Err(error),
            Err(message) => {
                self.undo.clear();
                self.redo.clear();
                Err(panicked(command, message))
            }
        }
    }
}

/// The error for a caught panic in `command`. The log line names the command but not the
/// message, which can hold user content (QA-7).
fn panicked(command: CommandId, message: String) -> CommandError {
    tracing::error!(%command, "command panicked; the guard caught it and cleared undo history");
    CommandError::Panicked { command, message }
}

/// Runs `f`, catching a panic and returning its message.
///
/// `AssertUnwindSafe` is sound here because nothing observes a broken invariant afterwards
/// without being told: the caller clears the undo history, the one thing that assumes the
/// context is as the last step left it, and [`CommandError::Panicked`] says the context may
/// hold part of a change.
fn guard<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    panic::catch_unwind(AssertUnwindSafe(f)).map_err(|payload| {
        payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "a panic with no message".to_owned())
    })
}
