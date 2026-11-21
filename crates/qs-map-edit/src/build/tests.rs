// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is the test's failure"
)]

use std::path::PathBuf;

use qs_commands::Session;
use serde_json::{Value, json};

use super::{MAP_BUILD_PASSAGE, MAP_BUILD_ROOM};
use crate::testing::{MapContext, TestGame, TestProject, level_bsp};
use crate::{ClassInfo, GameFolders, MAP_NEW};

/// A session with a new map open in a project of its own, whose code defines `classes`, and
/// a game one of whose levels holds brick, stone, wood, lava and a sky when `game` is asked for.
fn opened(name: &str, classes: &[&str], game: bool) -> (PathBuf, Session<MapContext>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-build-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    let root = dir.join("mod");
    std::fs::create_dir_all(&root).unwrap();
    let folders = if game {
        let base = dir.join("game");
        std::fs::create_dir_all(base.join("id1/maps")).unwrap();
        std::fs::write(
            base.join("id1/maps/e1.bsp"),
            level_bsp(&["brick", "stone", "wood", "*lava1", "sky1"]),
        )
        .unwrap();
        GameFolders {
            base: Some(base),
            chosen: None,
        }
    } else {
        GameFolders::default()
    };
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
    let mut session = MapContext {
        project: TestProject {
            root: Some(root.clone()),
            classes: classes.iter().map(class).collect(),
            ..TestProject::default()
        },
        game: Some(TestGame {
            folders,
            ..TestGame::default()
        }),
        ..MapContext::default()
    }
    .into_session();
    session
        .execute(MAP_NEW, json!({ "path": "maps/built.map" }))
        .unwrap();
    (root, session)
}

/// The classnames of the open map's entities, and how many brushes its world has.
fn held(session: &Session<MapContext>) -> (Vec<String>, usize) {
    let file = session.context().map().unwrap().file();
    let names = file
        .entities
        .iter()
        .map(|entity| String::from_utf8_lossy(entity.classname().unwrap()).into_owned())
        .collect();
    (names, file.entities[0].brushes().count())
}

/// The textures every room of these tests names, so no game is needed.
fn named() -> Value {
    json!({ "wall": "w", "floor": "f", "ceiling": "c", "trim": "t", "liquid": "*lava1", "sky": "sky1" })
}

// No spec statement: L-49, `map.build_room` adds a room's brushes to the world and its lights
// as entities in one step that undo takes back, and answers its doorways, where the player
// stands and where things may; lights are not placed when none are asked for
#[test]
fn a_room_is_built_with_its_lights_as_one_step() {
    let (_, mut session) = opened("room", &[], false);
    let built = session
        .execute(
            MAP_BUILD_ROOM,
            json!({
                "shape": "hall", "middle": [0, 0], "floor": 0, "half": [256, 192], "height": 192,
                "doors": ["east", "south"], "arch": "arch", "textures": named(),
            }),
        )
        .unwrap();
    let (names, brushes) = held(&session);
    assert_eq!(built["brushes"], json!(brushes));
    assert_eq!(built["total"], json!(brushes));
    assert!(brushes > 20, "{brushes}");
    // The doorways are on the room's outer face, 64 of rock out from its space, at its floor.
    assert_eq!(
        built["doorways"],
        json!([
            { "side": "east", "at": [320.0, 0.0, 0.0] },
            { "side": "south", "at": [0.0, -256.0, 0.0] },
        ])
    );
    assert_eq!(built["stand"], json!([0.0, 0.0, 24.0]));
    assert_eq!(built["spots"].as_array().unwrap().len(), 8);
    // One light under the raised ceiling and a torch's light on each of eight pilasters: no
    // torch class is defined, so each is a light.
    assert_eq!(built["lights"], 9);
    assert_eq!(names.iter().filter(|name| *name == "light").count(), 9);
    assert_eq!(names.len(), 10);

    session.undo().unwrap();
    assert_eq!(held(&session), (vec!["worldspawn".to_owned()], 0));
    session.redo().unwrap();
    assert_eq!(held(&session).1, brushes);

    let dark = session
        .execute(
            MAP_BUILD_ROOM,
            json!({
                "shape": "vault", "middle": [1024, 0], "floor": 0, "half": [192, 192],
                "height": 224, "lights": "none", "textures": named(),
            }),
        )
        .unwrap();
    assert_eq!(dark["lights"], 0);
    assert_eq!(dark["doorways"], json!([]));
    assert_eq!(held(&session).0.len(), 10);
    assert_eq!(
        dark["total"].as_u64().unwrap(),
        dark["brushes"].as_u64().unwrap() + u64::try_from(brushes).unwrap()
    );
}

// No spec statement: L-49, a torch is placed where the project's code defines one
#[test]
fn torches_burn_where_the_project_has_them() {
    let (_, mut session) = opened("torch", &["light_torch_small_walltorch"], false);
    session
        .execute(
            MAP_BUILD_ROOM,
            json!({
                "shape": "hall", "middle": [0, 0], "floor": 0, "half": [192, 192], "height": 192,
                "textures": named(),
            }),
        )
        .unwrap();
    let (names, _) = held(&session);
    let torches = names
        .iter()
        .filter(|name| *name == "light_torch_small_walltorch")
        .count();
    assert_eq!(
        (
            torches,
            names.iter().filter(|name| *name == "light").count()
        ),
        (8, 1)
    );
}

