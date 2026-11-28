// SPDX-License-Identifier: GPL-3.0-or-later

//! The playability check on maps built for each finding: a room with the player start at its
//! west end and the exit at its east end, and what each test puts between them.

#![expect(
    clippy::unwrap_used,
    clippy::unwrap_in_result,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "test code: a failed unwrap or index is a failed test, and the room's units fit"
)]

use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Entity, Item, MapFile, Surface, TexDef};

use super::{ExitReach, FINE, Ground, JUMP, Moves, PlayError, Playability, Skill, Sought};

/// The room's half length, half width and height.
const LONG: f64 = 512.0;
const WIDE: f64 = 128.0;
const HIGH: f64 = 256.0;
/// How thick its walls are.
const WALL: f64 = 16.0;
/// Where the player starts.
const START: &str = "-400 0 24";

fn rock() -> Surface {
    Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    }
}

fn cuboid(min: [f64; 3], max: [f64; 3]) -> Brush {
    Brush::cuboid(min, max, &rock()).unwrap()
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

/// An entity named by its place in the lump, which a compile of these maps keeps.
#[expect(
    clippy::unnecessary_wraps,
    reason = "the check names entities through an Option"
)]
fn lump(place: usize) -> Option<usize> {
    Some(place)
}

/// What a test builds in the room.
#[derive(Default)]
struct Room {
    /// The world's brushes besides the room's; a floor at height 0 when `floor` is empty.
    world: Vec<Brush>,
    /// The floor's brushes, in place of a whole floor.
    floor: Vec<Brush>,
    /// The entities besides the start and the exit.
    entities: Vec<Entity>,
}

impl Room {
    /// The room's map: sealed, the start at its west end and a `trigger_changelevel` across
    /// its east end.
    fn map(self) -> Vec<u8> {
        let bottom = -HIGH;
        let mut brushes = vec![
            cuboid(
                [-LONG - WALL, -WIDE - WALL, bottom - WALL],
                [LONG + WALL, WIDE + WALL, bottom],
            ),
            cuboid(
                [-LONG - WALL, -WIDE - WALL, HIGH],
                [LONG + WALL, WIDE + WALL, HIGH + WALL],
            ),
            cuboid(
                [-LONG - WALL, -WIDE - WALL, bottom],
                [-LONG, WIDE + WALL, HIGH],
            ),
            cuboid(
                [LONG, -WIDE - WALL, bottom],
                [LONG + WALL, WIDE + WALL, HIGH],
            ),
            cuboid([-LONG, -WIDE - WALL, bottom], [LONG, -WIDE, HIGH]),
            cuboid([-LONG, WIDE, bottom], [LONG, WIDE + WALL, HIGH]),
        ];
        if self.floor.is_empty() {
            brushes.push(cuboid([-LONG, -WIDE, bottom], [LONG, WIDE, 0.0]));
        } else {
            brushes.extend(self.floor);
        }
        brushes.extend(self.world);
        let mut file = MapFile::new();
        file.push_entity(entity("worldspawn", &[], brushes));
        file.push_entity(entity(
            "info_player_start",
            &[("origin", START)],
            Vec::new(),
        ));
        file.push_entity(entity(
            "trigger_changelevel",
            &[("map", "next")],
            vec![cuboid([400.0, -WIDE, 0.0], [480.0, WIDE, 128.0])],
        ));
        for entity in self.entities {
            file.push_entity(entity);
        }
        file.to_bytes()
    }

    /// The map compiled and checked with `moves`.
    fn checked(self, moves: Moves) -> Result<Playability<usize>, PlayError> {
        let compiled = qs_map_bsp::Compiler::default()
            .compile(&self.map(), None)
            .unwrap();
        let bytes: &'static [u8] = Box::leak(compiled.bsp.into_boxed_slice());
        let bsp: &'static Bsp<'static> =
            Box::leak(Box::new(Bsp::read(bytes, BspLoader::default()).unwrap()));
        Ground::new(bsp, moves, Skill::Normal)?.playability(lump)
    }
}

