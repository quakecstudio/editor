// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::fmt::Write as _;

use qs_commands::Session;
use qs_map::{Brush, Entity, Face, Item, MapFile, Surface, TexDef};
use qs_map_edit::{EntityIndex, EntityProblem, MAP_FIX, MAP_ISSUES, MapIssue, MapIssues, QuickFix};
use serde_json::json;

use super::DEATHMATCH_PLAYERS;
use crate::map_compile::tests::{Context, open};
use crate::preview::engine::BotSkillParam;
use crate::{GameMode, GameOptions, SET_GAME};

/// A room 512 units square: a floor whose top is at z = 0, a pool of water in one corner and a
/// pillar 216 units tall in another.
fn room(starts: &[(&str, [f64; 3])]) -> MapFile {
    let mut text = String::from("{\n\"classname\" \"worldspawn\"\n}\n");
    for (classname, [x, y, z]) in starts {
        writeln!(
            text,
            "{{\n\"classname\" \"{classname}\"\n\"origin\" \"{x} {y} {z}\"\n\"angle\" \"90\"\n}}"
        )
        .unwrap();
    }
    let mut file = MapFile::parse(text.as_bytes()).unwrap();
    let surface = |texture: &[u8]| Surface {
        texture: texture.to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    };
    let floor = Brush::cuboid(
        [-256.0, -256.0, -16.0],
        [256.0, 256.0, 0.0],
        &surface(b"floor"),
    );
    let water = Brush::cuboid(
        [128.0, 128.0, 0.0],
        [256.0, 256.0, 64.0],
        &surface(b"*water"),
    );
    let pillar = Brush::cuboid(
        [-256.0, 192.0, 0.0],
        [-192.0, 256.0, 216.0],
        &surface(b"rock"),
    );
    file.entities[0]
        .items
        .extend([floor.unwrap(), water.unwrap(), pillar.unwrap()].map(Item::Brush));
    file
}

fn game(mode: GameMode, players: u8, bots: u8) -> GameOptions {
    GameOptions {
        mode,
        players,
        bots,
        bot_skill: None,
    }
}

/// The problems whose message holds `text`.
fn found<'a>(problems: &'a [EntityProblem], text: &str) -> Vec<&'a EntityProblem> {
    problems
        .iter()
        .filter(|problem| problem.message.contains(text))
        .collect()
}

/// Deathmatch starts 128 units apart along x, on the floor.
fn deathmatch_starts(count: usize) -> Vec<(&'static str, [f64; 3])> {
    (0..count)
        .map(|n| {
            let x = -192.0 + 128.0 * f64::from(u8::try_from(n).unwrap());
            ("info_player_deathmatch", [x, -128.0, 24.0])
        })
        .collect()
}

const TELEFRAG: &str = "to telefrag:";

// spec START-3.1: a deathmatch map is checked for four starts when deathmatch is not the game
// chosen; four is enough, three is not, and a map with none is no deathmatch map
// spec START-1.2: players share a deathmatch start when there are fewer than they
#[test]
fn a_deathmatch_map_needs_four_starts_by_default() {
    let single = GameOptions::default();
    let enough = room(&deathmatch_starts(DEATHMATCH_PLAYERS));
    assert!(single.start_problems(&enough).is_empty());
    let short = room(&deathmatch_starts(3));
    let problems = single.start_problems(&short);
    let short_of = found(&problems, "3 info_player_deathmatch for 4 players and bots");
    assert_eq!(short_of.len(), 1, "{problems:?}");
    assert!(short_of[0].message.contains("add 1 more"));
    assert!(short_of[0].message.contains("SelectSpawnPoint"));
    assert_eq!(short_of[0].fix, None);
    assert_eq!(
        short_of[0].entity.map(|e| e.get()),
        Some(1),
        "the first start"
    );
    let none = room(&[("info_player_start", [0.0, 0.0, 24.0])]);
    assert!(single.start_problems(&none).is_empty(), "no deathmatch map");
}

