// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::indexing_slicing,
    reason = "test code: a failed index is a failed test"
)]

use qs_bridge::check_schema;
use qs_sim::{NamedValue, QcValue};
use serde_json::json;

use super::{from_json, named_json, to_json};

fn every() -> Vec<QcValue> {
    vec![
        QcValue::Float(1.5),
        QcValue::Vector([1.0, -2.0, 3.0]),
        QcValue::String(b"player".to_vec()),
        QcValue::Entity(4),
        QcValue::Field("health".to_owned()),
        QcValue::Function("tick".to_owned()),
        QcValue::Void,
    ]
}

// spec BRIDGE-5.1: each type is written with its type and its value, as the value schema
// says, and read back as the same value
#[test]
fn values_round_trip_through_the_schema() {
    for value in every() {
        let written = to_json(&value);
        let params = json!({ "edict": 1, "field": "f", "value": written });
        assert!(
            check_schema("field.set.params", &params).is_ok(),
            "{written}"
        );
        assert_eq!(from_json(&written), Some(value));
    }
    assert_eq!(from_json(&json!({ "type": "matrix", "value": 1 })), None);
    assert_eq!(from_json(&json!({ "type": "float" })), None);
}

// spec BRIDGE-5.1: a field's or a global's value is a fieldValue, its name and its value
#[test]
fn named_values_are_field_values() {
    let fields = named_json(&[NamedValue {
        name: "health".to_owned(),
        value: QcValue::Float(100.0),
    }]);
    assert_eq!(fields[0]["name"], "health");
    let result = json!({ "time": 1.0, "edict": 1, "classname": "player", "fields": fields });
    assert!(check_schema("inspect.result", &result).is_ok());
}
