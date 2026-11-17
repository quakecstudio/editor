// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    reason = "test code: a failed unwrap is a failed test"
)]

use eframe::egui;
use qs_client::KeyDest;
use qs_sim::keys::{
    K_ALT, K_CTRL, K_ENTER, K_ESCAPE, K_F1, K_MOUSE1, K_MWHEELDOWN, K_MWHEELUP, K_SHIFT, K_UPARROW,
};

use super::{
    STARTUP, console_key, focus, keynum, menu_bound, modifier_keys, mouse_key, mouse_motion,
    runs_binding, wheel_key,
};
use qs_render::Look;

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

// spec INPUT-3.6: the host runs exec quake.rc, as Host_Init does
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

// spec INPUT-3.5: default.cfg is the host's own in QuakeSpasm's look, with W, A, S and D and
// mouse look; the game's in GLQuake's
// spec INPUT-4.4: config.cfg is the host's own file, empty before it has written one; every
// other file is the game's
#[test]
fn the_console_reads_the_hosts_default_and_config() {
    let game: qs_sim::ModelFiles = std::rc::Rc::new(|name: &str| {
        (name == "default.cfg" || name == "config.cfg").then(|| b"bind a +lookup\n".to_vec())
    });
    let files = super::host_files(game.clone(), Look::QuakeSpasm, || None);
    let text = String::from_utf8(files("default.cfg").unwrap()).unwrap();
    assert_eq!(files("DEFAULT.CFG").map(|t| t.len()), Some(text.len()));
    for line in [
        "unbindall",
        "bind w +forward",
        "bind a +moveleft",
        "bind s +back",
        "bind d +moveright",
        "bind MOUSE1 +attack",
        "+mlook",
    ] {
        assert!(text.lines().any(|l| l == line), "{line}");
    }
    assert_eq!(files("config.cfg").unwrap(), b"", "the game's is not read");
    assert_eq!(files("autoexec.cfg"), None);
    let saved = super::host_files(game, Look::GlQuake, || Some(b"bind q +jump\n".to_vec()));
    assert_eq!(saved("config.cfg").unwrap(), b"bind q +jump\n");
    assert_eq!(
        saved("default.cfg").unwrap(),
        b"bind a +lookup\n",
        "GLQuake's look"
    );
}

// spec INPUT-4.1: unbindall, the bindings, the archived settings, then +mlook when held
// spec INPUT-4.4: the host's own file, quakec-engine.cfg
#[test]
fn the_configuration_is_written_as_host_writeconfiguration_writes_it() {
    assert_eq!(
        super::configuration(
            "bind \"w\" \"+forward\"\n",
            "volume \"0.7\"\n",
            super::MouseLook::Held,
        ),
        "unbindall\nbind \"w\" \"+forward\"\nvolume \"0.7\"\n+mlook\n"
    );
    assert_eq!(
        super::configuration("", "", super::MouseLook::Released),
        "unbindall\n"
    );
    assert_eq!(super::CONFIG_FILE, "quakec-engine.cfg");
}

// spec PHYS-11.9: the host's key destination is the focus the world's frame reads: the game
// runs only with the keys going to it
#[test]
fn only_the_game_destination_runs_the_game() {
    assert_eq!(focus(KeyDest::Game), qs_sim::KeyFocus::Game);
    assert_eq!(focus(KeyDest::Console), qs_sim::KeyFocus::Elsewhere);
    assert_eq!(focus(KeyDest::Menu), qs_sim::KeyFocus::Elsewhere);
}
