// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use super::MapInspected;
use crate::map_edit::{MAP_INSPECT, MAP_OPEN, MAP_PLACE, register_map_edit};
use crate::{Config, Workbench};

/// The world, a group `Armory` with a brush from 0 to 128 by 128 by 64, and a monster outside it.
const MAP: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
    {\n\"classname\" \"func_group\"\n\"_tb_type\" \"_tb_group\"\n\
    \"_tb_name\" \"Armory\"\n\"_tb_id\" \"2\"\n\
    {\n( 0 0 0 ) ( 0 1 0 ) ( 0 0 1 ) t 0 0 0 1 1\n( 128 0 0 ) ( 128 0 1 ) ( 128 1 0 ) t 0 0 0 1 1\n\
    ( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 ) t 0 0 0 1 1\n( 0 128 0 ) ( 1 128 0 ) ( 0 128 1 ) t 0 0 0 1 1\n\
    ( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) t 0 0 0 1 1\n( 0 0 64 ) ( 0 1 64 ) ( 1 0 64 ) t 0 0 0 1 1\n}\n}\n\
    {\n\"classname\" \"monster_dog\"\n\"origin\" \"500 0 24\"\n\"targetname\" \"rex\"\n}\n";

// No spec statement: P5-08, MAP-16: map.inspect gives the rooms with their bounds and the
// entities with their places, origins and rooms, filtered by room and classname; an entity
// placed in a room is listed in it
#[test]
fn the_map_is_inspected_by_room() {
    let dir = std::env::temp_dir().join(format!("qs-wb-inspect-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("keep.map");
    std::fs::write(&path, MAP).unwrap();
    let mut registry = Registry::new();
    register_map_edit(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    assert!(
        session.execute(MAP_INSPECT, json!({})).is_err(),
        "no map open"
    );
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    let all: MapInspected =
        serde_json::from_value(session.execute(MAP_INSPECT, json!({})).unwrap()).unwrap();
    assert_eq!(all.rooms.len(), 1);
    assert_eq!(all.rooms[0].name, "Armory");
    assert_eq!(all.entities.len(), 3);
    assert_eq!(all.entities[2].targetname.as_deref(), Some("rex"));
    assert_eq!(all.entities[2].origin, Some([500.0, 0.0, 24.0]));
    assert_eq!(all.entities[2].room, None);
    assert_eq!(
        all.entities[1].room.as_deref(),
        Some("Armory"),
        "the group's own entity"
    );

    for _ in 0..3 {
        session
            .execute(
                MAP_PLACE,
                json!({
                    "classname": "monster_shambler",
                    "origin": [64.0, 64.0, 24.0],
                    "keys": [],
                }),
            )
            .unwrap();
    }
    let armory: MapInspected = serde_json::from_value(
        session
            .execute(
                MAP_INSPECT,
                json!({ "room": "armory", "classname": "monster_" }),
            )
            .unwrap(),
    )
    .unwrap();
    let places: Vec<_> = armory.entities.iter().map(|e| e.entity.get()).collect();
    assert_eq!(
        places,
        [3, 4, 5],
        "the three placed shamblers, not the dog outside"
    );
    assert!(
        session
            .execute(MAP_INSPECT, json!({ "nothing": 1 }))
            .is_err()
    );
    let unknown = session
        .execute(MAP_INSPECT, json!({ "room": "Amory" }))
        .unwrap_err()
        .to_string();
    assert!(unknown.contains("no room named Amory"), "{unknown}");
}
