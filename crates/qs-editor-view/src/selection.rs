// SPDX-License-Identifier: GPL-3.0-or-later

//! Selections and the edits made at every one of them.

use crate::buffer::{Buffer, Position};

/// One selection: the text between `anchor` and `head`, the caret at `head`. An empty selection
/// is a caret.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    /// Where the selection started.
    pub anchor: Position,
    /// Where it ends, the caret.
    pub head: Position,
}

impl Selection {
    /// A caret at `at`.
    #[must_use]
    pub const fn caret(at: Position) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    /// The selection's first position.
    #[must_use]
    pub fn start(&self) -> Position {
        self.anchor.min(self.head)
    }

    /// The selection's last position.
    #[must_use]
    pub fn end(&self) -> Position {
        self.anchor.max(self.head)
    }

    /// Whether it selects no text.
    #[must_use]
    pub fn is_caret(&self) -> bool {
        self.anchor == self.head
    }
}

/// Whether a movement selects as it moves (Shift held) or moves the caret alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extend {
    /// The anchor stays and the head moves.
    Select,
    /// Anchor and head move together.
    Move,
}

/// A caret movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    /// One character back.
    Left,
    /// One character forward.
    Right,
    /// One line up, at the column the caret was kept at.
    Up,
    /// One line down.
    Down,
    /// The line's start.
    Home,
    /// The line's end.
    End,
    /// `n` lines up.
    PageUp(usize),
    /// `n` lines down.
    PageDown(usize),
    /// The text's start.
    TextStart,
    /// The text's end.
    TextEnd,
}

/// The buffer and its selections: what an edit changes.
///
/// The selections are kept sorted by position and never overlapping; the primary one is the one
/// the view scrolls to and places IME text at.
#[derive(Clone, Debug)]
pub struct EditState {
    buffer: Buffer,
    selections: Vec<Selection>,
    primary: usize,
    /// The character column a vertical move keeps, set by the first of a run of vertical moves.
    goal: Option<usize>,
}

impl EditState {
    /// The text `text`, one caret at its start.
    #[must_use]
    pub fn new(text: &str) -> Self {
        Self {
            buffer: Buffer::new(text),
            selections: vec![Selection::default()],
            primary: 0,
            goal: None,
        }
    }

    /// The buffer.
    #[must_use]
    pub fn buffer(&self) -> &Buffer {
        &self.buffer
    }

    /// The selections, sorted and not overlapping.
    #[must_use]
    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    /// The primary selection.
    #[must_use]
    pub fn primary(&self) -> Selection {
        self.selections
            .get(self.primary)
            .copied()
            .unwrap_or_default()
    }

    /// Replaces the selections with `selections`, the first the primary one, clamped to the text,
    /// sorted and merged.
    pub fn set_selections(&mut self, selections: &[Selection]) {
        let clamped: Vec<Selection> = selections
            .iter()
            .map(|s| Selection {
                anchor: self.buffer.clamp(s.anchor),
                head: self.buffer.clamp(s.head),
            })
            .collect();
        let primary = clamped.first().copied().unwrap_or_default();
        self.selections = if clamped.is_empty() {
            vec![primary]
        } else {
            clamped
        };
        self.goal = None;
        self.normalize(primary);
    }

    /// Sorts and merges the selections, keeping as primary the one that holds `primary`.
    fn normalize(&mut self, primary: Selection) {
        self.selections.sort_by_key(Selection::start);
        let mut merged: Vec<Selection> = Vec::with_capacity(self.selections.len());
        for selection in self.selections.drain(..) {
            match merged.last_mut() {
                Some(last)
                    if selection.start() < last.end()
                        || (selection.start() == last.end()
                            && (selection.is_caret() || last.is_caret())) =>
                {
                    let (start, end) = (last.start(), last.end().max(selection.end()));
                    *last = if last.head < last.anchor {
                        Selection {
                            anchor: end,
                            head: start,
                        }
                    } else {
                        Selection {
                            anchor: start,
                            head: end,
                        }
                    };
                }
                Some(_) | None => merged.push(selection),
            }
        }
        self.primary = merged
            .iter()
            .position(|s| s.start() <= primary.head && primary.head <= s.end())
            .unwrap_or(0);
        self.selections = merged;
    }

    /// Moves every caret by `motion`, selecting as it goes with [`Extend::Select`]. Without it,
    /// a selection collapses to its start for `Left` and its end for `Right`.
    pub fn motion(&mut self, motion: Motion, extend: Extend) {
        let vertical = matches!(
            motion,
            Motion::Up | Motion::Down | Motion::PageUp(_) | Motion::PageDown(_)
        );
        let goal = if vertical {
            let head = self.primary().head;
            Some(
                self.goal
                    .unwrap_or_else(|| self.buffer.char_column(head.line, head.col)),
            )
        } else {
            None
        };
        let primary = self.primary();
        let mut moved = Vec::with_capacity(self.selections.len());
        let mut new_primary = primary;
        for selection in &self.selections {
            let head = match (motion, extend, selection.is_caret()) {
                (Motion::Left, Extend::Move, false) => selection.start(),
                (Motion::Right, Extend::Move, false) => selection.end(),
                _ => self.moved(selection.head, motion, goal),
            };
            let next = match extend {
                Extend::Select => Selection {
                    anchor: selection.anchor,
                    head,
                },
                Extend::Move => Selection::caret(head),
            };
            if *selection == primary {
                new_primary = next;
            }
            moved.push(next);
        }
        self.selections = moved;
        self.normalize(new_primary);
        self.goal = goal;
    }

