// SPDX-License-Identifier: GPL-3.0-or-later

use eframe::egui;
use qs_client::KeyDest;
use qs_sim::keys::{
    K_ALT, K_CTRL, K_ENTER, K_ESCAPE, K_F1, K_MOUSE1, K_MWHEELDOWN, K_MWHEELUP, K_SHIFT, K_UPARROW,
};

use super::{
    STARTUP, console_key, keynum, menu_bound, modifier_keys, mouse_key, mouse_motion, runs_binding,
    wheel_key,
};

// spec INPUT-3.1: letters in lower case, digits and punctuation as typed, the named keys by
// keys.h, the modifiers from egui's state, the mouse's buttons and the wheel
#[test]
fn window_keys_are_ids_numbers() {
    assert_eq!(keynum(egui::Key::W), Some(b'w'));
    assert_eq!(keynum(egui::Key::Num7), Some(b'7'));
    assert_eq!(keynum(egui::Key::Backslash), Some(b'\\'));
    assert_eq!(keynum(egui::Key::Comma), Some(b','));
    assert_eq!(keynum(egui::Key::Enter), Some(K_ENTER));
    assert_eq!(keynum(egui::Key::ArrowUp), Some(K_UPARROW));
    assert_eq!(keynum(egui::Key::F12), Some(K_F1 + 11));
    assert_eq!(keynum(egui::Key::F13), None);
    assert_eq!(keynum(egui::Key::Copy), None);
    let held = egui::Modifiers {
        shift: true,
        mac_cmd: true,
        ..egui::Modifiers::NONE
    };
    assert_eq!(
        modifier_keys(held),
        [(K_SHIFT, true), (K_CTRL, true), (K_ALT, false)]
    );
    assert_eq!(mouse_key(egui::PointerButton::Primary), Some(K_MOUSE1));
    assert_eq!(mouse_key(egui::PointerButton::Middle), Some(K_MOUSE1 + 2));
    assert_eq!(mouse_key(egui::PointerButton::Extra1), None);
    assert_eq!(wheel_key(1.0), Some(K_MWHEELUP));
    assert_eq!(wheel_key(-2.0), Some(K_MWHEELDOWN));
    assert_eq!(wheel_key(0.0), None);
}

// spec INPUT-3.2: the game runs every binding; the console only keys that are not consolekeys;
// a menu only the F keys; never Escape or the backquote
#[test]
fn bindings_run_by_the_key_destination() {
    assert!(runs_binding(b'w', KeyDest::Game));
    assert!(!runs_binding(b'w', KeyDest::Console));
    assert!(runs_binding(K_F1, KeyDest::Console));
    assert!(runs_binding(K_F1 + 11, KeyDest::Menu));
    assert!(!runs_binding(b'w', KeyDest::Menu));
    for dest in [KeyDest::Game, KeyDest::Console, KeyDest::Menu] {
        assert!(!runs_binding(K_ESCAPE, dest));
        assert!(!runs_binding(b'`', dest));
    }
    assert!(console_key(b' ') && console_key(b'~').eq(&false) && console_key(K_ENTER));
    assert!(!console_key(K_MOUSE1));
    assert!(menu_bound(K_ESCAPE) && !menu_bound(K_F1 + 12));
}

// spec INPUT-3.3: the host runs exec quake.rc, as Host_Init does
#[test]
fn the_host_starts_with_quake_rc() {
    assert_eq!(STARTUP, b"exec quake.rc");
}

// spec INPUT-3.4: the mouse moves the player only while the keys are the game's
#[test]
fn the_mouse_is_the_games_only() {
    assert_eq!(mouse_motion(KeyDest::Game, [3.0, -1.0]), [3.0, -1.0]);
    assert_eq!(mouse_motion(KeyDest::Console, [3.0, -1.0]), [0.0; 2]);
    assert_eq!(mouse_motion(KeyDest::Menu, [3.0, -1.0]), [0.0; 2]);
}
