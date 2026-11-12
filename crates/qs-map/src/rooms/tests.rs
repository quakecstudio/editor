// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use crate::{MapFile, RoomKind};

/// A brush filling the box `min` to `max`, its faces as qbsp reads them.
fn cube(min: [i32; 3], max: [i32; 3]) -> String {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    format!(
        "{{\n( {x0} 0 0 ) ( {x0} 1 0 ) ( {x0} 0 1 ) t 0 0 0 1 1\n\
         ( {x1} 0 0 ) ( {x1} 0 1 ) ( {x1} 1 0 ) t 0 0 0 1 1\n\
         ( 0 {y0} 0 ) ( 0 {y0} 1 ) ( 1 {y0} 0 ) t 0 0 0 1 1\n\
         ( 0 {y1} 0 ) ( 1 {y1} 0 ) ( 0 {y1} 1 ) t 0 0 0 1 1\n\
         ( 0 0 {z0} ) ( 1 0 {z0} ) ( 0 1 {z0} ) t 0 0 0 1 1\n\
         ( 0 0 {z1} ) ( 0 1 {z1} ) ( 1 0 {z1} ) t 0 0 0 1 1\n}}\n"
    )
}

/// An entity of `classname` with `keys`, each a key and a value, and `body` inside it.
fn entity(classname: &str, keys: &[(&str, &str)], body: &str) -> String {
    let mut out = format!("{{\n\"classname\" \"{classname}\"\n");
    for (key, value) in keys {
        out.push_str(&format!("\"{key}\" \"{value}\"\n"));
    }
    out.push_str(body);
    out.push_str("}\n");
    out
}

/// A layer, `_tb_type` `_tb_layer`, of `name` and `id`, in nothing.
fn layer(name: &str, id: &str) -> String {
    let keys = [
        ("_tb_type", "_tb_layer"),
        ("_tb_name", name),
        ("_tb_id", id),
    ];
    entity("func_group", &keys, "")
}

/// A group of `name` (none when empty) and `id`, in `parent`'s group or layer, holding `body`.
fn group(name: &str, id: &str, parent: (&str, &str), body: &str) -> String {
    let mut keys = vec![("_tb_type", "_tb_group"), ("_tb_id", id)];
    if !name.is_empty() {
        keys.push(("_tb_name", name));
    }
    if !parent.0.is_empty() {
        keys.push(parent);
    }
    entity("func_group", &keys, body)
}

/// A layer `Castle` (id 1) holding a group `Armory` (id 2) with a 128 by 128 by 64 brush, a
/// group with no name (id 3) inside the armory with an 8-unit brush, and a group `Hall` (id 4)
/// in nothing with a brush from 512 to 640; then monsters and others to place.
fn map() -> MapFile {
    let text = [
        entity("worldspawn", &[], ""),
        layer("Castle", "1"),
        group(
            "Armory",
            "2",
            ("_tb_layer", "1"),
            &cube([0, 0, 0], [128, 128, 64]),
        ),
        group("", "3", ("_tb_group", "2"), &cube([0, 0, 0], [8, 8, 8])),
        group("Hall", "4", ("", ""), &cube([512, 0, 0], [640, 128, 64])),
        entity(
            "monster_ogre",
            &[("origin", "900 0 0"), ("_tb_group", "2")],
            "",
        ),
        entity("monster_army", &[("origin", "64 64 32")], ""),
        entity("monster_dog", &[("origin", "300 0 0")], ""),
        entity("light", &[("_tb_layer", "1"), ("_tb_group", "4")], ""),
        entity("info_null", &[("_tb_group", "-2")], ""),
        entity(
            "monster_wizard",
            &[("origin", "4 4 4"), ("_tb_group", "3")],
            "",
        ),
        entity(
            "monster_knight",
            &[("origin", "576 64 32"), ("_tb_group", "9")],
            "",
        ),
    ]
    .concat();
    MapFile::parse(text.as_bytes()).unwrap()
}

// spec MAP-9.1: a layer and a group are func_group entities with _tb_type _tb_layer or
// _tb_group, named by _tb_name, with an id; one with no name is no room
// spec MAP-9.3: a room's bounds take its brushes and those of the rooms in it, a container
// with no name passing its own on; one with no brush has none
#[test]
fn layers_and_groups_are_rooms() {
    let rooms = map().rooms();
    let names: Vec<_> = rooms
        .list()
        .iter()
        .map(|r| (r.name.as_str(), r.kind))
        .collect();
    assert_eq!(
        names,
        [
            ("Castle", RoomKind::Layer),
            ("Armory", RoomKind::Group),
            ("Hall", RoomKind::Group)
        ]
    );
    let castle = rooms.list()[0].bounds.unwrap();
    let armory = rooms.list()[1].bounds.unwrap();
    assert_eq!(castle, armory, "the castle's only brushes are the armory's");
    assert!((armory[1][0] - 128.0).abs() < 1e-9, "{armory:?}");
    assert!(!rooms.list()[1].holds([600.0, 64.0, 32.0]));
    assert!(rooms.list()[2].holds([600.0, 64.0, 32.0]));
}

// spec MAP-9.2: an entity is in the room its _tb_layer names, else its _tb_group; a value
// that is not a whole number names none
// spec MAP-9.4: a room's own entity is in it; an entity is in the nearest named room around
// what it names, else the smallest room whose bounds hold its origin, and in every room
// around that one
#[test]
fn an_entity_is_in_the_room_it_names_or_stands_in() {
    let map = map();
    let rooms = map.rooms();
    let of = |n: usize| -> Vec<String> {
        rooms
            .of(&map.entities[n])
            .iter()
            .map(|room| room.name.clone())
            .collect()
    };
    assert_eq!(
        of(2),
        ["Armory", "Castle"],
        "a room's own entity, then the layer around it"
    );
    assert_eq!(of(3), ["Armory", "Castle"], "a nameless group's own entity");
    assert_eq!(
        of(5),
        ["Armory", "Castle"],
        "by its key, wherever it stands"
    );
    assert_eq!(
        of(6),
        ["Armory", "Castle"],
        "by its origin, the armory before the castle"
    );
    assert!(of(7).is_empty(), "outside every room");
    assert_eq!(of(8), ["Castle"], "_tb_layer comes first");
    assert!(
        of(9).is_empty(),
        "a negative id names none, and it has no origin"
    );
    assert_eq!(of(10), ["Armory", "Castle"], "through the nameless group");
    assert_eq!(of(11), ["Hall"], "a stale key falls back to its origin");
    assert!(rooms.holds(&map.entities[10], "castle"));
    assert!(!rooms.holds(&map.entities[11], "Armory"));
    assert!(rooms.named_any("ARMORY"));
    assert!(!rooms.named_any("Amory"));
}
