// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use qc_progs::Progs;

use super::{
    Callback, FIRE_MAX_SECONDS, FireBy, FireError, FireOptions, FireTrace, TraceEnd, TraceEvent,
};
use crate::scenario::{SCN_FRAMETIME, Seconds};
use crate::world::tests::{Running, entity, ledge, running};
use crate::world::{CLIENT_EDICT, HostCommand, QcValue};

/// A program with id's way of firing targets (`subs.qc`, `SUB_UseTargets`) and a class for
/// each thing a trace shows.
const TRIGGERS: &str = r#"
entity(entity start, .string fld, string match) find = #18;
void(string s) changelevel = #70;
.string killtarget;
.float delay;
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
    if (activator.classname == "player" && self.message != "")
        centerprint(activator, self.message);
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
void() button_use = { self.enemy = other; activator = other; SUB_UseTargets(); };
void() func_button = { self.use = button_use; };
void() door_done = { self.frame = 1; };
void() door_use = {
    self.think = door_done;
    self.nextthink = time + 0.5;
    sound(self, 0, "doors/open.wav", 1, 1);
};
void() func_door = { precache_sound("doors/open.wav"); self.use = door_use; };
void() info_null = {};
void() item_touch = { activator = other; SUB_UseTargets(); remove(self); };
void() item_key = { self.touch = item_touch; };
void() exit_use = { changelevel("next"); };
void() trigger_exit = { self.use = exit_use; };
.void() think1;
void() bad_use = { self.think1(); };
void() trigger_bad = { self.use = bad_use; };
void() tick = { self.count = self.count + 1; self.nextthink = time + 0.1; };
void() misc_clock = { self.think = tick; self.nextthink = time + 0.1; };
void() clock_use = { self.think = tick; self.nextthink = time + 0.1; };
void() trigger_clock = { self.use = clock_use; };
void() teleport_use = {
    local entity t;
    t = find(world, targetname, self.target);
    other.origin = t.origin;
};
void() trigger_teleport = { self.use = teleport_use; };
void() PutClientInServer = { self.classname = "player"; self.health = 100; };
"#;

/// The edict of the map's first entity after the world's and the client's (EDICT-4).
const FIRST: usize = 2;

fn fired(entities: &str, n: usize, options: FireOptions) -> (Running, FireTrace) {
    let mut run = running(&ledge(), TRIGGERS, entities, Vec::new());
    let bytes = run.progs.clone();
    let progs = Progs::read(&bytes).unwrap();
    let trace = run.world.fire(&progs, n, options).unwrap();
    (run, trace)
}

fn button_and(second: &str) -> String {
    format!(
        "{}{second}",
        entity("func_button", "0 0 24", "\"target\" \"d\"\n")
    )
}

fn ran(trace: &FireTrace) -> Vec<(Callback, usize, &str)> {
    trace
        .events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::Ran {
                callback,
                entity,
                function,
                ..
            } => Some((*callback, *entity, function.as_str())),
            _ => None,
        })
        .collect()
}

// spec FIRE-1.1, FIRE-2.1, FIRE-2.3, FIRE-2.6: a button fired runs its use, looks its target up,
// runs the door's use and its think, with the door's sound
#[test]
fn a_button_fired_opens_its_door() {
    let map = button_and(&entity("func_door", "0 0 24", "\"targetname\" \"d\"\n"));
    let (_, trace) = fired(&map, FIRST, FireOptions::default());
    assert_eq!(trace.fired_by, Callback::Use);
    assert_eq!(
        ran(&trace),
        [
            (Callback::Use, FIRST, "button_use"),
            (Callback::Use, FIRST + 1, "door_use"),
            (Callback::Think, FIRST + 1, "door_done"),
        ]
    );
    let looked: Vec<_> = trace
        .events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::Looked {
                by,
                name,
                from_start,
                found,
                ..
            } => Some((*by, name.as_str(), *from_start, *found)),
            _ => None,
        })
        .collect();
    assert_eq!(
        looked,
        [
            (FIRST, "d", true, Some(FIRST + 1)),
            (FIRST, "d", false, None)
        ],
        "the lookup from the world that found the door, then the one after it that found no more"
    );
    assert!(trace.events.iter().any(|event| matches!(event,
        TraceEvent::Sound { entity, sample, .. } if *entity == FIRST + 1 && sample == "doors/open.wav")));
    let classnames: Vec<_> = trace
        .events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::Ran { classname, .. } => Some(classname.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(classnames, ["func_button", "func_door", "func_door"]);
}

