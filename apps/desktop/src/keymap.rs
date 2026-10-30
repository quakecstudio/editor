// SPDX-License-Identifier: GPL-3.0-or-later

//! The key bindings (PRIN-3, UI-10): each shortcut runs a command by id, so a key, a menu item,
//! the palette, the CLI and the agent do the same thing; the palette shows each command's.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

/// A shortcut and the command, which takes no parameters, it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    /// The command's id.
    pub command: &'static str,
    /// The keys, Cmd on macOS and Ctrl elsewhere for [`Modifiers::COMMAND`].
    pub shortcut: KeyboardShortcut,
}

/// Opens the command palette, as in VS Code and Zed.
pub const OPEN_PALETTE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::P);

/// Every binding.
pub const BINDINGS: [Binding; 2] = [
    Binding {
        command: "edit.undo",
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Z),
    },
    Binding {
        command: "edit.redo",
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Z),
    },
];

/// The shortcut bound to `command`, if any.
#[must_use]
pub fn shortcut(command: &str) -> Option<KeyboardShortcut> {
    BINDINGS
        .iter()
        .find(|binding| binding.command == command)
        .map(|binding| binding.shortcut)
}
