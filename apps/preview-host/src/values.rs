// SPDX-License-Identifier: GPL-3.0-or-later

//! BRIDGE-5: QuakeC values as the bridge writes them, `{ "type": ..., "value": ... }`, and read
//! back for `field/set`.

use qs_sim::{NamedValue, QcValue};
use serde_json::{Value, json};

/// `value` as a bridge value (BRIDGE-5.1); a string's bytes that are not UTF-8 are replaced.
pub(crate) fn to_json(value: &QcValue) -> Value {
    match value {
        QcValue::Float(f) => json!({ "type": "float", "value": f }),
        QcValue::Vector(v) => json!({ "type": "vector", "value": v }),
        QcValue::String(text) => {
            json!({ "type": "string", "value": String::from_utf8_lossy(text) })
        }
        QcValue::Entity(n) => json!({ "type": "entity", "value": n }),
        QcValue::Field(name) => json!({ "type": "field", "value": name }),
        QcValue::Function(name) => json!({ "type": "function", "value": name }),
        QcValue::Void => json!({ "type": "void" }),
    }
}

/// Each named value as a `fieldValue` (BRIDGE-5.1).
pub(crate) fn named_json(values: &[NamedValue]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|named| json!({ "name": named.name, "value": to_json(&named.value) }))
            .collect(),
    )
}

/// The QuakeC value a bridge value names, `None` for one that does not read; the schema has
/// checked its shape (BRIDGE-4.2).
pub(crate) fn from_json(value: &Value) -> Option<QcValue> {
    let inner = value.get("value");
    let number = |v: &Value| v.as_f64().map(f64_to_f32);
    Some(match value.get("type")?.as_str()? {
        "float" => QcValue::Float(number(inner?)?),
        "vector" => {
            let items = inner?.as_array()?;
            let mut vector = [0.0; 3];
            for (out, item) in vector.iter_mut().zip(items) {
                *out = number(item)?;
            }
            QcValue::Vector(vector)
        }
        "string" => QcValue::String(inner?.as_str()?.as_bytes().to_vec()),
        "entity" => QcValue::Entity(usize::try_from(inner?.as_u64()?).ok()?),
        "field" => QcValue::Field(inner?.as_str()?.to_owned()),
        "function" => QcValue::Function(inner?.as_str()?.to_owned()),
        "void" => QcValue::Void,
        _ => return None,
    })
}

/// A JSON number as the float a QuakeC value holds, rounded as C rounds a double to a float.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a bridge number is a float's value, which C's (float) cast rounds to"
)]
const fn f64_to_f32(value: f64) -> f32 {
    value as f32
}

#[cfg(test)]
mod tests;
