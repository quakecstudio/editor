// SPDX-License-Identifier: GPL-3.0-or-later

//! The registry: every command by id, with its parameter schema.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::builtin::Builtin;
use crate::command::{Command, Effect};
use crate::error::{CommandError, RegisterError};
use crate::id::CommandId;

/// What the registry tells callers about one command: the `commands.list` entry, the palette
/// row and the MCP tool definition.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CommandInfo {
    id: CommandId,
    summary: &'static str,
    params: Value,
}

impl CommandInfo {
    pub(crate) const fn new(id: CommandId, summary: &'static str, params: Value) -> Self {
        Self {
            id,
            summary,
            params,
        }
    }

    /// The command's stable id.
    #[must_use]
    pub const fn id(&self) -> &CommandId {
        &self.id
    }

    /// One sentence saying what it does.
    #[must_use]
    pub const fn summary(&self) -> &'static str {
        self.summary
    }

    /// The JSON Schema (2020-12) of its parameters, always of `"type": "object"`.
    #[must_use]
    pub const fn params(&self) -> &Value {
        &self.params
    }
}

/// A command with its type erased, so commands with different parameter, output and error
/// types share one registry. Implemented for every [`Command`].
///
/// It is `Send + Sync`, as [`Command`] is, so a [`Registry`] is `Send + Sync` and a session
/// over a `Send` context is `Send`.
pub(crate) trait ErasedCommand<C>: Send + Sync {
    /// Deserialises `params`, runs the command and serialises its output.
    fn invoke(
        &self,
        id: &CommandId,
        context: &mut C,
        params: Value,
    ) -> Result<Invoked<C>, CommandError>;
}

/// What a command that ran returned: its output as JSON, if it serialised, and its effect.
/// The effect is kept apart from the output's `Result`, so a change stays undoable when only
/// the output fails to serialise.
pub(crate) struct Invoked<C> {
    pub(crate) output: Result<Value, serde_json::Error>,
    pub(crate) effect: Effect<C>,
}

impl<C, T: Command<C>> ErasedCommand<C> for T {
    fn invoke(
        &self,
        id: &CommandId,
        context: &mut C,
        params: Value,
    ) -> Result<Invoked<C>, CommandError> {
        let params =
            serde_json::from_value(params).map_err(|error| CommandError::InvalidParams {
                command: id.clone(),
                error,
            })?;
        let outcome = self
            .run(context, params)
            .map_err(|error| CommandError::Failed {
                command: id.clone(),
                error: Box::new(error),
            })?;
        let (output, effect) = outcome.into_parts();
        Ok(Invoked {
            output: serde_json::to_value(output),
            effect,
        })
    }
}

/// Who carries out a command.
pub(crate) enum Handler<C> {
    /// One of the session's own commands, which work on the session rather than the context.
    Builtin(Builtin),
    /// A registered command.
    Command(Box<dyn ErasedCommand<C>>),
}

pub(crate) struct Entry<C> {
    pub(crate) info: CommandInfo,
    pub(crate) handler: Handler<C>,
}

/// Every command a session can run, keyed by id (PRIN-3).
///
/// A new registry holds the built-in commands ([`crate::BUILTIN_IDS`]); [`Registry::register`]
/// adds the rest. Listing is in id order, so the generated reference and `commands.list` are
/// stable.
pub struct Registry<C> {
    entries: BTreeMap<CommandId, Entry<C>>,
}

