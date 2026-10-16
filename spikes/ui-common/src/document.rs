// SPDX-License-Identifier: GPL-3.0-or-later
//! The editor model both prototypes render: lines, cursors, IME composition and an inline diff.
//!
//! It is deliberately framework-free, so the two prototypes differ only in how they lay out,
//! paint and receive input. Lines are a `Vec<String>`: the real editor will use a rope, but a
//! line vector is O(line length) per keystroke, which is all the spike's script needs.

use std::ops::Range;

use crate::lexer::{LineState, Span, Symbols, TokenKind, lex_line};

/// A caret position: a line index and a byte offset into that line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
}

/// A proposed edit shown inline: `removed` old lines above the added ones, as in mockup 03.
#[derive(Clone, Debug)]
pub struct DiffHunk {
    /// The first buffer line the hunk replaced; the added lines start here.
    pub start: usize,
    pub added: usize,
    pub removed: Vec<String>,
}

/// What one visible row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowKind {
    Normal,
    Added,
    Removed,
}

/// One display row, ready to paint.
#[derive(Clone, Debug)]
pub struct Row {
    pub kind: RowKind,
    /// The 1-based line number to show in the gutter, if the row is a buffer line.
    pub number: Option<usize>,
    /// The buffer line, if any: carets and IME text are placed on it.
    pub line: Option<usize>,
    pub text: String,
    pub spans: Vec<Span>,
}

/// The document, its cursors and its view state.
#[derive(Debug)]
pub struct Document {
    lines: Vec<String>,
    /// `starts[i]` is the lexer state line `i` starts in.
    starts: Vec<LineState>,
    symbols: Symbols,
    /// Cursors, kept sorted and on distinct lines.
    pub cursors: Vec<Cursor>,
    /// IME composition text shown at the primary cursor, not yet in the buffer.
    pub marked: Option<String>,
    pub diff: Option<DiffHunk>,
    /// The first visible display row.
    pub scroll_row: usize,
}

