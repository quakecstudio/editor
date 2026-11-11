// SPDX-License-Identifier: GPL-3.0-or-later

//! The key bindings (PRIN-3, UI-10): each shortcut runs a command by id, so a key, a menu item,
//! the palette, the CLI and the agent do the same thing; the palette shows each command's.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{Key, KeyboardShortcut, Modifiers};
use serde::Deserialize;

/// Where a binding's keys are taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// Only while the editor does not have the keyboard: the editor takes its own keys, with
    /// its own undo history.
    Shell,
    /// Wherever the keyboard is, as a focus command must reach out of the editor (QA-9).
    Everywhere,
}

/// A shortcut and the command, which takes no parameters, it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The command's id.
    pub command: &'static str,
    /// The keys, Cmd on macOS and Ctrl elsewhere for [`Modifiers::COMMAND`].
    pub shortcut: KeyboardShortcut,
    /// Where the keys are taken.
    pub scope: Scope,
}

/// Opens a project folder with the system's dialog, as Zed's Cmd+O does.
pub const OPEN_FOLDER: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);

/// Opens the file finder, as in VS Code and Zed.
pub const OPEN_FILE_FINDER: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);

/// Opens the command palette, as in VS Code and Zed.
pub const OPEN_PALETTE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::P);

/// Every binding. The focus keys put the editor on Cmd+1 as VS Code does, then the left,
/// right and bottom panels on Cmd+2 to Cmd+4; Cmd+0 stays egui's zoom reset.
pub const BINDINGS: [Binding; 6] = [
    Binding {
        command: "edit.undo",
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Z),
        scope: Scope::Shell,
    },
    Binding {
        command: "edit.redo",
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z),
        scope: Scope::Shell,
    },
    Binding {
        command: qs_layout::FOCUS_EDITOR,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1),
        scope: Scope::Everywhere,
    },
    Binding {
        command: qs_layout::FOCUS_LEFT_PANEL,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Num2),
        scope: Scope::Everywhere,
    },
    Binding {
        command: qs_layout::FOCUS_RIGHT_PANEL,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Num3),
        scope: Scope::Everywhere,
    },
    Binding {
        command: qs_layout::FOCUS_BOTTOM_PANEL,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Num4),
        scope: Scope::Everywhere,
    },
];

/// Every binding in the order its keys are matched, the most modifiers first: egui's
/// `consume_shortcut` takes a shortcut whatever extra Shift is held ("you should match most
/// specific shortcuts first"), so Shift+Cmd+Z is matched as redo before Cmd+Z can take it as
/// undo.
#[must_use]
pub fn in_match_order() -> Vec<Binding> {
    let mut bindings = BINDINGS.to_vec();
    bindings.sort_by_key(|binding| std::cmp::Reverse(held(binding.shortcut.modifiers)));
    bindings
}

/// How many modifier keys `modifiers` holds; `COMMAND` is the platform's Cmd or Ctrl, counted
/// once.
fn held(modifiers: Modifiers) -> usize {
    [
        modifiers.alt,
        modifiers.shift,
        modifiers.command || modifiers.ctrl || modifiers.mac_cmd,
    ]
    .into_iter()
    .filter(|&down| down)
    .count()
}

/// The shortcut bound to `command`, if any.
#[must_use]
pub fn shortcut(command: &str) -> Option<KeyboardShortcut> {
    BINDINGS
        .iter()
        .find(|binding| binding.command == command)
        .map(|binding| binding.shortcut)
}

/// One binding the keymap holds: the command it runs, its keys and where they are taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    /// The command's id.
    pub command: String,
    /// The keys.
    pub shortcut: KeyboardShortcut,
    /// Where the keys are taken.
    pub scope: Scope,
}

/// The keymap file's own form: `[[binding]]` tables of `keys` and `command`, as Zed's
/// `keymap.json` binds a key sequence to an action (its docs, `key-bindings.md`).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeymapFile {
    #[serde(default)]
    binding: Vec<FileBinding>,
}

/// One `[[binding]]` of the keymap file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileBinding {
    /// The keys in Zed's spelling: modifiers and a key joined by `-`, such as `secondary-b`.
    keys: String,
    /// The command it runs.
    command: String,
}

