// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/keys.c, Key_StringToKeynum, Key_KeynumToString, Key_SetBinding, Key_Unbind_f, Key_Unbindall_f, Key_Bind_f, Key_Event

//! The keys' bindings (`docs/spec/engine/input.md` INPUT-1): a key's name and number, the
//! command each key runs, and what a key going down or up adds to the console's buffer.

/// `K_TAB`.
pub const K_TAB: u8 = 9;
/// `K_ENTER`.
pub const K_ENTER: u8 = 13;
/// `K_ESCAPE`.
pub const K_ESCAPE: u8 = 27;
/// `K_SPACE`.
pub const K_SPACE: u8 = 32;
/// `K_BACKSPACE`.
pub const K_BACKSPACE: u8 = 127;
/// `K_UPARROW`.
pub const K_UPARROW: u8 = 128;
/// `K_DOWNARROW`.
pub const K_DOWNARROW: u8 = 129;
/// `K_LEFTARROW`.
pub const K_LEFTARROW: u8 = 130;
/// `K_RIGHTARROW`.
pub const K_RIGHTARROW: u8 = 131;
/// `K_ALT`.
pub const K_ALT: u8 = 132;
/// `K_CTRL`.
pub const K_CTRL: u8 = 133;
/// `K_SHIFT`.
pub const K_SHIFT: u8 = 134;
/// `K_F1`; `K_F2` to `K_F12` follow it.
pub const K_F1: u8 = 135;
/// `K_INS`.
pub const K_INS: u8 = 147;
/// `K_DEL`.
pub const K_DEL: u8 = 148;
/// `K_PGDN`.
pub const K_PGDN: u8 = 149;
/// `K_PGUP`.
pub const K_PGUP: u8 = 150;
/// `K_HOME`.
pub const K_HOME: u8 = 151;
/// `K_END`.
pub const K_END: u8 = 152;
/// `K_MOUSE1`; `K_MOUSE2` and `K_MOUSE3` follow it.
pub const K_MOUSE1: u8 = 200;
/// `K_JOY1`; `K_JOY2` to `K_JOY4` follow it.
pub const K_JOY1: u8 = 203;
/// `K_AUX1`; `K_AUX2` to `K_AUX32` follow it.
pub const K_AUX1: u8 = 207;
/// `K_MWHEELUP`.
pub const K_MWHEELUP: u8 = 239;
/// `K_MWHEELDOWN`.
pub const K_MWHEELDOWN: u8 = 240;
/// `K_PAUSE`.
pub const K_PAUSE: u8 = 255;

/// `keynames`' names that are not runs of numbered keys, in its order (INPUT-1.1).
const NAMED: [(&str, u8); 25] = [
    ("TAB", K_TAB),
    ("ENTER", K_ENTER),
    ("ESCAPE", K_ESCAPE),
    ("SPACE", K_SPACE),
    ("BACKSPACE", K_BACKSPACE),
    ("UPARROW", K_UPARROW),
    ("DOWNARROW", K_DOWNARROW),
    ("LEFTARROW", K_LEFTARROW),
    ("RIGHTARROW", K_RIGHTARROW),
    ("ALT", K_ALT),
    ("CTRL", K_CTRL),
    ("SHIFT", K_SHIFT),
    ("INS", K_INS),
    ("DEL", K_DEL),
    ("PGDN", K_PGDN),
    ("PGUP", K_PGUP),
    ("HOME", K_HOME),
    ("END", K_END),
    ("PAUSE", K_PAUSE),
    ("MWHEELUP", K_MWHEELUP),
    ("MWHEELDOWN", K_MWHEELDOWN),
    // "because a raw semicolon seperates commands".
    ("SEMICOLON", b';'),
    ("MOUSE1", K_MOUSE1),
    ("MOUSE2", K_MOUSE1 + 1),
    ("MOUSE3", K_MOUSE1 + 2),
];