impl Document {
    /// Builds a document from text. An empty text is one empty line.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let symbols = Symbols::scan(lines.iter().map(String::as_str));
        let mut doc = Self {
            starts: Vec::new(),
            lines,
            symbols,
            cursors: vec![Cursor { line: 0, col: 0 }],
            marked: None,
            diff: None,
            scroll_row: 0,
        };
        doc.relex_from(0);
        doc
    }

    #[must_use]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    #[must_use]
    pub fn line(&self, index: usize) -> &str {
        self.lines.get(index).map_or("", String::as_str)
    }

    /// The number of display rows: buffer lines plus the removed lines of an open diff.
    #[must_use]
    pub fn row_count(&self) -> usize {
        let removed = self.diff.as_ref().map_or(0, |d| d.removed.len());
        self.lines.len().saturating_add(removed)
    }

    /// The primary cursor (the first one).
    #[must_use]
    pub fn primary(&self) -> Cursor {
        self.cursors
            .first()
            .copied()
            .unwrap_or(Cursor { line: 0, col: 0 })
    }

    /// Recomputes lexer start states from `line` on, stopping once a state stops changing.
    fn relex_from(&mut self, line: usize) {
        self.starts.resize(self.lines.len(), LineState::default());
        let mut index = line;
        let mut state = self.starts.get(index).copied().unwrap_or_default();
        while let Some(text) = self.lines.get(index) {
            if let Some(slot) = self.starts.get_mut(index) {
                *slot = state;
            }
            let (_, next) = lex_line(text, state, &self.symbols);
            index = index.saturating_add(1);
            let Some(old) = self.starts.get(index).copied() else {
                break;
            };
            if old == next && index > line.saturating_add(1) {
                break;
            }
            state = next;
        }
    }

    /// Moves the primary cursor to `line`, column 0, drops the others and scrolls it into view
    /// with `context` rows above it.
    pub fn goto_line(&mut self, line: usize, context: usize) {
        let line = line.min(self.lines.len().saturating_sub(1));
        self.cursors = vec![Cursor { line, col: 0 }];
        self.scroll_row = line.saturating_sub(context);
    }

    /// Adds a cursor on the line below the last one, at the same column (clamped).
    pub fn add_cursor_below(&mut self) {
        let Some(last) = self.cursors.last().copied() else {
            return;
        };
        let line = last.line.saturating_add(1);
        if line >= self.lines.len() {
            return;
        }
        let col = clamp_to_char(self.line(line), last.col);
        self.cursors.push(Cursor { line, col });
    }

    /// Keeps only the primary cursor.
    pub fn single_cursor(&mut self) {
        self.cursors.truncate(1);
    }

    /// Inserts `text` (no newlines) at every cursor.
    pub fn insert(&mut self, text: &str) {
        self.marked = None;
        for cursor in &mut self.cursors {
            if let Some(line) = self.lines.get_mut(cursor.line) {
                let col = clamp_to_char(line, cursor.col);
                line.insert_str(col, text);
                cursor.col = col.saturating_add(text.len());
            }
        }
        let first = self.primary().line;
        self.relex_from(first);
    }

    /// Splits the line at every cursor, bottom-up so earlier line indices stay valid.
    pub fn newline(&mut self) {
        self.marked = None;
        let count = self.cursors.len();
        for i in (0..count).rev() {
            let Some(cursor) = self.cursors.get(i).copied() else {
                continue;
            };
            let Some(line) = self.lines.get_mut(cursor.line) else {
                continue;
            };
            let col = clamp_to_char(line, cursor.col);
            let indent: String = line
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let tail = line.split_off(col);
            let next_line = cursor.line.saturating_add(1);
            self.lines.insert(next_line, format!("{indent}{tail}"));
            self.starts.insert(next_line, LineState::default());
            for later in self.cursors.iter_mut().skip(i.saturating_add(1)) {
                later.line = later.line.saturating_add(1);
            }
            if let Some(c) = self.cursors.get_mut(i) {
                *c = Cursor {
                    line: next_line,
                    col: indent.len(),
                };
            }
            if let Some(diff) = &mut self.diff
                && diff.start > cursor.line
            {
                diff.start = diff.start.saturating_add(1);
            }
        }
        let first = self.primary().line.saturating_sub(1);
        self.relex_from(first);
    }

    /// Deletes the character before every cursor that is not at the start of its line.
    pub fn backspace(&mut self) {
        self.marked = None;
        for cursor in &mut self.cursors {
            let Some(line) = self.lines.get_mut(cursor.line) else {
                continue;
            };
            let col = clamp_to_char(line, cursor.col);
            let Some((start, _)) = line.get(..col).and_then(|s| s.char_indices().next_back())
            else {
                continue;
            };
            line.replace_range(start..col, "");
            cursor.col = start;
        }
        let first = self.primary().line;
        self.relex_from(first);
    }

    /// Replaces `range` (bytes of the primary cursor's line) with `text`; the IME commit path.
    pub fn replace_in_line(&mut self, range: Range<usize>, text: &str) {
        self.marked = None;
        let cursor = self.primary();
        if let Some(line) = self.lines.get_mut(cursor.line) {
            let start = clamp_to_char(line, range.start);
            let end = clamp_to_char(line, range.end.max(start));
            line.replace_range(start..end, text);
            if let Some(c) = self.cursors.first_mut() {
                c.col = start.saturating_add(text.len());
            }
        }
        self.relex_from(cursor.line);
    }

    /// Scrolls by `delta` rows, clamped to the document.
    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.row_count().saturating_sub(1);
        self.scroll_row = self.scroll_row.saturating_add_signed(delta).min(max);
    }

    /// Opens a diff over `len` lines at the primary cursor, as if an agent proposed rewriting
    /// them: the old lines become removed rows and each new line gets a trailing comment.
    pub fn open_diff(&mut self, len: usize) {
        let start = self.primary().line;
        let end = start.saturating_add(len).min(self.lines.len());
        let removed: Vec<String> = self.lines.get(start..end).unwrap_or(&[]).to_vec();
        for line in self.lines.get_mut(start..end).unwrap_or(&mut []) {
            line.push_str(" // reviewed");
        }
        self.diff = Some(DiffHunk {
            start,
            added: end.saturating_sub(start),
            removed,
        });
        self.relex_from(start);
    }

    /// Accepts the open diff: its removed rows disappear.
    pub fn close_diff(&mut self) {
        self.diff = None;
    }

    /// Maps a buffer line to its display row.
    #[must_use]
    pub fn row_of_line(&self, line: usize) -> usize {
        match &self.diff {
            Some(d) if line >= d.start => line.saturating_add(d.removed.len()),
            _ => line,
        }
    }

    /// The display rows `first..first + count`, lexed and classified.
    #[must_use]
    pub fn rows(&self, first: usize, count: usize) -> Vec<Row> {
        let mut rows = Vec::with_capacity(count);
        for row in first..first.saturating_add(count) {
            let Some(entry) = self.row(row) else { break };
            rows.push(entry);
        }
        rows
    }

    fn row(&self, row: usize) -> Option<Row> {
        let (kind, line_index, removed_text) = match &self.diff {
            Some(d) if row >= d.start && row < d.start.saturating_add(d.removed.len()) => {
                let i = row.saturating_sub(d.start);
                (RowKind::Removed, None, d.removed.get(i).cloned())
            }
            Some(d) if row >= d.start => {
                let line = row.saturating_sub(d.removed.len());
                let kind = if line < d.start.saturating_add(d.added) {
                    RowKind::Added
                } else {
                    RowKind::Normal
                };
                (kind, Some(line), None)
            }
            _ => (RowKind::Normal, Some(row), None),
        };
        let (text, state) = match (line_index, removed_text) {
            (Some(line), _) => (
                self.lines.get(line)?.clone(),
                self.starts.get(line).copied().unwrap_or_default(),
            ),
            (None, Some(text)) => (text, LineState::default()),
            (None, None) => return None,
        };
        let (spans, _) = lex_line(&text, state, &self.symbols);
        Some(Row {
            kind,
            number: line_index.map(|l| l.saturating_add(1)),
            line: line_index,
            text,
            spans,
        })
    }

    /// The text of the whole buffer, for tests and for the accessibility value.
    #[must_use]
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }
}