/// The bindings in force (P4-37, UI-21): [`BINDINGS`], with the user's keymap file's over
/// them, a user binding taking a default's keys from it, and what is wrong in the file.
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<Bound>,
    problems: Vec<String>,
    file: Option<PathBuf>,
    read_at: Option<SystemTime>,
}

impl Keymap {
    /// The default bindings alone.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            bindings: BINDINGS
                .iter()
                .map(|b| Bound {
                    command: b.command.to_owned(),
                    shortcut: b.shortcut,
                    scope: b.scope,
                })
                .collect(),
            ..Self::default()
        }
    }

    /// The defaults with the keymap file at `file` over them, and `exists` saying whether a
    /// command id may be bound: a command with no parameters in the registry.
    #[must_use]
    pub fn read(file: PathBuf, bindable: &dyn Fn(&str) -> bool) -> Self {
        let mut keymap = Self::defaults();
        keymap.read_at = modified(&file);
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        keymap.apply(&text, bindable);
        keymap.file = Some(file);
        keymap
    }

    /// Reads the file again when it has changed since it was read, so an edit takes effect
    /// without a restart; whether it did.
    pub fn reload_if_changed(&mut self, bindable: &dyn Fn(&str) -> bool) -> bool {
        let Some(file) = self.file.clone() else {
            return false;
        };
        if modified(&file) == self.read_at {
            return false;
        }
        *self = Self::read(file, bindable);
        true
    }

    /// The user's bindings in `text` over the defaults, each problem noted.
    fn apply(&mut self, text: &str, bindable: &dyn Fn(&str) -> bool) {
        let parsed: KeymapFile = match toml::from_str(text) {
            Ok(parsed) => parsed,
            Err(error) => {
                self.problems
                    .push(format!("keymap.toml: {}", error.message()));
                return;
            }
        };
        let mut taken: Vec<(KeyboardShortcut, String)> = Vec::new();
        let mut users = Vec::new();
        for binding in parsed.binding {
            let Some(shortcut) = parse_keys(&binding.keys) else {
                self.problems
                    .push(format!("{:?} are not keys this keymap knows", binding.keys));
                continue;
            };
            if !bindable(&binding.command) {
                self.problems.push(format!(
                    "{} is not a command that can be bound: none of that id takes no parameters",
                    binding.command
                ));
                continue;
            }
            if let Some((_, first)) = taken.iter().find(|(keys, _)| *keys == shortcut) {
                self.problems.push(format!(
                    "{} is bound twice, to {first} and to {}; the first is kept",
                    binding.keys, binding.command
                ));
                continue;
            }
            taken.push((shortcut, binding.command.clone()));
            self.bindings.retain(|b| b.shortcut != shortcut);
            users.push(Bound {
                command: binding.command,
                shortcut,
                scope: Scope::Shell,
            });
        }
        // The user's first, so a command's shortcut, as the palette shows it, is theirs.
        self.bindings.splice(0..0, users);
    }

    /// Every binding in the order its keys are matched, the most modifiers first, as
    /// [`in_match_order`] orders the defaults.
    #[must_use]
    pub fn in_match_order(&self) -> Vec<Bound> {
        let mut bindings = self.bindings.clone();
        bindings.sort_by_key(|binding| std::cmp::Reverse(held(binding.shortcut.modifiers)));
        bindings
    }

    /// The shortcut bound to `command`, if any.
    #[must_use]
    pub fn shortcut(&self, command: &str) -> Option<KeyboardShortcut> {
        self.bindings
            .iter()
            .find(|binding| binding.command == command)
            .map(|binding| binding.shortcut)
    }

    /// What is wrong in the keymap file, each in words.
    #[must_use]
    pub fn problems(&self) -> &[String] {
        &self.problems
    }

    /// The keymap file, when one is kept.
    #[must_use]
    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }
}

