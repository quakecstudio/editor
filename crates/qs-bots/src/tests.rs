// SPDX-License-Identifier: GPL-3.0-or-later

//! The bots' graph through a compiled room, and their minds on edicts made for each rule.

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp_const,
    reason = "test code: a failed unwrap or index is a failed test, and the values are exact"
)]

use qs_bsp::{Bsp, BspLoader};
use qs_map::{Brush, Entity, Item, MapFile, Surface, TexDef};
use qs_map_gen::playable::{Ground, Moves, Skill};
use qs_sim::spawn::Deathmatch;
use qs_sim::{Body, BotSkill, ClientId, UserCmd};

use crate::aim::{FIRE_CONE, angle_between};
use crate::{Aim, Ammo, BotRng, Bots, Mind, Mode, View, Weapons, choose, follow_need, item_need};

/// A frame of 1/72 of a second.
const FRAME: f64 = 1.0 / 72.0;
/// The bot's slot in these tests, edict 2.
const SLOT: usize = 1;
/// `FL_CLIENT` and `FL_MONSTER`.
const CLIENT: f32 = 8.0;
const MONSTER: f32 = 32.0;
/// `IT_SHOTGUN`, `IT_SUPER_SHOTGUN`, `IT_ROCKET_LAUNCHER` and `IT_AXE`.
const SHOTGUN: f32 = 1.0;
const SUPER_SHOTGUN: f32 = 2.0;
const ROCKETS: f32 = 32.0;
const AXE: f32 = 4096.0;

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

fn entity(classname: &str, pairs: &[(&str, &str)], brushes: Vec<Brush>) -> Entity {
    let mut entity = Entity::new();
    entity.set(b"classname", classname.as_bytes()).unwrap();
    for (key, value) in pairs {
        entity.set(key.as_bytes(), value.as_bytes()).unwrap();
    }
    entity.items.extend(brushes.into_iter().map(Item::Brush));
    entity
}

/// A sealed room from x -512 to 512: the start at its west end, a wall at x 0 to 16 with a door
/// in it that opens upward when touched, then a lift (`func_plat`) from x 192 to 320 that
/// rises 128 to a floor at that height across the east end.
fn lift_and_door() -> Vec<u8> {
    let (long, wide, high, wall) = (512.0, 128.0, 384.0, 16.0);
    let brushes = vec![
        cuboid(
            [-long - wall, -wide - wall, -64.0],
            [long + wall, wide + wall, -48.0],
        ),
        cuboid(
            [-long - wall, -wide - wall, high],
            [long + wall, wide + wall, high + wall],
        ),
        cuboid(
            [-long - wall, -wide - wall, -48.0],
            [-long, wide + wall, high],
        ),
        cuboid(
            [long, -wide - wall, -48.0],
            [long + wall, wide + wall, high],
        ),
        cuboid([-long, -wide - wall, -48.0], [long, -wide, high]),
        cuboid([-long, wide, -48.0], [long, wide + wall, high]),
        // The floor west of the lift, the lift's shaft and the high floor east of it.
        cuboid([-long, -wide, -48.0], [192.0, wide, 0.0]),
        cuboid([192.0, -wide, -48.0], [320.0, wide, -16.0]),
        cuboid([320.0, -wide, -48.0], [long, wide, 128.0]),
        // The wall with the doorway.
        cuboid([0.0, -wide, 0.0], [16.0, -32.0, high]),
        cuboid([0.0, 32.0, 0.0], [16.0, wide, high]),
        cuboid([0.0, -32.0, 128.0], [16.0, 32.0, high]),
    ];
    let mut file = MapFile::new();
    file.push_entity(entity("worldspawn", &[], brushes));
    file.push_entity(entity(
        "info_player_start",
        &[("origin", "-400 0 24")],
        Vec::new(),
    ));
    file.push_entity(entity(
        "info_player_deathmatch",
        &[("origin", "448 0 152")],
        Vec::new(),
    ));
    file.push_entity(entity(
        "func_door",
        &[("angle", "-1")],
        vec![cuboid([0.0, -32.0, 0.0], [16.0, 32.0, 128.0])],
    ));
    file.push_entity(entity(
        "func_plat",
        &[("height", "128")],
        vec![cuboid([192.0, -wide, 120.0], [320.0, wide, 128.0])],
    ));
    file.to_bytes()
}

fn compiled(map: &[u8]) -> Vec<u8> {
    qs_map_bsp::Compiler::default()
        .compile(map, None)
        .unwrap()
        .bsp
}