impl<C: 'static> Registry<C> {
    /// A registry holding only the built-in commands.
    #[must_use]
    pub fn new() -> Self {
        let mut entries = BTreeMap::new();
        for builtin in Builtin::ALL {
            let info = builtin.info();
            entries.insert(
                info.id.clone(),
                Entry {
                    info,
                    handler: Handler::Builtin(builtin),
                },
            );
        }
        Self { entries }
    }

    /// Adds `command`.
    ///
    /// # Errors
    ///
    /// [`RegisterError`] when its id is malformed or taken, or its parameters are not a JSON
    /// object. The registry is unchanged.
    pub fn register<T: Command<C>>(&mut self, command: T) -> Result<(), RegisterError> {
        let id =
            CommandId::new(T::ID).map_err(|error| RegisterError::InvalidId { id: T::ID, error })?;
        if self.entries.contains_key(&id) {
            return Err(RegisterError::Duplicate(id));
        }
        let params = schema_of::<T::Params>();
        if params.get("type").and_then(Value::as_str) != Some("object") {
            return Err(RegisterError::ParamsNotObject(id));
        }
        let info = CommandInfo {
            id: id.clone(),
            summary: T::SUMMARY,
            params,
        };
        self.entries.insert(
            id,
            Entry {
                info,
                handler: Handler::Command(Box::new(command)),
            },
        );
        Ok(())
    }

    /// Every command, in id order.
    pub fn commands(&self) -> impl Iterator<Item = &CommandInfo> {
        self.entries.values().map(|entry| &entry.info)
    }

    /// The command with id `id`, if there is one.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&CommandInfo> {
        self.entries.get(id).map(|entry| &entry.info)
    }

    pub(crate) fn entry(&self, id: &str) -> Option<&Entry<C>> {
        self.entries.get(id)
    }

    /// The command reference, `docs/commands.md`: every command's id, summary and parameter
    /// schema, in id order.
    #[must_use]
    pub fn markdown(&self) -> String {
        let mut out = String::from(
            "<!-- Generated by `cargo xtask commands` from the qs-commands registry. \
             Do not edit by hand. -->\n\n\
             # Commands\n\n\
             Every command in QuakeC Studio's command registry (PRIN-3): the one list behind the \
             menus, keystrokes, command palette, CLI, agent tools and MCP tools. Each command \
             has a stable id and takes one JSON object as its parameters, described by the \
             JSON Schema (2020-12) below. Commands that change state can be undone with \
             `edit.undo`.\n\n\
             | Command | Summary |\n| --- | --- |\n",
        );
        // The heading `# Commands` already has the anchor `commands`.
        let mut anchors = Anchors::new(["commands"]);
        for info in self.commands() {
            let cell = info.summary.replace('|', "\\|").replace('\n', " ");
            out.push_str(&format!(
                "| [`{}`](#{}) | {cell} |\n",
                info.id,
                anchors.next(info.id.as_str())
            ));
        }
        for info in self.commands() {
            // Serialising a `Value` cannot fail.
            let schema = serde_json::to_string_pretty(&info.params).unwrap_or_default();
            out.push_str(&format!(
                "\n## `{}`\n\n{}\n\nParameters:\n\n```json\n{schema}\n```\n",
                info.id, info.summary
            ));
        }
        out
    }
}

impl<C: 'static> Default for Registry<C> {
    fn default() -> Self {
        Self::new()
    }
}

/// The JSON Schema of `T` as a JSON value.
pub(crate) fn schema_of<T: schemars::JsonSchema>() -> Value {
    schemars::schema_for!(T).to_value()
}

/// The anchors GitHub gives a document's headings, in order: the heading text lowercased with
/// punctuation other than `-` and `_` dropped (an id is lowercase already, so only its `.`
/// goes), and `-1`, `-2` and so on after a text an earlier heading already had.
struct Anchors {
    seen: BTreeMap<String, usize>,
}

impl Anchors {
    /// Anchors for the headings after those with the texts `earlier`.
    fn new<const N: usize>(earlier: [&str; N]) -> Self {
        let mut anchors = Self {
            seen: BTreeMap::new(),
        };
        for text in earlier {
            anchors.next(text);
        }
        anchors
    }

    /// The anchor of the next heading, `` ## `id` ``.
    fn next(&mut self, id: &str) -> String {
        let base: String = id.chars().filter(|&c| c != '.').collect();
        let count = self.seen.entry(base.clone()).or_insert(0);
        let anchor = match *count {
            0 => base,
            n => format!("{base}-{n}"),
        };
        *count = count.saturating_add(1);
        anchor
    }
}
