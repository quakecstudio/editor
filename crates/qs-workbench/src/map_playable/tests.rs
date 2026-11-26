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

use super::{ExitReach, MAP_PLAYABILITY, Playability};
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