// spec BOT-2.1, BOT-2.3: the route from the start passes the door, rides the lift and ends on
// the high floor; no route leads back down the lift's shaft to the start from the high floor
// other than the fall, and a route to the start itself is the start alone
#[test]
fn a_route_passes_a_door_and_rides_a_lift() {
    let bsp_bytes = compiled(&lift_and_door());
    let bsp = Bsp::read(&bsp_bytes, BspLoader::default()).unwrap();
    let ground = Ground::new(&bsp, Moves::Player, Skill::Normal).unwrap();
    assert_eq!(ground.spawns(), [[-400.0, 0.0, 24.0], [448.0, 0.0, 152.0]]);
    let route = ground
        .path([-400.0, 0.0, 24.0], [448.0, 0.0, 152.0])
        .unwrap();
    let points = route.points();
    let through_door = points
        .iter()
        .any(|p| (0.0..=16.0).contains(&p[0]) && p[1].abs() <= 32.0 && p[2] < 64.0);
    assert!(through_door, "{points:?}");
    let rides = points
        .windows(2)
        .any(|w| (192.0..=320.0).contains(&w[0][0]) && w[1][2] - w[0][2] > 64.0);
    assert!(rides, "a move up the lift: {points:?}");
    let end = points.last().unwrap();
    assert!(end[0] > 320.0 && end[2] > 128.0, "{end:?}");
    assert!(route.length() > 800.0);
    let graph = Bots::graph_of(&bsp_bytes, Deathmatch::Off).unwrap();
    let start = graph.nearest([-400.0, 0.0, 24.0]).unwrap();
    let alone = graph.route(start, start).unwrap();
    assert_eq!(alone.points().len(), 1);
    assert_eq!(alone.length(), 0.0);
}

// spec BOT-2.2: the place of a point is the nearest, a floor above or below counting twice
// its height, and a point off the graph still finds the nearest of all
#[test]
fn a_points_place_is_the_nearest() {
    let graph = Bots::graph_of(&compiled(&lift_and_door()), Deathmatch::Off).unwrap();
    let low = graph.nearest([-200.0, 0.0, 24.0]).unwrap();
    let point = graph.point(low).unwrap();
    assert!(
        (point[0] + 200.0).abs() <= 16.0 && point[2] < 40.0,
        "{point:?}"
    );
    let far = graph.nearest([-200.0, 0.0, 5000.0]).unwrap();
    assert!(graph.point(far).is_some(), "off the graph, still a place");
    assert!(graph.place(graph.len()).is_none());
    assert!(!graph.is_empty());
}

// spec BOT-2.1: a spawn that is not reached adds its own places; none for a map with no
// spawns, and an unreadable map is an error
#[test]
fn the_graph_starts_from_the_spawns() {
    let bsp_bytes = compiled(&lift_and_door());
    let dm = Bots::graph_of(&bsp_bytes, Deathmatch::On).unwrap();
    let sp = Bots::graph_of(&bsp_bytes, Deathmatch::Off).unwrap();
    assert!(dm.len() >= sp.len());
    let high = dm.nearest([448.0, 0.0, 152.0]).unwrap();
    let low = dm.nearest([-400.0, 0.0, 24.0]).unwrap();
    assert!(
        dm.route(high, low).is_some(),
        "the high floor falls back down"
    );
    assert!(Bots::graph_of(b"not a bsp", Deathmatch::Off).is_err());
}

/// A living client's edict `n` at `origin`, looking east, with the shotgun and the axe.
fn body(n: usize, origin: [f32; 3]) -> Body {
    let [x, y, z] = origin;
    Body {
        number: n,
        classname: b"player".to_vec(),
        origin,
        velocity: [0.0; 3],
        absmin: [x - 17.0, y - 17.0, z - 25.0],
        absmax: [x + 17.0, y + 17.0, z + 33.0],
        view_ofs: [0.0, 0.0, 22.0],
        v_angle: [0.0; 3],
        health: 100.0,
        armorvalue: 0.0,
        items: SHOTGUN + AXE,
        weapon: SHOTGUN,
        ammo: [25.0, 0.0, 0.0, 0.0],
        deadflag: 0.0,
        flags: CLIENT,
        solid: 3.0,
        takedamage: 2.0,
        frags: 0.0,
        team: 0.0,
        modelindex: 1.0,
        waterlevel: 0.0,
    }
}

/// An item of `classname` that lies where it can be picked up, edict `n`.
fn item(n: usize, classname: &str) -> Body {
    Body {
        classname: classname.as_bytes().to_vec(),
        flags: 0.0,
        solid: 1.0,
        takedamage: 0.0,
        ..body(n, [0.0; 3])
    }
}

fn bot() -> ClientId {
    ClientId::new(SLOT).unwrap()
}

/// What `me` sees among `bodies` at `time`, of `seen`.
fn view<'a>(me: &'a Body, bodies: &'a [Body], seen: &'a [usize], time: f64) -> View<'a> {
    View {
        me,
        bodies,
        seen,
        mode: Mode::Deathmatch,
        time,
        frametime: FRAME,
    }
}