// spec FIRE-2.7: the fields a reached entity had written are listed with their values then
#[test]
fn the_fields_written_are_listed() {
    let map = button_and(&entity("func_door", "0 0 24", "\"targetname\" \"d\"\n"));
    let (_, trace) = fired(&map, FIRST, FireOptions::default());
    let frame = trace.events.iter().rev().find_map(|event| match event {
        TraceEvent::Changed { entity, fields, .. } if *entity == FIRST + 1 => fields
            .iter()
            .find(|field| field.name == "frame")
            .map(|field| field.value.clone()),
        _ => None,
    });
    assert_eq!(frame, Some(QcValue::Float(1.0)));
}

// spec FIRE-3.1, FIRE-3.2: frames run until nothing reached has a think pending
#[test]
fn the_trace_ends_quiet_once_the_door_has_thought() {
    let map = button_and(&entity("func_door", "0 0 24", "\"targetname\" \"d\"\n"));
    let (_, trace) = fired(&map, FIRST, FireOptions::default());
    assert_eq!(trace.end, TraceEnd::Quiet);
    assert!(
        trace.seconds >= 0.5 && trace.seconds < 0.6,
        "the door thinks half a second on: {}",
        trace.seconds
    );
}

// spec FIRE-2.5: a delay is a spawned entity that fires later, in simulated time, and is removed
#[test]
fn a_delay_is_honoured_in_simulated_time() {
    let map = format!(
        "{}{}",
        entity(
            "func_button",
            "0 0 24",
            "\"target\" \"d\"\n\"delay\" \"2\"\n"
        ),
        entity("func_door", "0 0 24", "\"targetname\" \"d\"\n")
    );
    let (_, trace) = fired(&map, FIRST, FireOptions::default());
    let spawned = trace.events.iter().find_map(|event| match event {
        TraceEvent::Spawned {
            entity, classname, ..
        } => Some((*entity, classname.as_str())),
        _ => None,
    });
    let (delayed, classname) = spawned.unwrap();
    assert_eq!(classname, "DelayedUse");
    let times: Vec<f32> = trace
        .events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::Ran { time, function, .. } if function == "door_use" => Some(*time),
            _ => None,
        })
        .collect();
    let fired_at = match trace.events[0] {
        TraceEvent::Ran { time, .. } => time,
        _ => panic!("the first event is the button's use"),
    };
    assert_eq!(times.len(), 1);
    assert!(
        (times[0] - fired_at - 2.0).abs() < 0.05,
        "the door is used 2 seconds after the firing: {} then {}",
        fired_at,
        times[0]
    );
    assert!(trace.events.iter().any(|event| matches!(event,
        TraceEvent::Removed { entity, classname, .. } if *entity == delayed && classname == "DelayedUse")));
}

// spec FIRE-2.5: a killtarget removes its entities, and id's code then fires no target
#[test]
fn a_killtarget_removes_and_fires_no_target() {
    let map = format!(
        "{}{}{}",
        entity(
            "func_button",
            "0 0 24",
            "\"target\" \"d\"\n\"killtarget\" \"k\"\n"
        ),
        entity("func_door", "0 0 24", "\"targetname\" \"d\"\n"),
        entity("info_null", "0 0 24", "\"targetname\" \"k\"\n")
    );
    let (run, trace) = fired(&map, FIRST, FireOptions::default());
    assert_eq!(ran(&trace), [(Callback::Use, FIRST, "button_use")]);
    assert!(trace.events.iter().any(|event| matches!(event,
        TraceEvent::Removed { entity, classname, .. } if *entity == FIRST + 2 && classname == "info_null")));
    assert!(!run.world.in_use(FIRST + 2));
}

