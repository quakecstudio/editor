// SPDX-License-Identifier: GPL-3.0-or-later

//! The running game's entities as the IDE shows them (P3-13 to P3-15): every edict in use by its
//! classname, kept from the host's `inspect entities` answer and its `edict/spawned`,
//! `edict/removed` and `map/loaded` events, and the selected edict's fields, kept from
//! `inspect edict` and its `edict/changed` events (BRIDGE-3, BRIDGE-4).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// One field of the selected edict, with its bridge value (BRIDGE-5.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Field {
    /// The field's name.
    pub name: String,
    /// Its value: `{ "type": ..., "value": ... }`.
    pub value: Value,
}

impl Field {
    /// The value's type: `float`, `vector`, `string`, `entity`, `function` or `field`.
    #[must_use]
    pub fn kind(&self) -> &str {
        self.value.get("type").and_then(Value::as_str).unwrap_or("")
    }

    /// Whether it holds a function, whose definition a click opens.
    #[must_use]
    pub fn is_function(&self) -> bool {
        self.kind() == "function"
    }
}

/// The selected edict: its number, its classname and its fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Selected {
    /// The edict's number.
    pub edict: u32,
    /// Its classname.
    pub classname: String,
    /// Its fields, in the order the host gave them.
    pub fields: Vec<Field>,
}

impl Selected {
    /// The value of field `name`, written as the Properties panel shows it.
    #[must_use]
    pub fn shown(&self, name: &str, entities: &Entities) -> Option<String> {
        let field = self.fields.iter().find(|f| f.name == name)?;
        Some(entities.show(&field.value))
    }
}

/// Edicts of one classname, for the outliner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The classname, empty for an edict that has none.
    pub classname: String,
    /// The edicts, lowest first.
    pub edicts: Vec<u32>,
}

/// The game's edicts in use and the selected one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entities {
    rows: BTreeMap<u32, String>,
    selected: Option<Selected>,
}

