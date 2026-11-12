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

/// A map with a layer `Upstairs` (id 1), a group `Armory` (id 2) holding a brush, and a group
/// with no name; a monster in the armory by its key, one in it by place, one outside.
fn map() -> MapFile {
    let text = format!(
        "{{\n\"classname\" \"worldspawn\"\n}}\n\
         {{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_layer\"\n\"_tb_name\" \"Upstairs\"\n\"_tb_id\" \"1\"\n}}\n\
         {{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_name\" \"Armory\"\n\"_tb_id\" \"2\"\n{}}}\n\
         {{\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\"_tb_id\" \"3\"\n{}}}\n\
         {{\n\"classname\" \"monster_ogre\"\n\"origin\" \"900 0 0\"\n\"_tb_group\" \"2\"\n}}\n\
         {{\n\"classname\" \"monster_army\"\n\"origin\" \"64 64 32\"\n}}\n\
         {{\n\"classname\" \"monster_dog\"\n\"origin\" \"500 0 0\"\n}}\n\
         {{\n\"classname\" \"light\"\n\"_tb_layer\" \"1\"\n\"_tb_group\" \"2\"\n}}\n\
         {{\n\"classname\" \"info_null\"\n\"_tb_group\" \"-2\"\n}}\n",
        cube([0, 0, 0], [128, 128, 64]),
        cube([0, 0, 0], [8, 8, 8]),
    );
    MapFile::parse(text.as_bytes()).unwrap()
}

// spec MAP-9.1: a layer and a group are func_group entities with _tb_type _tb_layer or
// _tb_group, named by _tb_name, with an id; one with no name is no room
// spec MAP-9.3: a room's bounds are the box around its brushes; one with none has none
#[test]
fn layers_and_groups_are_rooms() {
    let rooms = map().rooms();
    let names: Vec<_> = rooms
        .iter()
        .map(|r| (r.name.as_str(), r.kind, r.id))
        .collect();
    assert_eq!(
        names,
        [
            ("Upstairs", RoomKind::Layer, 1),
            ("Armory", RoomKind::Group, 2)
        ]
    );
    assert_eq!(rooms[0].bounds, None);
    let [min, max] = rooms[1].bounds.unwrap();
    assert!(min.iter().all(|c| c.abs() < 1e-9), "{min:?}");
    assert!(
        (max[0] - 128.0).abs() < 1e-9 && (max[2] - 64.0).abs() < 1e-9,
        "{max:?}"
    );
}

// spec MAP-9.2: an entity is in the room its _tb_layer names, else its _tb_group; a value
// that is not a whole number names none
// spec MAP-9.4: a room's own entity is in it; an entity naming no room is in the first room
// whose bounds hold its origin
#[test]
fn an_entity_is_in_the_room_it_names_or_stands_in() {
    let map = map();
    let rooms = map.rooms();
    let room = |n: usize| map.entities[n].room(&rooms).map(|r| r.name.as_str());
    assert_eq!(room(4), Some("Armory"), "by its key, wherever it stands");
    assert_eq!(room(5), Some("Armory"), "by its origin");
    assert_eq!(room(6), None, "outside every room");
    assert_eq!(room(7), Some("Upstairs"), "_tb_layer comes first");
    assert_eq!(
        room(8),
        None,
        "a negative id names none, and it has no origin"
    );
    assert_eq!(room(2), Some("Armory"), "a room's own entity");
    assert_eq!(room(3), None, "a group with no name is no room");
}