/// When the file at `path` was last changed, if it is there.
fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// `keys` in Zed's spelling as a shortcut: modifiers (`secondary`, Cmd on macOS and Ctrl
/// elsewhere; `cmd`, `ctrl`, `alt`, `shift`) and one key, joined by `-`, such as
/// `secondary-shift-b` or `ctrl-f5`. A key sequence is not taken.
#[must_use]
pub fn parse_keys(keys: &str) -> Option<KeyboardShortcut> {
    if keys.contains(' ') {
        return None;
    }
    let mut parts: Vec<&str> = keys.split('-').collect();
    // `-` itself as the key, as in `secondary--`.
    if keys.ends_with("--") {
        parts.pop();
        parts.pop();
        parts.push("-");
    }
    let (key, modifiers) = parts.split_last()?;
    let mut held = Modifiers::NONE;
    for modifier in modifiers {
        held = held.plus(match *modifier {
            "secondary" => Modifiers::COMMAND,
            "cmd" => Modifiers::MAC_CMD,
            "ctrl" => Modifiers::CTRL,
            "alt" => Modifiers::ALT,
            "shift" => Modifiers::SHIFT,
            _ => return None,
        });
    }
    let mut name = (*key).to_owned();
    if let Some(first) = name.get(..1).map(str::to_uppercase) {
        name.replace_range(..1, &first);
    }
    Key::from_name(&name).map(|key| KeyboardShortcut::new(held, key))
}

/// The keymap file in the app's config directory `config`.
#[must_use]
pub fn keymap_path(config: &Path) -> PathBuf {
    config.join("keymap.toml")
}

#[must_use]
/// Whether `id` names a command of `registry` a key may run: one that takes no parameters, as
/// a shortcut gives it none.
pub fn bindable<C: 'static>(registry: &qs_commands::Registry<C>, id: &str) -> bool {
    registry.get(id).is_some_and(|info| {
        info.params()
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .is_none_or(serde_json::Map::is_empty)
    })
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        reason = "test code: a failed index is a failed test"
    )]

    use super::*;

    // No spec statement: P4-37, Zed's key spelling: secondary is Cmd on macOS and Ctrl
    // elsewhere, the other modifiers as named, one key; anything else is not keys
    #[test]
    fn keys_parse_as_zed_spells_them() {
        assert_eq!(
            parse_keys("secondary-shift-b"),
            Some(KeyboardShortcut::new(
                Modifiers::COMMAND.plus(Modifiers::SHIFT),
                Key::B
            ))
        );
        assert_eq!(
            parse_keys("ctrl-f5"),
            Some(KeyboardShortcut::new(Modifiers::CTRL, Key::F5))
        );
        assert_eq!(
            parse_keys("alt-enter"),
            Some(KeyboardShortcut::new(Modifiers::ALT, Key::Enter))
        );
        assert_eq!(
            parse_keys("secondary--"),
            Some(KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus))
        );
        for bad in ["", "hyper-b", "secondary-nosuchkey", "ctrl-k ctrl-c"] {
            assert_eq!(parse_keys(bad), None, "{bad}");
        }
    }

    // No spec statement: P4-37, the acceptance criterion's other half: a user binding takes its
    // keys from a default, and the file's conflicts, unknown commands and bad keys are
    // reported, the rest of it still applied
    #[test]
    fn the_file_overrides_the_defaults_and_reports_conflicts() {
        let bindable = |id: &str| id != "nosuch.command";
        let mut keymap = Keymap::defaults();
        keymap.apply(
            r#"
[[binding]]
keys = "secondary-z"
command = "project.build"

[[binding]]
keys = "secondary-shift-b"
command = "project.check"

[[binding]]
keys = "secondary-shift-b"
command = "project.test"

[[binding]]
keys = "secondary-k"
command = "nosuch.command"

[[binding]]
keys = "hyper-x"
command = "project.build"
"#,
            &bindable,
        );
        let undo = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        assert_eq!(keymap.shortcut("project.build"), Some(undo));
        assert_eq!(keymap.shortcut("edit.undo"), None, "its keys were taken");
        assert_eq!(
            keymap.shortcut("project.check"),
            parse_keys("secondary-shift-b")
        );
        assert_eq!(keymap.shortcut("project.test"), None);
        let problems = keymap.problems();
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems[0].contains("bound twice, to project.check and to project.test"));
        assert!(problems[1].contains("nosuch.command"));
        assert!(problems[2].contains("hyper-x"));
        let mut broken = Keymap::defaults();
        broken.apply("[[binding]]\nkeys = 1\n", &bindable);
        assert!(broken.problems()[0].starts_with("keymap.toml:"));
        assert_eq!(
            broken.shortcut("edit.undo"),
            Some(undo),
            "the defaults stay"
        );
    }
}
