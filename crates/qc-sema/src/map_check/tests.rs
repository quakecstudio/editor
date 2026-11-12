// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use super::{EntitySchema, FieldValue, MapDiagnosticKind, distance};
use crate::{Db, FileId, ModuleInput};

/// A module as id's `defs.qc` declares its fields, with spawn functions that read them.
const SOURCE: &str = "entity self; entity world; float time;\n\
    .string classname, model, target, targetname, killtarget, message, wad;\n\
    .float spawnflags, wait, speed, health, style, light_lev; .vector angles, origin;\n\
    .void() think, use; .entity enemy; .float() th_pain; ..float aimed;\n\
    void() worldspawn = {};\n\
    /*QUAKED func_door (0 .5 .8) ?\n*/\n\
    void() func_door = { if (!self.speed) self.speed = 100; if (!self.wait) self.wait = 1; };\n\
    /*QUAKED light (0 1 0) (-8 -8 -8) (8 8 8)\n*/\n\
    void() light = { self.style = self.light_lev; };\n\
    /*QUAKED trigger_once (.5 .5 .5) ?\n*/\n\
    void() trigger_once = { self.think = self.use; };\n\
    entity(entity start, .string fld, string match) find = #18;\n\
    void() SUB_UseTargets = { local entity t;\n\
        t = find (world, targetname, self.target); t = find (t, targetname, self.killtarget); };\n";

fn schema() -> EntitySchema {
    let mut db = Db::new();
    db.set_file(FileId::new(0), "defs.qc".into(), SOURCE.as_bytes().to_vec());
    let mut input = ModuleInput::default();
    input.compiled.push(FileId::new(0));
    db.entity_schema(&input)
}

/// The diagnostics of `map`, each as its entity, line and kind.
fn check(map: &str) -> Vec<(usize, u32, MapDiagnosticKind)> {
    let file = qs_map::MapFile::parse(map.as_bytes()).unwrap();
    schema()
        .check(&file)
        .into_iter()
        .map(|d| (d.entity, d.line, d.kind))
        .collect()
}

const WORLD: &str = "{\n\"classname\" \"worldspawn\"\n\"wad\" \"gfx/base.wad\"\n}\n";

// spec MAP-8.1: an entity with no classname, or one naming no function, is reported, with the
// closest function within two edits
#[test]
fn an_unknown_class_is_reported_with_the_closest_function() {
    let map = format!(
        "{WORLD}{{\n\"classname\" \"func_dor\"\n}}\n{{\n\"origin\" \"0 0 0\"\n}}\n\
         {{\n\"classname\" \"monster_nothing_like_it\"\n}}\n{{\n\"classname\" \"light\"\n}}\n"
    );
    assert_eq!(
        check(&map),
        [
            (
                1,
                6,
                MapDiagnosticKind::UnknownClass {
                    classname: "func_dor".to_owned(),
                    suggestion: Some("func_door".to_owned()),
                }
            ),
            (2, 8, MapDiagnosticKind::NoClassname),
            (
                3,
                12,
                MapDiagnosticKind::UnknownClass {
                    classname: "monster_nothing_like_it".to_owned(),
                    suggestion: None,
                }
            ),
        ]
    );
}