// spec FIRE-2.4: a target with no use, of which only use was read, answered nothing
#[test]
fn a_target_with_no_use_answers_nothing() {
    let map = button_and(&entity("info_null", "0 0 24", "\"targetname\" \"d\"\n"));
    let (_, trace) = fired(&map, FIRST, FireOptions::default());
    let unanswered: Vec<_> = trace
        .events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::Unanswered {
                by,
                entity,
                classname,
                ..
            } => Some((*by, *entity, classname.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(unanswered, [(FIRST, FIRST + 1, "info_null")]);
}

// spec FIRE-2.4: a target with a use, and one looked up and read, are answered
#[test]
fn a_door_and_a_teleport_destination_are_answered() {
    let door = button_and(&entity("func_door", "0 0 24", "\"targetname\" \"d\"\n"));
    let teleport = format!(
        "{}{}",
        entity("trigger_teleport", "0 0 24", "\"target\" \"d\"\n"),
        entity("info_null", "8 0 24", "\"targetname\" \"d\"\n")
    );
    for map in [door, teleport] {
        let (_, trace) = fired(&map, FIRST, FireOptions::default());
        assert!(
            !trace
                .events
                .iter()
                .any(|event| matches!(event, TraceEvent::Unanswered { .. })),
            "{:?}",
            trace.events
        );
    }
}

// spec FIRE-1.2: touch fires an entity that has no use, and either takes it
#[test]
fn an_item_is_fired_by_its_touch() {
    let map = format!(
        "{}{}",
        entity("item_key", "0 0 24", "\"target\" \"d\"\n"),
        entity("func_door", "0 0 24", "\"targetname\" \"d\"\n")
    );
    for by in [FireBy::Either, FireBy::Touch] {
        let options = FireOptions {
            by,
            ..FireOptions::default()
        };
        let (_, trace) = fired(&map, FIRST, options);
        assert_eq!(trace.fired_by, Callback::Touch);
        assert_eq!(
            ran(&trace)[..2],
            [
                (Callback::Touch, FIRST, "item_touch"),
                (Callback::Use, FIRST + 1, "door_use")
            ]
        );
    }
}

// spec FIRE-1.1, FIRE-1.3: an edict not in use and an entity with no such function are refused
#[test]
fn what_cannot_be_fired_is_refused() {
    let map = format!(
        "{}{}",
        entity("info_null", "0 0 24", ""),
        entity("func_button", "0 0 24", "")
    );
    let mut run = running(&ledge(), TRIGGERS, &map, Vec::new());
    let bytes = run.progs.clone();
    let progs = Progs::read(&bytes).unwrap();
    let fire = |run: &mut Running, n, by| {
        run.world.fire(
            &progs,
            n,
            FireOptions {
                by,
                ..FireOptions::default()
            },
        )
    };
    assert_eq!(
        fire(&mut run, 99, FireBy::Either),
        Err(FireError::NotInUse(99))
    );
    for by in [FireBy::Either, FireBy::Use, FireBy::Touch] {
        assert_eq!(
            fire(&mut run, FIRST, by),
            Err(FireError::NothingToFire { entity: FIRST, by })
        );
    }
    assert_eq!(
        fire(&mut run, FIRST + 1, FireBy::Touch),
        Err(FireError::NothingToFire {
            entity: FIRST + 1,
            by: FireBy::Touch
        })
    );
    assert_eq!(
        FireError::NothingToFire {
            entity: 3,
            by: FireBy::Touch
        }
        .to_string(),
        "edict 3 has no touch function"
    );
}

// spec FIRE-1.4, FIRE-2.6: the player in the game is other and activator, and reads the message
#[test]
fn the_player_is_other_and_activator() {
    let map = format!(
        "{}{}",
        entity(
            "func_button",
            "0 0 24",
            "\"target\" \"d\"\n\"message\" \"open\"\n"
        ),
        entity("func_door", "0 0 24", "\"targetname\" \"d\"\n")
    );
    let mut run = running(&ledge(), TRIGGERS, &map, Vec::new());
    let bytes = run.progs.clone();
    let progs = Progs::read(&bytes).unwrap();
    run.world.enter(&progs, SCN_FRAMETIME).unwrap();
    let trace = run
        .world
        .fire(&progs, FIRST, FireOptions::default())
        .unwrap();
    let enemy = run
        .world
        .edict_fields(&progs, FIRST)
        .unwrap()
        .into_iter()
        .find(|field| field.name == "enemy")
        .map(|field| field.value);
    assert_eq!(enemy, Some(QcValue::Entity(CLIENT_EDICT)));
    assert!(
        trace
            .events
            .iter()
            .any(|event| matches!(event, TraceEvent::Print { text, .. } if text == "open"))
    );
}

// spec FIRE-1.4: with no client in the game, other is the world
#[test]
fn with_no_player_other_is_the_world() {
    let map = button_and("");
    let (run, _) = fired(&map, FIRST, FireOptions::default());
    assert_eq!(run.world.entity_field(FIRST, "enemy"), 0);
}

// spec FIRE-3.3: a command left for the host ends the trace and is not run
#[test]
fn a_changelevel_ends_the_trace() {
    let map = entity("trigger_exit", "0 0 24", "");
    let (run, trace) = fired(&map, FIRST, FireOptions::default());
    assert_eq!(
        trace.end,
        TraceEnd::Host(HostCommand::Changelevel("next".to_owned()))
    );
    assert_eq!(run.world.map(), "test");
}

// spec FIRE-3.4: a function that fails ends the trace with the error, its events kept
#[test]
fn an_error_ends_the_trace() {
    let map = entity("trigger_bad", "0 0 24", "");
    let (_, trace) = fired(&map, FIRST, FireOptions::default());
    assert!(
        matches!(&trace.end, TraceEnd::Error(text) if text.contains("NULL function")),
        "{:?}",
        trace.end
    );
    assert_eq!(ran(&trace), [(Callback::Use, FIRST, "bad_use")]);
}

// spec FIRE-3.1: a chain that never rests runs the length given, and no longer than the most
#[test]
fn the_trace_ends_at_its_length() {
    let map = entity("trigger_clock", "0 0 24", "");
    let options = FireOptions {
        length: Seconds::new(1.0),
        ..FireOptions::default()
    };
    let (_, trace) = fired(&map, FIRST, options);
    assert_eq!(trace.end, TraceEnd::Length);
    assert!(trace.seconds >= 1.0 && trace.seconds < 1.0 + SCN_FRAMETIME.get());
    let thinks = ran(&trace)
        .iter()
        .filter(|(callback, ..)| *callback == Callback::Think)
        .count();
    assert!((9..=10).contains(&thinks), "{thinks} thinks in a second");

    let long = FireOptions {
        length: Seconds::new(1.0e9),
        frametime: Seconds::new(50.0),
        ..FireOptions::default()
    };
    let (_, trace) = fired(&map, FIRST, long);
    assert_eq!(trace.end, TraceEnd::Length);
    assert!(trace.seconds <= FIRE_MAX_SECONDS.get() + 50.0);
}

// spec FIRE-2.2: the thinks of an entity the firing did not reach are not in the trace
#[test]
fn what_the_firing_did_not_reach_is_not_recorded() {
    let map = format!(
        "{}{}{}",
        entity("func_button", "0 0 24", "\"target\" \"d\"\n"),
        entity("func_door", "0 0 24", "\"targetname\" \"d\"\n"),
        entity("misc_clock", "0 0 24", "")
    );
    let (run, trace) = fired(&map, FIRST, FireOptions::default());
    let progs = Progs::read(&run.progs).unwrap();
    let count = run
        .world
        .edict_fields(&progs, FIRST + 2)
        .unwrap()
        .into_iter()
        .find(|field| field.name == "count")
        .map(|field| field.value);
    assert!(
        matches!(count, Some(QcValue::Float(count)) if count >= 4.0),
        "the clock ran through the trace: {count:?}"
    );
    assert!(trace.events.iter().all(|event| match event {
        TraceEvent::Ran { entity, .. } | TraceEvent::Changed { entity, .. } => *entity != FIRST + 2,
        _ => true,
    }));
}

// spec FIRE-2.1: a function called that is none of the entity's four callbacks is not recorded
#[test]
fn a_helper_is_not_a_callback() {
    let map = button_and("");
    let (_, trace) = fired(&map, FIRST, FireOptions::default());
    assert_eq!(ran(&trace), [(Callback::Use, FIRST, "button_use")]);
}

// spec FIRE-3.5: the same world fired the same way gives the same trace
#[test]
fn a_trace_is_the_same_every_time() {
    let map = format!(
        "{}{}",
        entity(
            "func_button",
            "0 0 24",
            "\"target\" \"d\"\n\"delay\" \"1\"\n"
        ),
        entity("func_door", "0 0 24", "\"targetname\" \"d\"\n")
    );
    let (_, first) = fired(&map, FIRST, FireOptions::default());
    let (_, second) = fired(&map, FIRST, FireOptions::default());
    assert_eq!(first, second);
    assert!(first.events.len() > 4);
}
