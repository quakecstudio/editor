// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/keys.c, Key_Init, Key_Event

//! The window's keys as id's key numbers, and which of them run their binding for the key
//! destination (`docs/spec/engine/input.md` INPUT-3).

use eframe::egui;
use qs_client::KeyDest;
use qs_sim::keys::{
    K_ALT, K_BACKSPACE, K_CTRL, K_DEL, K_DOWNARROW, K_END, K_ENTER, K_ESCAPE, K_F1, K_HOME, K_INS,
    K_LEFTARROW, K_MOUSE1, K_MWHEELDOWN, K_MWHEELUP, K_PGDN, K_PGUP, K_RIGHTARROW, K_SHIFT,
    K_SPACE, K_TAB, K_UPARROW,
};

/// The function keys, F1 to F12.
const FUNCTION_KEYS: u8 = 12;

/// The key number egui's `key` is (INPUT-3.1): a letter as its lower case, a digit or a
/// punctuation key as the character it types unshifted, the rest by `keys.h`; none for a key
/// Quake has no number for.
pub(crate) fn keynum(key: egui::Key) -> Option<u8> {
    use egui::Key;
    let named = match key {
        Key::Tab => K_TAB,
        Key::Enter => K_ENTER,
        Key::Escape => K_ESCAPE,
        Key::Space => K_SPACE,
        Key::Backspace => K_BACKSPACE,
        Key::ArrowUp => K_UPARROW,
        Key::ArrowDown => K_DOWNARROW,
        Key::ArrowLeft => K_LEFTARROW,
        Key::ArrowRight => K_RIGHTARROW,
        Key::Insert => K_INS,
        Key::Delete => K_DEL,
        Key::PageDown => K_PGDN,
        Key::PageUp => K_PGUP,
        Key::Home => K_HOME,
        Key::End => K_END,
        Key::Comma => b',',
        Key::Period => b'.',
        Key::Slash => b'/',
        Key::Backslash => b'\\',
        Key::Semicolon => b';',
        Key::Minus => b'-',
        Key::Equals => b'=',
        Key::OpenBracket => b'[',
        Key::CloseBracket => b']',
        Key::Quote => b'\'',
        Key::Backtick => b'`',
        _ => {
            let function = [
                Key::F1,
                Key::F2,
                Key::F3,
                Key::F4,
                Key::F5,
                Key::F6,
                Key::F7,
                Key::F8,
                Key::F9,
                Key::F10,
                Key::F11,
                Key::F12,
            ];
            if let Some(n) = function.iter().position(|f| *f == key) {
                return K_F1.checked_add(u8::try_from(n).ok()?);
            }
            let [c] = key.name().as_bytes() else {
                return None;
            };
            return c.is_ascii_alphanumeric().then(|| c.to_ascii_lowercase());
        }
    };
    Some(named)
}

/// The key numbers of the modifier keys egui reports as state, not as keys: Shift, Ctrl
/// (Command on macOS is Ctrl's) and Alt, each with whether it is down.
pub(crate) fn modifier_keys(modifiers: egui::Modifiers) -> [(u8, bool); 3] {
    [
        (K_SHIFT, modifiers.shift),
        (K_CTRL, modifiers.ctrl || modifiers.mac_cmd),
        (K_ALT, modifiers.alt),
    ]
}

/// The key number of a mouse button, `K_MOUSE1` to `K_MOUSE3` (INPUT-3.1).
pub(crate) fn mouse_key(button: egui::PointerButton) -> Option<u8> {
    match button {
        egui::PointerButton::Primary => Some(K_MOUSE1),
        egui::PointerButton::Secondary => K_MOUSE1.checked_add(1),
        egui::PointerButton::Middle => K_MOUSE1.checked_add(2),
        egui::PointerButton::Extra1 | egui::PointerButton::Extra2 => None,
    }
}

/// The wheel's key for a scroll of `y`: up for more than 0, down for less (INPUT-3.1).
pub(crate) fn wheel_key(y: f32) -> Option<u8> {
    if y > 0.0 {
        Some(K_MWHEELUP)
    } else if y < 0.0 {
        Some(K_MWHEELDOWN)
    } else {
        None
    }
}

/// `consolekeys` (INPUT-3.2): the keys the console's line takes, every printable character
/// but the backquote and `~`, and Enter, Tab, the arrows, Backspace, the page keys, Shift and
/// the wheel.
pub(crate) const fn console_key(key: u8) -> bool {
    match key {
        b'`' | b'~' => false,
        32..=126 => true,
        _ => matches!(
            key,
            K_ENTER
                | K_TAB
                | K_LEFTARROW
                | K_RIGHTARROW
                | K_UPARROW
                | K_DOWNARROW
                | K_BACKSPACE
                | K_PGUP
                | K_PGDN
                | K_SHIFT
                | K_MWHEELUP
                | K_MWHEELDOWN
        ),
    }
}

/// `menubound` (INPUT-3.2): Escape and the function keys, which run their binding in a menu.
pub(crate) const fn menu_bound(key: u8) -> bool {
    key == K_ESCAPE || (key >= K_F1 && key < K_F1 + FUNCTION_KEYS)
}

