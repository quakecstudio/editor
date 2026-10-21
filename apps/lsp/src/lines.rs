// SPDX-License-Identifier: GPL-3.0-or-later

//! Byte offsets to protocol positions and back.
//!
//! LSP 3.18, "Text Documents": "the protocol specifies the following end-of-line sequences:
//! '\n', '\r\n' and '\r'", and a position's character offset counts code units of the encoding
//! the client and server agree on, UTF-16 unless both pick another ("the only mandatory
//! encoding is UTF-16"). "If the character value is greater than the line length it defaults
//! back to the line length." (`specs/lsp/3.18/types/textDocuments.md`, `types/position.md`).
//!
//! QuakeC source is bytes, not always UTF-8: id's character set puts graphics at 0x80 to 0xFF.
//! A file the editor has open is the UTF-8 text the client sent; a file read from disk is
//! counted as the client decodes it as UTF-8, each sequence of bytes that is not UTF-8 being
//! one U+FFFD, one code unit in either encoding, as `String::from_utf8_lossy` and a WHATWG
//! decoder replace it.

use serde::{Deserialize, Serialize};

/// What a position's character offset counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-16 code units, the protocol's default.
    #[default]
    Utf16,
    /// UTF-8 code units, bytes.
    Utf8,
}

impl Encoding {
    /// The `PositionEncodingKind` that names it.
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Utf16 => "utf-16",
            Self::Utf8 => "utf-8",
        }
    }

    /// The code units `text` is in this encoding.
    fn units(self, text: &[u8]) -> usize {
        let mut out = 0_usize;
        for chunk in text.utf8_chunks() {
            let valid = match self {
                Self::Utf16 => chunk.valid().encode_utf16().count(),
                Self::Utf8 => chunk.valid().len(),
            };
            let replacement = usize::from(!chunk.invalid().is_empty());
            out = out.saturating_add(valid).saturating_add(replacement);
        }
        out
    }
}

/// A position in a document: a zero-based line and a character offset in that line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Position {
    /// The line, from 0.
    pub line: u32,
    /// The character offset in the line, in code units of the agreed encoding.
    pub character: u32,
}

/// A range in a document; its end is exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    /// The first position in it.
    pub start: Position,
    /// The position after it.
    pub end: Position,
}

/// Where each line of a text starts.
#[derive(Clone, Debug)]
pub struct LineIndex<'a> {
    text: &'a [u8],
    /// The byte offset each line starts at, the first 0.
    starts: Vec<usize>,
    encoding: Encoding,
}

impl<'a> LineIndex<'a> {
    /// The lines of `text`, positions counted in `encoding`.
    #[must_use]
    pub fn new(text: &'a [u8], encoding: Encoding) -> Self {
        let mut starts = vec![0];
        let mut at = 0_usize;
        while let Some(&b) = text.get(at) {
            at = at.saturating_add(1);
            let next = text.get(at).copied();
            if b == b'\n' || (b == b'\r' && next != Some(b'\n')) {
                starts.push(at);
            }
        }
        Self {
            text,
            starts,
            encoding,
        }
    }

    /// The bytes of line `line` without its line break, from its start.
    fn line(&self, line: usize) -> (usize, &'a [u8]) {
        let start = self.starts.get(line).copied().unwrap_or(self.text.len());
        let next = self.starts.get(line.saturating_add(1)).copied();
        let mut end = next.unwrap_or(self.text.len());
        while end > start
            && self
                .text
                .get(end.saturating_sub(1))
                .is_some_and(|&b| b == b'\n' || b == b'\r')
        {
            end = end.saturating_sub(1);
        }
        (start, self.text.get(start..end).unwrap_or_default())
    }

    /// The position of byte `offset`; an offset past the end is the end.
    #[must_use]
    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self
            .starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let (start, bytes) = self.line(line);
        let within = offset.saturating_sub(start).min(bytes.len());
        let character = self.encoding.units(bytes.get(..within).unwrap_or_default());
        Position {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            character: u32::try_from(character).unwrap_or(u32::MAX),
        }
    }

    /// The range of bytes `start..end`.
    #[must_use]
    pub fn range(&self, start: usize, end: usize) -> Range {
        Range {
            start: self.position(start),
            end: self.position(end),
        }
    }

    /// The byte offset of `position`: a character past the line's end is its end, and a line
    /// past the last is the end of the text.
    #[must_use]
    pub fn offset(&self, position: Position) -> usize {
        let Ok(line) = usize::try_from(position.line) else {
            return self.text.len();
        };
        if line >= self.starts.len() {
            return self.text.len();
        }
        let (start, bytes) = self.line(line);
        let wanted = usize::try_from(position.character).unwrap_or(usize::MAX);
        let mut units = 0_usize;
        let mut at = 0_usize;
        for chunk in bytes.utf8_chunks() {
            for c in chunk.valid().chars() {
                let width = match self.encoding {
                    Encoding::Utf16 => c.len_utf16(),
                    Encoding::Utf8 => c.len_utf8(),
                };
                if units.saturating_add(width) > wanted {
                    return start.saturating_add(at);
                }
                units = units.saturating_add(width);
                at = at.saturating_add(c.len_utf8());
            }
            if !chunk.invalid().is_empty() {
                if units >= wanted {
                    return start.saturating_add(at);
                }
                units = units.saturating_add(1);
                at = at.saturating_add(chunk.invalid().len());
            }
        }
        start.saturating_add(at)
    }
}