// No spec statement: L-49, `map.build_passage` joins two doorways with stairs between their
// floors and a light, and a door asked for is a func_door, locked with the silver key's flag
#[test]
fn a_passage_is_built_between_two_doorways() {
    let (_, mut session) = opened("passage", &[], false);
    let passage = session
        .execute(
            MAP_BUILD_PASSAGE,
            json!({
                "from": [320, 0, 0], "to": [704, 0, 64], "door": "locked", "textures": named(),
            }),
        )
        .unwrap();
    assert_eq!(passage["steps"], 4);
    assert_eq!(passage["door"], 2);
    let file = session.context().map().unwrap().file();
    assert_eq!(file.entities[1].classname(), Some(b"light".as_slice()));
    let door = &file.entities[2];
    assert_eq!(door.classname(), Some(b"func_door".as_slice()));
    assert_eq!(door.value(b"spawnflags"), Some(b"16".as_slice()));
    assert_eq!(door.value(b"angle"), Some(b"-1".as_slice()));
    assert_eq!(door.brushes().count(), 1);
    session.undo().unwrap();
    assert_eq!(held(&session), (vec!["worldspawn".to_owned()], 0));

    let plain = session
        .execute(
            MAP_BUILD_PASSAGE,
            json!({
                "from": [0, 256, 0], "to": [0, 512, 0], "arch": "arch", "door": "door",
                "textures": named(),
            }),
        )
        .unwrap();
    assert_eq!(plain["steps"], 0);
    let file = session.context().map().unwrap().file();
    assert_eq!(file.entities[2].value(b"spawnflags"), None);
}

// No spec statement: L-49, what is not named of the textures is a theme's from the game's
// levels, the same for one theme number; with no game and a texture not named it says so
#[test]
fn textures_not_named_are_a_themes() {
    let (_, mut session) = opened("themed", &[], true);
    let room = |session: &mut Session<MapContext>, x: i32, textures: Value| {
        session
            .execute(
                MAP_BUILD_ROOM,
                json!({
                    "shape": "pit", "middle": [x, 0], "floor": 0, "half": [224, 224],
                    "height": 192, "lights": "none", "textures": textures,
                }),
            )
            .unwrap();
        let file = session.context().map().unwrap().file();
        let mut used: Vec<String> = file.entities[0]
            .brushes()
            .flat_map(|brush| brush.faces.iter())
            .map(|face| String::from_utf8_lossy(face.texture()).into_owned())
            .collect();
        used.sort();
        used.dedup();
        used
    };
    let first = room(&mut session, 0, json!({}));
    for name in &first {
        assert!(
            ["brick", "stone", "wood", "*lava1"].contains(&name.as_str()),
            "{name}"
        );
    }
    assert!(first.contains(&"*lava1".to_owned()));
    session.undo().unwrap();
    assert_eq!(room(&mut session, 0, json!({ "theme": 0 })), first);
    session.undo().unwrap();
    // A texture named takes the theme's place.
    let walled = room(&mut session, 0, json!({ "wall": "mine" }));
    assert!(walled.contains(&"mine".to_owned()));

    let (_, mut bare) = opened("no-game", &[], false);
    let error = bare
        .execute(
            MAP_BUILD_ROOM,
            json!({
                "shape": "hall", "middle": [0, 0], "floor": 0, "half": [192, 192],
                "height": 192, "textures": { "wall": "w" },
            }),
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("no game was found"), "{error}");
}

// No spec statement: L-49, a room or a passage that cannot be built as asked is refused, saying
// what its shape needs, and nothing is added
#[test]
fn what_cannot_be_built_is_refused() {
    let (_, mut session) = opened("refused", &[], false);
    let mut refused = |id: &str, mut params: Value| {
        params["textures"] = named();
        let error = session.execute(id, params).unwrap_err().to_string();
        assert_eq!(held(&session), (vec!["worldspawn".to_owned()], 0));
        error
    };
    let room = |change: Value| {
        let mut params = json!({
            "shape": "chamber", "middle": [0, 0], "floor": 0, "half": [224, 224], "height": 192,
        });
        for (key, value) in change.as_object().unwrap() {
            params[key] = value.clone();
        }
        params
    };
    let error = refused(MAP_BUILD_ROOM, room(json!({ "half": [192, 192] })));
    assert!(
        error.contains("a chamber is at least 224 in each half-width and 192 high"),
        "{error}"
    );
    let error = refused(MAP_BUILD_ROOM, room(json!({ "half": [256, 224] })));
    assert!(error.contains("as wide as it is long"), "{error}");
    let error = refused(MAP_BUILD_ROOM, room(json!({ "floor": 10 })));
    assert!(error.contains("a multiple of 16"), "{error}");
    let error = refused(MAP_BUILD_ROOM, room(json!({ "wall": 16 })));
    assert!(error.contains("a wall is from 32 to 256 thick"), "{error}");
    let error = refused(MAP_BUILD_ROOM, room(json!({ "doors": ["east", "east"] })));
    assert!(error.contains("a side has one doorway"), "{error}");
    let error = refused(
        MAP_BUILD_ROOM,
        room(json!({ "shape": "split", "height": 192 })),
    );
    assert!(
        error.contains("a split is at least 224 in each half-width and 256 high"),
        "{error}"
    );

    let error = refused(
        MAP_BUILD_PASSAGE,
        json!({ "from": [0, 0, 0], "to": [256, 64, 0] }),
    );
    assert!(error.contains("in line along x or along y"), "{error}");
    let error = refused(
        MAP_BUILD_PASSAGE,
        json!({ "from": [0, 0, 0], "to": [0, 0, 64] }),
    );
    assert!(error.contains("in line along x or along y"), "{error}");
    let error = refused(
        MAP_BUILD_PASSAGE,
        json!({ "from": [0, 0, 0], "to": [96, 0, 64] }),
    );
    assert!(error.contains("it needs a length of 128"), "{error}");
}
