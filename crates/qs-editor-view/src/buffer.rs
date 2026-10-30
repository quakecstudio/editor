// SPDX-License-Identifier: GPL-3.0-or-later

//! The text the view edits: lines, each with the identity the layout cache keys it by.

/// The identity of one line's text. An edit to a line gives it a new id; a line moved by an edit
/// above it keeps its id, so the layout cache relays out only the lines an edit changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LineId(u64);

/// A place in the text: a line index and a byte offset into that line, on a character boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Position {
    /// The line, from 0.
    pub line: usize,
    /// The byte offset into the line.
    pub col: usize,
}

impl Position {
    /// The position at byte `col` of line `line`.
    #[must_use]
    pub const fn new(line: usize, col: usize) -> Self {
        Self { line, col }
    }
}

/// The lines of a document. Lines hold no line breaks; the text is the lines joined by `\n`.
///
/// A line vector edits in time proportional to the edited line and moves no text for an edit
/// elsewhere, which is what a view needs for files of tens of thousands of lines.
#[derive(Clone, Debug)]
pub struct Buffer {
    lines: Vec<(LineId, String)>,
    next_id: u64,
}

impl Buffer {
    /// The buffer of `text`, split at `\n`; a `\r` before a `\n` is dropped. An empty text is
    /// one empty line.
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut buffer = Self {
            lines: Vec::new(),
            next_id: 0,
        };
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line).to_owned();
            let id = buffer.fresh_id();
            buffer.lines.push((id, line));
        }
        buffer
    }

    fn fresh_id(&mut self) -> LineId {
        let id = LineId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    /// The number of lines, at least 1.
    #[must_use]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Line `index`'s text, empty past the end.
    #[must_use]
    pub fn line(&self, index: usize) -> &str {
        self.lines.get(index).map_or("", |(_, text)| text.as_str())
    }

    /// Line `index`'s identity, `None` past the end.
    #[must_use]
    pub fn line_id(&self, index: usize) -> Option<LineId> {
        self.lines.get(index).map(|(id, _)| *id)
    }

    /// The whole text, the lines joined by `\n`.
    #[must_use]
    pub fn text(&self) -> String {
        let mut text = String::new();
        for (i, (_, line)) in self.lines.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            text.push_str(line);
        }
        text
    }

    /// The last position of the text.
    #[must_use]
    pub fn end(&self) -> Position {
        let line = self.lines.len().saturating_sub(1);
        Position::new(line, self.line(line).len())
    }

    /// `at` moved into the text: its line clamped to the last, its column to the line and down
    /// to a character boundary.
    #[must_use]
    pub fn clamp(&self, at: Position) -> Position {
        let line = at.line.min(self.lines.len().saturating_sub(1));
        Position::new(line, floor_char(self.line(line), at.col))
    }

    /// The text from `start` to `end`, `start` not after `end`.
    #[must_use]
    pub fn slice(&self, start: Position, end: Position) -> String {
        let (start, end) = (self.clamp(start), self.clamp(end));
        if start.line == end.line {
            return self
                .line(start.line)
                .get(start.col..end.col)
                .unwrap_or("")
                .to_owned();
        }
        let mut text = self
            .line(start.line)
            .get(start.col..)
            .unwrap_or("")
            .to_owned();
        for line in start.line.saturating_add(1)..end.line {
            text.push('\n');
            text.push_str(self.line(line));
        }
        text.push('\n');
        text.push_str(self.line(end.line).get(..end.col).unwrap_or(""));
        text
    }

    /// Replaces the text from `start` to `end` with `text`, which may hold line breaks, and
    /// returns the position after the inserted text. The edited lines get new ids; the lines
    /// after them keep theirs.
    pub fn replace(&mut self, start: Position, end: Position, text: &str) -> Position {
        let (start, end) = (self.clamp(start), self.clamp(end));
        let (start, end) = if end < start {
            (end, start)
        } else {
            (start, end)
        };
        let head = self
            .line(start.line)
            .get(..start.col)
            .unwrap_or("")
            .to_owned();
        let tail = self.line(end.line).get(end.col..).unwrap_or("").to_owned();
        let mut pieces: Vec<&str> = text.split('\n').collect();
        let last = pieces.pop().unwrap_or("");
        let mut new_lines = Vec::with_capacity(pieces.len().saturating_add(1));
        let mut first = true;
        for piece in pieces {
            let piece = piece.strip_suffix('\r').unwrap_or(piece);
            let line = if first {
                format!("{head}{piece}")
            } else {
                piece.to_owned()
            };
            first = false;
            new_lines.push(line);
        }
        let after = if first {
            Position::new(start.line, head.len().saturating_add(last.len()))
        } else {
            Position::new(start.line.saturating_add(new_lines.len()), last.len())
        };
        let last_line = if first {
            format!("{head}{last}{tail}")
        } else {
            format!("{last}{tail}")
        };
        new_lines.push(last_line);
        let ids: Vec<(LineId, String)> = new_lines
            .into_iter()
            .map(|line| (self.fresh_id(), line))
            .collect();
        let range = start.line..end.line.saturating_add(1).min(self.lines.len());
        self.lines.splice(range, ids);
        after
    }

    /// The position one character before `at`, the end of the previous line at a line's start;
    /// `at` itself at the start of the text.
    #[must_use]
    pub fn previous(&self, at: Position) -> Position {
        let at = self.clamp(at);
        let line = self.line(at.line);
        match line
            .get(..at.col)
            .and_then(|s| s.char_indices().next_back())
        {
            Some((col, _)) => Position::new(at.line, col),
            None if at.line > 0 => {
                let up = at.line.saturating_sub(1);
                Position::new(up, self.line(up).len())
            }
            None => at,
        }
    }

    /// The position one character after `at`, the start of the next line at a line's end; `at`
    /// itself at the end of the text.
    #[must_use]
    pub fn next(&self, at: Position) -> Position {
        let at = self.clamp(at);
        let line = self.line(at.line);
        match line.get(at.col..).and_then(|s| s.chars().next()) {
            Some(c) => Position::new(at.line, at.col.saturating_add(c.len_utf8())),
            None if at.line.saturating_add(1) < self.lines.len() => {
                Position::new(at.line.saturating_add(1), 0)
            }
            None => at,
        }
    }

    /// The byte offset of `at` in [`Buffer::text`]: the lengths of the lines before it, each
    /// with its `\n`, then its column.
    #[must_use]
    pub fn offset(&self, at: Position) -> usize {
        let at = self.clamp(at);
        self.lines
            .iter()
            .take(at.line)
            .fold(at.col, |sum, (_, line)| {
                sum.saturating_add(line.len()).saturating_add(1)
            })
    }

    /// The position of byte `offset` of [`Buffer::text`], the end past it, down to a character
    /// boundary.
    #[must_use]
    pub fn position(&self, offset: usize) -> Position {
        let mut rest = offset;
        for (index, (_, line)) in self.lines.iter().enumerate() {
            if rest <= line.len() {
                return Position::new(index, floor_char(line, rest));
            }
            rest = rest.saturating_sub(line.len().saturating_add(1));
        }
        self.end()
    }

    /// The number of characters before byte `col` of line `line`.
    #[must_use]
    pub fn char_column(&self, line: usize, col: usize) -> usize {
        let text = self.line(line);
        text.get(..floor_char(text, col))
            .map_or(0, |s| s.chars().count())
    }

    /// The byte offset of character `chars` of line `line`, its end past the last character.
    #[must_use]
    pub fn byte_column(&self, line: usize, chars: usize) -> usize {
        let text = self.line(line);
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(col, _)| col)
    }
}

/// `col` clamped to `text` and down to a character boundary.
#[must_use]
pub fn floor_char(text: &str, col: usize) -> usize {
    let mut col = col.min(text.len());
    while !text.is_char_boundary(col) {
        col = col.saturating_sub(1);
    }
    col
}
