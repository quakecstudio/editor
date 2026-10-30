// SPDX-License-Identifier: GPL-3.0-or-later

//! The editor's undo history: each step the replacements one edit made, with the selections
//! before and after it, so undo makes the inverse replacements and puts the selections back.
//!
//! The model is every code editor's (VS Code, Zed): typing is grouped, so one undo takes back
//! a run of typed text, and any other edit or a caret moved elsewhere starts a new step.

use crate::buffer::{Buffer, Position};
use crate::selection::Selection;

/// One replacement an edit made: `removed` from `start` to `end` became `inserted`, which ends
/// at `after`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Replace {
    pub(crate) start: Position,
    pub(crate) end: Position,
    pub(crate) after: Position,
    pub(crate) removed: String,
    pub(crate) inserted: String,
}

/// What kind of edit a step is, for grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditKind {
    /// Typed text with no line break, grouped with the typing before it.
    Typing,
    /// Any other edit, a step of its own.
    Other,
}

/// One undoable edit.
#[derive(Clone, Debug)]
pub(crate) struct Step {
    /// The replacements, in the order they were made.
    replaces: Vec<Replace>,
    before: Vec<Selection>,
    after: Vec<Selection>,
    kind: EditKind,
}

/// The steps to undo and to redo.
#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl History {
    /// Records an edit: its replacements and the selections around it. Typing that goes on from
    /// where the last typing left the carets joins that step. Any edit clears what was undone.
    pub(crate) fn record(
        &mut self,
        replaces: Vec<Replace>,
        before: Vec<Selection>,
        after: Vec<Selection>,
        kind: EditKind,
    ) {
        self.redo.clear();
        if kind == EditKind::Typing
            && let Some(last) = self.undo.last_mut()
            && last.kind == EditKind::Typing
            && last.after == before
        {
            last.replaces.extend(replaces);
            last.after = after;
            return;
        }
        self.undo.push(Step {
            replaces,
            before,
            after,
            kind,
        });
    }

    /// Takes back the last step in `buffer`; the selections to restore, `None` with nothing to
    /// undo.
    pub(crate) fn undo(&mut self, buffer: &mut Buffer) -> Option<Vec<Selection>> {
        let step = self.undo.pop()?;
        for replace in step.replaces.iter().rev() {
            buffer.replace(replace.start, replace.after, &replace.removed);
        }
        let selections = step.before.clone();
        self.redo.push(step);
        Some(selections)
    }

    /// Makes the last undone step again in `buffer`; the selections after it, `None` with
    /// nothing to redo.
    pub(crate) fn redo(&mut self, buffer: &mut Buffer) -> Option<Vec<Selection>> {
        let step = self.redo.pop()?;
        for replace in &step.replaces {
            buffer.replace(replace.start, replace.end, &replace.inserted);
        }
        let selections = step.after.clone();
        // A redone step is not grouped with typing that follows it.
        self.undo.push(Step {
            kind: EditKind::Other,
            ..step
        });
        Some(selections)
    }

    /// Forgets every step, as when the whole text is replaced from outside.
    pub(crate) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