/// A wall across the room at x 0 to 16 with a door of `pairs` in its middle, which opens
/// upward (`angle` -1).
fn walled(pairs: &[(&str, &str)]) -> (Vec<Brush>, Entity) {
    let wall = vec![
        cuboid([0.0, -WIDE, 0.0], [16.0, -32.0, HIGH]),
        cuboid([0.0, 32.0, 0.0], [16.0, WIDE, HIGH]),
        cuboid([0.0, -32.0, 128.0], [16.0, 32.0, HIGH]),
    ];
    let mut door = entity(
        "func_door",
        pairs,
        vec![cuboid([0.0, -32.0, 0.0], [16.0, 32.0, 128.0])],
    );
    door.set(b"angle", b"-1").unwrap();
    (wall, door)
}

// No spec statement: L-77, an open room is finished: the exit is reached, nothing is a trap
#[test]
fn an_open_room_can_be_finished() {
    let found = Room::default().checked(Moves::Player).unwrap();
    assert_eq!(found.exit, ExitReach::Reached);
    assert_eq!(found.traps, Vec::new());
    assert_eq!(found.unreached, Vec::new());
    assert_eq!(found.not_judged, Vec::new());
    assert_eq!(found.exits, vec![2]);
}

// No spec statement: L-77, an exit behind a door whose key is behind that door is not reached,
// and with the key in front of the door it is (doors.qc, DOOR_GOLD_KEY 8)
#[test]
fn a_key_behind_its_own_door_keeps_the_exit_out_of_reach() {
    let behind = |x: &str| {
        let (wall, door) = walled(&[("spawnflags", "8")]);
        let key = entity("item_key2", &[("origin", &format!("{x} 0 24"))], Vec::new());
        Room {
            world: wall,
            entities: vec![door, key],
            ..Room::default()
        }
        .checked(Moves::Player)
        .unwrap()
    };
    let locked = behind("200");
    assert_eq!(locked.exit, ExitReach::NotReached);
    assert_eq!(locked.unreached.len(), 1, "{:?}", locked.unreached);
    assert_eq!(locked.unreached[0].kind, Sought::Key);
    assert_eq!(behind("-200").exit, ExitReach::Reached);
}

// No spec statement: L-77, a door with a targetname opens once something fires it: a
// trigger_once the player walks into, through a trigger_relay (triggers.qc)
#[test]
fn a_door_opens_when_a_trigger_fires_it_through_a_relay() {
    let fired = |trigger: bool| {
        let (wall, door) = walled(&[("targetname", "gate")]);
        let relay = entity(
            "trigger_relay",
            &[
                ("targetname", "pass"),
                ("target", "gate"),
                ("origin", "0 0 300"),
            ],
            Vec::new(),
        );
        let mut entities = vec![door, relay];
        if trigger {
            entities.push(entity(
                "trigger_once",
                &[("target", "pass")],
                vec![cuboid([-300.0, -WIDE, 0.0], [-280.0, WIDE, 64.0])],
            ));
        }
        Room {
            world: wall,
            entities,
            ..Room::default()
        }
        .checked(Moves::Player)
        .unwrap()
        .exit
    };
    assert_eq!(fired(true), ExitReach::Reached);
    assert_eq!(fired(false), ExitReach::NotReached);
}

// No spec statement: L-77, a secret no way leads into is never reached
#[test]
fn a_secret_with_no_way_in_is_never_reached() {
    // A closet sealed on every side, the secret inside it.
    let closet = vec![
        cuboid([-100.0, 40.0, 0.0], [100.0, 48.0, 128.0]),
        cuboid([-100.0, 40.0, 128.0], [100.0, WIDE, 136.0]),
        cuboid([-108.0, 40.0, 0.0], [-100.0, WIDE, 136.0]),
        cuboid([100.0, 40.0, 0.0], [108.0, WIDE, 136.0]),
    ];
    let secret = entity(
        "trigger_secret",
        &[],
        vec![cuboid([-32.0, 64.0, 0.0], [32.0, 112.0, 64.0])],
    );
    let found = Room {
        world: closet,
        entities: vec![secret],
        ..Room::default()
    }
    .checked(Moves::Player)
    .unwrap();
    assert_eq!(found.exit, ExitReach::Reached);
    assert_eq!(found.unreached.len(), 1, "{:?}", found.unreached);
    assert_eq!(found.unreached[0].kind, Sought::Secret);
    assert_eq!(found.unreached[0].entity, 3);
}

