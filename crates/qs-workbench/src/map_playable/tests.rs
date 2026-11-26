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

use qs_map_edit::EntityIndex;

use crate::map_filter::Skill;

use super::{ExitReach, Ground, JUMP, Moves, PlayError, Playability, Sought};

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
fn lump(place: usize) -> Option<EntityIndex> {
    Some(EntityIndex::new(place))
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
    fn checked(self, moves: Moves) -> Result<Playability, PlayError> {
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
    assert_eq!(found.exits, vec![EntityIndex::new(2)]);
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
    assert_eq!(found.unreached[0].entity, EntityIndex::new(3));
}

/// The room with a ledge `height` high along its north side and a health box on it.
fn ledge(height: f64, moves: Moves) -> Playability {
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
fn pit(depth: f64, moves: Moves) -> Playability {
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
        .map(|n| (n.entity.get(), n.classname.as_str()))
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