// spec START-3.1: played as deathmatch, a map is checked for a start for each player and bot
// spec START-1.2: with no deathmatch start every player spawns at the info_player_start
#[test]
fn a_deathmatch_chosen_needs_a_start_for_each_player_and_bot() {
    let seven = game(GameMode::Deathmatch, 2, 5);
    let problems = seven.start_problems(&room(&deathmatch_starts(4)));
    assert_eq!(
        found(&problems, "4 info_player_deathmatch for 7 players").len(),
        1
    );
    assert!(
        seven
            .start_problems(&room(&deathmatch_starts(7)))
            .is_empty()
    );
    let none = room(&[("info_player_start", [0.0, 0.0, 24.0])]);
    let problems = game(GameMode::Deathmatch, 1, 0).start_problems(&none);
    let missing = found(&problems, "no info_player_deathmatch");
    assert_eq!(missing.len(), 1, "{problems:?}");
    assert!(missing[0].message.contains("\"info_player_start\""));
    assert_eq!(missing[0].line, 0, "about the map, not an entity");
}

// spec START-3.2: coop chosen for two or more, a map with no info_player_coop is told every
// player spawns on the info_player_start; the fix places one beside it, facing its way
// spec START-1.1: with no coop start every coop player spawns at the info_player_start
#[test]
fn coop_needs_a_coop_start() {
    let map = room(&[("info_player_start", [0.0, 0.0, 24.0])]);
    let coop = game(GameMode::Coop, 1, 1);
    let problems = coop.start_problems(&map);
    let missing = found(&problems, "no info_player_coop");
    assert_eq!(missing.len(), 1, "{problems:?}");
    assert_eq!(missing[0].entity.map(|e| e.get()), Some(1), "the start");
    assert_eq!(
        missing[0].fix,
        Some(QuickFix::AddCoopStart {
            origin: [64.0, 0.0, 24.0],
            angle: 90.0,
        })
    );
    assert!(GameOptions::default().start_problems(&map).is_empty());
    assert!(
        game(GameMode::Coop, 1, 0).start_problems(&map).is_empty(),
        "one player alone telefrags no one"
    );
    let with_one = room(&[
        ("info_player_start", [0.0, 0.0, 24.0]),
        ("info_player_coop", [96.0, 0.0, 24.0]),
    ]);
    assert!(coop.start_problems(&with_one).is_empty());
    let no_start = room(&[]);
    let problems = coop.start_problems(&no_start);
    assert_eq!(
        found(&problems, "no info_player_coop")[0].fix,
        None,
        "nowhere to put one"
    );
}

// spec START-3.5: a coop start is put where a player stands: not off the floor's edge, but in the
// next place around
#[test]
fn a_new_coop_start_stands_on_the_floor() {
    let map = room(&[("info_player_start", [-224.0, -224.0, 24.0])]);
    let problems = game(GameMode::Coop, 2, 0).start_problems(&map);
    let fix = found(&problems, "no info_player_coop")[0].fix.clone();
    assert_eq!(
        fix,
        Some(QuickFix::AddCoopStart {
            origin: [-160.0, -224.0, 24.0],
            angle: 90.0,
        }),
        "west of it is off the floor"
    );
}

// spec START-3.3: a start whose player box is in a solid brush is found, with a fix that moves it
// up to the nearest height with room; one standing on the floor, or in water, is not
// spec START-1.3: the player's box is '-16 -16 -24' to '16 16 32', 1 unit above the start
#[test]
fn a_start_in_solid_is_moved_out() {
    let sunk = room(&[("info_player_start", [0.0, 0.0, 10.0])]);
    let problems = GameOptions::default().start_problems(&sunk);
    let stuck = found(&problems, "this info_player_start is in solid");
    assert_eq!(stuck.len(), 1, "{problems:?}");
    assert!(stuck[0].message.contains("spot.origin + '0 0 1'"));
    assert_eq!(
        stuck[0].fix,
        Some(QuickFix::MoveTo {
            entity: EntityIndex::new(1),
            origin: [0.0, 0.0, 23.0],
        }),
        "the box's bottom, 24 below and 1 up, rests on the floor"
    );
    for z in [23.0, 24.0, 200.0] {
        let standing = room(&[("info_player_start", [0.0, 0.0, z])]);
        assert!(
            GameOptions::default().start_problems(&standing).is_empty(),
            "{z}"
        );
    }
    let swimming = room(&[("info_player_deathmatch", [192.0, 192.0, 24.0])]);
    assert!(
        found(
            &GameOptions::default().start_problems(&swimming),
            "in solid"
        )
        .is_empty(),
        "water is not solid"
    );
    let buried = room(&[("info_player_start", [0.0, 0.0, -200.0])]);
    let problems = GameOptions::default().start_problems(&buried);
    assert!(
        found(&problems, "in solid").is_empty(),
        "below the floor is not in it"
    );
    let deep = room(&[("info_player_coop", [-224.0, 224.0, 100.0])]);
    let problems = GameOptions::default().start_problems(&deep);
    assert_eq!(
        found(&problems, "in solid")[0].fix,
        None,
        "no room within 64 units"
    );
}

