// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use qs_sim::{NamedValue, QcValue, State, UserCmd};

use super::{Route, Step, differences, on_map};

fn named(name: &str, value: QcValue) -> NamedValue {
    NamedValue {
        name: name.to_owned(),
        value,
    }
}

// spec DIFF-16.1: a route is read as the engine oracle reads its script
// spec DIFF-16.6: a console line is a step of its own, its text as written
#[test]
fn a_route_reads_as_the_oracle_reads_it() {
    let route = Route::parse(
        "# a comment\nengine -nosound\nmap start\nframetime 0.02\nmove 0 90 0 200 -350 0 1 2\n\
         frames 3\ncmd god\nconsole alias a \"sv_gravity 1\"; a\n",
    )
    .unwrap();
    assert_eq!(route.engine, ["-nosound"]);
    assert_eq!(route.map, "start");
    assert!((route.frametime - 0.02).abs() < f64::EPSILON);
    assert_eq!(
        route.steps,
        [
            Step::Move(UserCmd {
                angles: [0.0, 90.0, 0.0],
                forward: 200.0,
                side: -350.0,
                up: 0.0,
                buttons: 1,
                impulse: 2,
            }),
            Step::Frames(3),
            Step::Command("god".to_owned()),
            Step::Console("alias a \"sv_gravity 1\"; a".to_owned()),
        ]
    );
    assert!(Route::parse("frames 1\n").is_err(), "no map");
    assert!(Route::parse("map a\nframes 0\n").is_err());
    assert!(Route::parse("map a\nmove 1 2 3\n").is_err());
    assert!(Route::parse("map a\nmove 0 0 0 40000 0 0 0 0\n").is_err());
    assert!(Route::parse("map a\nframetime 2\n").is_err());
    assert!(Route::parse("map a\nfly\n").is_err());
}

// spec DIFF-16.3, DIFF-16.4: a frame's differences are named with both values, floats by
// their bits, at most 12, and an edict one side has not reached reads as zeros
#[test]
fn the_first_difference_is_named() {
    let state = |health: f32, edicts: usize| State {
        globals: vec![named("time", QcValue::Float(1.0))],
        edicts: (0..edicts)
            .map(|_| vec![named("health", QcValue::Float(health))])
            .collect(),
    };
    assert!(differences(&state(0.0, 2), &state(0.0, 2)).is_empty());
    assert!(
        differences(&state(0.0, 2), &state(0.0, 3)).is_empty(),
        "a zero edict past the end"
    );
    assert_eq!(
        differences(&state(5.0, 2), &state(6.0, 2)),
        [
            "edict 0 field health: id's server Float(5.0), qs-sim Float(6.0)",
            "edict 1 field health: id's server Float(5.0), qs-sim Float(6.0)"
        ]
    );
    assert_eq!(
        differences(&state(0.0, 2), &state(-0.0, 2)).len(),
        2,
        "by bits"
    );
    assert_eq!(differences(&state(5.0, 1), &state(5.0, 2)).len(), 1);
    assert_eq!(
        differences(&state(5.0, 20), &state(6.0, 20)).len(),
        12,
        "at most 12"
    );
}

// spec DIFF-16.5: a route on another map is the same script with its map line replaced
#[test]
fn a_route_on_another_map_replaces_its_map_line() {
    let text = "# start\nmap start\nframes 2\n";
    assert_eq!(on_map(text, "e1m1"), "# start\nmap e1m1\nframes 2\n");
    let route = Route::parse(&on_map(text, "dm3")).unwrap();
    assert_eq!(route.map, "dm3");
    assert_eq!(route.steps, [Step::Frames(2)]);
}
