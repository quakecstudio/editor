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

// No spec statement: L-48, every brush of a level is a valid solid whose faces' points are whole
// (MAP-3.3), within 4096 units of the map's middle (NET-1.1)
#[test]
fn every_brush_is_valid_and_on_the_grid() {
    for seed in 0..40 {
        for size in [Size::Small, Size::Medium, Size::Large] {
            let level = level(seed, size);
            for entity in &level.map.entities {
                for brush in entity.brushes() {
                    assert!(brush.is_valid(), "seed {seed}");
                    // Within what the game can send of a position (NET-1.1).
                    for corner in brush.vertices() {
                        assert!(corner.iter().all(|at| at.abs() < 4096.0), "seed {seed}");
                    }
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

// No spec statement: L-48 and L-74, a level has its size's rooms on its path and more off it,
// one start, one exit in a chamber, and each key its locks need
#[test]
fn a_level_has_a_start_an_exit_and_its_rooms() {
    for (size, rooms) in [(Size::Small, 5), (Size::Medium, 8), (Size::Large, 12)] {
        let level = level(3, size);
        assert!(level.rooms.len() > rooms, "{size:?}: {}", level.rooms.len());
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
        let locks = &level.report.locks;
        assert_eq!(
            count(b"item_key1"),
            usize::from(locks.contains(&LockKind::SilverKey))
        );
        assert_eq!(
            count(b"item_key2"),
            usize::from(locks.contains(&LockKind::GoldKey))
        );
        assert_eq!(
            count(b"func_button"),
            usize::from(locks.contains(&LockKind::Button))
        );
        assert_eq!(
            count(b"trigger_counter"),
            usize::from(locks.contains(&LockKind::Arena))
        );
        assert!(level.monsters > 0);
        assert_eq!(level.rooms[level.exit].shape, RoomShape::Chamber);
    }
}

// No spec statement: L-48 and L-74, only classes the game defines are placed: a lock whose
// classes the game lacks is an open door or an opening, and no door class leaves none
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
    // With every class, a large level has locked doors and torches.
    let whole = level(&Classes::Any);
    assert_eq!(whole.report.locks.len(), 3);
    assert!(holds(&whole, b"func_door"));
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
        // No lock is left on a door, and no door waits for a button or an arena.
        assert_eq!(entity.value(b"spawnflags"), None);
        assert_eq!(entity.value(b"targetname"), None);
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

// No spec statement: L-74, a liquid's hazard is read from its texture's name: lava is deadly,
// slime survivable, any other liquid harmless
#[test]
fn a_liquid_is_as_hazardous_as_its_name() {
    let of = |name: &str| theme::Hazard::of(&theme::surface(name));
    assert_eq!(of("*lava1"), theme::Hazard::Deadly);
    assert_eq!(of("*LAVA1"), theme::Hazard::Deadly);
    assert_eq!(of("*slime0"), theme::Hazard::Survivable);
    assert_eq!(of("*water1"), theme::Hazard::Harmless);
    assert_eq!(of("*04water"), theme::Hazard::Harmless);
}

/// The solids a pit room of `liquid` builds in a room of half-width 256.
fn pit_solids(liquid: &str) -> usize {
    let plan = layout::RoomPlan {
        middle: [0.0, 0.0],
        shape: RoomShape::Pit,
        floor: 0.0,
        half: [256.0, 256.0],
        height: 256.0,
        turned: Side::East,
        depth: 0,
        role: mission::Role::Way,
    };
    let wall = theme::surface("wall");
    let paints = theme::Paints::named(
        wall.clone(),
        wall.clone(),
        wall.clone(),
        wall.clone(),
        wall.clone(),
        Some(theme::surface(liquid)),
        None,
    );
    let mut mass = mass::Mass::of_blocks(
        &[mass::Bounds {
            min: [-384.0, -384.0, -384.0],
            max: [384.0, 384.0, 704.0],
        }],
        &wall,
    )
    .unwrap();
    rooms::Room::new(&plan, rooms::Part::Way)
        .build(&mut mass, &paints)
        .unwrap()
        .solids
        .len()
}

// No spec statement: L-74, a pit of lava has a rail along its edge and its bridges, which a pit
// of water or slime does not need: sixteen rails
#[test]
fn only_a_pit_of_lava_is_railed() {
    let water = pit_solids("*water1");
    assert_eq!(pit_solids("*slime0"), water);
    assert_eq!(pit_solids("*lava1"), water + 16);
}

// No spec statement: L-74, every medium and large level is on two floors or more, has a loop,
// two kinds of lock, a space seen before it is reached and a secret, by its report
#[test]
fn a_level_is_designed() {
    let mut shortcuts = 0;
    for seed in 0..64 {
        for size in [Size::Medium, Size::Large] {
            let report = level(seed, size).report;
            let what = format!("seed {seed} {size:?}: {report:?}");
            assert!(report.floors >= 2, "{what}");
            assert!(report.loops >= 1, "{what}");
            let mut kinds = report.locks.clone();
            kinds.sort_unstable();
            kinds.dedup();
            assert!(kinds.len() >= 2, "{what}");
            assert!(report.teasers >= 1, "{what}");
            assert_eq!(report.secrets, 1, "{what}");
            shortcuts += report.shortcuts;
        }
    }
    assert!(shortcuts > 0, "no level has a teleporter back to its start");
}

// No spec statement: L-74, the plan kept is the best of those drawn: it scores at least the
// first, which a level of one candidate would have been
#[test]
fn the_best_plan_is_kept() {
    let textures = id_names();
    let mut better = 0;
    for seed in 0..16 {
        let request = Request {
            seed,
            size: Size::Large,
            textures: &textures,
            classes: &Classes::Any,
            name: "random",
        };
        let has = layout::Has {
            liquid: layout::Liquid::There,
            sky: layout::Sky::There,
        };
        let mut rng = Rng::new(seed);
        let mut first = Plan::draw(Size::Large, has, &mut rng).unwrap();
        let first_report = Mission::arrange(&mut first, Size::Large, &Classes::Any, &mut rng);
        let first = Score::of(&first, &first_report);
        let (kept, kept_report) = request.best(has, &mut Rng::new(seed)).unwrap();
        let kept = Score::of(&kept, &kept_report);
        assert!(kept >= first, "seed {seed}");
        if kept > first {
            better += 1;
        }
    }
    assert!(better > 0, "no seed's first plan was bettered");
}