/// The room with a ledge `height` high along its north side and a health box on it.
fn ledge(height: f64, moves: Moves) -> Playability<usize> {
    let ledge = vec![cuboid([100.0, 32.0, 0.0], [300.0, WIDE, height])];
    let health = entity(
        "item_health",
        &[("origin", &format!("200 80 {height}"))],
        Vec::new(),
    );
    Room {
        world: ledge,
        entities: vec![health],
        ..Room::default()
    }
    .checked(moves)
    .unwrap()
}

// No spec statement: L-77, an item on a ledge higher than a player jumps is never reached, and
// one on a ledge as high as the jump is
#[test]
fn an_item_above_the_jump_is_never_reached() {
    let high = ledge(f64::from(JUMP) + 1.0, Moves::Player);
    assert_eq!(high.unreached.len(), 1, "{:?}", high.unreached);
    assert_eq!(high.unreached[0].kind, Sought::Item);
    assert_eq!(high.unreached[0].classname, "item_health");
    assert_eq!(ledge(f64::from(JUMP), Moves::Player).unreached, Vec::new());
    // The generator's moves take no jump: a ledge above a step is out of reach.
    assert_eq!(ledge(f64::from(JUMP), Moves::Generator).unreached.len(), 1);
}

/// The room with a pit `depth` deep beside the way to the exit, which it does not cross.
fn pit(depth: f64, moves: Moves) -> Playability<usize> {
    let bottom = -HIGH;
    let floor = vec![
        cuboid([-LONG, -WIDE, bottom], [LONG, WIDE, -depth]),
        cuboid([-LONG, -WIDE, -depth], [-64.0, WIDE, 0.0]),
        cuboid([64.0, -WIDE, -depth], [LONG, WIDE, 0.0]),
        cuboid([-64.0, -32.0, -depth], [64.0, WIDE, 0.0]),
    ];
    Room {
        floor,
        ..Room::default()
    }
    .checked(moves)
    .unwrap()
}

// No spec statement: L-77, a pit deeper than a player jumps, with no way out, is a trap whose
// point is inside it; the same pit a unit shallower than the jump is not
#[test]
fn a_pit_deeper_than_the_jump_is_a_trap() {
    let deep = pit(f64::from(JUMP) + 1.0, Moves::Player);
    assert_eq!(deep.exit, ExitReach::Reached);
    assert_eq!(deep.traps.len(), 1, "{:?}", deep.traps);
    let [x, y, z] = deep.traps[0].at;
    assert!(
        (-64.0..=64.0).contains(&x) && (-WIDE as f32..=-32.0).contains(&y),
        "{x} {y}"
    );
    assert!(z < 0.0, "the point is in the pit: {z}");
    let shallow = pit(f64::from(JUMP) - 1.0, Moves::Player);
    assert_eq!(shallow.traps, Vec::new());
    // A step is all the generator's moves climb.
    assert_eq!(pit(f64::from(JUMP) - 1.0, Moves::Generator).traps.len(), 1);
}

// No spec statement: L-77, PRIN-8: an entity of a class the check does not know, a mod's or
// id's func_train, is listed as not judged rather than passed or failed
#[test]
fn a_class_the_check_does_not_know_is_not_judged() {
    let train = entity(
        "func_train",
        &[],
        vec![cuboid([-200.0, -32.0, 0.0], [-136.0, 32.0, 8.0])],
    );
    let modded = entity("func_mover", &[("origin", "0 0 64")], Vec::new());
    let found = Room {
        entities: vec![train, modded],
        ..Room::default()
    }
    .checked(Moves::Player)
    .unwrap();
    let names: Vec<(usize, &str)> = found
        .not_judged
        .iter()
        .map(|n| (n.entity, n.classname.as_str()))
        .collect();
    assert_eq!(names, vec![(3, "func_train"), (4, "func_mover")]);
}

