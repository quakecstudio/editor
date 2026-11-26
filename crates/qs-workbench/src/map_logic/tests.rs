// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use std::fmt::Write as _;
use std::path::PathBuf;

use qc_project::FsLoader;
use qs_commands::{Registry, Session};
use serde_json::{Value, json};

use super::{
    CallbackKind, FireEnd, FiredEvent, LogicIssues, LogicKind, MAP_FIRE, MAP_LOGIC_ISSUES, MapFired,
};
use crate::vm_run::tests::system_defs;
use crate::{Config, OPEN_PROJECT, Workbench, register};
use qs_map_edit::{MAP_ISSUES, MAP_OPEN, MAP_SET_KEY, MapIssues};

/// A program with id's way of firing targets (`subs.qc`, `SUB_UseTargets`), its classes, and
/// a mod's own relay, which fires what its own key `fires` names.
const CODE: &str = r#"
void(entity e) remove = #15;
entity() spawn = #14;
void(entity e, string m) setmodel = #3;
entity(entity start, .string fld, string match) find = #18;
string(string s) precache_model = #20;
.string killtarget;
.string fires;
.float delay;
.float count;
.void() think1;
entity activator;
void() SUB_Null = {};
void() SUB_UseTargets;
void() DelayThink = { activator = self.enemy; SUB_UseTargets(); remove(self); };
void() SUB_UseTargets = {
    local entity t, stemp, otemp, act;
    if (self.delay) {
        t = spawn();
        t.classname = "DelayedUse";
        t.nextthink = time + self.delay;
        t.think = DelayThink;
        t.enemy = activator;
        t.killtarget = self.killtarget;
        t.target = self.target;
        return;
    }
    if (self.killtarget) {
        t = world;
        do {
            t = find(t, targetname, self.killtarget);
            if (!t)
                return;
            remove(t);
        } while (1);
    }
    if (self.target) {
        act = activator;
        t = world;
        do {
            t = find(t, targetname, self.target);
            if (!t)
                return;
            stemp = self;
            otemp = other;
            self = t;
            other = stemp;
            if (self.use != SUB_Null) {
                if (self.use)
                    self.use();
            }
            self = stemp;
            other = otemp;
            activator = act;
        } while (1);
    }
};
void() worldspawn = { precache_model("progs/player.mdl"); };
void() fire_targets = { activator = other; SUB_UseTargets(); };
void() func_button = { self.use = fire_targets; };
void() trigger_relay = { self.use = fire_targets; };
void() door_done = { self.frame = 1; };
void() door_use = { self.think = door_done; self.nextthink = time + 0.5; };
void() func_door = { setmodel(self, self.model); self.use = door_use; };
void() info_null = {};
void() counter_use = {
    self.count = self.count - 1;
    if (self.count > 0)
        return;
    activator = other;
    SUB_UseTargets();
};
void() trigger_counter = { if (!self.count) self.count = 2; self.use = counter_use; };
void() bad_use = { self.think1(); };
void() trigger_bad = { self.use = bad_use; };
void() mod_use = {
    local entity t;
    t = find(world, targetname, self.fires);
    while (t) {
        if (t.use) {
            other = self;
            self = t;
            self.use();
            self = other;
        }
        t = find(t, targetname, self.fires);
    }
};
void() mod_relay = { self.use = mod_use; };
"#;

/// The world's entity, with a brush.
const WORLD: &str = "{\n\"classname\" \"worldspawn\"\n\
    {\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) floor 0 0 0 1 1\n\
    ( 0 0 64 ) ( 1 0 64 ) ( 0 1 64 ) floor 0 0 0 1 1\n}\n}\n";

fn entity(classname: &str, keys: &[(&str, &str)]) -> String {
    let mut text = format!("{{\n\"classname\" \"{classname}\"\n\"origin\" \"0 0 32\"\n");
    for (key, value) in keys {
        _ = writeln!(text, "\"{key}\" \"{value}\"");
    }
    text.push_str("}\n");
    text
}

