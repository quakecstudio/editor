// SPDX-License-Identifier: GPL-3.0-or-later

use super::{Button, Input, MoveSettings};

/// id's defaults (CON-7.1).
fn defaults() -> MoveSettings {
    MoveSettings {
        upspeed: 200.0,
        forwardspeed: 200.0,
        backspeed: 200.0,
        sidespeed: 350.0,
        movespeedkey: 2.0,
        yawspeed: 140.0,
        pitchspeed: 150.0,
        anglespeedkey: 1.5,
        sensitivity: 3.0,
        m_pitch: 0.022,
        m_yaw: 0.022,
        m_forward: 1.0,
        m_side: 0.8,
        lookspring: 0.0,
        lookstrafe: 0.0,
    }
}

// spec INPUT-2.1: two keys hold a button, a third is refused, it goes up when both are up, a
// release with no key lets it go, and -mlook with lookspring starts the drift
#[test]
fn keys_hold_and_release_buttons() {
    let mut input = Input::new();
    assert_eq!(input.press(Button::Forward, Some(119)), None);
    assert_eq!(input.press(Button::Forward, Some(119)), None, "a repeat");
    assert_eq!(input.press(Button::Forward, Some(200)), None);
    assert_eq!(
        input.press(Button::Forward, Some(5)),
        Some("Three keys down for a button!\n")
    );
    input.release(Button::Forward, Some(119), 0.0);
    assert!(input.held(Button::Forward), "the other key still holds it");
    input.release(Button::Forward, Some(7), 0.0);
    input.release(Button::Forward, Some(200), 0.0);
    assert!(!input.held(Button::Forward));
    input.press(Button::Jump, None);
    input.press(Button::Jump, Some(32));
    input.release(Button::Jump, None, 0.0);
    assert!(!input.held(Button::Jump), "typed -jump lets every key go");
    input.press(Button::MLook, Some(92));
    assert!(!input.release(Button::MLook, Some(92), 0.0).start);
    input.press(Button::MLook, Some(92));
    assert!(input.release(Button::MLook, Some(92), 1.0).start);
    assert_eq!(Button::named("moveup"), Some(Button::Up));
    assert_eq!(Button::named("nothing"), None);
}

// spec INPUT-2.2: a press held is 0.5 its first frame and 1 after; pressed and released in one
// frame 0.25; released 0
// spec INPUT-2.4: the speeds, strafe, klook, the speed key and the mouse
// spec CON-7.4: the move's speeds, the run factor and the mouse's turn come from the settings
#[test]
fn the_buttons_make_the_move() {
    let settings = defaults();
    let mut input = Input::new();
    let mut angles = [0.0; 3];
    input.press(Button::Forward, Some(119));
    let (cmd, _) = input.frame(&settings, 0.1, &mut angles, [0.0; 2]);
    assert_eq!(cmd.forward, 100.0, "0.5 its first frame");
    let (cmd, _) = input.frame(&settings, 0.1, &mut angles, [0.0; 2]);
    assert_eq!(cmd.forward, 200.0);
    input.press(Button::Speed, Some(134));
    input.press(Button::MoveLeft, Some(44));
    input.release(Button::MoveLeft, Some(44), 0.0);
    let (cmd, _) = input.frame(&settings, 0.1, &mut angles, [0.0; 2]);
    assert_eq!(cmd.forward, 400.0);
    assert_eq!(
        cmd.side,
        -350.0 * 0.25 * 2.0,
        "pressed and released in a frame"
    );
    input.release(Button::Forward, Some(119), 0.0);
    let (cmd, _) = input.frame(&settings, 0.1, &mut angles, [0.0; 2]);
    assert_eq!(cmd.forward, 0.0, "released");
    input.release(Button::Speed, Some(134), 0.0);
    input.frame(&settings, 0.1, &mut angles, [0.0; 2]);
    // strafe turns left and right into moves; klook stops forward moving.
    input.press(Button::Strafe, Some(132));
    input.press(Button::Right, Some(131));
    input.frame(&settings, 0.1, &mut angles, [0.0; 2]);
    let (cmd, _) = input.frame(&settings, 0.1, &mut angles, [0.0; 2]);
    assert_eq!((cmd.side, angles[1]), (350.0, 0.0));
    let mut klook = Input::new();
    klook.press(Button::KLook, Some(1));
    klook.press(Button::Forward, Some(119));
    let mut look = [0.0; 3];
    let (cmd, drift) = klook.frame(&settings, 0.1, &mut look, [0.0; 2]);
    assert_eq!(cmd.forward, 0.0);
    assert!(drift.stop);
    assert!(look[0] < 0.0, "forward pitches up with klook");
    // The mouse: no mlook turns the yaw and moves forward; mlook pitches.
    let mut mouse = Input::new();
    let mut view = [0.0; 3];
    let (cmd, _) = mouse.frame(&settings, 0.0, &mut view, [10.0, 5.0]);
    // IN_MouseMove takes the yaw from where it is, with no anglemod.
    assert!((view[1] + 0.66).abs() < 1e-4);
    assert_eq!(cmd.forward, -15.0);
    mouse.press(Button::MLook, None);
    let (cmd, drift) = mouse.frame(&settings, 0.0, &mut view, [0.0, 100.0]);
    assert_eq!(cmd.forward, 0.0);
    assert!(drift.stop);
    assert!((view[0] - 6.6).abs() < 1e-4);
    let lookstrafe = MoveSettings {
        lookstrafe: 1.0,
        ..settings
    };
    let (cmd, _) = mouse.frame(&lookstrafe, 0.0, &mut view, [10.0, 0.0]);
    assert_eq!(cmd.side, 0.8 * 30.0);
    assert_eq!(cmd.angles, view);
}

