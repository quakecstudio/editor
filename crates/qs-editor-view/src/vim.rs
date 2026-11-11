// SPDX-License-Identifier: GPL-3.0-or-later

//! Vim mode (P4-37, UI-21): the code editor's keys as Vim reads them, a state machine over the
//! buffer and the caret. Each key's meaning is Vim's own help at vim/vim@81ebe329b38e0265f11b62b3fe073a94b5d2eafa
//! (`runtime/doc/motion.txt` and `change.txt`), quoted where a case is decided:
//!
//! - **Modes:** Normal, Insert and Visual (characterwise).
//! - **Motions:** `h` `l` `j` `k`, `0` `^` `$`, `w` `b` `e`, `gg` `G`, each with a count.
//!   motion.txt: "A word consists of a sequence of letters, digits and underscores, or a
//!   sequence of other non-blank characters, separated with white space (spaces, tabs, <EOL>)
//!   ... An empty line is also considered to be a word."
//! - **Operators:** `d`, `c` and `y` with a motion, doubled for whole lines (`dd`, `cc`, `yy`),
//!   or over a Visual selection; `x` is `dl`. A count before the operator and one before the
//!   motion multiply.
//! - **Registers:** `"x` names the register an operator or put uses, `a` to `z`; the unnamed
//!   register `"` always takes the text too. `p` and `P` put after and before, a linewise
//!   register below and above the line.
//! - **Repeat:** `.` repeats the last change: an operator, `x`, a put, or an insert from the key
//!   that began it to its Escape, with its count. A change made over a Visual selection is not
//!   kept for `.`.
//! - `u` asks for the editor's undo.

use std::collections::BTreeMap;

use crate::{Buffer, Position};

/// Vim's mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Keys are commands.
    #[default]
    Normal,
    /// Keys type text.
    Insert,
    /// Motions extend a selection from the anchor to the caret.
    Visual,
}

/// A key as Vim mode reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimKey {
    /// A character key.
    Char(char),
    /// Escape.
    Escape,
    /// Enter.
    Enter,
    /// Backspace.
    Backspace,
}

/// What the editor must do after a key besides what Vim mode did to the buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Nothing more.
    Done,
    /// Undo the last edit in the editor's history (`u`).
    Undo,
}

/// What a register holds: text, or whole lines.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Register {
    Chars(String),
    Lines(String),
}

/// How a motion's range is taken by an operator (motion.txt, "exclusive", "inclusive" and
/// "linewise").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Exclusive,
    Inclusive,
    Linewise,
}

/// Vim mode's state between keys.
#[derive(Clone, Debug, Default)]
pub struct Vim {
    mode: Mode,
    count: Option<usize>,
    operator: Option<(char, usize)>,
    register: Option<char>,
    awaiting_register: bool,
    awaiting_g: bool,
    anchor: Position,
    registers: BTreeMap<char, Register>,
    recording: Vec<VimKey>,
    last_change: Vec<VimKey>,
    replaying: bool,
    visual_change: bool,
}

impl Vim {
    /// Vim mode in Normal mode with empty registers.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The mode.
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// The Visual selection's anchor, in Visual mode.
    #[must_use]
    pub const fn anchor(&self) -> Option<Position> {
        match self.mode {
            Mode::Visual => Some(self.anchor),
            Mode::Normal | Mode::Insert => None,
        }
    }

    /// Takes `key`, changing `buffer` and moving `caret` as Vim does.
    pub fn key(&mut self, buffer: &mut Buffer, caret: &mut Position, key: VimKey) -> Effect {
        let recording = !self.replaying && !self.visual_change;
        if recording && (self.mode == Mode::Insert || !self.recording.is_empty()) {
            self.recording.push(key);
        }
        match self.mode {
            Mode::Insert => {
                self.insert_key(buffer, caret, key);
                Effect::Done
            }
            Mode::Normal | Mode::Visual => self.command_key(buffer, caret, key),
        }
    }

