// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::float_cmp_const,
    reason = "test code: a failed unwrap, index or match is a failed test; the values compared are \
              exact"
)]

use qc_progs::Progs;

use super::{Edict, Expected, Op, Report, SCN_FRAMETIME, Scenario, Step, Target};
use crate::LoadOptions;
use crate::world::tests::{Running, ledge, player_progs, running_from};

/// The progs the runs play: a walking player whose `PutClientInServer` sets its health to
/// 100, an `item_marker` in the map, a global counting server frames (the settle's two among
/// them) and a global drawing the random source each frame; the progs stops with a run error
/// when its count reaches `fail`, never when it is 0.
fn progs(fail: u32) -> Vec<u8> {
    let more = format!(
        "void(string e) error = #10;\nfloat frames;\nfloat drawn;\nfloat fail = {fail};\n\
        void() StartFrame = {{ frames = frames + 1; drawn = random(); \
        if (frames == fail) error(\"stopped\"); }};\n\
        void() item_marker = {{ self.count = 3; }};\n"
    );
    player_progs("", "", &more)
}

/// A world of [`progs`] failing in frame `fail` on the ledge with a marker, settled, not yet
/// connected, with `options`.
fn world(options: LoadOptions, fail: u32) -> Running {
    let entities = "{\n\"classname\" \"item_marker\"\n\"origin\" \"8 8 8\"\n}\n";
    let mut run = running_from(&ledge(), progs(fail), entities, Vec::new(), options);
    let progs = Progs::read(&run.progs).unwrap();
    run.world.settle(&progs).unwrap();
    run
}

/// `text` played in a fresh world with the scenario's seed.
fn played(text: &str) -> Report {
    let scenario = Scenario::parse(text).unwrap();
    let mut run = world(
        LoadOptions {
            seed: scenario.seed,
            ..LoadOptions::default()
        },
        0,
    );
    let progs = Progs::read(&run.progs).unwrap();
    scenario.run(&mut run.world, &progs)
}

// spec SCN-1.1: blank lines and # lines are skipped, a \r before a line's end is left out, and
// an unknown word is refused with its line
// spec SCN-1.2: the route's lines read as DIFF-16.1's, the last map counting and the frame
// 1/72 s when no frametime line is given
#[test]
fn a_route_is_a_scenario() {
    let text = "# a route\r\n\nengine -basedir x\nmap e1m1\nmap start\ncmd god\n\
        console sv_gravity 100\nmove 1 2 3 200 -350 0 1 2\nframes 3\n";
    let scenario = Scenario::parse(text).unwrap();
    assert_eq!(scenario.map, "start");
    assert_eq!(scenario.engine, ["-basedir x"]);
    assert_eq!(scenario.frametime, SCN_FRAMETIME);
    assert_eq!(scenario.seed, None);
    assert!(matches!(&scenario.steps[0], Step::Command(c) if c == "god"));
    assert!(matches!(&scenario.steps[1], Step::Console(c) if c == "sv_gravity 100"));
    let Step::Move(cmd) = scenario.steps[2] else {
        panic!("not a move");
    };
    assert_eq!(cmd.angles, [1.0, 2.0, 3.0]);
    assert_eq!(
        (cmd.forward, cmd.side, cmd.buttons, cmd.impulse),
        (200.0, -350.0, 1, 2)
    );
    assert_eq!(scenario.steps[3], Step::Frames(3));
    for (bad, line) in [
        ("map a\nfly\n", 2),
        ("map a\nframes 0\n", 2),
        ("map a\nmove 1 2 3\n", 2),
        ("map a\nmove 0 0 0 40000 0 0 0 0\n", 2),
        ("map a\nframetime 2\n", 2),
        ("map a\nframetime 0\n", 2),
    ] {
        assert_eq!(Scenario::parse(bad).unwrap_err().line, line, "{bad:?}");
    }
    let none = Scenario::parse("frames 1\n").unwrap_err();
    assert_eq!((none.line, none.to_string()), (0, "no map line".to_owned()));
    assert!(
        Scenario::parse("map a\nfly\n")
            .unwrap_err()
            .to_string()
            .starts_with("line 2: ")
    );
}

