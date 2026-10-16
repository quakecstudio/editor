// SPDX-License-Identifier: GPL-3.0-or-later
//! The scripted input both prototypes replay, one action per frame.
//!
//! Driving the model directly, rather than synthesising OS key events, is what makes the two
//! runs comparable: each frame applies the same edit to the same [`Document`], then the
//! framework lays out, paints and presents it. The OS input path (key events and the IME) is
//! checked by hand; see `docs/spikes/ui-framework.md`.

use crate::document::Document;

/// One scripted step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Idle,
    Goto(usize),
    Type(char),
    Newline,
    Backspace,
    Scroll(isize),
    AddCursorBelow,
    SingleCursor,
    ImePreedit(String),
    ImeCommit(String),
    OpenDiff(usize),
    CloseDiff,
}

/// A named run of frames; statistics are reported per phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    Warmup,
    Type,
    Scroll,
    MultiCursor,
    Ime,
    Diff,
}

impl Phase {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Warmup => "warmup",
            Self::Type => "type",
            Self::Scroll => "scroll",
            Self::MultiCursor => "multicursor",
            Self::Ime => "ime",
            Self::Diff => "diff",
        }
    }

    /// Whether the phase counts toward the pass threshold (everything but warm-up).
    #[must_use]
    pub fn measured(self) -> bool {
        !matches!(self, Self::Warmup)
    }
}

/// The line the script starts editing at: the middle of a 50,000-line buffer.
pub const START_LINE: usize = 25_000;
/// Rows kept above the cursor when the script jumps.
pub const CONTEXT_ROWS: usize = 10;

const TYPED: &str = "self.velocity = self.velocity * 0.7; // slow down";

/// The whole script, in order. Its length is fixed, so every run has the same frames.
#[must_use]
pub fn script() -> Vec<(Phase, Action)> {
    let mut steps = Vec::new();
    for _ in 0..120 {
        steps.push((Phase::Warmup, Action::Idle));
    }
    steps.push((Phase::Type, Action::Goto(START_LINE)));
    for _ in 0..12 {
        for c in TYPED.chars() {
            steps.push((Phase::Type, Action::Type(c)));
        }
        steps.push((Phase::Type, Action::Newline));
    }
    for _ in 0..40 {
        steps.push((Phase::Type, Action::Backspace));
    }
    for _ in 0..600 {
        steps.push((Phase::Scroll, Action::Scroll(3)));
    }
    for _ in 0..600 {
        steps.push((Phase::Scroll, Action::Scroll(-3)));
    }
    steps.push((
        Phase::MultiCursor,
        Action::Goto(START_LINE.saturating_add(400)),
    ));
    for _ in 0..4 {
        steps.push((Phase::MultiCursor, Action::AddCursorBelow));
    }
    for _ in 0..5 {
        for c in TYPED.chars() {
            steps.push((Phase::MultiCursor, Action::Type(c)));
        }
        steps.push((Phase::MultiCursor, Action::Newline));
    }
    steps.push((Phase::MultiCursor, Action::SingleCursor));
    for _ in 0..20 {
        for preedit in ["n", "に", "にh", "にほ", "にほn", "にほん"] {
            steps.push((Phase::Ime, Action::ImePreedit(preedit.to_owned())));
        }
        steps.push((Phase::Ime, Action::ImeCommit("日本".to_owned())));
    }
    for _ in 0..4 {
        steps.push((Phase::Diff, Action::OpenDiff(8)));
        for _ in 0..60 {
            steps.push((Phase::Diff, Action::Scroll(1)));
        }
        for _ in 0..60 {
            steps.push((Phase::Diff, Action::Scroll(-1)));
        }
        steps.push((Phase::Diff, Action::CloseDiff));
    }
    steps
}

/// Applies one action and keeps the primary cursor visible in a view `visible_rows` tall.
pub fn apply(doc: &mut Document, action: &Action, visible_rows: usize) {
    match action {
        Action::Idle => return,
        Action::Goto(line) => doc.goto_line(*line, CONTEXT_ROWS),
        Action::Type(c) => {
            let mut buf = [0u8; 4];
            doc.insert(c.encode_utf8(&mut buf));
        }
        Action::Newline => doc.newline(),
        Action::Backspace => doc.backspace(),
        Action::Scroll(delta) => {
            doc.scroll_by(*delta);
            return;
        }
        Action::AddCursorBelow => doc.add_cursor_below(),
        Action::SingleCursor => doc.single_cursor(),
        Action::ImePreedit(text) => doc.marked = Some(text.clone()),
        Action::ImeCommit(text) => doc.insert(text),
        Action::OpenDiff(len) => doc.open_diff(*len),
        Action::CloseDiff => doc.close_diff(),
    }
    reveal(doc, visible_rows);
}

/// Scrolls so the primary cursor's row is on screen.
pub fn reveal(doc: &mut Document, visible_rows: usize) {
    let row = doc.row_of_line(doc.primary().line);
    if row < doc.scroll_row {
        doc.scroll_row = row;
    } else if row >= doc.scroll_row.saturating_add(visible_rows) {
        doc.scroll_row = row.saturating_sub(visible_rows.saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_runs_cleanly_on_a_large_buffer() {
        let text: String = (0..50_000)
            .map(|i| format!("float x{i} = {i};\n"))
            .collect();
        let mut doc = Document::new(&text);
        let steps = script();
        assert!(steps.len() > 2_500);
        for (_, action) in &steps {
            apply(&mut doc, action, 40);
        }
        assert!(doc.diff.is_none());
        assert_eq!(doc.cursors.len(), 1);
        assert!(doc.text().contains("日本"));
        assert!(doc.line_count() > 50_000);
    }
}