    /// A key in Insert mode.
    fn insert_key(&mut self, buffer: &mut Buffer, caret: &mut Position, key: VimKey) {
        match key {
            VimKey::Char(c) => *caret = buffer.replace(*caret, *caret, &c.to_string()),
            VimKey::Enter => *caret = buffer.replace(*caret, *caret, "\n"),
            VimKey::Backspace => {
                if caret.col > 0 {
                    let before = buffer.previous(*caret);
                    *caret = buffer.replace(before, *caret, "");
                }
            }
            VimKey::Escape => {
                // change.txt, Insert mode's <Esc>: the cursor moves back one character.
                self.mode = Mode::Normal;
                if caret.col > 0 {
                    *caret = buffer.previous(*caret);
                }
                self.finish_change();
            }
        }
    }

    /// Ends the change being recorded, keeping it for `.`.
    fn finish_change(&mut self) {
        if self.visual_change {
            self.visual_change = false;
            self.recording.clear();
            return;
        }
        if !self.replaying && !self.recording.is_empty() {
            self.last_change = std::mem::take(&mut self.recording);
        }
    }

    /// Starts recording a change with `key`, the key that began it.
    fn begin_change(&mut self, keys: &[VimKey]) {
        if !self.replaying && self.recording.is_empty() {
            self.recording.extend_from_slice(keys);
        }
    }

    /// The count typed so far, 1 when none; it is then cleared.
    fn take_count(&mut self) -> usize {
        self.count.take().unwrap_or(1).max(1)
    }

    /// A key in Normal or Visual mode.
    fn command_key(&mut self, buffer: &mut Buffer, caret: &mut Position, key: VimKey) -> Effect {
        let c = match key {
            VimKey::Char(c) => c,
            VimKey::Escape => {
                self.reset_pending();
                self.mode = Mode::Normal;
                return Effect::Done;
            }
            VimKey::Enter => 'j',
            VimKey::Backspace => 'h',
        };
        if self.awaiting_register {
            self.awaiting_register = false;
            if c.is_ascii_lowercase() || c == '"' {
                self.register = Some(c);
            }
            return Effect::Done;
        }
        if self.awaiting_g {
            self.awaiting_g = false;
            if c == 'g' {
                self.motion(buffer, caret, 'g');
            }
            return Effect::Done;
        }
        if c.is_ascii_digit() && (c != '0' || self.count.is_some()) {
            let digit = usize::from(u8::try_from(c).unwrap_or(b'0').saturating_sub(b'0'));
            self.count = Some(
                self.count
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(digit),
            );
            return Effect::Done;
        }
        match c {
            '"' => self.awaiting_register = true,
            'g' => self.awaiting_g = true,
            'h' | 'l' | 'j' | 'k' | '0' | '^' | '$' | 'w' | 'b' | 'e' | 'G' => {
                self.motion(buffer, caret, c);
            }
            'd' | 'c' | 'y' | 'x' if self.mode == Mode::Visual => {
                let c = if c == 'x' { 'd' } else { c };
                // A Visual change is not kept for `.`: its extent is the selection's, which a
                // replay of its keys would not have.
                self.recording.clear();
                self.visual_change = true;
                let (start, end) = order(self.anchor, *caret);
                let end = after_char(buffer, end);
                self.mode = Mode::Normal;
                self.apply(buffer, caret, c, start, end, Kind::Exclusive);
            }
            'd' | 'c' | 'y' => match self.operator {
                Some((op, count)) if op == c => {
                    let lines = count.saturating_mul(self.take_count());
                    self.operator = None;
                    let last = caret.line.saturating_add(lines.saturating_sub(1));
                    let last = last.min(buffer.line_count().saturating_sub(1));
                    let start = Position::new(caret.line, 0);
                    self.apply(
                        buffer,
                        caret,
                        c,
                        start,
                        Position::new(last, 0),
                        Kind::Linewise,
                    );
                }
                _ => {
                    let count = self.take_count();
                    self.begin_change(&counted(count, c));
                    self.operator = Some((c, count));
                }
            },
            'x' => {
                let count = self.take_count();
                self.begin_change(&counted(count, 'x'));
                let mut end = *caret;
                for _ in 0..count {
                    if end.col < buffer.line(end.line).len() {
                        end = buffer.next(end);
                    }
                }
                self.apply(buffer, caret, 'd', *caret, end, Kind::Exclusive);
            }
            'p' | 'P' => {
                let count = self.take_count();
                self.begin_change(&counted(count, c));
                self.put(buffer, caret, c == 'p', count);
                self.finish_change();
            }
            'i' | 'a' | 'A' | 'I' | 'o' | 'O' => {
                self.reset_pending();
                self.begin_change(&[VimKey::Char(c)]);
                self.enter_insert(buffer, caret, c);
            }
            'v' => {
                self.reset_pending();
                self.mode = if self.mode == Mode::Visual {
                    Mode::Normal
                } else {
                    self.anchor = *caret;
                    Mode::Visual
                };
            }
            'u' => {
                self.reset_pending();
                return Effect::Undo;
            }
            '.' => {
                let count = self.take_count();
                let keys = self.last_change.clone();
                self.replaying = true;
                for _ in 0..count {
                    for key in &keys {
                        self.key(buffer, caret, *key);
                    }
                }
                self.replaying = false;
            }
            _ => self.reset_pending(),
        }
        Effect::Done
    }

