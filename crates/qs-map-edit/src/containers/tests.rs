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
    assert_eq!(
        group.container(),
        Some((RoomKind::Group, ContainerId::new(1)))
    );
    assert_eq!(group.value(b"_tb_name"), Some(&b"lamp"[..]));
    assert_eq!(group.brushes().count(), 1);
    assert_eq!(
        grouped.entities[0].brushes().count(),
        1,
        "one world brush left"
    );
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
    assert!(
        session
            .execute(MAP_GROUP, json!({ "name": "none" }))
            .is_err()
    );
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
    assert_eq!(
        layer.container(),
        Some((RoomKind::Layer, ContainerId::new(1)))
    );
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
    assert_eq!(
        copy.container(),
        Some((RoomKind::Group, ContainerId::new(3)))
    );
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

// No spec statement: P7-08: map.make_brush_entity makes world brushes a brush entity of a class,
// as TrenchBroom's Create Brush Entity, and map.make_structural gives its brushes back to the
// world and removes it, as its Make Structural; a point entity, a group or the world is refused
#[test]
fn brushes_become_a_brush_entity_and_structural_again() {
    use super::{MAP_MAKE_BRUSH_ENTITY, MAP_MAKE_STRUCTURAL};

    let mut session = session("brush-entity");
    let before = map(&session);
    session
        .execute(
            MAP_MAKE_BRUSH_ENTITY,
            json!({ "classname": "func_door", "brushes": [1] }),
        )
        .unwrap();
    let made = map(&session);
    let door = made.entities.last().unwrap();
    assert_eq!(door.classname(), Some(&b"func_door"[..]));
    assert_eq!(door.brushes().count(), 1);
    assert_eq!(made.entities[0].brushes().count(), 1);
    let place = made.entities.len() - 1;
    session
        .execute(MAP_MAKE_STRUCTURAL, json!({ "entity": place }))
        .unwrap();
    let back = map(&session);
    assert_eq!(back.entities.len(), before.entities.len());
    assert_eq!(back.entities[0].brushes().count(), 2);
    session.undo().unwrap();
    session.undo().unwrap();
    assert_eq!(map(&session), before);
    for entity in [0, 1] {
        assert!(
            session
                .execute(MAP_MAKE_STRUCTURAL, json!({ "entity": entity }))
                .is_err(),
            "{entity}"
        );
    }
    assert!(
        session
            .execute(
                MAP_MAKE_BRUSH_ENTITY,
                json!({ "classname": "func_wall", "brushes": [] })
            )
            .is_err()
    );
}

// No spec statement: P7-08: map.extract_linked moves entities of a linked group, and the same
// entities of every group linked to it, into new groups linked to each other, as TrenchBroom's
// Extract Linked Groups; extracting all of a group, or from a group not linked, is refused
#[test]
fn entities_are_extracted_into_new_linked_groups() {
    use super::MAP_EXTRACT_LINKED;

    let mut session = session("extract");
    session
        .execute(MAP_GROUP, json!({ "name": "pair", "entities": [1, 2] }))
        .unwrap();
    let group = map(&session).entities.len() - 1;
    assert!(
        session
            .execute(
                MAP_EXTRACT_LINKED,
                json!({ "group": group, "entities": [1] })
            )
            .is_err(),
        "not linked yet"
    );
    session
        .execute(MAP_LINK_DUPLICATE, json!({ "group": group }))
        .unwrap();
    let before = map(&session);
    let link = before.entities[group].link_id().unwrap();
    session
        .execute(
            MAP_EXTRACT_LINKED,
            json!({ "group": group, "entities": [1] }),
        )
        .unwrap();
    let after = map(&session);
    let groups: Vec<(ContainerId, Option<String>)> = after
        .entities
        .iter()
        .filter_map(|e| match e.container() {
            Some((RoomKind::Group, id)) => Some((id, e.link_id())),
            _ => None,
        })
        .collect();
    assert_eq!(groups.len(), 4, "{groups:?}");
    let new_link = groups[2].1.clone().unwrap();
    assert_ne!(new_link, link);
    assert_eq!(groups[3].1.as_deref(), Some(new_link.as_str()));
    assert_eq!(groups[1].1.as_deref(), Some(link.as_str()));
    // The light left the first group for the first new one; the ogre stayed.
    assert_eq!(after.entities[1].group(), Some(groups[2].0));
    assert_eq!(after.entities[2].group(), Some(groups[0].0));
    session.undo().unwrap();
    assert_eq!(map(&session), before);
    assert!(
        session
            .execute(
                MAP_EXTRACT_LINKED,
                json!({ "group": group, "entities": [1, 2] })
            )
            .is_err(),
        "all of it"
    );
}