/// Where the server's frame sees the keys going for the key destination `dest` (PHYS-11.9): to
/// the game, or elsewhere, to the console or a menu, which pauses a single player game.
pub(crate) const fn focus(dest: KeyDest) -> qs_sim::KeyFocus {
    match dest {
        KeyDest::Game => qs_sim::KeyFocus::Game,
        KeyDest::Console | KeyDest::Menu => qs_sim::KeyFocus::Elsewhere,
    }
}

/// `Key_Event`'s test for a key going down (INPUT-3.2): whether `key` runs its binding with
/// keys going to `dest`, rather than going to the console's line or the menu. Escape never
/// does; the backquote, which the window toggles the console with, is left to it.
pub(crate) const fn runs_binding(key: u8, dest: KeyDest) -> bool {
    if key == K_ESCAPE || key == b'`' {
        return false;
    }
    match dest {
        KeyDest::Game => true,
        KeyDest::Console => !console_key(key),
        KeyDest::Menu => menu_bound(key),
    }
}

/// The host's own `default.cfg` in QuakeSpasm's look, as QuakeSpasm's `quakespasm.pak` covers
/// id1's (INPUT-3.5): the movement on W, A, S and D and the mouse looking.
pub(crate) const DEFAULT_CFG: &[u8] = br#"// QuakeC Studio's default bindings in QuakeSpasm's look (INPUT-3.5). Like QuakeSpasm's own,
// they put the movement on W, A, S and D, the mouse on attack and the wheel on the weapons;
// the host's own config.cfg (INPUT-4.4) and the game's autoexec.cfg run after them.
unbindall

bind w +forward
bind s +back
bind a +moveleft
bind d +moveright
bind e +moveup
bind c +movedown
bind SPACE +jump
bind SHIFT +speed
bind ALT +strafe
bind CTRL +attack
bind MOUSE1 +attack

bind UPARROW +forward
bind DOWNARROW +back
bind LEFTARROW +left
bind RIGHTARROW +right
bind , +moveleft
bind . +moveright
bind PGDN +lookup
bind DEL +lookdown
bind END centerview

bind TAB +showscores

bind 1 "impulse 1"
bind 2 "impulse 2"
bind 3 "impulse 3"
bind 4 "impulse 4"
bind 5 "impulse 5"
bind 6 "impulse 6"
bind 7 "impulse 7"
bind 8 "impulse 8"
bind 0 "impulse 0"
bind / "impulse 10"
bind MWHEELDOWN "impulse 10"
bind MWHEELUP "impulse 12"

bind F1 "help"
bind F2 "menu_save"
bind F3 "menu_load"
bind F4 "menu_options"
bind F6 "echo Quicksaving...; wait; save quick"
bind F9 "echo Quickloading...; wait; load quick"
bind F10 "quit"

bind \ +mlook
+mlook
"#;

/// The host's configuration file, `quakec-engine.cfg` in its config directory (INPUT-4.4).
pub(crate) fn config_path() -> Option<std::path::PathBuf> {
    qs_paths::current()
        .ok()
        .map(|dirs| dirs.config().join(CONFIG_FILE))
}

/// `files` as the host's console reads them (INPUT-3.5, INPUT-4.4): `config.cfg` is the
/// host's own configuration, the text `config` gives, or nothing before the host has written
/// one; with `own_default`, `default.cfg` is [`DEFAULT_CFG`]; every other file is the game's.
pub(crate) fn host_files(
    files: qs_sim::ModelFiles,
    own_default: bool,
    config: impl Fn() -> Option<Vec<u8>> + 'static,
) -> qs_sim::ModelFiles {
    std::rc::Rc::new(move |name: &str| {
        if name.eq_ignore_ascii_case("config.cfg") {
            Some(config().unwrap_or_default())
        } else if own_default && name.eq_ignore_ascii_case("default.cfg") {
            Some(DEFAULT_CFG.to_vec())
        } else {
            files(name)
        }
    })
}

/// The configuration the host writes as `Host_WriteConfiguration` writes `config.cfg`
/// (INPUT-4.1): `unbindall`, every binding, the archived settings, and `+mlook` when mouse
/// look is held, as QuakeSpasm adds it.
pub(crate) fn configuration(bindings: &str, settings: &str, mlook: bool) -> String {
    let mut text = String::from("unbindall\n");
    text.push_str(bindings);
    text.push_str(settings);
    if mlook {
        text.push_str("+mlook\n");
    }
    text
}

/// The host's configuration's file name in its config directory (INPUT-4.4).
pub(crate) const CONFIG_FILE: &str = "quakec-engine.cfg";

/// `Host_Init`'s "`Cbuf_InsertText ("exec quake.rc\n");`", the line the host runs once
/// before its first frame (INPUT-3.3).
pub(crate) const STARTUP: &[u8] = b"exec quake.rc";

/// The mouse's `motion` for the move with the keys going to `dest`: QuakeSpasm's
/// `IN_MouseMove` drops it unless they are the game's (INPUT-3.4).
pub(crate) const fn mouse_motion(dest: KeyDest, motion: [f32; 2]) -> [f32; 2] {
    match dest {
        KeyDest::Game => motion,
        KeyDest::Console | KeyDest::Menu => [0.0; 2],
    }
}

#[cfg(test)]
mod tests;