    /// Forgets a pending count, operator and register.
    fn reset_pending(&mut self) {
        self.count = None;
        self.operator = None;
        self.register = None;
        if !self.replaying {
            self.recording.clear();
        }
    }

    /// Insert mode at the place `c` names (change.txt: `i` before the cursor, `a` after it, `I`
    /// before the first non-blank, `A` at the end of the line, `o` and `O` on a new line below
    /// and above).
    fn enter_insert(&mut self, buffer: &mut Buffer, caret: &mut Position, c: char) {
        let line = buffer.line(caret.line).to_owned();
        match c {
            'a' if !line.is_empty() => *caret = buffer.next(*caret),
            'A' => caret.col = line.len(),
            'I' => caret.col = first_non_blank(&line),
            'o' => {
                *caret = buffer.replace(
                    Position::new(caret.line, line.len()),
                    Position::new(caret.line, line.len()),
                    "\n",
                );
            }
            'O' => {
                let at = Position::new(caret.line, 0);
                buffer.replace(at, at, "\n");
                *caret = at;
            }
            _ => {}
        }
        self.mode = Mode::Insert;
    }

    /// Moves the caret by motion `c`, or with an operator pending applies it over the motion.
    fn motion(&mut self, buffer: &mut Buffer, caret: &mut Position, c: char) {
        let count = self.count.take();
        let pending = self.operator.take();
        let total = pending
            .map_or(1, |(_, n)| n)
            .saturating_mul(count.unwrap_or(1));
        // motion.txt, `cw`: "Special case: When the cursor is in a word, "cw" and "cW" do not
        // include the white space after a word, they only change up to the end of the word."
        let c = match pending {
            Some(('c', _)) if c == 'w' && !is_blank_at(buffer, *caret) => 'e',
            _ => c,
        };
        let line_count = buffer.line_count();
        let (target, kind) = match c {
            'h' => (step_left(buffer, *caret, total), Kind::Exclusive),
            'l' => (
                step_right(buffer, *caret, total, pending.is_some()),
                Kind::Exclusive,
            ),
            'j' => (
                vertical(buffer, *caret, caret.line.saturating_add(total)),
                Kind::Linewise,
            ),
            'k' => (
                vertical(buffer, *caret, caret.line.saturating_sub(total)),
                Kind::Linewise,
            ),
            '0' => (Position::new(caret.line, 0), Kind::Exclusive),
            '^' => (
                Position::new(caret.line, first_non_blank(buffer.line(caret.line))),
                Kind::Exclusive,
            ),
            '$' => {
                let line = caret
                    .line
                    .saturating_add(total.saturating_sub(1))
                    .min(line_count.saturating_sub(1));
                (last_char(buffer, line), Kind::Inclusive)
            }
            'w' => {
                let mut at = *caret;
                for _ in 0..total {
                    at = word_forward(buffer, at);
                }
                // motion.txt, `w` under an operator: a word motion to the next line stops at
                // the end of this one.
                if pending.is_some() && at.line > caret.line && at.col == 0 {
                    at = Position::new(caret.line, buffer.line(caret.line).len());
                }
                if pending.is_none() {
                    at = normal_clamp(buffer, at);
                }
                (at, Kind::Exclusive)
            }
            'b' => {
                let mut at = *caret;
                for _ in 0..total {
                    at = word_backward(buffer, at);
                }
                (at, Kind::Exclusive)
            }
            'e' => {
                let mut at = *caret;
                for _ in 0..total {
                    at = word_end(buffer, at);
                }
                (at, Kind::Inclusive)
            }
            // motion.txt, `G`: "Goto line [count], default last line"; `gg`: "Goto line
            // [count], default first line", each "on the first non-blank character".
            'G' | 'g' => {
                let default = if c == 'G' { line_count } else { 1 };
                let line = count
                    .unwrap_or(default)
                    .clamp(1, line_count)
                    .saturating_sub(1);
                (
                    Position::new(line, first_non_blank(buffer.line(line))),
                    Kind::Linewise,
                )
            }
            _ => return,
        };
        match pending {
            Some((op, _)) => {
                let (start, end) = order(*caret, target);
                let end = if kind == Kind::Inclusive {
                    after_char(buffer, end)
                } else {
                    end
                };
                self.apply(buffer, caret, op, start, end, kind);
            }
            None => *caret = normal_clamp(buffer, target),
        }
    }