// spec BOT-4.2: a weapon not held is worth its value and one held 5; health its missing part
// below 100; each armour below its value; ammunition 20 when low for a weapon held, else 2;
// a powerup 60; an item picked up, or anything else, nothing
#[test]
fn each_kind_of_need_is_weighed() {
    let weapons = Weapons::id();
    let me = body(2, [0.0; 3]);
    let need = |classname: &str, me: &Body| item_need(me, &item(9, classname), &weapons);
    assert_eq!(need("weapon_rocketlauncher", &me), 100.0);
    assert_eq!(need("weapon_supershotgun", &me), 50.0);
    let armed = Body {
        items: SHOTGUN + SUPER_SHOTGUN,
        ..body(2, [0.0; 3])
    };
    assert_eq!(need("weapon_supershotgun", &armed), 5.0);
    assert_eq!(need("item_health", &me), 0.0, "full health");
    let hurt = Body {
        health: 40.0,
        ..body(2, [0.0; 3])
    };
    assert_eq!(need("item_health", &hurt), 60.0);
    assert_eq!(need("item_armor1", &me), 30.0);
    assert_eq!(need("item_armorInv", &me), 80.0);
    let armoured = Body {
        armorvalue: 150.0,
        ..body(2, [0.0; 3])
    };
    assert_eq!(need("item_armor2", &armoured), 0.0);
    assert_eq!(need("item_armorInv", &armoured), 80.0);
    assert_eq!(need("item_shells", &me), 20.0, "25 of 100 shells");
    assert_eq!(need("item_rockets", &me), 2.0, "no weapon uses them");
    let stocked = Body {
        ammo: [80.0, 0.0, 0.0, 0.0],
        ..body(2, [0.0; 3])
    };
    assert_eq!(need("item_shells", &stocked), 2.0);
    assert_eq!(need("item_artifact_super_damage", &me), 60.0);
    assert_eq!(need("info_notnull", &me), 0.0);
    let taken = Body {
        solid: 0.0,
        ..item(9, "weapon_rocketlauncher")
    };
    assert_eq!(item_need(&me, &taken, &weapons), 0.0, "picked up");
    assert_eq!(Ammo::of_box(b"item_spikes"), Some(Ammo::Nails));
    assert_eq!(Ammo::Nails.most(), 200.0);
}

// spec BOT-4.1: the goal of the greatest need over one plus its route's length over 256, none
// for a goal with no route or no need
#[test]
fn the_goal_weighs_need_by_distance() {
    assert_eq!(choose(&[(100.0, Some(2560.0)), (50.0, Some(0.0))]), Some(1));
    assert_eq!(choose(&[(100.0, Some(256.0)), (60.0, Some(0.0))]), Some(1));
    assert_eq!(choose(&[(100.0, Some(256.0)), (40.0, Some(0.0))]), Some(0));
    assert_eq!(choose(&[(100.0, None), (1.0, Some(9000.0))]), Some(1));
    assert_eq!(choose(&[(0.0, Some(0.0))]), None);
    assert_eq!(choose(&[]), None);
    assert_eq!(crate::weigh(30.0, 256.0), 15.0);
}

// spec BOT-4.3: a bot needs its friend once more than 256 away, 100 and 1 more each unit past
#[test]
fn a_bot_follows_a_far_friend() {
    assert_eq!(follow_need(100.0), 0.0);
    assert_eq!(follow_need(256.0), 0.0);
    assert_eq!(follow_need(300.0), 144.0);
}

// spec BOT-5.1: each skill's aim errs within its bound, the bound smaller at each skill up,
// and its errors spread across the bound
#[test]
fn the_aim_error_at_each_skill() {
    let bounds: Vec<f32> = BotSkill::ALL.iter().map(|&s| Aim::of(s).error).collect();
    assert_eq!(bounds, [12.0, 7.0, 4.0, 2.0]);
    for skill in BotSkill::ALL {
        let aim = Aim::of(skill);
        let mut rng = BotRng::new(7, bot());
        let mut widest = 0.0_f32;
        for _ in 0..2000 {
            for e in aim.draw_error(&mut rng) {
                assert!(e.abs() <= aim.error, "{skill:?}: {e}");
                widest = widest.max(e.abs());
            }
        }
        assert!(widest > aim.error * 0.95, "{skill:?}: {widest}");
    }
}

// spec BOT-5.2, BOT-5.3: the turn rate and reaction time rise and fall with skill; a bot fires
// only once it has seen its foe for its reaction time and looks within 15 degrees of its aim
#[test]
fn a_bot_fires_after_its_reaction_time_when_on_target() {
    let turns: Vec<f32> = BotSkill::ALL.iter().map(|&s| Aim::of(s).turn).collect();
    assert_eq!(turns, [180.0, 360.0, 540.0, 900.0]);
    let reactions: Vec<f32> = BotSkill::ALL.iter().map(|&s| Aim::of(s).reaction).collect();
    assert_eq!(reactions, [0.6, 0.4, 0.25, 0.15]);
    assert_eq!(FIRE_CONE, 15.0);
    let me = body(2, [0.0, 0.0, 24.0]);
    let foe = body(3, [512.0, 0.0, 24.0]);
    let bodies = [me.clone(), foe];
    let mut mind = Mind::new(bot(), BotSkill::Nightmare, 1);
    let fires = |mind: &mut Mind, time: f64| {
        mind.decide(&view(&bodies[0], &bodies, &[3], time), None, &Weapons::id())
            .buttons
            & 1
    };
    assert_eq!(fires(&mut mind, 10.0), 0, "just seen");
    assert_eq!(fires(&mut mind, 10.1), 0, "before 0.15 s");
    assert_eq!(fires(&mut mind, 10.2), 1, "seen long enough, ahead");
    let behind = Body {
        v_angle: [0.0, 180.0, 0.0],
        ..me
    };
    let bodies = [behind, bodies[1].clone()];
    let mut turning = Mind::new(bot(), BotSkill::Nightmare, 1);
    let cmd = turning.decide(&view(&bodies[0], &bodies, &[3], 20.0), None, &Weapons::id());
    let turned = angle_between(180.0, cmd.angles[1]).abs();
    assert!((turned - 900.0 * (1.0 / 72.0)).abs() < 0.01, "{turned}");
    let later = turning.decide(&view(&bodies[0], &bodies, &[3], 21.0), None, &Weapons::id());
    assert_eq!(later.buttons & 1, 0, "seen long, but facing away");
}