// spec START-3.3: the move out of solid goes to a height where a player stands and telefrags no
// start of its group, not merely the first out of solid
#[test]
fn a_start_moved_out_of_solid_telefrags_no_one() {
    let stacked = room(&[
        ("info_player_deathmatch", [0.0, 0.0, 70.0]),
        ("info_player_deathmatch", [0.0, 0.0, 10.0]),
    ]);
    let problems = GameOptions::default().start_problems(&stacked);
    let stuck = found(&problems, "in solid");
    assert_eq!(stuck.len(), 1, "{problems:?}");
    // Up from 10 every height telefrags the start at 70 until it floats; down is solid floor.
    assert_eq!(stuck[0].fix, None, "{:?}", stuck[0].fix);
    let beside = room(&[
        ("info_player_deathmatch", [128.0, 0.0, 24.0]),
        ("info_player_deathmatch", [0.0, 0.0, 10.0]),
    ]);
    let problems = GameOptions::default().start_problems(&beside);
    assert_eq!(
        found(&problems, "in solid")[0].fix,
        Some(QuickFix::MoveTo {
            entity: EntityIndex::new(2),
            origin: [0.0, 0.0, 23.0],
        })
    );
}

/// A brush entity of `classname` holding a box from `min` to `max`.
fn brush_entity(classname: &str, min: [f64; 3], max: [f64; 3]) -> Entity {
    let mut entity = Entity::new();
    entity.set(b"classname", classname.as_bytes()).unwrap();
    let surface = Surface {
        texture: b"rock".to_vec(),
        texdef: TexDef::Standard {
            shift: [0.0; 2],
            rotate: 0.0,
            scale: [1.0; 2],
        },
    };
    entity
        .items
        .push(Item::Brush(Brush::cuboid(min, max, &surface).unwrap()));
    entity
}

// spec START-3.3: the brushes of func_detail, func_group and func_wall are solid, and those of
// func_illusionary and func_detail_illusionary, and of a trigger, are not
#[test]
fn brush_entities_are_solid_as_the_game_makes_them() {
    for (classname, solid) in [
        ("func_detail", true),
        ("func_group", true),
        ("func_wall", true),
        ("func_illusionary", false),
        ("func_detail_illusionary", false),
        ("trigger_multiple", false),
    ] {
        let mut map = room(&[("info_player_start", [0.0, 0.0, 24.0])]);
        map.push_entity(brush_entity(classname, [-8.0, -8.0, 0.0], [8.0, 8.0, 32.0]));
        let problems = GameOptions::default().start_problems(&map);
        assert_eq!(
            !found(&problems, "in solid").is_empty(),
            solid,
            "{classname}"
        );
    }
}

/// A face of the plane whose outward normal is `normal` through `point`, its points ordered so
/// its plane faces out.
fn face(normal: [f64; 3], point: [f64; 3]) -> Face {
    let [nx, ny, nz] = normal;
    let cross =
        |[a, b, c]: [f64; 3], [x, y, z]: [f64; 3]| [b * z - c * y, c * x - a * z, a * y - b * x];
    // Two directions along the plane: across a helper axis, then across that.
    let helper = if nz.abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let first = cross(normal, helper);
    let along = [first, cross(normal, first)];
    let [px, py, pz] = point;
    let step = |[x, y, z]: [f64; 3]| [px + x * 64.0, py + y * 64.0, pz + z * 64.0];
    let texdef = TexDef::Standard {
        shift: [0.0; 2],
        rotate: 0.0,
        scale: [1.0; 2],
    };
    let made = Face::new([point, step(along[0]), step(along[1])], b"rock", texdef).unwrap();
    let out = made.plane().unwrap().normal;
    if out[0] * nx + out[1] * ny + out[2] * nz > 0.0 {
        made
    } else {
        Face::new([point, step(along[1]), step(along[0])], b"rock", texdef).unwrap()
    }
}

