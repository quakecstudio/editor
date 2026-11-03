// SPDX-License-Identifier: GPL-3.0-or-later

//! The key bindings (PRIN-3, UI-10): each shortcut runs a command by id, so a key, a menu item,
//! the palette, the CLI and the agent do the same thing; the palette shows each command's.

use eframe::egui::{Key, KeyboardShortcut, Modifiers};

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

/// Opens the command palette, as in VS Code and Zed.
pub const OPEN_PALETTE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::P);

/// Every binding. The focus keys follow VS Code: Cmd+0 the side bar, Cmd+1 the editor, then
/// the right and bottom panels in turn.
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
        command: qs_layout::FOCUS_LEFT_PANEL,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0),
        scope: Scope::Everywhere,
    },
    Binding {
        command: qs_layout::FOCUS_EDITOR,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1),
        scope: Scope::Everywhere,
    },
    Binding {
        command: qs_layout::FOCUS_RIGHT_PANEL,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Num2),
        scope: Scope::Everywhere,
    },
    Binding {
        command: qs_layout::FOCUS_BOTTOM_PANEL,
        shortcut: KeyboardShortcut::new(Modifiers::COMMAND, Key::Num3),
        scope: Scope::Everywhere,
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
