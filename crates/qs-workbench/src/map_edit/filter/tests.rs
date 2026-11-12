// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::json;

use super::{MAP_FILTER, MapFiltered};
use crate::map_edit::MAP_OPEN;
use crate::vm_run::tests::system_defs;
use crate::{Config, OPEN_PROJECT, Workbench, register};

/// The spawn functions: a thing, an item its own code removes in deathmatch, and the world.
const CODE: &str = "void(entity e) remove = #15;\n\
    void() worldspawn = {};\n\
    void() info_thing = { self.health = 5; };\n\
    void() item_dm = { if (deathmatch) remove(self); };\n";

/// The world, a thing not on easy (spawnflags 256), the item, and an entity with no class.
const MAP: &str = "{\n\"classname\" \"worldspawn\"\n}\n\
    {\n\"classname\" \"info_thing\"\n\"origin\" \"0 0 0\"\n\"spawnflags\" \"256\"\n}\n\
    {\n\"classname\" \"item_dm\"\n\"origin\" \"8 0 0\"\n}\n\
    {\n\"origin\" \"16 0 0\"\n}\n";

fn removed(session: &mut Session<Workbench<FsLoader>>, params: serde_json::Value) -> Vec<usize> {
    let found: MapFiltered =
        serde_json::from_value(session.execute(MAP_FILTER, params).unwrap()).unwrap();
    found.removed.iter().map(|entity| entity.get()).collect()
}

// spec DIFF-3.5: the filter is the game's: the entities a skill's spawnflags rule out, those
// the QuakeC removes in a kind of game, and those with no class (EDICT-8.4, EDICT-8.5), found by
// spawning a .map with no compiled BSP in an empty world
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
    assert_eq!(removed(&mut session, json!({})), [3]);
    assert_eq!(removed(&mut session, json!({ "skill": "easy" })), [1, 3]);
    assert_eq!(
        removed(
            &mut session,
            json!({ "skill": "hard", "mode": "deathmatch" })
        ),
        [2, 3]
    );
    assert_eq!(removed(&mut session, json!({ "mode": "coop" })), [3]);
}