// No spec statement: L-77, a map with no exit has no trap, and says so
#[test]
fn a_map_with_no_exit_has_no_trap() {
    let mut map = MapFile::parse(&Room::default().map()).unwrap();
    map.entities
        .retain(|e| e.classname() != Some(b"trigger_changelevel".as_slice()));
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&map.to_bytes(), None)
        .unwrap();
    let bsp = Bsp::read(&compiled.bsp, BspLoader::default()).unwrap();
    let found = Ground::new(&bsp, Moves::Player, Skill::Normal)
        .unwrap()
        .playability(lump)
        .unwrap();
    assert_eq!(found.exit, ExitReach::NoExit);
    assert_eq!(found.traps, Vec::new());
}

// No spec statement: L-77, a map with no player start cannot be checked
#[test]
fn a_map_with_no_start_cannot_be_checked() {
    let mut map = MapFile::parse(&Room::default().map()).unwrap();
    map.entities
        .retain(|e| e.classname() != Some(b"info_player_start".as_slice()));
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&map.to_bytes(), None)
        .unwrap();
    let bsp = Bsp::read(&compiled.bsp, BspLoader::default()).unwrap();
    let ground = Ground::new(&bsp, Moves::Player, Skill::Normal).unwrap();
    assert_eq!(ground.playability(lump), Err(PlayError::NoStart));
}

/// The room cut in two by a wall across it at x 0 to 16, with no way through.
fn split() -> Vec<Brush> {
    vec![cuboid([0.0, -WIDE, 0.0], [16.0, WIDE, HIGH])]
}

/// The split room with a teleporter named `tp` across its west half, to the east half, and,
/// when `fired`, a `trigger_once` east of it that fires it.
fn teleported(fired: bool) -> ExitReach {
    let mut entities = vec![
        entity(
            "trigger_teleport",
            &[("target", "dest"), ("targetname", "tp")],
            vec![cuboid([-300.0, -WIDE, 0.0], [-280.0, WIDE, 128.0])],
        ),
        entity(
            "info_teleport_destination",
            &[("targetname", "dest"), ("origin", "300 0 0")],
            Vec::new(),
        ),
    ];
    if fired {
        entities.push(entity(
            "trigger_once",
            &[("target", "tp")],
            vec![cuboid([-200.0, -WIDE, 0.0], [-180.0, WIDE, 64.0])],
        ));
    }
    Room {
        world: split(),
        entities,
        ..Room::default()
    }
    .checked(Moves::Player)
    .unwrap()
    .exit
}

// No spec statement: L-77, a teleporter with a targetname sends no one until it is fired
// (triggers.qc, teleport_touch: "not fired yet")
#[test]
fn a_named_teleporter_waits_to_be_fired() {
    assert_eq!(teleported(false), ExitReach::NotReached);
    assert_eq!(teleported(true), ExitReach::Reached);
}

/// The room with a secret near its ceiling, out of a player's reach, shot when it has `health`.
fn high_secret(pairs: &[(&str, &str)]) -> Playability<usize> {
    let secret = entity(
        "trigger_secret",
        pairs,
        vec![cuboid([-64.0, -16.0, 200.0], [-32.0, 16.0, 232.0])],
    );
    Room {
        entities: vec![secret],
        ..Room::default()
    }
    .checked(Moves::Player)
    .unwrap()
}

// No spec statement: L-77, a secret with health is found by shooting it, from where a player sees
// it (triggers.qc, trigger_multiple: a trigger with health is killed, not touched)
#[test]
fn a_shot_secret_is_found_from_where_it_is_seen() {
    assert_eq!(high_secret(&[("health", "1")]).unreached, Vec::new());
    assert_eq!(high_secret(&[]).unreached.len(), 1);
}

