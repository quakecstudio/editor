// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qs_commands::Session;
use qs_map::{ContainerId, MapFile, RoomKind};
use serde_json::json;

use super::{MAP_ADD_LAYER, MAP_GROUP, MAP_LINK_DUPLICATE, MAP_SEPARATE, MAP_UNGROUP};
use crate::MAP_OPEN;
use crate::testing::MapContext;

/// A session on a map of two world brushes, a light and an ogre.
fn session(name: &str) -> Session<MapContext> {
    let dir = std::env::temp_dir().join(format!("qs-containers-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rooms.map");
    let brush = |(a, b): (i32, i32)| {
        format!(
            "{{\n( {a} 0 0 ) ( {a} 1 0 ) ( {a} 0 1 ) rock 0 0 0 1 1\n\
             ( {b} 0 0 ) ( {b} 0 1 ) ( {b} 1 0 ) rock 0 0 0 1 1\n\
             ( 0 0 0 ) ( 0 0 1 ) ( 1 0 0 ) rock 0 0 0 1 1\n\
             ( 0 64 0 ) ( 1 64 0 ) ( 0 64 1 ) rock 0 0 0 1 1\n\
             ( 0 0 0 ) ( 1 0 0 ) ( 0 1 0 ) rock 0 0 0 1 1\n\
             ( 0 0 64 ) ( 0 1 64 ) ( 1 0 64 ) rock 0 0 0 1 1\n}}\n"
        )
    };
    let text = format!(
        "{{\n\"classname\" \"worldspawn\"\n{}{}}}\n\
         {{\n\"classname\" \"light\"\n\"origin\" \"32 32 32\"\n}}\n\
         {{\n\"classname\" \"monster_ogre\"\n\"origin\" \"96 32 24\"\n}}\n",
        brush((0, 64)),
        brush((128, 192))
    );
    std::fs::write(&path, text).unwrap();
    let mut session = MapContext::session();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    session
}

fn map(session: &Session<MapContext>) -> MapFile {
    session.context().map().unwrap().file().clone()
}

// No spec statement: P7-08: map.group puts entities and world brushes in a new group as
// TrenchBroom writes one (MAP-9.1, MAP-9.2): a func_group of _tb_type _tb_group with a fresh
// _tb_id holding the brushes, the entities naming it in _tb_group; map.ungroup takes them out
// again and removes it; each one undoable step; an empty selection or no group is refused
#[test]
fn a_selection_is_grouped_and_ungrouped() {
    let mut session = session("group");
    let before = map(&session);
    session
        .execute(
            MAP_GROUP,
            json!({ "name": "lamp", "entities": [1], "brushes": [1] }),
        )
        .unwrap();
    let grouped = map(&session);
    let group = grouped.entities.last().unwrap();
    assert_eq!(group.container(), Some((RoomKind::Group, ContainerId::new(1))));
    assert_eq!(group.value(b"_tb_name"), Some(&b"lamp"[..]));
    assert_eq!(group.brushes().count(), 1);
    assert_eq!(grouped.entities[0].brushes().count(), 1, "one world brush left");
    assert_eq!(grouped.entities[1].group(), Some(ContainerId::new(1)));
    assert_eq!(grouped.entities[2].group(), None);
    let group_place = grouped.entities.len() - 1;
    session
        .execute(MAP_UNGROUP, json!({ "group": group_place }))
        .unwrap();
    let ungrouped = map(&session);
    assert_eq!(ungrouped.entities.len(), before.entities.len());
    assert_eq!(ungrouped.entities[0].brushes().count(), 2);
    assert_eq!(ungrouped.entities[1].group(), None);
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(map(&session), before);
    assert!(session.execute(MAP_GROUP, json!({ "name": "none" })).is_err());
    assert!(session.execute(MAP_UNGROUP, json!({ "group": 1 })).is_err());
    assert!(
        session
            .execute(MAP_GROUP, json!({ "name": "w", "entities": [0] }))
            .is_err()
    );
}

// No spec statement: P7-08: map.add_layer adds a layer as TrenchBroom writes one, its _tb_id one
// more than the greatest; map.link_duplicate copies a group and its entities as a group linked
// to it, both given one _tb_linked_group_id (MAP-9.5); map.separate unlinks one, and refuses a
// group not linked
#[test]
fn layers_are_added_and_groups_linked_and_separated() {
    let mut session = session("link");
    session
        .execute(MAP_ADD_LAYER, json!({ "name": "lights" }))
        .unwrap();
    let layer = map(&session).entities.last().unwrap().clone();
    assert_eq!(layer.container(), Some((RoomKind::Layer, ContainerId::new(1))));
    assert_eq!(layer.value(b"_tb_name"), Some(&b"lights"[..]));
    session
        .execute(MAP_GROUP, json!({ "name": "pair", "entities": [1, 2] }))
        .unwrap();
    let group = map(&session).entities.len() - 1;
    assert_eq!(
        map(&session).entities[group].container(),
        Some((RoomKind::Group, ContainerId::new(2)))
    );
    session
        .execute(MAP_LINK_DUPLICATE, json!({ "group": group }))
        .unwrap();
    let linked = map(&session);
    let source = &linked.entities[group];
    let copy = &linked.entities[group + 1];
    assert!(source.link_id().is_some());
    assert_eq!(source.link_id(), copy.link_id());
    assert_eq!(copy.container(), Some((RoomKind::Group, ContainerId::new(3))));
    let copied: Vec<_> = linked.entities[group + 2..]
        .iter()
        .map(|e| (e.classname().unwrap().to_vec(), e.group()))
        .collect();
    assert_eq!(
        copied,
        [
            (b"light".to_vec(), Some(ContainerId::new(3))),
            (b"monster_ogre".to_vec(), Some(ContainerId::new(3)))
        ]
    );
    session
        .execute(MAP_SEPARATE, json!({ "group": group + 1 }))
        .unwrap();
    assert_eq!(map(&session).entities[group + 1].link_id(), None);
    assert!(
        session
            .execute(MAP_SEPARATE, json!({ "group": group + 1 }))
            .is_err()
    );
}