/// A session with the project of [`CODE`] open and the map of `entities` after the world.
fn session(name: &str, entities: &[String]) -> (Session<Workbench<FsLoader>>, PathBuf) {
    let dir = std::env::temp_dir().join(format!("qs-wb-logic-{name}-{}", std::process::id()));
    let _gone = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("maps")).unwrap();
    std::fs::write(dir.join("defs.qc"), system_defs()).unwrap();
    std::fs::write(dir.join("world.qc"), CODE).unwrap();
    std::fs::write(dir.join("progs.src"), "progs.dat\ndefs.qc\nworld.qc\n").unwrap();
    std::fs::write(
        dir.join("maps/test.map"),
        format!("{WORLD}{}", entities.concat()),
    )
    .unwrap();
    let mut registry = Registry::new();
    register(&mut registry).unwrap();
    // `map.issues`, which a context with a game offers.
    qs_map_edit::register_map_game(&mut registry).unwrap();
    let mut session = Session::new(Workbench::new(FsLoader, Config::default()), registry);
    session
        .execute(OPEN_PROJECT, json!({ "path": dir }))
        .unwrap();
    session
        .execute(MAP_OPEN, json!({ "path": dir.join("maps/test.map") }))
        .unwrap();
    (session, dir)
}

fn fire(session: &mut Session<Workbench<FsLoader>>, params: Value) -> MapFired {
    serde_json::from_value(session.execute(MAP_FIRE, params).unwrap()).unwrap()
}

fn issues(session: &mut Session<Workbench<FsLoader>>) -> LogicIssues {
    serde_json::from_value(session.execute(MAP_LOGIC_ISSUES, json!({})).unwrap()).unwrap()
}

/// The entity and the kind of each issue.
fn kinds(found: &LogicIssues) -> Vec<(usize, LogicKind)> {
    found
        .issues
        .iter()
        .map(|issue| (issue.entity.get(), issue.kind))
        .collect()
}

fn button_and_door() -> Vec<String> {
    vec![
        entity("func_button", &[("target", "d")]),
        entity("func_door", &[("targetname", "d")]),
    ]
}