// No spec statement: L-77, a health or ammo box reaches 32 from its origin along x and y, and a
// weapon's is centred on it (items.qc, setsize); the player's box touches either grown by 15
// across (SV_LinkEdict, FL_ITEM)
#[test]
fn an_item_is_touched_where_its_box_is() {
    let room = Room {
        entities: vec![
            entity("item_health", &[("origin", "0 0 0")], Vec::new()),
            entity("weapon_nailgun", &[("origin", "0 0 0")], Vec::new()),
        ],
        ..Room::default()
    };
    let compiled = qs_map_bsp::Compiler::default()
        .compile(&room.map(), None)
        .unwrap();
    let bsp = Bsp::read(&compiled.bsp, BspLoader::default()).unwrap();
    let ground = Ground::new(&bsp, Moves::Player, Skill::Normal).unwrap();
    let touched = |wanted: usize, x: f32| {
        let at = ground.wanted[wanted].at;
        ground.reaches([x, 0.0, 24.0], at)
    };
    // The health's box is 0 to 32; grown, 15 more; and the player's half and a unit.
    assert!(touched(0, 32.0 + 15.0 + 17.0));
    assert!(!touched(0, 32.0 + 15.0 + 17.0 + FINE));
    assert!(touched(0, -15.0 - 17.0));
    assert!(!touched(0, -15.0 - 17.0 - FINE));
    // The nailgun's is -16 to 16.
    assert!(touched(1, 16.0 + 15.0 + 17.0));
    assert!(!touched(1, 16.0 + 15.0 + 17.0 + FINE));
}

/// The room with a pit 200 deep across it from x -64 to 64, bridged by a plat named `lift` at
/// its top, which waits there until fired.
fn bridged() -> Playability<usize> {
    let bottom = -HIGH;
    let floor = vec![
        cuboid([-LONG, -WIDE, bottom], [-64.0, WIDE, 0.0]),
        cuboid([64.0, -WIDE, bottom], [LONG, WIDE, 0.0]),
        cuboid([-64.0, -WIDE, bottom], [64.0, WIDE, -200.0]),
    ];
    let plat = entity(
        "func_plat",
        &[("targetname", "lift"), ("height", "190")],
        vec![cuboid([-64.0, -WIDE, -8.0], [64.0, WIDE, 0.0])],
    );
    Room {
        floor,
        entities: vec![plat],
        ..Room::default()
    }
    .checked(Moves::Player)
    .unwrap()
}

// No spec statement: L-77, a plat with a targetname is solid at its top until fired (plats.qc:
// state STATE_UP), so a player walks across it and does not fall into its shaft
#[test]
fn a_plat_that_waits_is_a_floor() {
    let found = bridged();
    assert_eq!(found.exit, ExitReach::Reached);
    assert_eq!(found.traps, Vec::new());
}

/// The room with stairs of ten steps of 8 from x -320, each 32 deep, under a ceiling 80 above
/// each step: room for a step of 8 at a step's edge, under the one before, and not for a whole
/// climb of 18; and the floor past them at the stairs' top.
fn low_stairs() -> ExitReach {
    let bottom = -HIGH;
    let mut floor = vec![cuboid([-LONG, -WIDE, bottom], [-320.0, WIDE, 0.0])];
    let mut world = Vec::new();
    for step in 0..10_u8 {
        let x0 = -320.0 + f64::from(step) * 32.0;
        let top = (f64::from(step) + 1.0) * 8.0;
        floor.push(cuboid([x0, -WIDE, bottom], [x0 + 32.0, WIDE, top]));
        world.push(cuboid([x0, -WIDE, top + 80.0], [x0 + 32.0, WIDE, HIGH]));
    }
    floor.push(cuboid([0.0, -WIDE, bottom], [LONG, WIDE, 80.0]));
    Room {
        floor,
        world,
        ..Room::default()
    }
    .checked(Moves::Generator)
    .unwrap()
    .exit
}