// spec BOT-3.3: a dead bot moves nothing and presses fire every other frame, releasing it in
// between, so the mod's PlayerDeathThink sees every button released, then one pressed
#[test]
fn a_dead_bot_presses_fire_in_turn_to_respawn() {
    let dead = Body {
        deadflag: 2.0,
        health: -20.0,
        ..body(2, [0.0; 3])
    };
    let bodies = [dead];
    let mut mind = Mind::new(bot(), BotSkill::Normal, 1);
    let mut presses = Vec::new();
    for frame in 0..4_u8 {
        let cmd = mind.decide(
            &view(&bodies[0], &bodies, &[], f64::from(frame)),
            None,
            &Weapons::id(),
        );
        assert_eq!((cmd.forward, cmd.side, cmd.impulse), (0.0, 0.0, 0));
        presses.push(cmd.buttons);
    }
    assert_eq!(presses, [1, 0, 1, 0]);
}

// spec BOT-3.1: in deathmatch the other living, drawn clients are foes; in coop the living
// monsters that can be hurt, and never the bot itself or the dead
#[test]
fn foes_in_each_kind_of_game() {
    use crate::mind::is_foe;
    let me = body(2, [0.0; 3]);
    let other = body(3, [0.0; 3]);
    let monster = Body {
        flags: MONSTER,
        classname: b"monster_army".to_vec(),
        ..body(9, [0.0; 3])
    };
    let dead = Body {
        deadflag: 1.0,
        ..body(4, [0.0; 3])
    };
    let unseen = Body {
        modelindex: 0.0,
        ..body(5, [0.0; 3])
    };
    let hardy = Body {
        takedamage: 0.0,
        ..monster.clone()
    };
    assert!(is_foe(&me, &other, Mode::Deathmatch));
    assert!(!is_foe(&me, &monster, Mode::Deathmatch));
    assert!(!is_foe(&me, &me, Mode::Deathmatch));
    assert!(!is_foe(&me, &dead, Mode::Deathmatch));
    assert!(!is_foe(&me, &unseen, Mode::Deathmatch));
    assert!(is_foe(&me, &monster, Mode::Coop));
    assert!(!is_foe(&me, &other, Mode::Coop));
    assert!(!is_foe(&me, &hardy, Mode::Coop));
}

// spec BOT-3.2: a foe is in sight within 2048 and 90 degrees of the view; the bot fights the
// nearest it sees, and keeps fighting the one it fights while it sees it
#[test]
fn a_bot_sees_ahead_and_fights_the_nearest() {
    use crate::mind::in_sight;
    let me = body(2, [0.0, 0.0, 24.0]);
    assert!(in_sight(&me, &body(3, [1000.0, 0.0, 24.0])).is_some());
    assert!(
        in_sight(&me, &body(3, [0.0, 1000.0, 24.0])).is_some(),
        "90 degrees"
    );
    assert!(
        in_sight(&me, &body(3, [-1000.0, 10.0, 24.0])).is_none(),
        "behind"
    );
    assert!(
        in_sight(&me, &body(3, [3000.0, 0.0, 24.0])).is_none(),
        "too far"
    );
    let near = body(3, [300.0, 0.0, 24.0]);
    let far = body(4, [600.0, 0.0, 24.0]);
    let bodies = [me, near, far];
    let mut mind = Mind::new(bot(), BotSkill::Nightmare, 1);
    let aim_at = |mind: &mut Mind, seen: &[usize], time: f64| {
        mind.decide(&view(&bodies[0], &bodies, seen, time), None, &Weapons::id())
    };
    aim_at(&mut mind, &[4], 0.0);
    // The far foe is fought until it is out of sight, though the near one comes into it.
    aim_at(&mut mind, &[3, 4], 0.1);
    assert_eq!(
        aim_at(&mut mind, &[3, 4], 0.2).buttons & 1,
        1,
        "still the first, seen 0.2 s"
    );
    assert_eq!(
        aim_at(&mut mind, &[3], 0.21).buttons & 1,
        0,
        "the near one, just seen"
    );
}

// spec BOT-6.1, BOT-6.2: id's table is best first with W_ChangeWeapon's impulses; a bot sends
// the impulse of the best weapon it can fire, at most every half second, none when in hand
#[test]
fn a_bot_holds_its_best_weapon() {
    let weapons = Weapons::id();
    let armed = Body {
        items: SHOTGUN + ROCKETS + AXE,
        ammo: [10.0, 0.0, 5.0, 0.0],
        ..body(2, [0.0; 3])
    };
    assert_eq!(weapons.best(&armed).unwrap().impulse, 7);
    let empty = Body {
        ammo: [10.0, 0.0, 0.0, 0.0],
        ..armed.clone()
    };
    assert_eq!(weapons.best(&empty).unwrap().impulse, 2, "no rockets");
    let bare = Body {
        ammo: [0.0; 4],
        ..empty
    };
    assert_eq!(
        weapons.best(&bare).unwrap().impulse,
        1,
        "the axe needs nothing"
    );
    let bodies = [armed];
    let mut mind = Mind::new(bot(), BotSkill::Normal, 1);
    let impulse = |mind: &mut Mind, time: f64| {
        mind.decide(&view(&bodies[0], &bodies, &[], time), None, &weapons)
            .impulse
    };
    assert_eq!(impulse(&mut mind, 1.0), 7);
    assert_eq!(impulse(&mut mind, 1.2), 0, "within half a second");
    assert_eq!(impulse(&mut mind, 1.6), 7);
    let held = Body {
        weapon: ROCKETS,
        ..bodies[0].clone()
    };
    let bodies = [held];
    assert_eq!(
        Mind::new(bot(), BotSkill::Normal, 1)
            .decide(&view(&bodies[0], &bodies, &[], 1.0), None, &weapons)
            .impulse,
        0
    );
    let own = Weapons::new(Vec::new());
    assert!(
        own.best(&bodies[0]).is_none(),
        "a mod's table may hold none"
    );
}