// spec MAP-8.2: a misspelled key is reported with the closest field, one the class reads
// first; _ keys, wad and mapversion, angle and light (read as angles and light_lev) and
// vector components are not reported
#[test]
fn a_misspelled_key_is_reported_with_a_fix() {
    let map = format!(
        "{WORLD}{{\n\"classname\" \"func_door\"\n\"sped\" \"50\"\n\"wiat\" \"2\"\n\
         \"_minlight\" \"5\"\n\"angle\" \"90\"\n\"origin_x\" \"4\"\n\"mapversion\" \"220\"\n}}\n\
         {{\n\"classname\" \"light\"\n\"light\" \"200\"\n\"zzzz\" \"1\"\n}}\n"
    );
    let found = check(&map);
    let keys: Vec<_> = found
        .iter()
        .filter_map(|(_, line, kind)| match kind {
            MapDiagnosticKind::UnknownKey { key, suggestion } => {
                Some((*line, key.as_str(), suggestion.as_deref()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        keys,
        [
            (7, "sped", Some("speed")),
            (8, "wiat", Some("wait")),
            (17, "zzzz", None)
        ],
        "{found:?}"
    );
}

// spec MAP-8.2: at one distance, the field the entity's class reads is the suggestion: `wiat`
// is two edits from both `wad` and `wait`, and func_door reads `wait`
#[test]
fn a_key_the_class_reads_is_suggested_first() {
    let map = format!("{WORLD}{{\n\"classname\" \"func_door\"\n\"wiat\" \"1\"\n}}\n");
    let found = check(&map);
    assert!(
        found.iter().any(|(_, _, kind)| *kind
            == MapDiagnosticKind::UnknownKey {
                key: "wiat".to_owned(),
                suggestion: Some("wait".to_owned())
            }),
        "{found:?}"
    );
}

// spec MAP-8.3: a value its field's type cannot read is reported: a float that is no number, a
// vector that is not three numbers, an entity that is no whole number
// spec MAP-8.4: a function or field value naming nothing is reported
#[test]
fn a_value_of_the_wrong_type_is_reported() {
    let map = format!(
        "{WORLD}{{\n\"classname\" \"func_door\"\n\"speed\" \"fast\"\n\"wait\" \"-1.5\"\n\
         \"origin\" \"1 2\"\n\"angles\" \"0 90 0\"\n\"enemy\" \"x\"\n\"think\" \"nothing\"\n\
         \"use\" \"func_door\"\n\"aimed\" \"speed\"\n\"angle\" \"inf\"\n}}\n"
    );
    let bad: Vec<_> = check(&map)
        .into_iter()
        .filter_map(|(_, _, kind)| match kind {
            MapDiagnosticKind::BadValue { key, expected, .. } => Some((key, expected)),
            _ => None,
        })
        .collect();
    assert_eq!(
        bad,
        [
            ("speed".to_owned(), FieldValue::Float),
            ("origin".to_owned(), FieldValue::Vector),
            ("enemy".to_owned(), FieldValue::Entity),
            ("think".to_owned(), FieldValue::Function),
            ("angle".to_owned(), FieldValue::Float),
        ]
    );
    let map = format!("{WORLD}{{\n\"classname\" \"func_door\"\n\"aimed\" \"nofield\"\n}}\n");
    assert!(check(&map).iter().any(|(_, _, kind)| matches!(
        kind,
        MapDiagnosticKind::BadValue {
            expected: FieldValue::Field,
            ..
        }
    )));
}

// spec MAP-8.9: a key the code fires entities by that names no targetname is reported, and a
// targetname no such key names
#[test]
fn dangling_targets_and_targetnames_are_reported() {
    let map = format!(
        "{WORLD}{{\n\"classname\" \"trigger_once\"\n\"target\" \"door1\"\n\"killtarget\" \"gone\"\n}}\n\
         {{\n\"classname\" \"func_door\"\n\"targetname\" \"door1\"\n}}\n\
         {{\n\"classname\" \"func_door\"\n\"targetname\" \"lonely\"\n}}\n"
    );
    assert_eq!(
        check(&map),
        [
            (
                1,
                8,
                MapDiagnosticKind::NoTarget {
                    key: "killtarget".to_owned(),
                    name: "gone".to_owned()
                }
            ),
            (
                3,
                16,
                MapDiagnosticKind::Untargeted {
                    name: "lonely".to_owned()
                }
            ),
        ]
    );
}

// spec MAP-8.2: a clean map has no diagnostics
#[test]
fn a_clean_map_has_none() {
    let map = format!(
        "{WORLD}{{\n\"classname\" \"trigger_once\"\n\"target\" \"d\"\n}}\n\
         {{\n\"classname\" \"func_door\"\n\"targetname\" \"d\"\n\"speed\" \"50\"\n\
         \"origin\" \"1 2 3\"\n\"enemy\" \"1\"\n\"think\" \"func_door\"\n\"message\" \"hi\"\n}}\n"
    );
    assert_eq!(check(&map), []);
}

#[test]
fn distance_counts_edits() {
    assert_eq!(distance("", ""), 0);
    assert_eq!(distance("abc", ""), 3);
    assert_eq!(distance("", "ab"), 2);
    assert_eq!(distance("speed", "sped"), 1);
    assert_eq!(distance("wait", "wiat"), 2);
    assert_eq!(distance("kitten", "sitting"), 3);
}