// No spec statement: L-77, PHYS-8.3: a step is climbed under a ceiling too low for a whole
// step's climb, as the engine's step up stops where it meets the ceiling (SV_WalkMove)
#[test]
fn stairs_under_a_low_ceiling_are_climbed() {
    assert_eq!(low_stairs(), ExitReach::Reached);
}

// No spec statement: L-77, a monster whose targetname something fires waits in a closet until
// fired, and is not taken to die, firing its target, because a player sees it: here the relay
// that would release it is never fired
#[test]
fn a_monster_that_waits_is_not_killed_on_sight() {
    let (wall, door) = walled(&[("targetname", "gate")]);
    let monster = entity(
        "monster_army",
        &[
            ("origin", "-200 0 24"),
            ("targetname", "m"),
            ("target", "gate"),
        ],
        Vec::new(),
    );
    let release = entity(
        "trigger_relay",
        &[
            ("origin", "-300 0 300"),
            ("targetname", "never"),
            ("target", "m"),
        ],
        Vec::new(),
    );
    let found = Room {
        world: wall,
        entities: vec![door, monster, release],
        ..Room::default()
    }
    .checked(Moves::Player)
    .unwrap();
    assert_eq!(found.exit, ExitReach::NotReached);
}

// No spec statement: L-77, a monster whose targetname only a killtarget uses is no closet monster:
// it is taken to die once a player sees it, firing its target
#[test]
fn a_monster_named_only_for_removal_dies_on_sight() {
    let (wall, door) = walled(&[("targetname", "gate")]);
    let monster = entity(
        "monster_army",
        &[
            ("origin", "-200 0 24"),
            ("targetname", "boss"),
            ("target", "gate"),
        ],
        Vec::new(),
    );
    let remover = entity(
        "trigger_relay",
        &[("origin", "-300 0 300"), ("killtarget", "boss")],
        Vec::new(),
    );
    let found = Room {
        world: wall,
        entities: vec![door, monster, remover],
        ..Room::default()
    }
    .checked(Moves::Player)
    .unwrap();
    assert_eq!(found.exit, ExitReach::Reached);
}

// spec BOT-2.1: a spawn a search already reached adds nothing, and one past a wall adds the
// places beyond it; the spawns are each start of the three kinds
#[test]
fn a_spawn_already_reached_adds_nothing_to_the_graph() {
    let beyond = entity(
        "info_player_deathmatch",
        &[("origin", "200 0 24")],
        Vec::new(),
    );
    let near = entity("info_player_coop", &[("origin", "-392 8 24")], Vec::new());
    let map = Room {
        world: split(),
        entities: vec![beyond, near],
        ..Room::default()
    }
    .map();
    let compiled = qs_map_bsp::Compiler::default().compile(&map, None).unwrap();
    let bsp = Bsp::read(&compiled.bsp, BspLoader::default()).unwrap();
    let ground = Ground::new(&bsp, Moves::Player, Skill::Normal).unwrap();
    assert_eq!(
        ground.spawns(),
        [[-400.0, 0.0, 24.0], [200.0, 0.0, 24.0], [-392.0, 8.0, 24.0]]
    );
    let [start, far, close] = [ground.spawns()[0], ground.spawns()[1], ground.spawns()[2]];
    let alone = ground.graph(&[start]).unwrap().len();
    assert_eq!(
        ground.graph(&[start, close]).unwrap().len(),
        alone,
        "8 from the start"
    );
    let both = ground.graph(&[start, far]).unwrap();
    assert!(both.len() > alone, "past the wall");
    let (a, b) = (both.nearest(start).unwrap(), both.nearest(far).unwrap());
    assert!(both.route(a, b).is_none(), "no way through the wall");
    let short = ground.path(start, far).unwrap();
    let end = short.points().last().unwrap();
    assert!(end[0] < 0.0, "as near as the start's side reaches: {end:?}");
}