    /// Operator `op` over `start` to `end`, whole lines from `start`'s to `end`'s when
    /// `kind` is linewise.
    fn apply(
        &mut self,
        buffer: &mut Buffer,
        caret: &mut Position,
        op: char,
        start: Position,
        end: Position,
        kind: Kind,
    ) {
        let register = self.register.take();
        let linewise = kind == Kind::Linewise;
        let (from, to) = if linewise {
            line_span(buffer, start.line, end.line)
        } else {
            (start, end)
        };
        let text = if linewise {
            let mut lines: Vec<&str> = Vec::new();
            for line in start.line..=end.line {
                lines.push(buffer.line(line));
            }
            Register::Lines(format!("{}\n", lines.join("\n")))
        } else {
            Register::Chars(buffer.slice(from, to))
        };
        if let Some(name) = register.filter(|r| *r != '"') {
            self.registers.insert(name, text.clone());
        }
        self.registers.insert('"', text);
        match op {
            'y' => {
                // A yank changes nothing, so it is not what `.` repeats.
                *caret = normal_clamp(buffer, start);
                if self.visual_change {
                    self.finish_change();
                } else if !self.replaying {
                    self.recording.clear();
                }
            }
            'd' => {
                buffer.replace(from, to, "");
                let line = start.line.min(buffer.line_count().saturating_sub(1));
                *caret = if linewise {
                    Position::new(line, first_non_blank(buffer.line(line)))
                } else {
                    normal_clamp(buffer, start)
                };
                self.finish_change();
            }
            'c' => {
                if linewise {
                    // change.txt, `cc`: the lines are changed into one empty line.
                    let last = end.line.min(buffer.line_count().saturating_sub(1));
                    let last_len = buffer.line(last).len();
                    buffer.replace(
                        Position::new(start.line, 0),
                        Position::new(last, last_len),
                        "",
                    );
                    *caret = Position::new(start.line, 0);
                } else {
                    buffer.replace(from, to, "");
                    *caret = start;
                }
                self.mode = Mode::Insert;
            }
            _ => {}
        }
    }

