// SPDX-License-Identifier: GPL-3.0-or-later

//! The command line (UI-21, P7-17), as AutoCAD's command window: a command is typed by its id or
//! a short alias, with a `/` before it or not, as a chat's slash commands are, and each of its
//! parameters is asked for in turn from its schema, a required one until it is given and an
//! optional one taking its default when nothing is typed. What the line makes is a command id
//! and a parameter object, run as any other command is; the window draws the line, lists what
//! [`Registry::listed`] finds for what is typed, and runs what the line makes.

use serde_json::{Map, Value};

use crate::palette::search;
use crate::params::{Field, FieldKind, Input, ParamError, fields, object};
use crate::registry::{CommandInfo, Registry};

/// What the line asks for next: one parameter of the command being entered.
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    /// The command being entered.
    pub command: String,
    /// The parameter asked for.
    pub field: Field,
    /// Its place among the command's parameters, from 1, and how many there are.
    pub place: (usize, usize),
}

/// What entering a line did.
#[derive(Clone, Debug, PartialEq)]
pub enum Entered {
    /// Nothing was typed and nothing is being entered.
    Nothing,
    /// The next parameter is asked for.
    Ask(Prompt),
    /// The command is complete: run it with these parameters.
    Run {
        /// The command's id.
        id: String,
        /// Its parameter object.
        params: Value,
    },
    /// No command is named so.
    Unknown(String),
    /// Several commands are named so: their ids.
    Ambiguous(Vec<String>),
    /// What was typed is no value for the parameter, which is asked for again.
    Refused(ParamError),
}

/// A command being entered: its parameters, and the values given so far.
#[derive(Clone, Debug, PartialEq)]
struct Entering {
    command: String,
    fields: Vec<Field>,
    given: Map<String, Value>,
    at: usize,
}

/// The command line's state: the command being entered, and the lines entered before.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommandLine {
    entering: Option<Entering>,
    history: Vec<String>,
}

impl<C: 'static> Registry<C> {
    /// The commands `typed` names: the one whose id it is; else those whose name after the
    /// last `.` it is (`undo` for `edit.undo`); else those whose id or name starts with it.
    /// One is the command; several are a choice to narrow.
    ///
    /// One `/` before the name, and the space around it, are no part of it.
    #[must_use]
    pub fn named(&self, typed: &str) -> Vec<&CommandInfo> {
        self.named_plain(spoken(typed))
    }

    /// As [`Registry::named`], of a name with its `/` and the space around it taken off
    /// already, so that they are taken off once.
    fn named_plain(&self, typed: &str) -> Vec<&CommandInfo> {
        if typed.is_empty() {
            return Vec::new();
        }
        if let Some(exact) = self.get(typed) {
            return vec![exact];
        }
        let name = |info: &&CommandInfo| {
            let id = info.id().as_str();
            id.rsplit('.').next().unwrap_or(id).to_owned()
        };
        let named: Vec<&CommandInfo> = self.commands().filter(|info| name(info) == typed).collect();
        if !named.is_empty() {
            return named;
        }
        self.commands()
            .filter(|info| info.id().as_str().starts_with(typed) || name(info).starts_with(typed))
            .collect()
    }

    /// The commands to offer for what is `typed` (L-44): those it names ([`Registry::named`])
    /// first, then those whose summary or id it matches as the palette matches
    /// ([`search`]), best first, each once. With nothing typed, or `/` alone,
    /// every command, in the registry's order.
    #[must_use]
    pub fn listed(&self, typed: &str) -> Vec<&CommandInfo> {
        let typed = spoken(typed);
        if typed.is_empty() {
            return self.commands().collect();
        }
        let mut listed = self.named_plain(typed);
        for found in search(self.commands(), typed) {
            if !listed.iter().any(|info| info.id() == found.info.id()) {
                listed.push(found.info);
            }
        }
        listed
    }
}

/// The mark a command may be typed after, as a chat's slash commands are.
const SLASH: char = '/';

/// What `typed` says with the space around it and one [`SLASH`] before it taken off.
fn spoken(typed: &str) -> &str {
    let typed = typed.trim();
    typed.strip_prefix(SLASH).unwrap_or(typed).trim_start()
}

impl CommandLine {
    /// A line with nothing entered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// What the line asks for now, when a command is being entered.
    #[must_use]
    pub fn prompt(&self) -> Option<Prompt> {
        let entering = self.entering.as_ref()?;
        let field = entering.fields.get(entering.at)?.clone();
        Some(Prompt {
            command: entering.command.clone(),
            field,
            place: (entering.at.saturating_add(1), entering.fields.len()),
        })
    }

    /// The lines that named a command, oldest first, for the Up and Down keys.
    #[must_use]
    pub fn history(&self) -> &[String] {
        &self.history
    }

