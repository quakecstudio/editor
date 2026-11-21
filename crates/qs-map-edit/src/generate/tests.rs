// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is the test's failure"
)]

use std::path::PathBuf;

use qs_commands::Session;
use serde_json::json;

use std::time::Duration;

use super::{MAP_GENERATE, seed_at};
use crate::testing::{MapContext, TestGame, TestProject, level_bsp};
use crate::{ClassInfo, GameFolders, MAP_ENTITY, MAP_ISSUES};

/// A project in a folder of its own, whose code defines `classes`, and a game one of whose
/// levels holds `textures`.
fn project(name: &str, classes: &[&str], textures: &[&str]) -> (PathBuf, Session<MapContext>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-generate-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    let game = dir.join("game");
    std::fs::create_dir_all(game.join("id1/maps")).unwrap();
    if !textures.is_empty() {
        std::fs::write(game.join("id1/maps/e1.bsp"), level_bsp(textures)).unwrap();
    }
    let root = dir.join("mod");
    std::fs::create_dir_all(&root).unwrap();
    let class = |name: &&str| ClassInfo {
        name: (*name).to_owned(),
        found: "documented".to_owned(),
        shape: "point".to_owned(),
        color: None,
        size: None,
        model: None,
        doc: Vec::new(),
        keys: Vec::new(),
        spawnflags: Vec::new(),
    };
    let context = MapContext {
        project: TestProject {
            root: Some(root.clone()),
            classes: classes.iter().map(class).collect(),
            ..TestProject::default()
        },
        game: Some(TestGame {
            folders: GameFolders {
                base: Some(game),
                chosen: None,
            },
            ..TestGame::default()
        }),
        ..MapContext::default()
    };
    (root, context.into_session())
}

/// The classnames of the open map's entities.
fn classnames(session: &Session<MapContext>) -> Vec<String> {
    let map = session.context().map().unwrap();
    map.file()
        .entities
        .iter()
        .map(|entity| String::from_utf8_lossy(entity.classname().unwrap()).into_owned())
        .collect()
}

// No spec statement: L-48, `map.generate` writes the level into the project and opens it, its exit leading back to it; one seed is one level, and a file there already is kept
#[test]
fn a_level_is_made_in_the_project_and_opened() {
    let (root, mut session) = project("made", &[], &["brick", "stone", "wood", "*lava1", "sky1"]);
    let made = session
        .execute(MAP_GENERATE, json!({ "seed": 5, "size": "small" }))
        .unwrap();
    // Joined part by part, as the command joins it: a path is compared as it is written.
    let path = root.join("maps").join("random5.map");
    assert_eq!(made["path"], json!(path));
    assert_eq!(made["seed"], 5);
    assert_eq!(made["theme"], "plain");
    assert_eq!(made["rooms"].as_array().unwrap().len(), 5);
    assert!(made["monsters"].as_u64().unwrap() > 0);
    // It is the open map, and the file is what is open.
    let open = session.context().map().unwrap();
    assert_eq!(open.path(), path);
    assert_eq!(std::fs::read(&path).unwrap(), open.file().to_bytes());
    assert_eq!(made["entities"], json!(open.file().entities.len()));
    let names = classnames(&session);
    assert_eq!(names[0], "worldspawn");
    for needed in ["info_player_start", "trigger_changelevel", "light"] {
        assert!(names.iter().any(|name| name == needed), "{needed}");
    }
    // Its exit leads back to it.
    let exit = names
        .iter()
        .position(|n| n == "trigger_changelevel")
        .unwrap();
    let read = session
        .execute(MAP_ENTITY, json!({ "entity": exit }))
        .unwrap();
    assert!(read.to_string().contains("random5"), "{read}");

    // The same seed again is the same file, which is there already.
    let again = session.execute(MAP_GENERATE, json!({ "seed": 5, "size": "small" }));
    assert!(again.unwrap_err().to_string().ends_with("is there already"));
    // At a path of its own it is the same level.
    session
        .execute(
            MAP_GENERATE,
            json!({ "seed": 5, "size": "small", "path": "maps/again.map" }),
        )
        .unwrap();
    let first = std::fs::read_to_string(&path).unwrap();
    let second = std::fs::read_to_string(root.join("maps/again.map")).unwrap();
    assert_eq!(first.replace("random5", "again"), second);
}

// No spec statement: L-48, with no seed one is drawn, and the level is named for it and of medium size
#[test]
fn a_seed_is_drawn_when_none_is_given() {
    let (root, mut session) = project("drawn", &[], &["brick"]);
    let made = session.execute(MAP_GENERATE, json!({})).unwrap();
    let seed = made["seed"].as_u64().unwrap();
    assert_eq!(
        made["path"],
        json!(root.join("maps").join(format!("random{seed}.map")))
    );
    assert_eq!(made["rooms"].as_array().unwrap().len(), 8);
}