// spec SCN-1.3: one seed, a whole number from 0 to 4294967295
// spec SCN-1.4: one start, x y z yaw
// spec SCN-1.6: a screenshot is named with letters, digits, - and _
#[test]
fn seed_start_and_screenshot_lines() {
    let scenario =
        Scenario::parse("map a\nseed 4294967295\nstart 1 2 3.5 90\nscreenshot hall-2_b\n").unwrap();
    assert_eq!(scenario.seed, Some(u32::MAX));
    let start = scenario.start.unwrap();
    assert_eq!((start.origin, start.yaw), ([1.0, 2.0, 3.5], 90.0));
    assert_eq!(scenario.steps, [Step::Screenshot("hall-2_b".to_owned())]);
    for bad in [
        "map a\nseed 1\nseed 2\n",
        "map a\nseed -1\n",
        "map a\nseed 4294967296\n",
        "map a\nstart 1 2 3\n",
        "map a\nstart 1 2 3 4\nstart 1 2 3 4\n",
        "map a\nscreenshot\n",
        "map a\nscreenshot a/b\n",
    ] {
        assert_eq!(
            Scenario::parse(bad).unwrap_err().line,
            bad.lines().count(),
            "{bad:?}"
        );
    }
}

// spec SCN-1.5: a target is a global or a field of an edict by number, player or classname;
// an op compares, the ordering ones floats only; a value is a number, a vector, a string or an
// edict
#[test]
fn assertions_read_their_target_op_and_value() {
    let scenario = Scenario::parse(
        "map a\nexpect global frames >= 4\nexpect field player origin == '1 2 3'\n\
         expect field classname=item_marker count != 2\nexpect field 3 netname == \"x y\"\n\
         expect field 0 enemy == #1\n",
    )
    .unwrap();
    let expectations: Vec<_> = scenario
        .steps
        .iter()
        .map(|step| match step {
            Step::Expect(e) => (e.line, e.target.clone(), e.op, e.value.clone()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        expectations,
        [
            (
                2,
                Target::Global("frames".into()),
                Op::Ge,
                Expected::Float(4.0)
            ),
            (
                3,
                Target::Field(Edict::Player, "origin".into()),
                Op::Eq,
                Expected::Vector([1.0, 2.0, 3.0])
            ),
            (
                4,
                Target::Field(Edict::Classname("item_marker".into()), "count".into()),
                Op::Ne,
                Expected::Float(2.0)
            ),
            (
                5,
                Target::Field(Edict::Number(3), "netname".into()),
                Op::Eq,
                Expected::String("x y".into())
            ),
            (
                6,
                Target::Field(Edict::Number(0), "enemy".into()),
                Op::Eq,
                Expected::Entity(1)
            ),
        ]
    );
    for bad in [
        "map a\nexpect global frames\n",
        "map a\nexpect global frames ~ 1\n",
        "map a\nexpect global frames < '1 2 3'\n",
        "map a\nexpect global name > \"a\"\n",
        "map a\nexpect field player\n",
        "map a\nexpect field monster health == 1\n",
        "map a\nexpect field classname= health == 1\n",
        "map a\nexpect field 1 health == lots\n",
        "map a\nexpect field 1 origin == '1 2'\n",
        "map a\nexpect edict 1 health == 1\n",
    ] {
        assert_eq!(Scenario::parse(bad).unwrap_err().line, 2, "{bad:?}");
    }
}

// spec SCN-2.1: the player enters in frame 4 and the start puts it there before frame 5;
// frames counts every frame run
// spec SCN-1.4: the start sets the origin and the yaw and stops the player
#[test]
fn the_player_enters_and_starts_where_the_scenario_says() {
    let report = played(
        "map test\nstart 16 32 64 90\nframes 4\nexpect field player origin == '16 32 64'\n\
         expect field player angles == '0 90 0'\nexpect field player velocity == '0 0 0'\n\
         expect field player health == 100\nexpect global frames == 6\nframes 2\n\
         expect global frames == 8\n",
    );
    assert!(report.passed, "{report:?}");
    assert_eq!(report.frames, 6);
    assert_eq!(report.checks.len(), 6);
    assert!(report.error.is_none());
}

// spec SCN-2.2: a failed comparison gives the value read; a target with no value, or a value
// of another type, says so; a failed assertion does not stop the run
#[test]
fn failed_assertions_say_what_was_read() {
    let report = played(
        "map test\nframes 4\nexpect field player health < 50\nexpect global nosuch == 1\n\
         expect field 40 health == 1\nexpect field classname=monster_dog health == 1\n\
         expect field player health == '1 2 3'\nexpect field player nosuch == 1\n\
         expect field classname=item_marker count == 3\nframes 1\nexpect global frames == 7\n",
    );
    assert!(!report.passed);
    let found: Vec<(bool, &str)> = report
        .checks
        .iter()
        .map(|c| (c.passed, c.found.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (false, "100"),
            (false, "no global nosuch"),
            (false, "edict 40 is not in use"),
            (false, "no edict in use with classname monster_dog"),
            (false, "100, not of the type expected"),
            (false, "no field nosuch"),
            (true, "3"),
            (true, "7"),
        ]
    );
    assert_eq!(report.checks[0].line, 3);
    assert_eq!(report.checks[0].text, "field player health < 50");
    assert_eq!(report.frames, 5, "the run went on");
}

// spec SCN-2.3: a run error stops the run at its frame, and the assertions after it are not
// checked
#[test]
fn a_run_error_stops_the_run() {
    let text =
        "map test\nframes 4\nexpect global frames == 6\nframes 8\nexpect global frames == 14\n";
    let report = played(text);
    assert!(report.passed, "{report:?}");
    let scenario = Scenario::parse(
        "map test\nframes 4\nexpect global frames == 6\nframes 8\nexpect global frames == 14\n",
    )
    .unwrap();
    // The settle's two frames count too (MOVE-6.2): the progs' frame 12 is the run's 10.
    let mut run = world(LoadOptions::default(), 12);
    let progs = Progs::read(&run.progs).unwrap();
    let stopped = scenario.run(&mut run.world, &progs);
    assert!(!stopped.passed);
    assert_eq!(stopped.frames, 10, "{stopped:?}");
    assert!(stopped.error.as_deref().unwrap().starts_with("frame 10: "));
    assert_eq!(
        stopped.checks.len(),
        1,
        "the assertion after it is not checked"
    );
}

// spec SCN-1.3: the seed is where the server's random source starts; none is MOVE-6.1's 1
// spec SCN-2.5: the same scenario gives the same result on every run
#[test]
fn the_seed_decides_the_draws_and_runs_repeat() {
    let drawn = |seed: &str| {
        let report = played(&format!(
            "map test\n{seed}frames 6\nexpect global drawn == -1\n"
        ));
        report.checks[0].found.clone()
    };
    assert_eq!(drawn(""), drawn("seed 1\n"));
    assert_ne!(drawn(""), drawn("seed 7\n"));
    let text = "map test\nseed 42\nstart 0 0 64 45\nmove 0 45 0 200 0 0 0 0\nframes 30\n\
        expect field player origin == '0 0 0'\nexpect global drawn > 0\nscreenshot here\n";
    let first = played(text);
    for _ in 0..9 {
        assert_eq!(played(text), first);
    }
}

// spec SCN-1.6: a screenshot keeps the frame and the player's eye: its origin plus view_ofs,
// and its v_angle
// spec SCN-2.4: the result lists the screenshots
#[test]
fn a_screenshot_keeps_the_players_eye() {
    let report = played(
        "map test\nstart 16 32 64 90\nmove 10 90 0 0 0 0 0 0\nframes 5\nscreenshot hall\n\
         expect field player origin == '0 0 0'\n",
    );
    let shot = &report.screenshots[0];
    assert_eq!((shot.name.as_str(), shot.frame), ("hall", 5));
    assert_eq!(shot.angles[1], 90.0);
    let origin: Vec<f32> = report.checks[0]
        .found
        .trim_matches('\'')
        .split(' ')
        .map(|n| n.parse().unwrap())
        .collect();
    assert_eq!(shot.eye, [origin[0], origin[1], origin[2] + 22.0]);
}