// spec BOT-8.1: each bot's numbers come from the seed and its slot: the same on every run, and
// another slot's or seed's differ
#[test]
fn each_bot_draws_its_own_numbers() {
    let draws = |seed, slot| {
        let mut rng = BotRng::new(seed, ClientId::new(slot).unwrap());
        (0..8).map(|_| rng.unit()).collect::<Vec<f32>>()
    };
    assert_eq!(draws(1, 1), draws(1, 1));
    assert_ne!(draws(1, 1), draws(1, 2));
    assert_ne!(draws(1, 1), draws(2, 1));
    let mut rng = BotRng::new(3, bot());
    for _ in 0..1000 {
        let u = rng.unit();
        assert!((0.0..1.0).contains(&u));
        assert!(rng.below(5) < 5);
    }
    assert_eq!(rng.below(0), 0);
}

// spec BOT-7.2, BOT-7.1: with no graph a bot stands and still fights; with one it runs toward
// its route at 400 units a second, split between forward and side by where it looks
#[test]
fn a_bot_runs_along_its_route() {
    let me = body(2, [-400.0, 0.0, 24.0]);
    let bodies = [me.clone(), item(9, "weapon_rocketlauncher")];
    let still = Mind::new(bot(), BotSkill::Normal, 1).decide(
        &view(&bodies[0], &bodies, &[], 0.0),
        None,
        &Weapons::id(),
    );
    assert_eq!((still.forward, still.side), (0.0, 0.0));
    let graph = Bots::graph_of(&compiled(&lift_and_door()), Deathmatch::Off).unwrap();
    let rocket = Body {
        origin: [-100.0, 0.0, 0.0],
        ..item(9, "weapon_rocketlauncher")
    };
    let bodies = [me, rocket];
    let mut mind = Mind::new(bot(), BotSkill::Normal, 1);
    let cmd: UserCmd = mind.decide(
        &view(&bodies[0], &bodies, &[], 0.0),
        Some(&graph),
        &Weapons::id(),
    );
    let speed = cmd.forward.hypot(cmd.side);
    assert!((speed - 400.0).abs() < 0.5, "{cmd:?}");
    assert!(cmd.forward > 300.0, "the item is ahead, east: {cmd:?}");
}

/// The move of a bot at `me` walking `route`, with no graph to choose another.
fn steered(me: &Body, route: Vec<[f32; 3]>) -> UserCmd {
    let bodies = [me.clone()];
    let mut mind = Mind::new(bot(), BotSkill::Normal, 1);
    mind.follow(route, 0.0);
    mind.decide(&view(&bodies[0], &bodies, &[], 0.0), None, &Weapons::id())
}

// spec BOT-4.4: a bot takes the next place once within 24 across and 48 up or down; it jumps
// onto a ledge up to 48 above within 64 across and along a move past 40 across; it waits under a
// place more than 48 above it within 24 across; it swims up in water toward a place above
#[test]
fn a_bot_walks_jumps_waits_and_swims_along_its_route() {
    let me = body(2, [0.0, 0.0, 24.0]);
    let walk = steered(&me, vec![[20.0, 0.0, 24.0], [36.0, 0.0, 24.0]]);
    assert_eq!(
        walk.buttons & 2,
        0,
        "the first place is reached, the next walked to"
    );
    assert!(walk.forward > 0.0);
    let ledge = steered(&me, vec![[30.0, 0.0, 60.0]]);
    assert_eq!(ledge.buttons & 2, 2, "36 up, 30 across");
    let step = steered(&me, vec![[30.0, 0.0, 40.0]]);
    assert_eq!(step.buttons & 2, 0, "16 up is a step");
    let gap = steered(&me, vec![[0.0, 0.0, 24.0], [90.0, 0.0, 24.0]]);
    assert_eq!(gap.buttons & 2, 2, "a move of 90 across is a leap");
    let far = steered(&me, vec![[90.0, 0.0, 24.0]]);
    assert_eq!(
        far.buttons & 2,
        0,
        "far from the route's start, but no leap"
    );
    let lift = steered(&me, vec![[10.0, 0.0, 24.0], [10.0, 0.0, 152.0]]);
    assert_eq!((lift.forward, lift.side), (0.0, 0.0), "waits for the lift");
    let wet = Body {
        waterlevel: 3.0,
        ..me.clone()
    };
    assert_eq!(steered(&wet, vec![[60.0, 0.0, 40.0]]).up, 200.0);
    assert_eq!(
        steered(&me, vec![[60.0, 0.0, 40.0]]).up,
        0.0,
        "out of water"
    );
    let south = steered(&me, vec![[0.0, -100.0, 24.0]]);
    assert!(
        south.forward.abs() < 40.0 && south.side > 395.0,
        "to the right: {south:?}"
    );
}