    /// Puts the register `count` times after (`p`) or before (`P`) the caret.
    fn put(&mut self, buffer: &mut Buffer, caret: &mut Position, after: bool, count: usize) {
        let name = self.register.take().unwrap_or('"');
        let Some(register) = self.registers.get(&name).cloned() else {
            return;
        };
        match register {
            Register::Chars(text) => {
                let text = text.repeat(count);
                let at = if after && !buffer.line(caret.line).is_empty() {
                    buffer.next(*caret)
                } else {
                    *caret
                };
                let end = buffer.replace(at, at, &text);
                *caret = normal_clamp(buffer, buffer.previous(end));
            }
            Register::Lines(text) => {
                let text = text.repeat(count);
                let line = if after {
                    caret.line.saturating_add(1)
                } else {
                    caret.line
                };
                if line >= buffer.line_count() {
                    let last = buffer.line_count().saturating_sub(1);
                    let end = Position::new(last, buffer.line(last).len());
                    let body = text.strip_suffix('\n').unwrap_or(&text);
                    buffer.replace(end, end, &format!("\n{body}"));
                } else {
                    let at = Position::new(line, 0);
                    buffer.replace(at, at, &text);
                }
                *caret = Position::new(line, first_non_blank(buffer.line(line)));
            }
        }
    }
}

/// The keys that typed `count` then `key`, as `.` replays a change: `3x`.
fn counted(count: usize, key: char) -> Vec<VimKey> {
    let mut keys: Vec<VimKey> = if count > 1 {
        count.to_string().chars().map(VimKey::Char).collect()
    } else {
        Vec::new()
    };
    keys.push(VimKey::Char(key));
    keys
}

/// `a` and `b` in order.
fn order(a: Position, b: Position) -> (Position, Position) {
    if b < a { (b, a) } else { (a, b) }
}

/// The position after the character at `at`, or `at` at the end of its line.
fn after_char(buffer: &Buffer, at: Position) -> Position {
    if at.col < buffer.line(at.line).len() {
        buffer.next(at)
    } else {
        at
    }
}

/// The text from line `first` to line `last` with the line break that joins them to the rest,
/// as a linewise delete removes it.
fn line_span(buffer: &Buffer, first: usize, last: usize) -> (Position, Position) {
    let lines = buffer.line_count();
    if last.saturating_add(1) < lines {
        (
            Position::new(first, 0),
            Position::new(last.saturating_add(1), 0),
        )
    } else if first > 0 {
        let above = first.saturating_sub(1);
        (
            Position::new(above, buffer.line(above).len()),
            Position::new(last, buffer.line(last).len()),
        )
    } else {
        (
            Position::new(first, 0),
            Position::new(last, buffer.line(last).len()),
        )
    }
}

/// The byte column of `line`'s first non-blank character, or its end when it has none.
fn first_non_blank(line: &str) -> usize {
    line.char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map_or(line.len(), |(at, _)| at)
}

/// The last character of `line`, or its start when it is empty.
fn last_char(buffer: &Buffer, line: usize) -> Position {
    let end = Position::new(line, buffer.line(line).len());
    if end.col == 0 {
        end
    } else {
        buffer.previous(end)
    }
}

/// `at` as a Normal mode caret, which rests on a character: never past the last one.
fn normal_clamp(buffer: &Buffer, at: Position) -> Position {
    let at = buffer.clamp(at);
    let len = buffer.line(at.line).len();
    if len > 0 && at.col >= len {
        last_char(buffer, at.line)
    } else {
        at
    }
}

/// `count` characters left on the line.
fn step_left(buffer: &Buffer, at: Position, count: usize) -> Position {
    let mut at = at;
    for _ in 0..count {
        if at.col == 0 {
            break;
        }
        at = buffer.previous(at);
    }
    at
}

/// `count` characters right on the line; an operator may reach the line's end.
fn step_right(buffer: &Buffer, at: Position, count: usize, operator: bool) -> Position {
    let len = buffer.line(at.line).len();
    let mut at = at;
    for _ in 0..count {
        if at.col >= len {
            break;
        }
        at = buffer.next(at);
    }
    if operator {
        at
    } else {
        normal_clamp(buffer, at)
    }
}

