// SPDX-License-Identifier: GPL-3.0-or-later

//! The editors' feature (P6-41): the editor and the second pane, the editors of the tabs not
//! shown, the go to symbol picker and the tab switcher, the multibuffer, and where the editor
//! was last taken, which the shell used to hold.

use std::collections::BTreeMap;
use std::path::PathBuf;

use qs_workbench::debug::Location;

use crate::editor::Editor;
use crate::multibuffer::Multibuffer;
use crate::picker::Picker;
use crate::project::Jump;

/// The editors' state.
#[derive(Default)]
pub(crate) struct EditorsFeature {
    /// The editor of the open file.
    pub(crate) editor: Option<Editor>,
    /// The editor of the second pane while the editor is split, showing a file not being
    /// edited, or the same one.
    pub(crate) second: Option<Editor>,
    /// The editors of the tabs not shown, with their carets and undo histories.
    pub(crate) parked: BTreeMap<PathBuf, Editor>,
    /// The editor's file and text revision whose characters have fonts.
    pub(crate) covered: Option<(PathBuf, u64)>,
    /// The line, from 1, of the file to show once it is open in the editor.
    pub(crate) pending_line: Option<(PathBuf, u32)>,
    /// The count of editor actions the layout had asked for when last carried out.
    pub(crate) editor_seen: u64,
    /// Where the game last stopped that the editor was taken to.
    pub(crate) shown_stop: Option<Location>,
    /// The go to symbol picker and the tab switcher, and the place each row stands for (P4-32).
    pub(crate) picker: (Picker, Vec<Jump>),
    /// The multibuffer the Search tab shows (P4-33).
    pub(crate) multibuffer: Multibuffer,
}