// spec BOT-4.5: an item the bot came within 48 of is passed over as a goal for ten seconds
#[test]
fn an_item_reached_is_passed_over() {
    let graph = Bots::graph_of(&compiled(&lift_and_door()), Deathmatch::Off).unwrap();
    let rocket = Body {
        origin: [-200.0, 0.0, 0.0],
        ..item(9, "weapon_rocketlauncher")
    };
    let mut mind = Mind::new(bot(), BotSkill::Normal, 1);
    let at = |mind: &mut Mind, x: f32, time: f64| {
        let bodies = [body(2, [x, 0.0, 24.0]), rocket.clone()];
        mind.decide(
            &view(&bodies[0], &bodies, &[], time),
            Some(&graph),
            &Weapons::id(),
        );
        mind.item_goal()
    };
    assert_eq!(at(&mut mind, -400.0, 0.0), Some(9));
    assert_eq!(
        at(&mut mind, -200.0, 1.0),
        None,
        "reached, and passed over at once"
    );
    assert_eq!(at(&mut mind, -200.0, 1.1), None);
    assert_eq!(at(&mut mind, -400.0, 5.2), None, "still within ten seconds");
    // It chooses again four seconds after it last chose: at 3, 7, then 12.9 or 13 seconds.
    let after = |last: f64| {
        let mut mind = Mind::new(bot(), BotSkill::Normal, 1);
        at(&mut mind, -400.0, 0.0);
        at(&mut mind, -200.0, 3.0);
        at(&mut mind, -400.0, 7.0);
        at(&mut mind, -400.0, last)
    };
    assert_eq!(after(12.9), None, "9.9 seconds on");
    assert_eq!(after(13.0), Some(9), "ten seconds on");
    let mut near = Mind::new(bot(), BotSkill::Normal, 1);
    assert_eq!(at(&mut near, -400.0, 0.0), Some(9));
    assert_eq!(at(&mut near, -240.0, 0.1), None, "within 48 across");
    let mut short = Mind::new(bot(), BotSkill::Normal, 1);
    assert_eq!(at(&mut short, -400.0, 0.0), Some(9));
    assert_eq!(at(&mut short, -251.0, 0.1), Some(9), "49 across");
}

// spec BOT-8.1: the source is SplitMix64: started at 0 (the seed GOLDEN for slot 0), its first
// numbers are SplitMix64's; its unit numbers fill 0 to 1, signed ones -1 to 1 on both sides,
// and whole numbers spread below their bound
#[test]
fn the_source_is_splitmix64() {
    let mut rng = BotRng::new(0x9E37_79B9_7F4A_7C15, ClientId::FIRST);
    assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
    assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
    assert_eq!(rng.next_u64(), 0x06C4_5D18_8009_454F);
    let mut rng = BotRng::new(5, bot());
    let (mut low, mut high, mut negative, mut positive) = (1.0_f32, 0.0_f32, false, false);
    let mut seen = [false; 5];
    for _ in 0..1000 {
        let u = rng.unit();
        low = low.min(u);
        high = high.max(u);
        let s = rng.signed();
        negative |= s < -0.5;
        positive |= s > 0.5;
        seen[rng.below(5)] = true;
    }
    assert!(low < 0.01 && high > 0.99, "{low} {high}");
    assert!(negative && positive);
    assert_eq!(seen, [true; 5]);
}

// spec BOT-5.1: each of the error's two parts spreads across its bound
#[test]
fn both_parts_of_the_error_spread() {
    let aim = Aim::of(BotSkill::Nightmare);
    let mut rng = BotRng::new(3, bot());
    let mut widest = [0.0_f32; 2];
    for _ in 0..2000 {
        let error = aim.draw_error(&mut rng);
        for (w, e) in widest.iter_mut().zip(error) {
            *w = w.max(e.abs());
        }
    }
    assert!(widest.iter().all(|&w| w > 1.9), "{widest:?}");
}

// spec BOT-5.1, BOT-5.2: the angles to a point: yaw counterclockwise from east, pitch down
// positive; the turn toward an aim the short way round, at most its step on each axis
#[test]
fn the_angles_and_turns_of_the_aim() {
    use crate::aim::{angles_to, turned};
    assert_eq!(angle_between(10.0, 30.0), 20.0);
    assert_eq!(angle_between(350.0, 10.0), 20.0);
    assert_eq!(angle_between(10.0, 350.0), -20.0);
    assert_eq!(angle_between(0.0, 180.0), -180.0);
    let [pitch, yaw] = angles_to([0.0, 0.0, 0.0], [0.0, 100.0, 0.0]);
    assert!((yaw - 90.0).abs() < 1e-4 && pitch.abs() < 1e-4);
    let [pitch, yaw] = angles_to([10.0, 10.0, 10.0], [10.0, -90.0, 110.0]);
    assert!(
        (yaw + 90.0).abs() < 1e-4 && (pitch + 45.0).abs() < 1e-4,
        "{pitch} {yaw}"
    );
    assert_eq!(turned([0.0, 350.0], [-30.0, 20.0], 10.0), [-10.0, 360.0]);
    assert_eq!(turned([0.0, 0.0], [5.0, -5.0], 10.0), [5.0, -5.0]);
}

