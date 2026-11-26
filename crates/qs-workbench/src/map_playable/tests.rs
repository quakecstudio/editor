// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use std::path::PathBuf;

use qs_map::{Brush, Entity, Item, MapFile, Surface, TexDef};
use qs_map_edit::testing::{Opening, room};
use qs_map_edit::{EntityIndex, MAP_ISSUES, MapIssues};
use serde_json::json;

use super::{ExitReach, MAP_PLAYABILITY, MAP_PLAYABILITY_FOUND, Playability};
use crate::map_compile::tests::open;

fn wall() -> Surface {
    Surface {
        texture: b"wall".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    }
}

/// An entity of `classname` with `pairs` and `brushes`.
fn entity(classname: &str, pairs: &[(&str, &str)], brushes: Vec<Brush>) -> Entity {
    let mut entity = Entity::new();
    entity.set(b"classname", classname.as_bytes()).unwrap();
    for (key, value) in pairs {
        entity.set(key.as_bytes(), value.as_bytes()).unwrap();
    }
    entity.items.extend(brushes.into_iter().map(Item::Brush));
    entity
}

/// The sealed test room, with a `func_group` holding a ledge too high to jump onto, an exit
/// along its west wall, and a health box on the ledge: 0 the world, 1 the start, 2 the group,
/// 3 the exit and 4 the health. The group's brushes join the world, so the compiled map's lump
/// has no entity for it.
fn ledged() -> PathBuf {
    let path = room("playable", "wall", Opening::Sealed);
    let mut file = MapFile::parse(&std::fs::read(&path).unwrap()).unwrap();
    let ledge = Brush::cuboid([48.0, 48.0, -128.0], [128.0, 128.0, -40.0], &wall()).unwrap();
    file.push_entity(entity("func_group", &[], vec![ledge]));
    let exit = Brush::cuboid([-128.0, -128.0, -128.0], [-112.0, 128.0, 0.0], &wall()).unwrap();
    file.push_entity(entity(
        "trigger_changelevel",
        &[("map", "next")],
        vec![exit],
    ));
    file.push_entity(entity(
        "item_health",
        &[("origin", "88 88 -40")],
        Vec::new(),
    ));
    std::fs::write(&path, file.to_bytes()).unwrap();
    path
}

// No spec statement: L-77, map.playability compiles the open map and names what it finds by
// its place in the open map, whatever the compile left out of the lump; its findings join
// map.issues
#[test]
fn map_playability_names_the_open_maps_entities() {
    let mut session = open(&ledged());
    let found: Playability<EntityIndex> =
        serde_json::from_value(session.execute(MAP_PLAYABILITY, json!({})).unwrap()).unwrap();
    assert_eq!(found.exit, ExitReach::Reached);
    assert_eq!(found.exits, [EntityIndex::new(3)]);
    let unreached: Vec<EntityIndex> = found.unreached.iter().map(|u| u.entity).collect();
    assert_eq!(unreached, [EntityIndex::new(4)]);
    let issues: MapIssues =
        serde_json::from_value(session.execute(MAP_ISSUES, json!({})).unwrap()).unwrap();
    let health = issues
        .issues
        .iter()
        .find(|issue| issue.entity == Some(EntityIndex::new(4)))
        .unwrap();
    assert!(health.message.contains("item_health"), "{}", health.message);
}

// No spec statement: L-77, MAP-14: an instance (misc_external_map) made a func_detail joins the
// world in the compile, as one does, and the findings after it still name their own entities
#[test]
fn an_instance_joined_to_the_world_keeps_the_names_after_it() {
    let path = ledged();
    let dir = path.parent().unwrap();
    let mut prefab = MapFile::new();
    let block = Brush::cuboid([0.0, 0.0, 0.0], [16.0, 16.0, 16.0], &wall()).unwrap();
    prefab.push_entity(entity("worldspawn", &[], vec![block]));
    std::fs::write(dir.join("block.map"), prefab.to_bytes()).unwrap();
    let mut file = MapFile::parse(&std::fs::read(&path).unwrap()).unwrap();
    let instance = entity(
        "misc_external_map",
        &[
            ("_external_map", "block.map"),
            ("_external_map_classname", "func_detail"),
            ("origin", "-112 -112 -128"),
        ],
        Vec::new(),
    );
    file.entities.insert(3, instance);
    std::fs::write(&path, file.to_bytes()).unwrap();
    let mut session = open(&path);
    let found: Playability<EntityIndex> =
        serde_json::from_value(session.execute(MAP_PLAYABILITY, json!({})).unwrap()).unwrap();
    assert_eq!(found.exits, [EntityIndex::new(4)]);
    let unreached: Vec<EntityIndex> = found.unreached.iter().map(|u| u.entity).collect();
    assert_eq!(unreached, [EntityIndex::new(5)]);
}

// No spec statement: L-77, map.playability with answer "later" answers nothing at once and
// searches on its own thread; map.playability_found then gives what "now" gives, and nothing
// once the map has changed
#[test]
fn a_later_answer_is_found_until_the_map_changes() {
    let mut session = open(&ledged());
    let now: Playability<EntityIndex> =
        serde_json::from_value(session.execute(MAP_PLAYABILITY, json!({})).unwrap()).unwrap();
    let started = session
        .execute(MAP_PLAYABILITY, json!({ "answer": "later" }))
        .unwrap();
    assert_eq!(started, serde_json::Value::Null);
    let found = loop {
        let found = session.execute(MAP_PLAYABILITY_FOUND, json!({})).unwrap();
        if !found.is_null() {
            break found;
        }
        std::thread::yield_now();
    };
    let later: Playability<EntityIndex> = serde_json::from_value(found).unwrap();
    assert_eq!(later, now);
    session
        .execute(
            "map.set_key",
            json!({ "entity": 4, "key": "targetname", "value": "moved" }),
        )
        .unwrap();
    let after = session.execute(MAP_PLAYABILITY_FOUND, json!({})).unwrap();
    assert_eq!(after, serde_json::Value::Null);
}