// spec CON-7.4: the options menu's Always Run, Mouse Speed and Invert Mouse change the move:
// `cl_forwardspeed` and `cl_backspeed` 400 run both ways, `sensitivity` scales the mouse's turn
// and move, and a negative `m_pitch` turns mouse look's pitch the other way
#[test]
fn the_options_change_the_move() {
    let run = MoveSettings {
        forwardspeed: 400.0,
        backspeed: 400.0,
        ..defaults()
    };
    let mut angles = [0.0; 3];
    let mut input = Input::new();
    input.press(Button::Back, Some(115));
    input.frame(&run, 0.1, &mut angles, [0.0; 2]);
    let (cmd, _) = input.frame(&run, 0.1, &mut angles, [0.0; 2]);
    assert_eq!(cmd.forward, -400.0);
    let (walk, _) = input.frame(&defaults(), 0.1, &mut angles, [0.0; 2]);
    assert_eq!(walk.forward, -200.0);

    let fast = MoveSettings {
        sensitivity: 6.0,
        ..defaults()
    };
    let mut view = [0.0; 3];
    let (cmd, _) = Input::new().frame(&fast, 0.0, &mut view, [10.0, 5.0]);
    assert!((view[1] + 1.32).abs() < 1e-4, "{view:?}");
    assert_eq!(cmd.forward, -30.0);

    let inverted = MoveSettings {
        m_pitch: -0.022,
        ..defaults()
    };
    let mut mouse = Input::new();
    mouse.press(Button::MLook, None);
    let mut view = [0.0; 3];
    mouse.frame(&inverted, 0.0, &mut view, [0.0, 100.0]);
    assert!((view[0] + 6.6).abs() < 1e-4, "{view:?}");
}

/// The yaw `a` as `anglemod` keeps it.
fn anglemod_of(a: f32) -> f32 {
    super::anglemod(a)
}

// spec INPUT-2.3: the keys turn by frametime and the yaw and pitch speeds, faster with speed;
// the pitch stays within -70 and 80; look keys stop the drift
#[test]
fn the_keys_turn_the_view() {
    let settings = defaults();
    let mut input = Input::new();
    let mut angles = [0.0; 3];
    input.press(Button::Left, Some(130));
    input.frame(&settings, 0.5, &mut angles, [0.0; 2]);
    input.frame(&settings, 0.5, &mut angles, [0.0; 2]);
    assert!((angles[1] - anglemod_of(35.0 + 70.0)).abs() < 1e-3);
    input.release(Button::Left, Some(130), 0.0);
    input.frame(&settings, 0.5, &mut angles, [0.0; 2]);
    input.press(Button::LookDown, Some(1));
    input.press(Button::Speed, Some(2));
    let (_, drift) = input.frame(&settings, 1.0, &mut angles, [0.0; 2]);
    assert!(drift.stop);
    assert_eq!(angles[0], 80.0, "clamped");
    let mut roll = [0.0, 0.0, 90.0];
    Input::new().frame(&settings, 0.0, &mut roll, [0.0; 2]);
    assert_eq!(roll[2], 50.0);
    assert_eq!(anglemod_of(-90.0), 270.0);
}

// spec INPUT-2.5: attack and jump fire for a press too short for a frame, once; the impulse is
// sent once
#[test]
fn the_buttons_and_impulse_are_sent() {
    let settings = defaults();
    let mut input = Input::new();
    let mut angles = [0.0; 3];
    input.press(Button::Attack, Some(200));
    input.release(Button::Attack, Some(200), 0.0);
    input.press(Button::Jump, Some(32));
    input.set_impulse(7);
    let (cmd, _) = input.frame(&settings, 0.1, &mut angles, [0.0; 2]);
    assert_eq!((cmd.buttons, cmd.impulse), (3, 7));
    let (cmd, _) = input.frame(&settings, 0.1, &mut angles, [0.0; 2]);
    assert_eq!(
        (cmd.buttons, cmd.impulse),
        (2, 0),
        "attack once; jump still held"
    );
}