    /// `at` moved by `motion`, a vertical move to character column `goal`.
    fn moved(&self, at: Position, motion: Motion, goal: Option<usize>) -> Position {
        let buffer = &self.buffer;
        let last = buffer.line_count().saturating_sub(1);
        let to_line = |line: usize| {
            let chars = goal.unwrap_or(0);
            Position::new(line, buffer.byte_column(line, chars))
        };
        match motion {
            Motion::Left => buffer.previous(at),
            Motion::Right => buffer.next(at),
            Motion::Up if at.line == 0 => Position::new(0, 0),
            Motion::Up => to_line(at.line.saturating_sub(1)),
            Motion::Down if at.line >= last => buffer.end(),
            Motion::Down => to_line(at.line.saturating_add(1)),
            Motion::PageUp(n) => to_line(at.line.saturating_sub(n)),
            Motion::PageDown(n) => to_line(at.line.saturating_add(n).min(last)),
            Motion::Home => Position::new(at.line, 0),
            Motion::End => Position::new(at.line, buffer.line(at.line).len()),
            Motion::TextStart => Position::default(),
            Motion::TextEnd => buffer.end(),
        }
    }

    /// Selects the whole text.
    pub fn select_all(&mut self) {
        let all = Selection {
            anchor: Position::default(),
            head: self.buffer.end(),
        };
        self.set_selections(&[all]);
    }

    /// Adds a caret on the line below the last selection's head, or above the first one's,
    /// at its character column; the new caret becomes the primary one.
    pub fn add_caret(&mut self, below: bool) {
        let from = if below {
            self.selections.last()
        } else {
            self.selections.first()
        };
        let Some(from) = from.copied() else {
            return;
        };
        let line = if below {
            from.head.line.saturating_add(1)
        } else if from.head.line == 0 {
            return;
        } else {
            from.head.line.saturating_sub(1)
        };
        if line >= self.buffer.line_count() {
            return;
        }
        let chars = self.buffer.char_column(from.head.line, from.head.col);
        let caret = Selection::caret(Position::new(line, self.buffer.byte_column(line, chars)));
        self.selections.push(caret);
        self.goal = None;
        self.normalize(caret);
    }

    /// Keeps only the primary selection, collapsed to its head.
    pub fn single_caret(&mut self) {
        let head = self.primary().head;
        self.set_selections(&[Selection::caret(head)]);
    }

    /// The text of every selection, joined by `\n`: what a copy takes.
    #[must_use]
    pub fn selected_text(&self) -> String {
        let mut parts = Vec::new();
        for selection in &self.selections {
            if !selection.is_caret() {
                parts.push(self.buffer.slice(selection.start(), selection.end()));
            }
        }
        parts.join("\n")
    }

    /// Replaces every selection with `text`, leaving a caret after it (typing and pasting).
    pub fn insert(&mut self, text: &str) {
        self.edit(|_, selection| (selection.start(), selection.end(), text.to_owned()));
    }

    /// Splits the line at every caret, the new line indented as the old one was.
    pub fn newline(&mut self) {
        self.edit(|buffer, selection| {
            let indent: String = buffer
                .line(selection.start().line)
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            (selection.start(), selection.end(), format!("\n{indent}"))
        });
    }

    /// Deletes every selection, or the character before each caret.
    pub fn backspace(&mut self) {
        self.edit(|buffer, selection| {
            let start = if selection.is_caret() {
                buffer.previous(selection.head)
            } else {
                selection.start()
            };
            (start, selection.end(), String::new())
        });
    }

    /// Deletes every selection, or the character after each caret.
    pub fn delete(&mut self) {
        self.edit(|buffer, selection| {
            let end = if selection.is_caret() {
                buffer.next(selection.head)
            } else {
                selection.end()
            };
            (selection.start(), end, String::new())
        });
    }

    /// Makes the edit `change` gives for each selection, from the last selection up, so each
    /// edit leaves the text before it unchanged; the carets already placed below are moved by
    /// what the edit added or removed. Each selection ends as a caret after its new text.
    fn edit(&mut self, change: impl Fn(&Buffer, &Selection) -> (Position, Position, String)) {
        let primary = self.primary;
        let mut carets: Vec<Position> = Vec::with_capacity(self.selections.len());
        for selection in self.selections.iter().rev() {
            let (start, end, text) = change(&self.buffer, selection);
            let end = self.buffer.clamp(end);
            let after = self.buffer.replace(start, end, &text);
            for caret in &mut carets {
                *caret = shifted(*caret, end, after);
            }
            carets.push(after);
        }
        carets.reverse();
        self.selections = carets.into_iter().map(Selection::caret).collect();
        let primary_head = self.selections.get(primary).copied().unwrap_or_default();
        self.goal = None;
        self.normalize(primary_head);
    }
}

/// `at`, a position at or after `end`, moved for an edit that replaced text ending at `end` with
/// text ending at `after`.
fn shifted(at: Position, end: Position, after: Position) -> Position {
    if at.line == end.line {
        Position::new(
            after.line,
            after.col.saturating_add(at.col.saturating_sub(end.col)),
        )
    } else {
        let line = at.line.saturating_sub(end.line).saturating_add(after.line);
        Position::new(line, at.col)
    }
}
