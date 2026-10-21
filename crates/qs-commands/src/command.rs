// SPDX-License-Identifier: GPL-3.0-or-later

//! What a command is: the trait a command implements, what it returns and how it is undone.

use std::error::Error;

use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// A command's own failure, or an undo step's, boxed so one registry holds commands with
/// different error types.
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// One action a person, the CLI, the agent or MCP can run (PRIN-3).
///
/// A command works on a context `C`, the state the session owns: an editor workspace, the
/// app's window layout, a test fixture. A command that only works with part of a context is
/// generic over it, `impl<C: HasDocument> Command<C> for Insert`, so one command serves every
/// context that has that part.
///
/// The parameters are a Rust type, so the registry derives their JSON Schema from it and every
/// caller's arguments are checked against it by deserialising them before [`Command::run`]
/// sees them. They must be a JSON object, as MCP requires of a tool's `inputSchema`; a command
/// with no parameters uses [`NoParams`].
pub trait Command<C>: 'static {
    /// The stable id, a [`crate::CommandId`]: callers and saved keybindings name the command
    /// by it, so it never changes once shipped.
    const ID: &'static str;

    /// One sentence saying what the command does, shown in the palette, `commands.list`, the
    /// generated reference and as the agent tool's description.
    const SUMMARY: &'static str;

    /// The parameters. Doc comments on the type and its fields become the schema's
    /// descriptions.
    type Params: DeserializeOwned + JsonSchema;

    /// What the command returns to its caller, serialised as JSON.
    type Output: Serialize;

    /// The command's own failure.
    type Error: Error + Send + Sync + 'static;

    /// Runs the command on `context`.
    ///
    /// # Errors
    ///
    /// The command's own error. A command that fails leaves `context` as it found it, so the
    /// session's undo history still matches the context.
    fn run(
        &self,
        context: &mut C,
        params: Self::Params,
    ) -> Result<Outcome<Self::Output, C>, Self::Error>;
}

/// The parameters of a command that takes none: the empty JSON object `{}`, and nothing else.
/// Its schema is the one MCP recommends for a tool with no parameters,
/// `{ "type": "object", "additionalProperties": false }` (MCP 2026-07-28, Tools, Tool).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(
    title = "No parameters",
    description = "The command takes no parameters: pass the empty object {}."
)]
pub struct NoParams {}

/// What a command that ran returns: its output, and how to undo what it changed.
pub struct Outcome<O, C> {
    output: O,
    effect: Effect<C>,
}

impl<O, C> Outcome<O, C> {
    /// A command that read `context` and changed nothing: undo has nothing to do for it.
    pub const fn unchanged(output: O) -> Self {
        Self {
            output,
            effect: Effect::Unchanged,
        }
    }

    /// A command that changed `context`; `step` undoes and redoes the change.
    pub fn undoable(output: O, step: impl UndoStep<C>) -> Self {
        Self {
            output,
            effect: Effect::Undoable(Box::new(step)),
        }
    }

    pub(crate) fn into_parts(self) -> (O, Effect<C>) {
        (self.output, self.effect)
    }
}

/// Whether a command changed its context, and if so the step that undoes it.
pub(crate) enum Effect<C> {
    Unchanged,
    Undoable(Box<dyn UndoStep<C>>),
}

/// How to take back one command's change and make it again (PRIN-3).
///
/// The session calls [`UndoStep::undo`] and [`UndoStep::redo`] alternately, starting with
/// `undo`, each on the context as the other left it.
pub trait UndoStep<C>: 'static {
    /// Takes the change back.
    ///
    /// # Errors
    ///
    /// Why it could not. A step that fails leaves the context as it found it, and stays next
    /// in line to be undone.
    fn undo(&mut self, context: &mut C) -> Result<(), BoxError>;

    /// Makes the change again after [`UndoStep::undo`].
    ///
    /// # Errors
    ///
    /// Why it could not. A step that fails leaves the context as it found it, and stays next
    /// in line to be redone.
    fn redo(&mut self, context: &mut C) -> Result<(), BoxError>;
}
