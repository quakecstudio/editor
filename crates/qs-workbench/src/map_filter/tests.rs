// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use super::{MAP_FILTER, MapFiltered};
use crate::vm_run::tests::system_defs;
use crate::{Config, OPEN_PROJECT, Workbench, register};
use qs_map::{Brush, Entity, Item, MapFile, Surface, TexDef};
use qs_map_edit::MAP_OPEN;

/// The spawn functions: a thing, an item its own code removes in deathmatch, and the world.
const CODE: &str = "void(entity e) remove = #15;\n\
    void() worldspawn = {};\n\
    void() info_thing = { self.health = 5; };\n\
    void() item_dm = { if (deathmatch) remove(self); };\n";

/// The world with a brush, a thing not on easy (spawnflags 256), an entity with no pairs, which
/// qbsp leaves out of the lump, the item, and an entity with no class.
const MAP: &str = "{\n\"classname\" \"worldspawn\"\n\
    {\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) floor 0 0 0 1 1\n\
    ( 0 0 64 ) ( 1 0 64 ) ( 0 1 64 ) floor 0 0 0 1 1\n}\n}\n\
    {\n\"classname\" \"info_thing\"\n\"origin\" \"0 0 0\"\n\"spawnflags\" \"256\"\n}\n\
    {\n}\n\
    {\n\"classname\" \"item_dm\"\n\"origin\" \"8 0 0\"\n}\n\
    {\n\"origin\" \"16 0 0\"\n}\n";

fn removed(session: &mut Session<Workbench<FsLoader>>, params: serde_json::Value) -> Vec<usize> {
    let found: MapFiltered =
        serde_json::from_value(session.execute(MAP_FILTER, params).unwrap()).unwrap();
    found.removed.iter().map(|entity| entity.get()).collect()
}

// spec DIFF-3.5: the filter is the game's: the entities a skill's spawnflags rule out, those
// the QuakeC removes in a kind of game, and those with no class (EDICT-8.4, EDICT-8.5), found by
// spawning the lump qbsp writes for a .map with brushes (MAP-2.6), which leaves out an entity
// with no pairs, in an empty world
#[test]
fn the_filter_is_what_the_game_removes() {
    let dir = std::env::temp_dir().join(format!("qs-wb-filter-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("defs.qc"), system_defs()).unwrap();
    std::fs::write(dir.join("world.qc"), CODE).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\ndefs.qc\nworld.qc\n").unwrap();
    std::fs::write(dir.join("maps/test.map"), MAP).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    assert!(
        session.execute(MAP_FILTER, json!({})).is_err(),
        "no map open"
    );
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    assert_eq!(removed(&mut session, json!({})), [2, 4]);
    assert_eq!(removed(&mut session, json!({ "skill": "easy" })), [1, 2, 4]);
    assert_eq!(
        removed(
            &mut session,
            json!({ "skill": "hard", "mode": "deathmatch" })
        ),
        [2, 3, 4]
    );
    assert_eq!(removed(&mut session, json!({ "mode": "coop" })), [2, 4]);
    assert!(
        !dir.join("progs.dat").exists(),
        "the filter compiles in memory and writes nothing"
    );
}

/// A thing whose code reads the size of its own brush model, and removes itself when it is
/// wider than 100.
const SIZED: &str = "void(entity e) remove = #15;\n\
    void(entity e, string m) setmodel = #3;\n\
    void() worldspawn = {};\n\
    void() func_thing = { setmodel(self, self.model); if (self.size_x > 100) remove(self); };\n";

/// The world, a small `func_group`, which id's qbsp keeps as an entity of its own, and a
/// `func_thing` 128 wide after it.
fn grouped() -> Vec<u8> {
    let floor = Surface {
        texture: b"floor".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    };
    let cuboid = |max: [f64; 3]| Brush::cuboid([0.0; 3], max, &floor).unwrap();
    let mut file = MapFile::new();
    for (classname, brush) in [
        ("worldspawn", cuboid([256.0, 256.0, 16.0])),
        ("func_group", cuboid([16.0, 16.0, 16.0])),
        ("func_thing", cuboid([128.0, 128.0, 16.0])),
    ] {
        let mut entity = Entity::new();
        entity.set(b"classname", classname.as_bytes()).unwrap();
        entity.items.push(Item::Brush(brush));
        file.push_entity(entity);
    }
    file.to_bytes()
}

// spec MAP-6.2: the world a map is spawned in is compiled as id's qbsp compiles it, so each
// brush entity's model is the one the lump names: past a func_group, which id's qbsp keeps as
// an entity with its own model, the thing reads its own brush's size
#[test]
fn a_brush_entity_past_a_group_spawns_with_its_own_model() {
    let dir = std::env::temp_dir().join(format!("qs-wb-filter-grouped-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("defs.qc"), system_defs()).unwrap();
    std::fs::write(dir.join("world.qc"), SIZED).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\ndefs.qc\nworld.qc\n").unwrap();
    std::fs::write(dir.join("maps/test.map"), grouped()).unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    // The group has no spawn function; the thing, 128 wide, removes itself.
    assert_eq!(removed(&mut session, json!({})), [1, 2]);
}
