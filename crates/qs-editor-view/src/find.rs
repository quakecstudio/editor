// SPDX-License-Identifier: GPL-3.0-or-later

//! Find and replace: the matches of a query in the text, moving to the next one, and replacing
//! one or every match as one undoable edit each.
//!
//! As in VS Code's and Zed's find bar, a query is matched as written or ignoring case, never
//! across a line break, and replace-all is one step of the undo history.

use crate::buffer::{Buffer, Position};
use crate::selection::{EditState, Selection};

/// Whether a query matches letters of either case.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Case {
    /// `a` matches `a` and `A`.
    #[default]
    Ignore,
    /// `a` matches only `a`.
    Match,
}

/// A query: its text and how case is matched.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    /// The text to find; it holds no line break.
    pub text: String,
    /// How case is matched.
    pub case: Case,
}

impl Query {
    /// Every match in `buffer`, in order and not overlapping, each the selection of its text.
    /// An empty query matches nothing.
    #[must_use]
    pub fn matches(&self, buffer: &Buffer) -> Vec<Selection> {
        if self.text.is_empty() || self.text.contains('\n') {
            return Vec::new();
        }
        let needle = self.fold(&self.text);
        let mut out = Vec::new();
        for line in 0..buffer.line_count() {
            let text = buffer.line(line);
            let haystack = self.fold(text);
            // Folding to lowercase may change a character's length, so a match is kept only
            // where the folded and the original text line up byte for byte.
            if haystack.len() != text.len() {
                continue;
            }
            let mut from = 0;
            while let Some(found) = haystack.get(from..).and_then(|rest| rest.find(&needle)) {
                let start = from.saturating_add(found);
                let end = start.saturating_add(needle.len());
                out.push(Selection {
                    anchor: Position::new(line, start),
                    head: Position::new(line, end),
                });
                from = end;
            }
        }
        out
    }

    /// `text` as compared: lowercased when case is ignored.
    fn fold(&self, text: &str) -> String {
        match self.case {
            Case::Ignore => text.to_lowercase(),
            Case::Match => text.to_owned(),
        }
    }
}

impl EditState {
    /// Selects the first match of `query` after the primary selection, wrapping to the start;
    /// false when there is none.
    pub fn find_next(&mut self, query: &Query) -> bool {
        let matches = query.matches(self.buffer());
        let after = self.primary().end();
        let Some(next) = matches
            .iter()
            .find(|m| m.start() >= after)
            .or_else(|| matches.first())
        else {
            return false;
        };
        self.set_selections(&[*next]);
        true
    }

    /// Selects the last match of `query` before the primary selection, wrapping to the end;
    /// false when there is none.
    pub fn find_previous(&mut self, query: &Query) -> bool {
        let matches = query.matches(self.buffer());
        let before = self.primary().start();
        let Some(previous) = matches
            .iter()
            .rev()
            .find(|m| m.end() <= before)
            .or_else(|| matches.last())
        else {
            return false;
        };
        self.set_selections(&[*previous]);
        true
    }

    /// Replaces the selected match of `query` with `with` and selects the next match; when the
    /// primary selection is not a match, only selects the next one. False when there is none.
    pub fn replace_next(&mut self, query: &Query, with: &str) -> bool {
        let selected = self.primary();
        if query.matches(self.buffer()).contains(&selected) {
            self.set_selections(&[selected]);
            self.insert_step(with);
        }
        self.find_next(query)
    }

    /// Replaces every match of `query` with `with`, as one step of the undo history; the
    /// number replaced.
    pub fn replace_all(&mut self, query: &Query, with: &str) -> usize {
        let matches = query.matches(self.buffer());
        if matches.is_empty() {
            return 0;
        }
        self.set_selections(&matches);
        self.insert_step(with);
        matches.len()
    }
}
