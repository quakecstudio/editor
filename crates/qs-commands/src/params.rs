// SPDX-License-Identifier: GPL-3.0-or-later

//! A command's parameters as a form (UI-10): the fields its JSON Schema describes, and the
//! parameter object made from what was entered in them, for the palette's parameter prompt.

use std::fmt;

use serde_json::{Map, Value};

/// One choice of a field that takes one of a fixed set of values.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    /// The value it stands for.
    pub value: Value,
    /// What it means, if the schema says.
    pub description: Option<String>,
}

/// What a field takes.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    /// One of these values: an `enum`, or a `oneOf` of `const`s.
    Choice(Vec<Choice>),
    /// A string.
    Text,
    /// A number.
    Number,
    /// A whole number.
    Integer,
    /// True or false.
    Boolean,
    /// Any other value, entered as JSON.
    Json,
}

/// One parameter of a command, as its schema describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// The parameter's name.
    pub name: String,
    /// What it is, if the schema says.
    pub description: Option<String>,
    /// What it takes.
    pub kind: FieldKind,
    /// Whether the command needs it.
    pub required: bool,
}

/// What was entered in one field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    /// The text typed: a [`FieldKind::Text`], number or JSON value; empty for none.
    Text(String),
    /// The index of the choice picked, or `None`.
    Choice(Option<usize>),
    /// A [`FieldKind::Boolean`]'s value.
    Bool(bool),
}

impl Input {
    /// What a field of `kind` starts with: empty, nothing picked, or false.
    #[must_use]
    pub fn empty(kind: &FieldKind) -> Self {
        match kind {
            FieldKind::Choice(_) => Self::Choice(None),
            FieldKind::Boolean => Self::Bool(false),
            FieldKind::Text | FieldKind::Number | FieldKind::Integer | FieldKind::Json => {
                Self::Text(String::new())
            }
        }
    }
}

/// Why the entered values are not a parameter object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParamError {
    /// A required field has nothing in it.
    Missing(String),
    /// A number field holds something else.
    NotNumber(String),
    /// A whole-number field holds something else.
    NotInteger(String),
    /// A JSON field holds something that is not JSON.
    NotJson(String),
    /// The inputs do not match the fields.
    Mismatch(String),
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(name) => write!(f, "`{name}` is required"),
            Self::NotNumber(name) => write!(f, "`{name}` takes a number"),
            Self::NotInteger(name) => write!(f, "`{name}` takes a whole number"),
            Self::NotJson(name) => write!(f, "`{name}` takes a JSON value"),
            Self::Mismatch(name) => write!(f, "`{name}` was entered as the wrong kind"),
        }
    }
}

impl std::error::Error for ParamError {}

/// The fields of the parameter object `schema` describes, in the order of its `properties`:
/// required ones first, then the rest, each in name order. A `$ref` to the schema's `$defs`
/// is followed.
#[must_use]
pub fn fields(schema: &Value) -> Vec<Field> {
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut fields: Vec<Field> = properties
        .iter()
        .map(|(name, property)| {
            let target = resolve(schema, property);
            let description = property
                .get("description")
                .or_else(|| target.get("description"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            Field {
                name: name.clone(),
                description,
                kind: kind(target),
                required: required.contains(&name.as_str()),
            }
        })
        .collect();
    fields.sort_by(|a, b| {
        b.required
            .cmp(&a.required)
            .then_with(|| a.name.cmp(&b.name))
    });
    fields
}

/// `property`, or the definition its `$ref` names in `schema`'s `$defs`.
fn resolve<'a>(schema: &'a Value, property: &'a Value) -> &'a Value {
    property
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|reference| reference.strip_prefix("#/$defs/"))
        .and_then(|name| schema.get("$defs").and_then(|defs| defs.get(name)))
        .unwrap_or(property)
}

/// What a property's schema takes.
fn kind(schema: &Value) -> FieldKind {
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        let choices = values
            .iter()
            .map(|value| Choice {
                value: value.clone(),
                description: None,
            })
            .collect();
        return FieldKind::Choice(choices);
    }
    if let Some(options) = schema.get("oneOf").and_then(Value::as_array) {
        let choices: Option<Vec<Choice>> = options
            .iter()
            .map(|option| {
                option.get("const").map(|value| Choice {
                    value: value.clone(),
                    description: option
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                })
            })
            .collect();
        if let Some(choices) = choices {
            return FieldKind::Choice(choices);
        }
    }
    match schema.get("type").and_then(Value::as_str) {
        Some("string") => FieldKind::Text,
        Some("number") => FieldKind::Number,
        Some("integer") => FieldKind::Integer,
        Some("boolean") => FieldKind::Boolean,
        Some(_) | None => FieldKind::Json,
    }
}