    /// Gives up the command being entered.
    pub fn cancel(&mut self) {
        self.entering = None;
    }

    /// What `typed` may be completed to: the ids of the commands it names while none is being
    /// entered, and the choices it starts while one of a fixed set is asked for.
    #[must_use]
    pub fn completions<C: 'static>(&self, registry: &Registry<C>, typed: &str) -> Vec<String> {
        match self.prompt() {
            None => registry
                .named(typed)
                .into_iter()
                .map(|info| info.id().as_str().to_owned())
                .collect(),
            Some(prompt) => match prompt.field.kind {
                FieldKind::Choice(choices) => choices
                    .iter()
                    .map(|choice| plain(&choice.value))
                    .filter(|value| value.starts_with(typed.trim()))
                    .collect(),
                FieldKind::Text
                | FieldKind::Number
                | FieldKind::Integer
                | FieldKind::Boolean
                | FieldKind::Json => Vec::new(),
            },
        }
    }

    /// Enters the line `typed`: the name of a command while none is being entered, else the
    /// value of the parameter asked for, nothing typed taking an optional one's default.
    pub fn enter<C: 'static>(&mut self, registry: &Registry<C>, typed: &str) -> Entered {
        let typed = typed.trim();
        if self.entering.is_none() {
            // A command's name may follow a `/`; a parameter's value is taken as typed, a path
            // that starts with one among them.
            let typed = spoken(typed);
            let found = registry.named_plain(typed);
            let info = match found.as_slice() {
                [] if typed.is_empty() => return Entered::Nothing,
                [] => return Entered::Unknown(typed.to_owned()),
                [info] => {
                    // Only a line that named a command is brought back: a parameter's value
                    // is no command.
                    self.history.push(typed.to_owned());
                    *info
                }
                several => {
                    return Entered::Ambiguous(
                        several
                            .iter()
                            .map(|info| info.id().as_str().to_owned())
                            .collect(),
                    );
                }
            };
            self.entering = Some(Entering {
                command: info.id().as_str().to_owned(),
                fields: fields(info.params()),
                given: Map::new(),
                at: 0,
            });
            return self.next();
        }
        if let Err(refused) = self.give(typed) {
            return Entered::Refused(refused);
        }
        self.next()
    }

    /// Takes `typed` as the value of the parameter asked for.
    fn give(&mut self, typed: &str) -> Result<(), ParamError> {
        let Some(entering) = &mut self.entering else {
            return Ok(());
        };
        let Some(field) = entering.fields.get(entering.at) else {
            return Ok(());
        };
        let input = match &field.kind {
            FieldKind::Choice(choices) => {
                let picked = choices
                    .iter()
                    .position(|choice| plain(&choice.value) == typed)
                    .or_else(|| {
                        let mut starting = choices
                            .iter()
                            .enumerate()
                            .filter(|(_, choice)| plain(&choice.value).starts_with(typed));
                        match (starting.next(), starting.next()) {
                            (Some((index, _)), None) if !typed.is_empty() => Some(index),
                            _ => None,
                        }
                    });
                if picked.is_none() && !typed.is_empty() {
                    return Err(ParamError::Mismatch(field.name.clone()));
                }
                Input::Choice(picked)
            }
            FieldKind::Boolean if typed.is_empty() => Input::Text(String::new()),
            FieldKind::Boolean => match typed {
                "yes" | "y" | "true" | "on" => Input::Bool(true),
                "no" | "n" | "false" | "off" => Input::Bool(false),
                _ => return Err(ParamError::Mismatch(field.name.clone())),
            },
            FieldKind::Text | FieldKind::Number | FieldKind::Integer | FieldKind::Json => {
                Input::Text(typed.to_owned())
            }
        };
        // A boolean left empty is left out, as any optional value is.
        let one = match (&field.kind, &input) {
            (FieldKind::Boolean, Input::Text(_)) if field.required => {
                return Err(ParamError::Missing(field.name.clone()));
            }
            (FieldKind::Boolean, Input::Text(_)) => Value::Object(Map::new()),
            _ => object(core::slice::from_ref(field), core::slice::from_ref(&input))?,
        };
        if let Value::Object(one) = one {
            entering.given.extend(one);
        }
        entering.at = entering.at.saturating_add(1);
        Ok(())
    }

    /// The next parameter to ask for, or the finished command.
    fn next(&mut self) -> Entered {
        if let Some(prompt) = self.prompt() {
            return Entered::Ask(prompt);
        }
        match self.entering.take() {
            Some(done) => Entered::Run {
                id: done.command,
                params: Value::Object(done.given),
            },
            None => Entered::Nothing,
        }
    }
}

/// A choice's value as it is typed: a string's text, any other value as JSON.
fn plain(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
