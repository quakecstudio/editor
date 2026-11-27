// SPDX-License-Identifier: GPL-3.0-or-later

//! What may be written at a place in a settings file (UI-24): a table's name in a `[...]`
//! header, a key of the table the place is in, and a value of the key before `=`, each with
//! what it does. Read from the settings file's schema and each [`Setting`]'s summary, as Zed
//! completes `settings.json` from its JSON schema.

use schemars::schema_for;
use serde_json::{Map, Value};

use crate::{Setting, SettingsFile};

/// One thing that may be written at the place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsCompletion {
    /// The text written: a table's name, a key, or a value as TOML writes it.
    pub label: String,
    /// What it is or does.
    pub detail: String,
}

/// What the place is in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place<'a> {
    /// In a `[...]` header, where a table's name goes.
    Header,
    /// At the start of a line in the table named so, or before any table, where a key goes.
    Key(Option<&'a str>),
    /// After `key =` in the table named so: the value.
    Value {
        /// The table, `None` before any.
        table: Option<&'a str>,
        /// The key.
        key: &'a str,
        /// Whether a `"` opens the value already.
        quoted: Quoted,
    },
}

/// Whether a value's `"` is written already.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Quoted {
    /// It is: a string value is written without its quotes.
    Open,
    /// It is not: a string value is written with them.
    Not,
}

impl SettingsFile {
    /// What may be written at byte `offset` of the settings file `text` (UI-24): in a header,
    /// the tables; at a key's place, the keys of the table it is in, or before any table the
    /// tables as headers; after `key =`, the values the key takes when they are a known set
    /// (the choices of its type, or `true` and `false`). Each with what it does.
    #[must_use]
    pub fn completions(text: &str, offset: usize) -> Vec<SettingsCompletion> {
        let before = text.get(..offset).unwrap_or(text);
        let line_start = before.rfind('\n').map_or(0, |at| at.saturating_add(1));
        let line = before.get(line_start..).unwrap_or_default();
        let table = before
            .get(..line_start)
            .unwrap_or_default()
            .lines()
            .rev()
            .find_map(header_name);
        let schema = schema_for!(SettingsFile);
        let root = schema.as_value();
        let tables = root.get("properties").and_then(Value::as_object);
        match Place::of(line, table) {
            Place::Header => tables.map_or_default(|tables| {
                tables
                    .iter()
                    .map(|(name, table)| SettingsCompletion {
                        label: name.clone(),
                        detail: described(table),
                    })
                    .collect()
            }),
            Place::Key(None) => tables.map_or_default(|tables| {
                tables
                    .iter()
                    .map(|(name, table)| SettingsCompletion {
                        label: format!("[{name}]"),
                        detail: described(table),
                    })
                    .collect()
            }),
            Place::Key(Some(table)) => keys_of(root, table).map_or_default(|keys| {
                keys.iter()
                    .map(|(key, schema)| SettingsCompletion {
                        label: key.clone(),
                        detail: Setting::at(table, key)
                            .map_or_else(|| described(schema), |s| s.summary().to_owned()),
                    })
                    .collect()
            }),
            Place::Value { table, key, quoted } => {
                let Some(schema) = table
                    .and_then(|table| keys_of(root, table))
                    .and_then(|keys| keys.get(key))
                else {
                    return Vec::new();
                };
                values_of(root, schema, quoted)
            }
        }
    }
}

impl<'a> Place<'a> {
    /// The place at the end of `line`, the text of its line before it, in `table`.
    fn of(line: &'a str, table: Option<&'a str>) -> Self {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            return Self::Header;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            return Self::Key(table);
        };
        let quoted = if value.trim_start().starts_with('"') {
            Quoted::Open
        } else {
            Quoted::Not
        };
        Self::Value {
            table,
            key: key.trim(),
            quoted,
        }
    }
}

impl Setting {
    /// The setting kept in `table` under `key`.
    fn at(table: &str, key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|setting| setting.place() == (table, key))
    }
}

/// The table a `[name]` or `[[name]]` header line names, its first part: `agent` for
/// `[agent.tool_permissions]`.
fn header_name(line: &str) -> Option<&str> {
    let inner = line.trim().strip_prefix('[')?;
    let inner = inner.trim_start_matches('[');
    let name = inner.split(']').next()?.trim();
    name.split('.').next().map(str::trim)
}

/// The schema `schema` stands for: the definition a `$ref` names, or the one of an `anyOf`
/// that is not `null`, as schemars writes an `Option`.
fn resolved<'a>(root: &'a Value, schema: &'a Value) -> &'a Value {
    if let Some(name) = schema
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        && let Some(found) = root.get("$defs").and_then(|defs| defs.get(name))
    {
        return resolved(root, found);
    }
    if let Some(choices) = schema.get("anyOf").and_then(Value::as_array)
        && let Some(found) = choices
            .iter()
            .find(|choice| choice.get("type").and_then(Value::as_str) != Some("null"))
    {
        return resolved(root, found);
    }
    schema
}

/// The keys of `table`, by name, with their schemas.
fn keys_of<'a>(root: &'a Value, table: &str) -> Option<&'a Map<String, Value>> {
    let schema = root.get("properties")?.get(table)?;
    resolved(root, schema).get("properties")?.as_object()
}

/// What `schema` says of itself, or of what it stands for.
fn described(schema: &Value) -> String {
    schema
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The values the key of `schema` takes when they are a known set, each as TOML writes it:
/// a choice of its type with what the choice's comment says, or `true` and `false`.
fn values_of(root: &Value, schema: &Value, quoted: Quoted) -> Vec<SettingsCompletion> {
    let schema = resolved(root, schema);
    let written = |value: &str| match quoted {
        Quoted::Open => value.to_owned(),
        Quoted::Not => format!("\"{value}\""),
    };
    if let Some(choices) = schema.get("oneOf").and_then(Value::as_array) {
        return choices
            .iter()
            .filter_map(|choice| {
                let value = choice.get("const")?.as_str()?;
                Some(SettingsCompletion {
                    label: written(value),
                    detail: described(choice),
                })
            })
            .collect();
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        return values
            .iter()
            .filter_map(Value::as_str)
            .map(|value| SettingsCompletion {
                label: written(value),
                detail: String::new(),
            })
            .collect();
    }
    if schema.get("type").and_then(Value::as_str) == Some("boolean") {
        return ["true", "false"]
            .into_iter()
            .map(|value| SettingsCompletion {
                label: value.to_owned(),
                detail: String::new(),
            })
            .collect();
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