/// `at` moved to line `line`, keeping its character column as far as the line allows.
fn vertical(buffer: &Buffer, at: Position, line: usize) -> Position {
    let line = line.min(buffer.line_count().saturating_sub(1));
    let chars = buffer.char_column(at.line, at.col);
    normal_clamp(buffer, Position::new(line, buffer.byte_column(line, chars)))
}

/// A character's word class: 0 white space, 1 a letter, digit or underscore (or a letter of
/// another script), 2 any other non-blank character.
fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// The character at `at`, `None` at the end of its line.
fn char_at(buffer: &Buffer, at: Position) -> Option<char> {
    buffer
        .line(at.line)
        .get(at.col..)
        .and_then(|rest| rest.chars().next())
}

/// Whether `at` is on white space or at the end of its line.
fn is_blank_at(buffer: &Buffer, at: Position) -> bool {
    char_at(buffer, at).is_none_or(char::is_whitespace)
}

/// The next position, across lines; `None` at the end of the text.
fn forward(buffer: &Buffer, at: Position) -> Option<Position> {
    if at.col < buffer.line(at.line).len() {
        Some(buffer.next(at))
    } else if at.line.saturating_add(1) < buffer.line_count() {
        Some(Position::new(at.line.saturating_add(1), 0))
    } else {
        None
    }
}

/// The previous position, across lines; `None` at the start of the text.
fn backward(buffer: &Buffer, at: Position) -> Option<Position> {
    if at.col > 0 {
        Some(buffer.previous(at))
    } else if at.line > 0 {
        let line = at.line.saturating_sub(1);
        Some(Position::new(line, buffer.line(line).len()))
    } else {
        None
    }
}

/// The class at `at`, the end of a line counting as white space; an empty line is its own
/// word, class 3.
fn class_at(buffer: &Buffer, at: Position) -> u8 {
    if buffer.line(at.line).is_empty() {
        return 3;
    }
    char_at(buffer, at).map_or(0, class)
}

/// `w`: the start of the next word, or the end of the text.
fn word_forward(buffer: &Buffer, at: Position) -> Position {
    let start = class_at(buffer, at);
    let mut pos = at;
    // Leave the word the caret is on.
    if start != 0 {
        while let Some(next) = forward(buffer, pos) {
            pos = next;
            if next.line != at.line || class_at(buffer, next) != start {
                break;
            }
        }
    }
    // Skip white space, stopping at an empty line or a word.
    loop {
        let here = class_at(buffer, pos);
        if (here != 0 && pos != at) || (here == 3 && pos.line != at.line) {
            return pos;
        }
        match forward(buffer, pos) {
            Some(next) => pos = next,
            None => return pos,
        }
    }
}

/// `b`: the start of this word, or of the one before when at a start already.
fn word_backward(buffer: &Buffer, at: Position) -> Position {
    let mut pos = at;
    // Step back over white space and line ends to the previous word's last character.
    loop {
        match backward(buffer, pos) {
            Some(prev) => pos = prev,
            None => return pos,
        }
        let here = class_at(buffer, pos);
        if here == 3 || here != 0 {
            break;
        }
    }
    if class_at(buffer, pos) == 3 {
        return pos;
    }
    let word = class_at(buffer, pos);
    while let Some(prev) = backward(buffer, pos) {
        if prev.line != pos.line || class_at(buffer, prev) != word {
            break;
        }
        pos = prev;
    }
    pos
}

/// `e`: the end of this word, or of the next when at an end already.
fn word_end(buffer: &Buffer, at: Position) -> Position {
    let mut pos = at;
    // Step forward at least once, then over white space.
    loop {
        match forward(buffer, pos) {
            Some(next) => pos = next,
            None => return normal_clamp(buffer, pos),
        }
        let here = class_at(buffer, pos);
        if here != 0 && here != 3 {
            break;
        }
    }
    let word = class_at(buffer, pos);
    while let Some(next) = forward(buffer, pos) {
        if next.line != pos.line || class_at(buffer, next) != word {
            break;
        }
        pos = next;
    }
    pos
}

#[cfg(test)]
mod tests;
