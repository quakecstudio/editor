// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap, index or panic is the test's failure"
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
// one start, one exit in a chamber, hall or vault, and each key its locks need
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
        assert!(matches!(
            level.rooms[level.exit].shape,
            RoomShape::Chamber | RoomShape::Hall | RoomShape::Vault
        ));
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

// No spec statement: L-74, a lock or a reward whose classes the game lacks is not reported or
// placed: an arena with no monster to fire its counter is a plain door, and a secret room has
// no megahealth where the game has no item_health
#[test]
fn what_the_game_lacks_is_not_reported_or_placed() {
    let textures = id_names();
    let named =
        |names: &[&str]| Classes::Named(names.iter().map(|name| (*name).to_owned()).collect());
    let classes = named(&[
        "func_door",
        "trigger_counter",
        "func_door_secret",
        "trigger_secret",
    ]);
    for seed in 0..8 {
        let level = Request {
            seed,
            size: Size::Large,
            textures: &textures,
            classes: &classes,
            name: "random",
        }
        .generate()
        .unwrap();
        assert!(
            !level.report.locks.contains(&LockKind::Arena),
            "seed {seed}"
        );
        assert_eq!(level.report.secrets, 1, "seed {seed}");
        let holds = |classname: &[u8]| {
            level
                .map
                .entities
                .iter()
                .any(|entity| entity.classname() == Some(classname))
        };
        assert!(
            !holds(b"trigger_counter") && !holds(b"item_health"),
            "seed {seed}"
        );
        assert!(
            holds(b"func_door_secret") && holds(b"trigger_secret"),
            "seed {seed}"
        );
    }
}

// No spec statement: L-74, bays asked of a shape that takes none are passed over, whatever its
// wall, and asked of one that takes them need a wall that holds them
#[test]
fn bays_need_a_wall_only_where_they_fit() {
    let kit = |shape: RoomShape| RoomKit {
        shape,
        middle: [0.0, 0.0],
        floor: 0.0,
        half: [shape.least_half(), shape.least_half()],
        height: shape.least_height(),
        turned: Side::North,
        doors: Vec::new(),
        arch: Arch::Cut,
        wall: KIT_WALL,
        parts: vec![RoomPart::Bays],
    };
    let textures = Textures {
        wall: "wall".to_owned(),
        floor: "floor".to_owned(),
        ceiling: "ceiling".to_owned(),
        trim: "trim".to_owned(),
        door: "door".to_owned(),
        liquid: None,
        sky: None,
        wall_lights: WallLights::Lamps,
    };
    assert!(kit(RoomShape::Chamber).build(&textures).is_ok());
    assert!(matches!(
        kit(RoomShape::Hall).build(&textures),
        Err(GenError::Kit(KitError::BayWall { .. }))
    ));
}

// No spec statement: L-74, every monster faces a way set by the level, not the default east;
// the world names a music track of id's, 2 to 11, which the server sends as the CD track; and
// no light is brighter than a room's main light
#[test]
fn monsters_face_in_and_the_level_has_music() {
    for seed in 0..8 {
        let level = level(seed, Size::Large);
        let mut faced = Vec::new();
        for entity in &level.map.entities {
            let classname = entity.classname().unwrap();
            if classname.starts_with(b"monster_") {
                let angle = entity.value(b"angle").unwrap();
                faced.push(angle.to_vec());
            }
            if classname == b"worldspawn" {
                let track: usize = std::str::from_utf8(entity.value(b"sounds").unwrap())
                    .unwrap()
                    .parse()
                    .unwrap();
                assert!((2..=11).contains(&track), "seed {seed}: track {track}");
            }
            if classname == b"light" {
                let light: f64 = std::str::from_utf8(entity.value(b"light").unwrap())
                    .unwrap()
                    .parse()
                    .unwrap();
                assert!(light <= 450.0, "seed {seed}: light {light}");
            }
        }
        faced.sort();
        faced.dedup();
        assert!(faced.len() > 1, "seed {seed}: every monster faces one way");
    }
}