// spec START-3.3: a box beside a sloped face of a brush, apart from it, is out of solid, and one
// across it is in: the solid is the brush's own shape, not its bounds
#[test]
fn a_start_beside_a_slope_is_out_of_solid() {
    // A ramp rising to the east: solid below the plane z = x, from x = 0 to 128.
    let mut ramp = Brush::new();
    for (normal, point) in [
        ([-1.0, 1.0, 0.0], [0.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [0.0, -64.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 64.0, 0.0]),
        ([0.0, 0.0, -1.0], [0.0, 0.0, 0.0]),
        ([1.0, 0.0, 0.0], [128.0, 0.0, 0.0]),
    ] {
        ramp.faces.push(face(normal, point));
    }
    ramp.faces[0] = face([-1.0, 0.0, 1.0], [0.0, 0.0, 0.0]);
    let mut map = room(&[("info_player_start", [16.0, 0.0, 90.0])]);
    map.entities[0].items.push(Item::Brush(ramp.clone()));
    let problems = GameOptions::default().start_problems(&map);
    assert!(
        found(&problems, "in solid").is_empty(),
        "above the slope, inside its bounds: {problems:?}"
    );
    let mut map = room(&[("info_player_start", [96.0, 0.0, 60.0])]);
    map.entities[0].items.push(Item::Brush(ramp));
    let problems = GameOptions::default().start_problems(&map);
    assert_eq!(found(&problems, "in solid").len(), 1, "under the slope");
}

// spec START-2.2: two deathmatch starts telefrag up to 35 units apart across and 59 up and down,
// as SV_LinkEdict grows both boxes by 1 and SV_TouchLinks counts touching; 36 and 60 are clear
// spec START-2.1: spawn_tdeath's trigger is the player's box grown by 1
// spec START-3.4: the later one is found, with a fix that moves it aside where it telefrags none
#[test]
fn starts_close_enough_to_telefrag_are_moved_apart() {
    let close = room(&[
        ("info_player_deathmatch", [0.0, 0.0, 24.0]),
        ("info_player_deathmatch", [35.0, 0.0, 24.0]),
    ]);
    let problems = GameOptions::default().start_problems(&close);
    let telefrag = found(&problems, TELEFRAG);
    assert_eq!(telefrag.len(), 1, "{problems:?}");
    assert_eq!(
        telefrag[0].entity.map(|e| e.get()),
        Some(2),
        "the later one"
    );
    assert!(
        telefrag[0]
            .message
            .contains("more than 35 units apart across")
    );
    assert!(telefrag[0].message.contains("more than 59 up and down"));
    assert_eq!(
        telefrag[0].fix,
        Some(QuickFix::MoveTo {
            entity: EntityIndex::new(2),
            origin: [99.0, 0.0, 24.0],
        })
    );
    for (at, telefrags) in [
        ([0.0, 35.0, 24.0], true),
        ([0.0, 0.0, 83.0], true),
        ([36.0, 0.0, 24.0], false),
        ([0.0, 36.0, 24.0], false),
        ([0.0, 0.0, 84.0], false),
    ] {
        let pair = room(&[
            ("info_player_deathmatch", [0.0, 0.0, 24.0]),
            ("info_player_deathmatch", at),
        ]);
        let problems = GameOptions::default().start_problems(&pair);
        assert_eq!(!found(&problems, TELEFRAG).is_empty(), telefrags, "{at:?}");
    }
    let apart = room(&[
        ("info_player_deathmatch", [0.0, 0.0, 24.0]),
        ("info_player_coop", [0.0, 0.0, 24.0]),
    ]);
    assert!(
        found(&GameOptions::default().start_problems(&apart), TELEFRAG).is_empty(),
        "a coop start and a deathmatch start are of no group together"
    );
}

// spec START-3.4: the coop starts and the first info_player_start are one group, as coop players
// take them in turn
// spec START-1.1: coop falls back to the first info_player_start after the coop starts
#[test]
fn coop_starts_telefrag_the_info_player_start() {
    let coop = room(&[
        ("info_player_start", [0.0, 0.0, 24.0]),
        ("info_player_coop", [20.0, 0.0, 24.0]),
    ]);
    let problems = GameOptions::default().start_problems(&coop);
    let telefrag = found(&problems, TELEFRAG);
    assert_eq!(telefrag.len(), 1, "{problems:?}");
    assert_eq!(
        telefrag[0].entity.map(|e| e.get()),
        Some(1),
        "the start comes after the coop starts"
    );
    assert!(telefrag[0].message.contains("entity 2's info_player_coop"));
    let second = room(&[
        ("info_player_start", [500.0, 0.0, 24.0]),
        ("info_player_start", [0.0, 0.0, 24.0]),
        ("info_player_coop", [20.0, 0.0, 24.0]),
    ]);
    assert!(
        found(&GameOptions::default().start_problems(&second), TELEFRAG).is_empty(),
        "only the first info_player_start is coop's"
    );
}

/// A session with a map of `starts` open, the game `game` chosen; its issues.
fn session_with(
    name: &str,
    starts: &[(&str, [f64; 3])],
    game: GameOptions,
) -> (Session<Context>, Vec<MapIssue>) {
    let dir = std::env::temp_dir().join(format!("qs-wb-starts-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("starts.map");
    std::fs::write(&path, room(starts).to_bytes()).unwrap();
    let mut session = open(&path);
    session.execute(SET_GAME, json!({ "game": game })).unwrap();
    let issues = issues(&mut session);
    (session, issues)
}

fn issues(session: &mut Session<Context>) -> Vec<MapIssue> {
    let found: MapIssues =
        serde_json::from_value(session.execute(MAP_ISSUES, json!({})).unwrap()).unwrap();
    found.issues
}

// spec START-3.1, START-3.2, START-3.3, START-3.4: through the workbench, map.issues checks the
// starts for the game preview.set_game chose, and each fix map.issues gives puts its issue right
#[test]
fn map_issues_checks_the_starts_for_the_game_chosen_and_fixes_them() {
    let (mut session, found) = session_with(
        "coop",
        &[
            ("info_player_start", [0.0, 0.0, 10.0]),
            ("info_player_deathmatch", [-128.0, -128.0, 24.0]),
            ("info_player_deathmatch", [-100.0, -128.0, 24.0]),
            ("info_player_deathmatch", [128.0, -128.0, 24.0]),
            ("info_player_deathmatch", [-128.0, 0.0, 24.0]),
        ],
        game(GameMode::Coop, 2, 1),
    );
    // The start comes out of the floor first, which leaves room beside it for a coop start.
    for name in ["in solid", "no info_player_coop", TELEFRAG] {
        let fix = issues(&mut session)
            .iter()
            .find(|issue| issue.message.contains(name))
            .and_then(|issue| issue.fix.clone())
            .unwrap_or_else(|| panic!("{name}: {found:?}"));
        session.execute(MAP_FIX, json!({ "fix": fix })).unwrap();
        assert!(
            !issues(&mut session)
                .iter()
                .any(|issue| issue.message.contains(name)),
            "{name} fixed"
        );
    }
    let left = issues(&mut session);
    assert!(
        !left
            .iter()
            .any(|issue| issue.message.contains("info_player")),
        "every start put right: {left:?}"
    );
    let refused = session.execute(
        SET_GAME,
        json!({ "game": { "mode": "single", "players": 2 } }),
    );
    assert!(refused.is_err(), "single player alone");
    let (_, found) = session_with(
        "deathmatch",
        &deathmatch_starts(2),
        GameOptions {
            bot_skill: Some(BotSkillParam::Hard),
            ..game(GameMode::Deathmatch, 1, 3)
        },
    );
    assert!(
        found
            .iter()
            .any(|issue| issue.message.contains("2 info_player_deathmatch for 4")),
        "{found:?}"
    );
}