// spec BOT-3.2, BOT-5.1: a bot aims from its eye at the middle of its foe's box, off by an error
// within its bound, the same error for half a second and a fresh one after
#[test]
fn a_bot_aims_at_its_foes_middle() {
    use crate::mind::{apart, eye, middle};
    let leaning = Body {
        view_ofs: [2.0, 3.0, 22.0],
        ..body(2, [5.0, 7.0, 24.0])
    };
    assert_eq!(eye(&leaning), [7.0, 10.0, 46.0]);
    let me = body(2, [0.0, 0.0, 24.0]);
    let foe = Body {
        absmin: [90.0, -10.0, 0.0],
        absmax: [110.0, 30.0, 92.0],
        ..body(3, [100.0, 0.0, 24.0])
    };
    assert_eq!(middle(&foe), [100.0, 10.0, 46.0]);
    assert_eq!(apart([0.0, 0.0, 0.0], [3.0, 4.0, -5.0]), (5.0, -5.0));
    assert_eq!(apart([1.0, 2.0, 3.0], [4.0, 6.0, 1.0]), (5.0, -2.0));
    let bodies = [me, foe];
    let mut mind = Mind::new(bot(), BotSkill::Easy, 1);
    let mut aims = Vec::new();
    for time in [0.0, 0.2, 0.4, 0.6] {
        mind.decide(&view(&bodies[0], &bodies, &[3], time), None, &Weapons::id());
        aims.push(mind.aim_at(&view(&bodies[0], &bodies, &[3], time)).unwrap());
    }
    let true_yaw = 10.0_f32.atan2(100.0).to_degrees();
    for [pitch, yaw] in &aims {
        assert!(
            pitch.abs() <= 12.0 && (yaw - true_yaw).abs() <= 12.0,
            "{aims:?}"
        );
    }
    assert_eq!(aims[0], aims[1], "within half a second");
    assert_eq!(aims[0], aims[2], "0.4 s on");
    assert_ne!(aims[0], aims[3], "past half a second");
    let [ep, ey] = Aim::of(BotSkill::Easy).draw_error(&mut BotRng::new(1, bot()));
    assert_eq!(
        aims[0],
        [ep, true_yaw + ey],
        "the first draw of its own numbers"
    );
}

// spec BOT-3.3: a dead bot keeps looking where it looked
#[test]
fn a_dead_bot_keeps_its_view() {
    let dead = Body {
        deadflag: 2.0,
        v_angle: [10.0, 45.0, 0.0],
        ..body(2, [0.0; 3])
    };
    let bodies = [dead];
    let cmd = Mind::new(bot(), BotSkill::Normal, 1).decide(
        &view(&bodies[0], &bodies, &[], 0.0),
        None,
        &Weapons::id(),
    );
    assert_eq!(cmd.angles, [10.0, 45.0, 0.0]);
}

// spec BOT-4.4: at each bound exactly: a step of 18 is walked and a ledge of 48 jumped, not
// waited under; a move of 40 is walked; a swimmer level with the next place does not swim up
#[test]
fn the_route_bounds_are_exact() {
    let me = body(2, [0.0, 0.0, 24.0]);
    assert_eq!(
        steered(&me, vec![[30.0, 0.0, 42.0]]).buttons & 2,
        0,
        "18 up"
    );
    let ledge = steered(&me, vec![[20.0, 0.0, 72.0]]);
    assert_eq!(ledge.buttons & 2, 2, "48 up, 20 across");
    assert!(ledge.forward > 0.0, "jumps, does not wait");
    assert_eq!(
        steered(&me, vec![[64.0, 0.0, 60.0]]).buttons & 2,
        2,
        "64 across"
    );
    assert_eq!(
        steered(&me, vec![[65.0, 0.0, 60.0]]).buttons & 2,
        0,
        "past 64 across"
    );
    let walk = steered(&me, vec![[0.0, 0.0, 24.0], [40.0, 0.0, 24.0]]);
    assert_eq!(walk.buttons & 2, 0, "a move of 40");
    let wet = Body {
        waterlevel: 2.0,
        ..me
    };
    assert_eq!(steered(&wet, vec![[60.0, 0.0, 24.0]]).up, 0.0, "level");
    assert_eq!(
        steered(&wet, vec![[60.0, 0.0, 25.0]]).up,
        200.0,
        "waist deep, above"
    );
}

// spec BOT-3.1, BOT-3.2: health 0 is dead; a foe 2048 away is in range
#[test]
fn the_sight_bounds_are_exact() {
    use crate::mind::{in_sight, is_foe};
    let me = body(2, [0.0, 0.0, 24.0]);
    let none = Body {
        health: 0.0,
        ..body(3, [0.0; 3])
    };
    assert!(!is_foe(&me, &none, Mode::Deathmatch));
    let edge = Body {
        absmin: [2048.0, 0.0, 46.0],
        absmax: [2048.0, 0.0, 46.0],
        ..body(3, [2048.0, 0.0, 46.0])
    };
    assert!(in_sight(&me, &edge).is_some());
}

// spec BOT-4.2, BOT-4.1: ammunition at exactly half is not low; of two goals that weigh the
// same, the first
#[test]
fn the_need_bounds_are_exact() {
    let weapons = Weapons::id();
    let half = Body {
        ammo: [50.0, 0.0, 0.0, 0.0],
        ..body(2, [0.0; 3])
    };
    assert_eq!(item_need(&half, &item(9, "item_shells"), &weapons), 2.0);
    assert_eq!(Ammo::of_box(b"item_cells"), Some(Ammo::Cells));
    assert_eq!(Ammo::Cells.most(), 100.0);
    assert_eq!(choose(&[(10.0, Some(0.0)), (10.0, Some(0.0))]), Some(0));
}