/// The parameter object `inputs` make for `fields`, one input per field in order. A field left
/// empty is left out, which is an error for a required one.
///
/// # Errors
///
/// [`ParamError`] for a required field left empty, a number, whole number or JSON field that
/// does not hold one, or an input of the wrong kind for its field.
pub fn object(fields: &[Field], inputs: &[Input]) -> Result<Value, ParamError> {
    let mut object = Map::new();
    for (field, input) in fields.iter().zip(inputs) {
        let name = || field.name.clone();
        let value = match (&field.kind, input) {
            (FieldKind::Choice(choices), Input::Choice(picked)) => {
                picked.and_then(|i| choices.get(i)).map(|c| c.value.clone())
            }
            (FieldKind::Boolean, Input::Bool(value)) => Some(Value::Bool(*value)),
            (_, Input::Text(text)) if text.trim().is_empty() => None,
            (FieldKind::Text, Input::Text(text)) => Some(Value::String(text.clone())),
            (FieldKind::Number, Input::Text(text)) => {
                let number: f64 = text
                    .trim()
                    .parse()
                    .map_err(|_| ParamError::NotNumber(name()))?;
                Some(
                    serde_json::Number::from_f64(number)
                        .map(Value::Number)
                        .ok_or_else(|| ParamError::NotNumber(name()))?,
                )
            }
            (FieldKind::Integer, Input::Text(text)) => {
                let number: i64 = text
                    .trim()
                    .parse()
                    .map_err(|_| ParamError::NotInteger(name()))?;
                Some(Value::from(number))
            }
            (FieldKind::Json, Input::Text(text)) => {
                Some(serde_json::from_str(text).map_err(|_| ParamError::NotJson(name()))?)
            }
            _ => return Err(ParamError::Mismatch(name())),
        };
        match value {
            Some(value) => {
                object.insert(field.name.clone(), value);
            }
            None if field.required => return Err(ParamError::Missing(name())),
            None => {}
        }
    }
    if inputs.len() != fields.len() {
        let name = fields
            .get(inputs.len())
            .map_or_else(String::new, |f| f.name.clone());
        return Err(ParamError::Mismatch(name));
    }
    Ok(Value::Object(object))
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        reason = "test code: a failed unwrap or index is a failed test"
    )]

    use serde_json::json;

    use super::{Choice, FieldKind, Input, ParamError, fields, object};

    fn toggle_panel() -> serde_json::Value {
        json!({
            "$defs": {"Panel": {"description": "A panel.", "oneOf": [
                {"const": "left", "description": "Project.", "type": "string"},
                {"const": "right", "description": "Properties.", "type": "string"}
            ]}},
            "properties": {"panel": {"$ref": "#/$defs/Panel", "description": "The panel."}},
            "required": ["panel"],
            "type": "object"
        })
    }

    fn every_kind() -> serde_json::Value {
        json!({
            "properties": {
                "size": {"type": "number", "format": "float"},
                "count": {"type": "integer"},
                "name": {"type": "string", "description": "A name."},
                "on": {"type": "boolean"},
                "theme": {"enum": ["dark", "light"]},
                "value": {}
            },
            "required": ["size", "name"],
            "type": "object"
        })
    }

    // UI-10: a field per property, required ones first, each kind read from its schema, a
    // `$ref` followed into `$defs`.
    #[test]
    fn a_schema_is_its_fields() {
        let found = fields(&toggle_panel());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "panel");
        assert!(found[0].required);
        assert_eq!(found[0].description.as_deref(), Some("The panel."));
        assert_eq!(
            found[0].kind,
            FieldKind::Choice(vec![
                Choice {
                    value: json!("left"),
                    description: Some("Project.".to_owned()),
                },
                Choice {
                    value: json!("right"),
                    description: Some("Properties.".to_owned()),
                },
            ])
        );
        let all = fields(&every_kind());
        let names: Vec<&str> = all.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["name", "size", "count", "on", "theme", "value"]);
        let kinds: Vec<&FieldKind> = all.iter().map(|f| &f.kind).collect();
        assert_eq!(kinds[0], &FieldKind::Text);
        assert_eq!(kinds[1], &FieldKind::Number);
        assert_eq!(kinds[2], &FieldKind::Integer);
        assert_eq!(kinds[3], &FieldKind::Boolean);
        assert!(matches!(kinds[4], FieldKind::Choice(c) if c.len() == 2));
        assert_eq!(kinds[5], &FieldKind::Json);
        assert_eq!(all[0].description.as_deref(), Some("A name."));
        assert!(
            fields(&json!({"type": "object"})).is_empty(),
            "no parameters"
        );
        let unresolved = json!({"properties": {"x": {"$ref": "#/$defs/Gone", "type": "string"}}});
        assert_eq!(fields(&unresolved)[0].kind, FieldKind::Text);
        let mixed = json!({"properties": {"x": {"oneOf": [{"const": 1}, {"type": "string"}]}}});
        assert_eq!(fields(&mixed)[0].kind, FieldKind::Json, "not all constants");
    }

    // UI-10: what was entered becomes the parameter object; an empty optional field is left
    // out.
    #[test]
    fn inputs_make_the_parameter_object() {
        let toggle = fields(&toggle_panel());
        assert_eq!(
            object(&toggle, &[Input::Choice(Some(1))]).unwrap(),
            json!({"panel": "right"})
        );
        let all = fields(&every_kind());
        let text = |s: &str| Input::Text(s.to_owned());
        let inputs = [
            text("x"),
            text(" 1.5 "),
            text("3"),
            Input::Bool(true),
            Input::Choice(Some(0)),
            text("[1, 2]"),
        ];
        assert_eq!(
            object(&all, &inputs).unwrap(),
            json!({"name": "x", "size": 1.5, "count": 3, "on": true, "theme": "dark", "value": [1, 2]})
        );
        let sparse = [
            text("x"),
            text("2"),
            text(""),
            Input::Bool(false),
            Input::Choice(None),
            text(" "),
        ];
        assert_eq!(
            object(&all, &sparse).unwrap(),
            json!({"name": "x", "size": 2.0, "on": false})
        );
        assert_eq!(object(&[], &[]).unwrap(), json!({}));
    }

    // UI-10: each way the entered values fail to make a parameter object.
    #[test]
    fn bad_inputs_are_refused() {
        let all = fields(&every_kind());
        let text = |s: &str| Input::Text(s.to_owned());
        let with = |i: usize, input: Input| {
            let mut inputs = vec![
                text("x"),
                text("1"),
                text(""),
                Input::Bool(false),
                Input::Choice(None),
                text(""),
            ];
            inputs[i] = input;
            object(&all, &inputs)
        };
        assert_eq!(
            with(0, text(" ")),
            Err(ParamError::Missing("name".to_owned()))
        );
        assert_eq!(
            with(1, text("big")),
            Err(ParamError::NotNumber("size".to_owned()))
        );
        assert_eq!(
            with(1, text("NaN")),
            Err(ParamError::NotNumber("size".to_owned()))
        );
        assert_eq!(
            with(2, text("1.5")),
            Err(ParamError::NotInteger("count".to_owned()))
        );
        assert_eq!(
            with(5, text("{")),
            Err(ParamError::NotJson("value".to_owned()))
        );
        assert_eq!(
            with(3, text("yes")),
            Err(ParamError::Mismatch("on".to_owned()))
        );
        assert_eq!(
            object(&all, &[text("x")]),
            Err(ParamError::Mismatch("size".to_owned())),
            "too few inputs"
        );
        let toggle = fields(&toggle_panel());
        assert_eq!(
            object(&toggle, &[Input::Choice(None)]),
            Err(ParamError::Missing("panel".to_owned()))
        );
        assert_eq!(
            object(&toggle, &[Input::Choice(Some(9))]),
            Err(ParamError::Missing("panel".to_owned())),
            "a choice past the end is no choice"
        );
        assert_eq!(
            ParamError::Missing("x".to_owned()).to_string(),
            "`x` is required"
        );
        assert_eq!(Input::empty(&FieldKind::Boolean), Input::Bool(false));
        assert_eq!(Input::empty(&FieldKind::Number), text(""));
        assert_eq!(
            Input::empty(&FieldKind::Choice(Vec::new())),
            Input::Choice(None)
        );
    }
}