// No spec statement: L-74, a level's lamps are of its theme's light texture, as id's levels
// mount their lights: a base's tlight, a city's light1
#[test]
fn lamps_are_of_the_themes_light_texture() {
    let lamps = |names: &[&str], prefix: &[u8]| {
        let textures = self::names(names);
        (0..8).any(|seed| {
            let level = Request {
                seed,
                size: Size::Medium,
                textures: &textures,
                classes: &Classes::Any,
                name: "random",
            }
            .generate()
            .unwrap();
            level.map.entities[0].brushes().any(|brush| {
                brush
                    .faces
                    .iter()
                    .any(|face| face.texture().starts_with(prefix))
            })
        })
    };
    // Only a base's names: tech walls, and its lamps.
    let base = &[
        "tech01_1",
        "tech04_1",
        "sfloor4_1",
        "ceiling4",
        "tech03_1",
        "adoor01_2",
        "tlight01",
    ];
    assert!(lamps(base, b"tlight"));
    let city = &[
        "city4_7",
        "city4_6",
        "ground1_6",
        "wood1_5",
        "wmet2_1",
        "door02_2",
        "light1_4",
    ];
    assert!(lamps(city, b"light1"));
}

// No spec statement: L-74, a lamp's light texture is scaled to the lamp and shifted so the
// lamp's corner is a corner of the texture, wherever the lamp is: s from its start, and t from
// its far edge (a face's t runs down it)
#[test]
fn a_lamp_shows_its_light_whole() {
    let lamp = theme::Lamp::sized(theme::surface("tlight02"), 64.0);
    for [s, t] in [
        [-784.0, 100.0],
        [0.0, 0.0],
        [1040.0, -2312.0],
        [-16.0, 48.0],
    ] {
        let surface = lamp.on([s, t]);
        let qs_map::TexDef::Standard { shift, scale, .. } = surface.texdef else {
            panic!("a standard texture")
        };
        assert_eq!(scale, [0.5, 0.5]);
        let [ss, st] = shift;
        assert_eq!((s / scale[0] + ss).rem_euclid(64.0), 0.0, "s at {s}");
        let far = t + theme::LAMP_SIZE;
        assert_eq!((-far / scale[1] + st).rem_euclid(64.0), 0.0, "t at {t}");
    }
    // A lamp of no light of the theme's is painted as it comes.
    let plain = theme::Lamp::plain(theme::surface("trim"));
    assert_eq!(plain.on([5.0, 7.0]), theme::surface("trim"));
}

// No spec statement: L-74, levels differ in their make: great rooms beside ordinary ones and
// junctions where corridors turn, the exit in rooms of more than one shape, and the hook at
// more than one place along the path
#[test]
fn levels_differ_in_scale_and_shape() {
    let mut halves = Vec::new();
    let mut exits = Vec::new();
    let mut junctions = 0;
    for seed in 0..32 {
        let level = level(seed, Size::Large);
        for room in &level.rooms {
            if room.shape == RoomShape::Junction {
                junctions += 1;
            }
        }
        exits.push(level.rooms[level.exit].shape);
        let textures = id_names();
        let request = Request {
            seed,
            size: Size::Large,
            textures: &textures,
            classes: &Classes::Any,
            name: "random",
        };
        let mut rng = Rng::new(seed);
        let paints = theme::Paints::choose(&textures, &mut rng).unwrap();
        let has = layout::Has {
            liquid: if paints.liquid.is_some() {
                layout::Liquid::There
            } else {
                layout::Liquid::Missing
            },
            sky: if paints.sky.is_some() {
                layout::Sky::There
            } else {
                layout::Sky::Missing
            },
        };
        let (plan, _) = request.best(has, &mut rng).unwrap();
        for room in &plan.rooms {
            halves.extend(room.half);
        }
    }
    let widest = halves.iter().copied().fold(0.0, f64::max);
    let narrowest = halves.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(widest >= 288.0, "no great room: {widest}");
    assert!(narrowest <= kit::JUNCTION_HALF, "no junction: {narrowest}");
    assert!(junctions > 0);
    exits.sort_by_key(|shape| shape.name());
    exits.dedup();
    assert!(exits.len() > 1, "the exit is always a {:?}", exits);
}
