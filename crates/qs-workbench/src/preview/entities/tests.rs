// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use serde_json::json;

use super::{Entities, Group, parse_value};

fn listed() -> Entities {
    let mut entities = Entities::default();
    entities.replace(&json!({ "entities": [
        { "edict": 0, "classname": "worldspawn" },
        { "edict": 1, "classname": "player" },
        { "edict": 7, "classname": "light" },
        { "edict": 5, "classname": "light" },
        { "classname": "no number" }
    ], "time": 1.0 }));
    entities
}

// spec BRIDGE-4.1: the outliner lists inspect entities' answer grouped by classname, filtered by
// a classname or a number without regard to case
#[test]
fn the_list_is_grouped_and_filtered() {
    let entities = listed();
    assert_eq!(entities.len(), 4);
    assert!(!entities.is_empty());
    let groups = entities.groups("");
    let names: Vec<_> = groups.iter().map(|g| g.classname.as_str()).collect();
    assert_eq!(names, ["light", "player", "worldspawn"]);
    assert_eq!(groups[0].edicts, [5, 7]);
    assert_eq!(
        entities.groups("PLAY"),
        [Group {
            classname: "player".to_owned(),
            edicts: vec![1],
        }]
    );
    assert_eq!(entities.groups(" 7 ")[0].edicts, [7]);
    assert!(entities.groups("nothing").is_empty());
}

// spec BRIDGE-3.1: edict/spawned adds a row, edict/removed drops it and its selection, and
// map/loaded asks for the list again
#[test]
fn events_keep_the_list() {
    let mut entities = listed();
    assert!(!entities.take_event(
        "edict/spawned",
        &json!({ "edict": 9, "classname": "ember" })
    ));
    assert_eq!(entities.classname(9), Some("ember"));
    entities.select(&json!({ "edict": 9, "classname": "ember", "fields": [] }));
    entities.take_event("edict/removed", &json!({ "edict": 9 }));
    assert_eq!(entities.classname(9), None);
    assert!(entities.selected().is_none());
    assert!(entities.take_event("map/loaded", &json!({ "map": "e1m1" })));
    assert!(!entities.take_event("console/line", &json!({})));
    entities.clear();
    assert!(entities.is_empty());
}

// spec BRIDGE-3.2: edict/changed sets the selected edict's fields, adds one it lacked and renames
// the row on a new classname; another edict's change touches only its row
#[test]
fn changes_reach_the_selection() {
    let mut entities = listed();
    entities.select(&json!({ "edict": 7, "classname": "light", "fields": [
        { "name": "health", "value": { "type": "float", "value": 10 } }
    ] }));
    entities.take_event(
        "edict/changed",
        &json!({ "edict": 7, "fields": [
        { "name": "health", "value": { "type": "float", "value": 4.5 } },
        { "name": "classname", "value": { "type": "string", "value": "torch" } },
        { "nameless": true }
    ] }),
    );
    let selected = entities.selected().unwrap();
    assert_eq!(selected.shown("health", &entities).unwrap(), "4.5");
    assert_eq!(selected.shown("classname", &entities).unwrap(), "\"torch\"");
    assert_eq!(selected.fields[0].kind(), "float");
    assert_eq!(entities.classname(7), Some("torch"));
    entities.take_event(
        "edict/changed",
        &json!({ "edict": 1, "fields": [
        { "name": "health", "value": { "type": "float", "value": 99 } }
    ] }),
    );
    assert_eq!(
        entities
            .selected()
            .unwrap()
            .shown("health", &entities)
            .unwrap(),
        "4.5"
    );
    // An answer that drops the selected edict deselects it.
    entities.replace(&json!({ "entities": [{ "edict": 1, "classname": "player" }] }));
    assert!(entities.selected().is_none());
}

// No spec statement: values are written as QuakeC writes them
#[test]
fn values_show_as_quakec_writes_them() {
    let entities = listed();
    let show = |value| entities.show(&value);
    assert_eq!(show(json!({ "type": "float", "value": 21.0 })), "21");
    assert_eq!(show(json!({ "type": "float", "value": 21.34 })), "21.34");
    assert_eq!(
        show(json!({ "type": "vector", "value": [412, -96.5, 88] })),
        "'412 -96.5 88'"
    );
    assert_eq!(
        show(json!({ "type": "string", "value": "ember" })),
        "\"ember\""
    );
    assert_eq!(show(json!({ "type": "entity", "value": 1 })), "player #1");
    assert_eq!(show(json!({ "type": "entity", "value": 40 })), "entity #40");
    assert_eq!(
        show(json!({ "type": "function", "value": "Ember_Touch" })),
        "Ember_Touch"
    );
    assert_eq!(show(json!({ "type": "pointer", "value": 12 })), "12");
}

// No spec statement: what a user types is read back as the field's kind of bridge value, and a
// text that is not one is refused
#[test]
fn typed_values_are_read_by_kind() {
    assert_eq!(
        parse_value("float", " 3.5 "),
        Some(json!({ "type": "float", "value": 3.5 }))
    );
    assert_eq!(
        parse_value("vector", "'1 2 3'").unwrap()["value"],
        json!([1.0, 2.0, 3.0])
    );
    assert_eq!(parse_value("vector", "1 2").map(|_| ()), None);
    assert_eq!(parse_value("string", "\"hi\"").unwrap()["value"], "hi");
    assert_eq!(parse_value("string", "hi").unwrap()["value"], "hi");
    assert_eq!(parse_value("entity", "player #1").unwrap()["value"], 1);
    assert_eq!(parse_value("entity", "4").unwrap()["value"], 4);
    assert_eq!(
        parse_value("function", "think").unwrap()["type"],
        "function"
    );
    assert_eq!(parse_value("float", "x"), None);
    assert_eq!(parse_value("function", ""), None);
    assert_eq!(parse_value("pointer", "1"), None);
}
