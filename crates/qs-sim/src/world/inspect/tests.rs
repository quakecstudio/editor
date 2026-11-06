// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_progs::Progs;

use super::QcValue;
use crate::LoadOptions;
use crate::world::tests::{ledge, player_from, player_progs};

fn value<'a>(values: &'a [super::NamedValue], name: &str) -> Option<&'a QcValue> {
    values.iter().find(|v| v.name == name).map(|v| &v.value)
}

// spec BRIDGE-5.1: an edict's fields are typed values: a float, a vector whole rather than
// its components, a string, an entity's number and a function's name; a free edict has none
#[test]
fn an_edicts_fields_are_typed_values() {
    let more = "float counter;\n.void() think;\nvoid() tick = {};\n";
    let put =
        "self.health = 42; self.classname = \"player\"; self.think = tick; self.owner = world;";
    let run = player_from(
        &ledge(),
        player_progs(put, "", more),
        LoadOptions::default(),
    );
    let progs = Progs::read(&run.progs).unwrap();
    let fields = run.world.edict_fields(&progs, 1).unwrap();
    assert_eq!(value(&fields, "health"), Some(&QcValue::Float(42.0)));
    assert_eq!(
        value(&fields, "classname"),
        Some(&QcValue::String(b"player".to_vec()))
    );
    assert_eq!(
        value(&fields, "think"),
        Some(&QcValue::Function("tick".to_owned()))
    );
    assert_eq!(value(&fields, "owner"), Some(&QcValue::Entity(0)));
    assert_eq!(
        value(&fields, "view_ofs"),
        Some(&QcValue::Vector([0.0, 0.0, 22.0]))
    );
    assert!(
        value(&fields, "origin_x").is_none(),
        "a vector's components are not listed"
    );
    assert_eq!(run.world.classname(&progs, 1), "player");
    assert!(
        run.world.edict_fields(&progs, 590).is_none(),
        "a free edict"
    );
    assert!(
        run.world.edict_fields(&progs, 100_000).is_none(),
        "past the edicts"
    );
}

// spec BRIDGE-5.1: the named globals are typed values, without the functions' and fields'
// own definitions
#[test]
fn the_named_globals_are_typed_values() {
    let more = "float counter;\nvoid() StartFrame = { counter = counter + 1; };\n";
    let mut run = player_from(&ledge(), player_progs("", "", more), LoadOptions::default());
    run.frames(2, 0.1).unwrap();
    let progs = Progs::read(&run.progs).unwrap();
    let globals = run.world.named_globals(&progs);
    assert_eq!(value(&globals, "counter"), Some(&QcValue::Float(2.0)));
    assert_eq!(
        value(&globals, "mapname"),
        Some(&QcValue::String(b"test".to_vec()))
    );
    assert!(value(&globals, "StartFrame").is_none());
    assert!(value(&globals, "v_forward_x").is_none());
    assert!(matches!(
        value(&globals, "v_forward"),
        Some(QcValue::Vector(_))
    ));
}