// spec FIRE-5.1: a button fired answers its use, the door's use and the door's think, each with
// its place in the map, and the trace ends quiet
#[test]
fn a_button_fired_answers_the_doors_use_and_its_move() {
    let (mut session, _) = session("door", &button_and_door());
    let fired = fire(&mut session, json!({ "entity": 1 }));
    assert_eq!(fired.fired_by, CallbackKind::Use);
    let ran: Vec<_> = fired
        .events
        .iter()
        .filter_map(|event| match event {
            FiredEvent::Ran {
                callback,
                entity,
                classname,
                function,
                ..
            } => Some((
                *callback,
                entity.entity.map(|place| place.get()),
                classname.as_str(),
                function.as_str(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        ran,
        [
            (CallbackKind::Use, Some(1), "func_button", "fire_targets"),
            (CallbackKind::Use, Some(2), "func_door", "door_use"),
            (CallbackKind::Think, Some(2), "func_door", "door_done"),
        ]
    );
    assert_eq!(fired.end, FireEnd::Quiet);
    assert!(fired.events.iter().any(|event| matches!(event,
        FiredEvent::Changed { entity, fields, .. }
            if entity.entity.map(|place| place.get()) == Some(2)
                && fields.iter().any(|field| field.name == "frame" && field.value == "1"))));
}

// spec FIRE-3.5, FIRE-5.1: the same map, skill and entity give the same answer, byte for byte
#[test]
fn the_same_firing_answers_the_same_bytes() {
    let (mut session, _) = session("same", &button_and_door());
    let params = json!({ "entity": 1, "skill": "hard" });
    let first = session.execute(MAP_FIRE, params.clone()).unwrap();
    let second = session.execute(MAP_FIRE, params).unwrap();
    assert_eq!(first.to_string(), second.to_string());
    assert!(first["events"].as_array().unwrap().len() > 3);
}

// spec FIRE-5.1: the length given is the trace's, and a spawned entity has no place in the map
#[test]
fn a_delayed_firing_runs_in_the_length_given() {
    let map = [
        entity("func_button", &[("target", "d"), ("delay", "2")]),
        entity("func_door", &[("targetname", "d")]),
    ];
    let (mut session, _) = session("delay", &map);
    let short = fire(&mut session, json!({ "entity": 1, "seconds": 1.0 }));
    assert_eq!(short.end, FireEnd::Length);
    assert!(
        !short.events.iter().any(
            |event| matches!(event, FiredEvent::Ran { function, .. } if function == "door_use")
        )
    );
    let long = fire(&mut session, json!({ "entity": 1 }));
    assert_eq!(long.end, FireEnd::Quiet);
    assert!(long.events.iter().any(
        |event| matches!(event, FiredEvent::Spawned { classname, .. } if classname == "DelayedUse")
    ));
    assert!(long.events.iter().any(|event| matches!(event,
        FiredEvent::Ran { entity, function, .. }
            if function == "DelayThink" && entity.entity.is_none())));
    assert!(
        long.events.iter().any(
            |event| matches!(event, FiredEvent::Ran { function, .. } if function == "door_use")
        )
    );
}

// spec FIRE-5.1, FIRE-1.3: an entity the game did not keep, and one with nothing to fire it by,
// are refused
#[test]
fn what_cannot_be_fired_is_refused_with_the_reason() {
    let map = [
        entity("func_button", &[("spawnflags", "256")]),
        entity("info_null", &[]),
    ];
    let (mut session, _) = session("refused", &map);
    let error = session
        .execute(MAP_FIRE, json!({ "entity": 1, "skill": "easy" }))
        .unwrap_err();
    assert!(
        error.to_string().contains("does not keep entity 1"),
        "{error}"
    );
    let error = session
        .execute(MAP_FIRE, json!({ "entity": 2 }))
        .unwrap_err();
    assert!(
        error.to_string().contains("has no use and no touch"),
        "{error}"
    );
    let error = session
        .execute(MAP_FIRE, json!({ "entity": 1, "by": "touch" }))
        .unwrap_err();
    assert!(error.to_string().contains("no touch function"), "{error}");
}

// spec FIRE-5.2: firing changes neither the map nor the project's files
#[test]
fn firing_changes_nothing() {
    let (mut session, dir) = session("nothing", &button_and_door());
    let before = std::fs::read(dir.join("maps/test.map")).unwrap();
    fire(&mut session, json!({ "entity": 1 }));
    issues(&mut session);
    assert_eq!(std::fs::read(dir.join("maps/test.map")).unwrap(), before);
    assert!(!dir.join("progs.dat").exists());
    assert_eq!(session.context().map().unwrap().file().to_bytes(), before);
}

// spec FIRE-4.1: a map whose chains all work has no issue, and every entity with targets is fired
#[test]
fn a_map_that_works_has_no_issue() {
    let mut map = button_and_door();
    map.push(entity("trigger_relay", &[("target", "d")]));
    let (mut session, _) = session("works", &map);
    let found = issues(&mut session);
    assert_eq!(found.issues, []);
    assert_eq!(found.fired, 2);
}

// spec FIRE-4.2: a target with no use is reported once, however many entities fire it
#[test]
fn a_target_with_no_use_is_reported() {
    let map = [
        entity("func_button", &[("target", "n")]),
        entity("info_null", &[("targetname", "n")]),
        entity("func_button", &[("target", "n")]),
    ];
    let (mut session, _) = session("nouse", &map);
    let found = issues(&mut session);
    assert_eq!(found.fired, 2);
    assert_eq!(kinds(&found), [(2, LogicKind::NothingToUse)]);
    assert_eq!(found.issues[0].fired.get(), 1);
    assert_eq!(
        found.issues[0].message,
        "entity 2 (info_null) is fired and has no use function: firing it does nothing"
    );
}

// spec FIRE-4.2, FIRE-4.1: a mod's own class firing by its own key is traced as id's are
#[test]
fn a_mods_own_trigger_is_traced() {
    let map = [
        entity("mod_relay", &[("fires", "d")]),
        entity("func_door", &[("targetname", "d")]),
    ];
    let (mut session, _) = session("mod", &map);
    let fired = fire(&mut session, json!({ "entity": 1 }));
    assert!(fired.events.iter().any(|event| matches!(event,
        FiredEvent::Looked { name, found: Some(found), .. }
            if name == "d" && found.entity.map(|place| place.get()) == Some(2))));
    assert!(fired.events.iter().any(|event| matches!(event,
        FiredEvent::Ran { function, .. } if function == "door_use")));
    let found = issues(&mut session);
    assert_eq!(found.fired, 1, "the key `fires` names entities to fire");
    assert_eq!(found.issues, []);
}

// spec FIRE-4.3: a chain that removes an entity and then looks for it is reported
#[test]
fn a_chain_that_removes_what_it_fires_is_reported() {
    let map = [
        entity("func_button", &[("target", "r")]),
        entity("trigger_relay", &[("targetname", "r"), ("killtarget", "x")]),
        entity("trigger_relay", &[("targetname", "r"), ("target", "x")]),
        entity("func_door", &[("targetname", "x")]),
    ];
    let (mut session, _) = session("removed", &map);
    let found = issues(&mut session);
    assert!(
        kinds(&found).contains(&(4, LogicKind::RemovedThenFired)),
        "{found:?}"
    );
}

// spec FIRE-4.4: a chain that comes back to an entity is reported, and so is one that stops
// the game
#[test]
fn a_loop_and_an_error_are_reported() {
    let map = [
        entity(
            "trigger_relay",
            &[("targetname", "a"), ("target", "b"), ("delay", "0.1")],
        ),
        entity(
            "trigger_relay",
            &[("targetname", "b"), ("target", "a"), ("delay", "0.1")],
        ),
        entity("func_button", &[("target", "bad")]),
        entity("trigger_bad", &[("targetname", "bad")]),
    ];
    let (mut session, _) = session("loop", &map);
    let found = issues(&mut session);
    let kinds = kinds(&found);
    assert!(kinds.contains(&(1, LogicKind::Loop)), "{found:?}");
    assert!(kinds.contains(&(2, LogicKind::Loop)), "{found:?}");
    assert!(kinds.contains(&(3, LogicKind::Error)), "{found:?}");
    let error = found
        .issues
        .iter()
        .find(|issue| issue.kind == LogicKind::Error)
        .unwrap();
    assert!(error.message.contains("NULL function"), "{}", error.message);
}

// spec FIRE-4.5: a counter fired by fewer entities than its count is reported, and one fired
// by as many is not
#[test]
fn a_counter_never_reached_is_reported() {
    let counter = |count: &str| {
        vec![
            entity("func_button", &[("target", "c")]),
            entity("func_button", &[("target", "c")]),
            entity(
                "trigger_counter",
                &[("targetname", "c"), ("target", "d"), ("count", count)],
            ),
            entity("func_door", &[("targetname", "d")]),
        ]
    };
    let (mut three, _) = session("counter3", &counter("3"));
    let found = issues(&mut three);
    assert_eq!(kinds(&found), [(3, LogicKind::NeverReached)]);
    assert_eq!(
        found.issues[0].message,
        "entity 3 fires its targets at its 3th firing, and 2 entities fire it: its targets are \
         never fired"
    );
    let (mut two, _) = session("counter2", &counter("2"));
    assert_eq!(issues(&mut two).issues, []);
}

// spec FIRE-4.6: an issue about an entity the game spawned itself alone is not reported
#[test]
fn an_issue_names_the_maps_own_entities() {
    let map = [
        entity("func_button", &[("target", "n"), ("delay", "0.5")]),
        entity("info_null", &[("targetname", "n")]),
    ];
    let (mut session, _) = session("own", &map);
    let found = issues(&mut session);
    assert_eq!(kinds(&found), [(2, LogicKind::NothingToUse)]);
}

// spec FIRE-5.3: the issues join map.issues until the map changes
#[test]
fn the_issues_join_the_maps_until_it_changes() {
    let map = [
        entity("func_button", &[("target", "n")]),
        entity("info_null", &[("targetname", "n")]),
    ];
    let (mut session, _) = session("join", &map);
    let listed = |session: &mut Session<Workbench<FsLoader>>| {
        let found: MapIssues =
            serde_json::from_value(session.execute(MAP_ISSUES, json!({})).unwrap()).unwrap();
        found
            .issues
            .iter()
            .filter(|issue| issue.message.contains("has no use function"))
            .map(|issue| issue.entity.map(|entity| entity.get()))
            .collect::<Vec<_>>()
    };
    assert_eq!(listed(&mut session), [], "not before it has run");
    issues(&mut session);
    assert_eq!(listed(&mut session), [Some(2)]);
    session
        .execute(
            MAP_SET_KEY,
            json!({ "entity": 1, "key": "target", "value": "other" }),
        )
        .unwrap();
    assert_eq!(listed(&mut session), [], "the map changed");
}

/// A box brush from `min` to `max`, as a `.map` writes it.
fn box_brush(min: [i32; 3], max: [i32; 3]) -> String {
    let [x0, y0, z0] = min;
    let [x1, y1, z1] = max;
    format!(
        "{{\n( {x0} 0 0 ) ( {x0} 1 0 ) ( {x0} 0 1 ) wall 0 0 0 1 1\n\
         ( {x1} 0 0 ) ( {x1} 0 1 ) ( {x1} 1 0 ) wall 0 0 0 1 1\n\
         ( 0 {y0} 0 ) ( 0 {y0} 1 ) ( 1 {y0} 0 ) wall 0 0 0 1 1\n\
         ( 0 {y1} 0 ) ( 1 {y1} 0 ) ( 0 {y1} 1 ) wall 0 0 0 1 1\n\
         ( 0 0 {z0} ) ( 1 0 {z0} ) ( 0 1 {z0} ) wall 0 0 0 1 1\n\
         ( 0 0 {z1} ) ( 0 1 {z1} ) ( 1 0 {z1} ) wall 0 0 0 1 1\n}}\n"
    )
}

// spec FIRE-5.4: a map is compiled in memory to be spawned, so a brush entity, whose model the
// program sets, is fired in a map with no BSP beside it
#[test]
fn a_brush_entity_of_an_uncompiled_map_is_fired() {
    let door = format!(
        "{{\n\"classname\" \"func_door\"\n\"targetname\" \"d\"\n{}}}\n",
        box_brush([0, 0, 0], [16, 64, 64])
    );
    let floor = format!(
        "{{\n\"classname\" \"worldspawn\"\n{}}}\n",
        box_brush([-128, -128, -16], [128, 128, 0])
    );
    let (mut session, dir) = session("brush", &[]);
    let path = dir.join("maps/brush.map");
    std::fs::write(
        &path,
        format!("{floor}{}{door}", entity("func_button", &[("target", "d")])),
    )
    .unwrap();
    session.execute(MAP_OPEN, json!({ "path": path })).unwrap();
    let fired = fire(&mut session, json!({ "entity": 1 }));
    assert!(fired.events.iter().any(|event| matches!(event,
        FiredEvent::Ran { function, entity, .. }
            if function == "door_use" && entity.entity.map(|place| place.get()) == Some(2))));
    assert!(!dir.join("maps/brush.bsp").exists(), "nothing is written");
}