/// The edict number a bridge value names (BRIDGE-5.1).
fn edict_of(value: &Value) -> Option<u32> {
    value
        .get("edict")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

/// A text field of a bridge object.
fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

impl Entities {
    /// No edict and no selection.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rows: BTreeMap::new(),
            selected: None,
        }
    }

    /// How many edicts are in use.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no edict is in use.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The classname of edict `edict`, when it is in use.
    #[must_use]
    pub fn classname(&self, edict: u32) -> Option<&str> {
        self.rows.get(&edict).map(String::as_str)
    }

    /// The selected edict, when there is one.
    #[must_use]
    pub const fn selected(&self) -> Option<&Selected> {
        self.selected.as_ref()
    }

    /// Empties the list and the selection, as when the game stops.
    pub fn clear(&mut self) {
        self.rows.clear();
        self.selected = None;
    }

    /// The list from an `inspect entities` answer (BRIDGE-4.9); a selected edict no longer in
    /// use is deselected.
    pub fn replace(&mut self, answer: &Value) {
        self.rows.clear();
        for entity in answer
            .get("entities")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(edict) = edict_of(entity) {
                self.rows.insert(edict, text(entity, "classname"));
            }
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|s| !self.rows.contains_key(&s.edict))
        {
            self.selected = None;
        }
    }

    /// The selection from an `inspect edict` answer (BRIDGE-4.9).
    pub fn select(&mut self, answer: &Value) {
        let Some(edict) = edict_of(answer) else {
            return;
        };
        let fields = answer
            .get("fields")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|f| {
                Some(Field {
                    name: f.get("name")?.as_str()?.to_owned(),
                    value: f.get("value")?.clone(),
                })
            })
            .collect();
        let classname = text(answer, "classname");
        self.rows.insert(edict, classname.clone());
        self.selected = Some(Selected {
            edict,
            classname,
            fields,
        });
    }

    /// Takes event `method` (BRIDGE-3.1): an edict spawned, removed or changed; `true` when the
    /// list must be read again, after `map/loaded`.
    pub fn take_event(&mut self, method: &str, params: &Value) -> bool {
        match method {
            "edict/spawned" => {
                if let Some(edict) = edict_of(params) {
                    self.rows.insert(edict, text(params, "classname"));
                }
            }
            "edict/removed" => {
                if let Some(edict) = edict_of(params) {
                    self.rows.remove(&edict);
                    if self.selected.as_ref().is_some_and(|s| s.edict == edict) {
                        self.selected = None;
                    }
                }
            }
            "edict/changed" => self.changed(params),
            "map/loaded" => return true,
            _ => {}
        }
        false
    }

    /// An `edict/changed` event: the selected edict's fields it names take their new values,
    /// and a new classname renames the row.
    fn changed(&mut self, params: &Value) {
        let Some(edict) = edict_of(params) else {
            return;
        };
        let changes = params
            .get("fields")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        for change in changes {
            let (Some(name), Some(value)) = (
                change.get("name").and_then(Value::as_str),
                change.get("value"),
            ) else {
                continue;
            };
            if name == "classname"
                && let Some(text) = value.get("value").and_then(Value::as_str)
            {
                self.rows.insert(edict, text.to_owned());
            }
            if let Some(selected) = self.selected.as_mut().filter(|s| s.edict == edict) {
                match selected.fields.iter_mut().find(|f| f.name == name) {
                    Some(field) => field.value = value.clone(),
                    None => selected.fields.push(Field {
                        name: name.to_owned(),
                        value: value.clone(),
                    }),
                }
            }
        }
    }

    /// The edicts grouped by classname, in classname order, those whose classname or number
    /// holds `filter` (without regard to ASCII case) only.
    #[must_use]
    pub fn groups(&self, filter: &str) -> Vec<Group> {
        let filter = filter.trim().to_ascii_lowercase();
        let mut groups: BTreeMap<&str, Vec<u32>> = BTreeMap::new();
        for (&edict, classname) in &self.rows {
            let matches = filter.is_empty()
                || classname.to_ascii_lowercase().contains(&filter)
                || edict.to_string().contains(&filter);
            if matches {
                groups.entry(classname).or_default().push(edict);
            }
        }
        groups
            .into_iter()
            .map(|(classname, edicts)| Group {
                classname: classname.to_owned(),
                edicts,
            })
            .collect()
    }

    /// A bridge value as the Properties panel writes it: a string in quotes, a vector in single
    /// quotes as QuakeC writes one, an entity as its classname and number, and a number,
    /// function or field as it is.
    #[must_use]
    pub fn show(&self, value: &Value) -> String {
        let inner = value.get("value");
        match value.get("type").and_then(Value::as_str) {
            Some("string") => format!("\"{}\"", inner.and_then(Value::as_str).unwrap_or("")),
            Some("vector") => {
                let parts: Vec<String> = inner
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(number)
                    .collect();
                format!("'{}'", parts.join(" "))
            }
            Some("entity") => {
                let edict = inner
                    .and_then(Value::as_u64)
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(0);
                let name = self.classname(edict).unwrap_or("entity");
                format!("{name} #{edict}")
            }
            Some("float") => inner.map(number).unwrap_or_default(),
            _ => inner.and_then(Value::as_str).map_or_else(
                || inner.map(Value::to_string).unwrap_or_default(),
                str::to_owned,
            ),
        }
    }
}

/// A number as QuakeC writes it: whole numbers with no point, and others in the fewest digits
/// that give back the same single-precision float, since QuakeC's floats are 32-bit and the
/// bridge widens them (`1.2140386`, not `1.214038610458374`).
fn number(value: &Value) -> String {
    match value.as_f64() {
        Some(n) if n.fract() == 0.0 && n.abs() < 1e15 => format!("{n:.0}"),
        Some(n) => {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a QuakeC float is an f32 the bridge widened; narrowing gives it back"
            )]
            let single = n as f32;
            format!("{single}")
        }
        None => value.to_string(),
    }
}

/// The bridge value of kind `kind` a user typed as `text`, as `field/set` takes it
/// (BRIDGE-5.1): a number, three numbers with or without single quotes, a string with or
/// without double quotes, an edict number with or without its `#`, or a function's name;
/// `None` when the text is not one.
#[must_use]
pub fn parse_value(kind: &str, text: &str) -> Option<Value> {
    let text = text.trim();
    let value = match kind {
        "float" => json!(text.parse::<f64>().ok()?),
        "vector" => {
            let parts: Vec<f64> = text
                .trim_matches('\'')
                .split_whitespace()
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            let [x, y, z] = parts.as_slice() else {
                return None;
            };
            json!([x, y, z])
        }
        "string" => {
            let inner = text
                .strip_prefix('"')
                .and_then(|t| t.strip_suffix('"'))
                .unwrap_or(text);
            json!(inner)
        }
        "entity" => {
            let number = text.rsplit('#').next().unwrap_or(text).trim();
            json!(number.parse::<u32>().ok()?)
        }
        "function" | "field" if !text.is_empty() => json!(text),
        _ => return None,
    };
    Some(json!({ "type": kind, "value": value }))
}

#[cfg(test)]
mod tests;
