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
    key == K_ESCAPE || (key >= K_F1 && key < K_F1 + 12)
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