/// `Key_StringToKeynum` (INPUT-1.1): a single character is its own number; else `keynames`'
/// name, any case; else none.
#[must_use]
pub fn key_from_name(name: &[u8]) -> Option<u8> {
    match name {
        [] => None,
        [c] => Some(*c),
        _ => {
            let name = core::str::from_utf8(name).ok()?;
            if let Some((_, key)) = NAMED.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
                return Some(*key);
            }
            numbered(name)
        }
    }
}

/// `F1` to `F12`, `JOY1` to `JOY4` and `AUX1` to `AUX32`, any case.
fn numbered(name: &str) -> Option<u8> {
    let runs: [(&str, u8, u8); 3] = [("F", K_F1, 12), ("JOY", K_JOY1, 4), ("AUX", K_AUX1, 32)];
    for (prefix, first, count) in runs {
        let Some(head) = name.get(..prefix.len()) else {
            continue;
        };
        if !head.eq_ignore_ascii_case(prefix) {
            continue;
        }
        let digits = name.get(prefix.len()..).unwrap_or_default();
        if digits.starts_with('0') {
            continue;
        }
        let Ok(n) = digits.parse::<u8>() else {
            continue;
        };
        if (1..=count).contains(&n) {
            return first.checked_add(n.saturating_sub(1));
        }
    }
    None
}

/// `Key_KeynumToString` (INPUT-1.1): a printable character as itself, else its `keynames`
/// name, else `<UNKNOWN KEYNUM>`.
#[must_use]
pub fn key_name(key: u8) -> String {
    if key > 32 && key < 127 {
        return char::from(key).to_string();
    }
    if let Some((name, _)) = NAMED.iter().find(|(_, k)| *k == key) {
        return (*name).to_owned();
    }
    let runs: [(&str, u8, u8); 3] = [("F", K_F1, 12), ("JOY", K_JOY1, 4), ("AUX", K_AUX1, 32)];
    for (prefix, first, count) in runs {
        let n = key.wrapping_sub(first);
        if key >= first && n < count {
            return format!("{prefix}{}", n.saturating_add(1));
        }
    }
    "<UNKNOWN KEYNUM>".to_owned()
}

/// `keybindings`: the command each key runs (INPUT-1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bindings {
    keys: Vec<Option<Vec<u8>>>,
}

impl Default for Bindings {
    fn default() -> Self {
        Self::new()
    }
}

impl Bindings {
    /// No key bound.
    #[must_use]
    pub fn new() -> Self {
        Self {
            keys: vec![None; 256],
        }
    }

    /// The command `key` runs, if it has one.
    #[must_use]
    pub fn binding(&self, key: u8) -> Option<&[u8]> {
        self.keys.get(usize::from(key)).and_then(Option::as_deref)
    }

    /// `M_FindKeysForCommand`: the first two keys whose binding is `command`, compared for its
    /// length as `strncmp (b, command, l)` does (INPUT-1.4).
    #[must_use]
    pub fn keys_for(&self, command: &[u8]) -> [Option<u8>; 2] {
        let mut found = [None, None];
        let mut n = 0;
        for (key, binding) in self.keys.iter().enumerate() {
            let Some(binding) = binding else {
                continue;
            };
            if binding.get(..command.len()) == Some(command) {
                if let Some(slot) = found.get_mut(n) {
                    *slot = u8::try_from(key).ok();
                }
                n = n.saturating_add(1);
                if n == 2 {
                    break;
                }
            }
        }
        found
    }

    /// `M_UnbindCommand`: every key whose binding starts with `command`, as
    /// `strncmp (b, command, l)` compares it, set to the empty binding (INPUT-1.4).
    pub fn unbind_command(&mut self, command: &[u8]) {
        for binding in self.keys.iter_mut().flatten() {
            if binding.get(..command.len()) == Some(command) {
                binding.clear();
            }
        }
    }

