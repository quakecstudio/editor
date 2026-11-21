// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is the test's failure"
)]

use super::*;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

fn id_names() -> Vec<String> {
    names(&[
        "tech01_1",
        "sfloor4_1",
        "ceiling4",
        "tech03_1",
        "adoor01_2",
        "city4_7",
        "ground1_6",
        "wood1_5",
        "wmet2_1",
        "door02_2",
        "*lava1",
        "*water1",
        "sky1",
        "trigger",
    ])
}

fn level(seed: u64, size: Size) -> Level {
    let textures = id_names();
    Request {
        seed,
        size,
        textures: &textures,
        classes: &Classes::Any,
        name: "random",
    }
    .generate()
    .unwrap()
}

// No spec statement: L-48, one seed gives one level, byte for byte, and another seed another
#[test]
fn one_seed_is_one_level() {
    let a = level(7, Size::Medium).map.to_bytes();
    let b = level(7, Size::Medium).map.to_bytes();
    let c = level(8, Size::Medium).map.to_bytes();
    assert_eq!(a, b);
    assert_ne!(a, c);
}

// No spec statement: L-48, every brush of a level is a valid solid whose faces' points are whole (MAP-3.3)
#[test]
fn every_brush_is_valid_and_on_the_grid() {
    for seed in 0..40 {
        for size in [Size::Small, Size::Medium, Size::Large] {
            let level = level(seed, size);
            for entity in &level.map.entities {
                for brush in entity.brushes() {
                    assert!(brush.is_valid(), "seed {seed}");
                    for face in &brush.faces {
                        for point in face.points() {
                            for at in point {
                                assert!((at - at.round()).abs() < 1e-6, "seed {seed}: {at}");
                            }
                        }
                    }
                }
            }
        }
    }
}

// No spec statement: L-48, a level has its size's rooms, one start, one exit in a chamber, and a key when a door is locked
#[test]
fn a_level_has_a_start_an_exit_and_its_rooms() {
    for (size, rooms) in [(Size::Small, 5), (Size::Medium, 8), (Size::Large, 12)] {
        let level = level(3, size);
        assert_eq!(level.rooms.len(), rooms);
        let count = |classname: &[u8]| {
            level
                .map
                .entities
                .iter()
                .filter(|entity| entity.classname() == Some(classname))
                .count()
        };
        assert_eq!(count(b"worldspawn"), 1);
        assert_eq!(count(b"info_player_start"), 1);
        assert_eq!(count(b"trigger_changelevel"), 1);
        assert_eq!(count(b"item_key1"), usize::from(level.locked));
        assert!(level.monsters > 0);
        assert_eq!(level.rooms[level.exit].shape, RoomShape::Chamber);
    }
}

// No spec statement: L-48, only classes the game defines are placed: no key locks no door, and no door class leaves none
#[test]
fn only_the_classes_there_are_placed() {
    let textures = id_names();
    let named =
        |names: &[&str]| Classes::Named(names.iter().map(|name| (*name).to_owned()).collect());
    let level = |classes: &Classes| {
        Request {
            seed: 1,
            size: Size::Large,
            textures: &textures,
            classes,
            name: "random",
        }
        .generate()
        .unwrap()
    };
    let always: &[&[u8]] = &[
        b"worldspawn",
        b"info_player_start",
        b"trigger_changelevel",
        b"light",
    ];
    let holds = |level: &Level, classname: &[u8]| {
        level
            .map
            .entities
            .iter()
            .any(|entity| entity.classname() == Some(classname))
    };
    // With every class, seed 1 has a locked door, its key and torches.
    let whole = level(&Classes::Any);
    assert!(whole.locked);
    assert!(holds(&whole, b"item_key1") && holds(&whole, b"func_door"));
    assert!(holds(&whole, b"light_torch_small_walltorch"));

    // A game with doors and no key locks none; one with neither has no door.
    let keyless = level(&named(&["monster_dog", "item_health", "func_door"]));
    assert!(!keyless.locked);
    assert!(holds(&keyless, b"func_door") && !holds(&keyless, b"item_key1"));
    assert!(keyless.monsters > 0);
    for entity in &keyless.map.entities {
        let classname = entity.classname().unwrap();
        let named: &[&[u8]] = &[b"monster_dog", b"item_health", b"func_door"];
        assert!(always.contains(&classname) || named.contains(&classname));
        // No lock is left on a door.
        assert_eq!(entity.value(b"spawnflags"), None);
    }
    let doorless = level(&named(&["monster_dog", "item_key1"]));
    assert!(!doorless.locked);
    assert!(!holds(&doorless, b"func_door") && !holds(&doorless, b"item_key1"));
}

// No spec statement: L-48, a game with no plain texture makes no level
#[test]
fn no_textures_is_an_error() {
    let request = Request {
        seed: 1,
        size: Size::Small,
        textures: &names(&["*lava1", "sky1", "trigger"]),
        classes: &Classes::Any,
        name: "random",
    };
    assert!(matches!(request.generate(), Err(GenError::NoTextures)));
}

// No spec statement: L-48, a game with no theme's textures is painted from what it has, with no pit and no courtyard when it has no liquid and no sky
#[test]
fn a_game_with_no_theme_is_painted_plain() {
    let textures = names(&["brick", "stone", "wood"]);
    let level = Request {
        seed: 1,
        size: Size::Small,
        textures: &textures,
        classes: &Classes::Any,
        name: "random",
    }
    .generate()
    .unwrap();
    assert_eq!(level.theme, "plain");
    // Every face is painted with a texture the game has: its trigger's too, with no
    // `trigger` texture to give it.
    for entity in &level.map.entities {
        for brush in entity.brushes() {
            for face in &brush.faces {
                let name = String::from_utf8_lossy(face.texture()).into_owned();
                assert!(textures.contains(&name), "{name}");
            }
        }
    }
    // Neither a pit nor a courtyard without a liquid and a sky to paint them.
    for seed in 0..20 {
        let level = Request {
            seed,
            size: Size::Large,
            textures: &textures,
            classes: &Classes::Any,
            name: "random",
        }
        .generate()
        .unwrap();
        for room in &level.rooms {
            assert!(!matches!(room.shape, RoomShape::Pit | RoomShape::Court));
        }
    }
}