// No spec statement: L-48, a project whose code defines the world gets only the classes it
// defines, which may be none of a level's; one whose classes cannot be read gets id's
#[test]
fn only_the_projects_classes_are_placed() {
    let always = [
        "worldspawn",
        "info_player_start",
        "trigger_changelevel",
        "light",
    ];
    let (_, mut session) = project(
        "classes",
        &["worldspawn", "monster_dog", "item_shells"],
        &["brick", "stone"],
    );
    session
        .execute(MAP_GENERATE, json!({ "seed": 2, "size": "large" }))
        .unwrap();
    let names = classnames(&session);
    assert!(names.iter().any(|name| name == "monster_dog"));
    for name in names {
        let named = ["monster_dog", "item_shells"];
        assert!(
            always.contains(&name.as_str()) || named.contains(&name.as_str()),
            "{name}"
        );
    }

    // A game of its own, with none of id's classes: no monster, door or key is put in it.
    let (_, mut session) = project("own", &["worldspawn", "monster_mine"], &["brick"]);
    let made = session
        .execute(MAP_GENERATE, json!({ "seed": 2, "size": "large" }))
        .unwrap();
    assert_eq!(
        (made["monsters"].as_u64(), made["locked"].as_bool()),
        (Some(0), Some(false))
    );
    for name in classnames(&session) {
        assert!(always.contains(&name.as_str()), "{name}");
    }

    // No world defined: the classes cannot be read, and id's are placed.
    let (_, mut session) = project("unread", &["monster_mine"], &["brick"]);
    session
        .execute(MAP_GENERATE, json!({ "seed": 2, "size": "large" }))
        .unwrap();
    let names = classnames(&session);
    assert!(names.iter().any(|name| name.starts_with("monster_")));
    assert!(names.iter().all(|name| name != "monster_mine"));
}

// No spec statement: L-48, a level of each size has no issue `map.issues` finds
#[test]
fn a_generated_level_has_no_issue() {
    let (_, mut session) = project("issues", &[], &["brick", "stone", "*lava1", "sky1"]);
    for (seed, size) in [(1, "small"), (2, "medium"), (3, "large")] {
        session
            .execute(MAP_GENERATE, json!({ "seed": seed, "size": size }))
            .unwrap();
        let found = session.execute(MAP_ISSUES, json!({})).unwrap();
        assert_eq!(found["issues"], json!([]), "seed {seed} {size}");
    }
}

// No spec statement: L-48, the seed drawn for a time is its seconds mixed with the fraction
// of its second: two times a moment apart draw two seeds, and one time always the same
#[test]
fn a_seed_is_drawn_from_the_time() {
    let at = |secs: u64, nanos: u32| seed_at(Duration::new(secs, nanos));
    assert_eq!(at(0, 0), 0);
    assert_eq!(at(1_800_000_000, 0), 1_800_000_000);
    assert_eq!(at(1_800_000_000, 5), at(1_800_000_000, 5));
    assert_ne!(at(1_800_000_000, 5), at(1_800_000_000, 6));
    assert_ne!(at(1_800_000_000, 5), at(1_800_000_001, 5));
}

// No spec statement: L-48, no level is made with no game, with no texture in the game's
// levels, with no project open, or outside the project
#[test]
fn no_level_is_made_without_a_project_or_a_texture() {
    // A game whose levels hold no texture.
    let (root, mut session) = project("bare", &[], &[]);
    let error = session
        .execute(MAP_GENERATE, json!({ "seed": 1 }))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no texture to paint a wall with"), "{error}");
    assert!(!root.join("maps").exists());

    // No game at all, as on the command line: it says that, not that textures are missing.
    let mut session = MapContext::in_project(&root);
    let error = session
        .execute(MAP_GENERATE, json!({ "seed": 1 }))
        .unwrap_err()
        .to_string();
    assert!(error.contains("no game was found"), "{error}");
    assert!(!root.join("maps").exists());

    let error = MapContext::session()
        .execute(MAP_GENERATE, json!({ "seed": 1 }))
        .unwrap_err()
        .to_string();
    assert!(error.ends_with("no project is open"), "{error}");

    // A path outside the project is refused, as `map.new` refuses it.
    let outside = std::env::temp_dir().join("qs-wb-generate-outside.map");
    let (_, mut session) = project("outside", &[], &["brick"]);
    let error = session
        .execute(MAP_GENERATE, json!({ "seed": 1, "path": outside }))
        .unwrap_err()
        .to_string();
    assert!(
        error.ends_with("a new map is a .map file in the project"),
        "{error}"
    );
}