/// Clamps `col` to the line and down to a character boundary.
#[must_use]
pub fn clamp_to_char(line: &str, col: usize) -> usize {
    let mut col = col.min(line.len());
    while !line.is_char_boundary(col) {
        col = col.saturating_sub(1);
    }
    col
}

/// A styled run of a row's display text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub range: Range<usize>,
    pub color: u32,
    pub italic: bool,
    /// IME composition text is underlined.
    pub underline: bool,
}

impl Row {
    /// The text to paint and its styled runs, with IME composition text spliced in at byte
    /// `col` when the row holds the primary cursor. The runs cover the text without gaps.
    #[must_use]
    pub fn display(&self, marked: Option<(usize, &str)>) -> (String, Vec<Run>) {
        let style = |kind: TokenKind, range: Range<usize>| Run {
            range,
            color: crate::theme::token_color(kind),
            italic: is_italic(kind),
            underline: false,
        };
        let Some((col, text)) = marked else {
            let runs = self
                .spans
                .iter()
                .map(|s| style(s.kind, s.range.clone()))
                .collect();
            return (self.text.clone(), runs);
        };
        let col = clamp_to_char(&self.text, col);
        let mut out = String::with_capacity(self.text.len().saturating_add(text.len()));
        out.push_str(self.text.get(..col).unwrap_or(""));
        out.push_str(text);
        out.push_str(self.text.get(col..).unwrap_or(""));
        let shift = text.len();
        let mut runs = Vec::with_capacity(self.spans.len().saturating_add(2));
        for span in &self.spans {
            let (start, end) = (span.range.start, span.range.end);
            if end <= col {
                runs.push(style(span.kind, start..end));
            } else if start >= col {
                runs.push(style(
                    span.kind,
                    start.saturating_add(shift)..end.saturating_add(shift),
                ));
            } else {
                runs.push(style(span.kind, start..col));
                runs.push(style(
                    span.kind,
                    col.saturating_add(shift)..end.saturating_add(shift),
                ));
            }
        }
        let at = runs
            .iter()
            .position(|r| r.range.start >= col)
            .unwrap_or(runs.len());
        runs.insert(
            at,
            Run {
                range: col..col.saturating_add(shift),
                color: crate::theme::TEXT,
                italic: false,
                underline: true,
            },
        );
        (out, runs)
    }
}