    /// Each of `commands`' first two keys by name, `M_FindKeysForCommand` then
    /// `Key_KeynumToString`, as the keys menu shows them (INPUT-1.4).
    #[must_use]
    pub fn names_for(&self, commands: &[&str]) -> Vec<[Option<String>; 2]> {
        commands
            .iter()
            .map(|command| {
                self.keys_for(command.as_bytes())
                    .map(|key| key.map(key_name))
            })
            .collect()
    }

    /// `Key_SetBinding (key, binding)` (INPUT-1.2): an empty binding is kept as one.
    pub fn set(&mut self, key: u8, binding: &[u8]) {
        if let Some(slot) = self.keys.get_mut(usize::from(key)) {
            *slot = Some(binding.to_vec());
        }
    }

    /// `Key_Bind_f` with the command's words `argv` (INPUT-1.2): two words print the binding,
    /// three or more set it to the rest joined by spaces; what it prints.
    pub fn bind(&mut self, argv: &[Vec<u8>]) -> Option<String> {
        if argv.len() != 2 && argv.len() != 3 {
            return Some("bind <key> [command] : attach a command to a key\n".to_owned());
        }
        let name = argv.get(1).map(Vec::as_slice).unwrap_or_default();
        let shown = String::from_utf8_lossy(name);
        let Some(key) = key_from_name(name) else {
            return Some(format!("\"{shown}\" isn't a valid key\n"));
        };
        let Some(command) = argv.get(2) else {
            return Some(match self.binding(key) {
                Some(bound) => format!("\"{shown}\" = \"{}\"\n", String::from_utf8_lossy(bound)),
                None => format!("\"{shown}\" is not bound\n"),
            });
        };
        self.set(key, command);
        None
    }

    /// `Key_Unbind_f` (INPUT-1.2): the key's binding made empty; what it prints.
    pub fn unbind(&mut self, argv: &[Vec<u8>]) -> Option<String> {
        let [_, name] = argv else {
            return Some("unbind <key> : remove commands from a key\n".to_owned());
        };
        let Some(key) = key_from_name(name) else {
            return Some(format!(
                "\"{}\" isn't a valid key\n",
                String::from_utf8_lossy(name)
            ));
        };
        self.set(key, b"");
        None
    }

    /// `Key_Unbindall_f` (INPUT-1.2): every bound key's binding made empty.
    pub fn unbind_all(&mut self) {
        for slot in self.keys.iter_mut().filter(|slot| slot.is_some()) {
            *slot = Some(Vec::new());
        }
    }

    /// `Key_Event`'s text for the game when `key` goes down (INPUT-1.3): a `+` binding with the
    /// key's number after it, else the binding, then a line feed; none for no binding.
    #[must_use]
    pub fn down(&self, key: u8) -> Option<Vec<u8>> {
        let bound = self.binding(key)?;
        if bound.is_empty() {
            return None;
        }
        let mut text = bound.to_vec();
        if bound.first() == Some(&b'+') {
            text.extend_from_slice(format!(" {key}").as_bytes());
        }
        text.push(b'\n');
        Some(text)
    }

    /// `Key_Event`'s text when `key` goes up (INPUT-1.3), whatever has the keys: for a `+`
    /// binding, `-` and the rest of it with the key's number, else none.
    #[must_use]
    pub fn up(&self, key: u8) -> Option<Vec<u8>> {
        let bound = self.binding(key)?;
        let rest = bound.strip_prefix(b"+")?;
        let mut text = b"-".to_vec();
        text.extend_from_slice(rest);
        text.extend_from_slice(format!(" {key}\n").as_bytes());
        Some(text)
    }

    /// "`if (key >= 200 && !keybindings[key])`": the line a key with no binding at all prints
    /// when it goes down (INPUT-1.3).
    #[must_use]
    pub fn unbound_notice(&self, key: u8) -> Option<String> {
        (key >= 200 && self.binding(key).is_none())
            .then(|| format!("{} is unbound, hit F4 to set.\n", key_name(key)))
    }
}

#[cfg(test)]
mod tests;