// spec BOT-4.3: in coop a bot far from its friend goes to it, and to none within 256 or dead
#[test]
fn a_bot_in_coop_goes_to_its_friend() {
    let graph = Bots::graph_of(&compiled(&lift_and_door()), Deathmatch::Off).unwrap();
    let goal = |friend: Body| {
        let bodies = [body(2, [-400.0, 0.0, 24.0]), friend];
        let mut mind = Mind::new(bot(), BotSkill::Normal, 1);
        let coop = View {
            mode: Mode::Coop,
            ..view(&bodies[0], &bodies, &[], 0.0)
        };
        mind.decide(&coop, Some(&graph), &Weapons::id());
        mind.friend_goal()
    };
    assert_eq!(goal(body(1, [-100.0, 0.0, 24.0])), Some(1));
    assert_eq!(goal(body(1, [-200.0, 0.0, 24.0])), None, "200 away");
    let dead = Body {
        deadflag: 2.0,
        ..body(1, [-100.0, 0.0, 24.0])
    };
    assert_eq!(goal(dead), None);
    let monster = Body {
        flags: MONSTER,
        ..body(1, [-100.0, 0.0, 24.0])
    };
    assert_eq!(goal(monster), None, "no client");
}

// spec BOT-4.1: a bot that has moved less than 32 in two seconds with a route jumps and chooses
// again; one that moves does not
#[test]
fn a_stuck_bot_jumps() {
    let graph = Bots::graph_of(&compiled(&lift_and_door()), Deathmatch::Off).unwrap();
    let rocket = Body {
        origin: [-100.0, 0.0, 0.0],
        ..item(9, "weapon_rocketlauncher")
    };
    let mut mind = Mind::new(bot(), BotSkill::Normal, 1);
    let jumps = |mind: &mut Mind, x: f32, time: f64| {
        let bodies = [body(2, [x, 0.0, 24.0]), rocket.clone()];
        mind.decide(
            &view(&bodies[0], &bodies, &[], time),
            Some(&graph),
            &Weapons::id(),
        )
        .buttons
            & 2
    };
    assert_eq!(jumps(&mut mind, -400.0, 0.0), 0);
    assert_eq!(
        jumps(&mut mind, -400.0, 2.0),
        0,
        "where it was at 0: its first check"
    );
    assert_eq!(jumps(&mut mind, -395.0, 3.0), 0, "a second after the check");
    assert_eq!(jumps(&mut mind, -390.0, 4.0), 2, "10 in two seconds");
    assert_eq!(jumps(&mut mind, -358.0, 6.0), 0, "32 in two seconds");
}

// spec BOT-4.1: a bot keeps its route until four seconds have passed, then chooses again
#[test]
fn a_bot_chooses_again_every_four_seconds() {
    let graph = Bots::graph_of(&compiled(&lift_and_door()), Deathmatch::Off).unwrap();
    let far = Body {
        origin: [-100.0, 0.0, 0.0],
        ..item(9, "weapon_supershotgun")
    };
    let near = Body {
        origin: [-300.0, 0.0, 0.0],
        ..item(10, "weapon_rocketlauncher")
    };
    let mut mind = Mind::new(bot(), BotSkill::Normal, 1);
    let goal = |mind: &mut Mind, items: &[Body], time: f64, x: f32| {
        let mut bodies = vec![body(2, [x, 0.0, 24.0])];
        bodies.extend_from_slice(items);
        mind.decide(
            &view(&bodies[0], &bodies, &[], time),
            Some(&graph),
            &Weapons::id(),
        );
        mind.item_goal()
    };
    assert_eq!(
        goal(&mut mind, std::slice::from_ref(&far), 10.0, -400.0),
        Some(9)
    );
    let both = [far, near];
    assert_eq!(
        goal(&mut mind, &both, 11.0, -380.0),
        Some(9),
        "its route holds"
    );
    assert_eq!(goal(&mut mind, &both, 13.9, -360.0), Some(9));
    assert_eq!(
        goal(&mut mind, &both, 14.0, -340.0),
        Some(10),
        "four seconds on"
    );
}

// spec BOT-6.1, BOT-7.2: the bots hold weapons by id's table unless given one, and have no
// graph until given one; a map that does not read says why
#[test]
fn the_bots_take_a_table_a_graph_and_a_seed() {
    let bots = Bots::default();
    assert_eq!(bots.weapons(), &Weapons::id());
    assert!(!bots.has_graph());
    assert!(bots.is_empty());
    assert_eq!(bots.len(), 0);
    let mut own = Bots::new(None)
        .with_weapons(Weapons::new(Vec::new()))
        .with_seed(9);
    assert_eq!(own.weapons(), &Weapons::new(Vec::new()));
    own.set_graph(Bots::graph_of(&compiled(&lift_and_door()), Deathmatch::Off).unwrap());
    assert!(own.has_graph());
    let error = Bots::graph_of(b"no", Deathmatch::Off).unwrap_err();
    assert!(
        error.to_string().starts_with("the map did not read"),
        "{error}"
    );
    assert!(own.skill(bot()).is_none());
}