/// The kinds that render in italics, as in the mockups (builtins and comments).
#[must_use]
pub fn is_italic(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Builtin | TokenKind::Comment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_at_several_cursors_edits_each_line() {
        let mut doc = Document::new("a\nb\nc");
        doc.add_cursor_below();
        doc.add_cursor_below();
        doc.insert("x");
        assert_eq!(doc.text(), "xa\nxb\nxc");
        doc.newline();
        assert_eq!(doc.text(), "x\na\nx\nb\nx\nc");
        assert_eq!(
            doc.cursors,
            vec![
                Cursor { line: 1, col: 0 },
                Cursor { line: 3, col: 0 },
                Cursor { line: 5, col: 0 }
            ]
        );
        doc.backspace();
        assert_eq!(doc.text(), "x\na\nx\nb\nx\nc");
    }

    #[test]
    fn backspace_removes_whole_characters() {
        let mut doc = Document::new("日本");
        doc.cursors = vec![Cursor { line: 0, col: 6 }];
        doc.backspace();
        assert_eq!(doc.text(), "日");
    }

    #[test]
    fn block_comment_state_follows_edits() {
        let mut doc = Document::new("a\nb\nc");
        doc.insert("/*");
        let rows = doc.rows(0, 3);
        assert!(
            rows.iter()
                .all(|r| r.spans.iter().all(|s| s.kind == TokenKind::Comment))
        );
        doc.backspace();
        doc.backspace();
        let rows = doc.rows(2, 1);
        assert_eq!(
            rows.first().and_then(|r| r.spans.first()).map(|s| s.kind),
            Some(TokenKind::Identifier)
        );
    }

    #[test]
    fn composition_text_is_spliced_in_and_underlined() {
        let doc = Document::new("local float x;");
        let rows = doc.rows(0, 1);
        let (text, runs) = rows[0].display(Some((8, "にほ")));
        assert_eq!(text, "local fl\u{306b}\u{307b}oat x;");
        let mut end = 0;
        for run in &runs {
            assert_eq!(run.range.start, end);
            end = run.range.end;
        }
        assert_eq!(end, text.len());
        assert_eq!(
            runs.iter()
                .filter(|r| r.underline)
                .map(|r| r.range.clone())
                .collect::<Vec<_>>(),
            vec![8..14]
        );
    }

    #[test]
    fn a_diff_inserts_removed_rows_above_added_ones() {
        let mut doc = Document::new("one\ntwo\nthree");
        doc.goto_line(1, 0);
        doc.open_diff(1);
        let kinds: Vec<RowKind> = doc.rows(0, 10).iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            vec![
                RowKind::Normal,
                RowKind::Removed,
                RowKind::Added,
                RowKind::Normal
            ]
        );
        assert_eq!(doc.row_of_line(2), 3);
        doc.close_diff();
        assert_eq!(doc.row_count(), 3);
    }
}
