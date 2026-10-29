// SPDX-License-Identifier: GPL-3.0-or-later

//! The `.map` tokenizer (MAP-1), written from `docs/spec/map.md`.
//!
//! One tokenizer serves both readers: [`crate::MapFile`], which keeps every byte, and
//! [`crate::qbsp`], which reproduces id's qbsp. Each token records the gap before it, so a
//! reader can ask whether it is on the same line as the token before (MAP-1.7) and get the
//! line number qbsp would report if it is not.

#[cfg(test)]
mod tests;

use core::fmt;

/// The size of qbsp's token buffer; a token holds at most one byte less (MAP-1.6).
pub const MAP_MAX_TOKEN: usize = 128;

/// Where the gap before a token first breaks the line: a LF, a comment or the end of the
/// file (MAP-1.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineBreak {
    /// The 1-based line of the break, counting every LF before it.
    pub line: u32,
    /// The line qbsp names in `Line <n> is incomplete`, which counts only the LFs between
    /// tokens (MAP-1.9).
    pub qbsp_line: u32,
}

/// One token (MAP-1.4, MAP-1.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token<'a> {
    /// Offset of its first byte, the opening quote of a quoted token.
    pub start: usize,
    /// Offset just past its last byte, the closing quote of a quoted token.
    pub end: usize,
    /// Its bytes, without the quotes of a quoted token.
    pub text: &'a [u8],
    /// Whether it was quoted.
    pub quoted: bool,
    /// The 1-based line it starts on, counting every LF before it.
    pub line: u32,
    /// The first line break in the gap before it, if any (MAP-1.7).
    pub gap_break: Option<LineBreak>,
}

impl Token<'_> {
    /// Whether the token's text is `word`. Quotes do not matter: qbsp compares the text
    /// alone, so `"{"` opens an entity as `{` does.
    #[must_use]
    pub fn is(&self, word: &[u8]) -> bool {
        self.text == word
    }
}

/// What [`Lexer::next_token`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lexed<'a> {
    /// A token.
    Token(Token<'a>),
    /// The end of the text; the break is where it ends the line (MAP-1.7).
    End(LineBreak),
}

/// What [`Lexer::skip`] found after the gap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gap {
    /// A token starts here; the gap's first line break, if any.
    Token(Option<LineBreak>),
    /// The text ends here, which breaks the line there or earlier.
    End(LineBreak),
}

/// A token the tokenizer cannot read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexError {
    /// The text ends inside a quoted token (MAP-1.4).
    EofInQuote {
        /// The line the token starts on.
        line: u32,
    },
    /// A token is longer than 127 bytes (MAP-1.6).
    TokenTooLarge {
        /// The line the token starts on.
        line: u32,
        /// The line qbsp reports (MAP-1.9).
        qbsp_line: u32,
    },
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EofInQuote { .. } => f.write_str("EOF inside quoted token"),
            Self::TokenTooLarge { qbsp_line, .. } => {
                write!(f, "Token too large on line {qbsp_line}")
            }
        }
    }
}

impl core::error::Error for LexError {}

/// The text qbsp reads: everything before the first NUL byte (MAP-1.1).
#[must_use]
pub fn map_text(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    bytes.get(..end).unwrap_or_default()
}

/// Whether qbsp treats `byte` as whitespace: 0x01 to 0x20 and, because it compares a signed
/// `char`, 0x80 to 0xFF (MAP-1.2). NUL never reaches it (MAP-1.1).
#[must_use]
pub const fn is_map_space(byte: u8) -> bool {
    byte <= b' ' || byte >= 0x80
}

/// How long a token may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenLimit {
    /// qbsp's: a token over 127 bytes is an error (MAP-1.6).
    Qbsp,
    /// Any length is read.
    Unlimited,
}

/// Reads `.map` tokens one at a time, as qbsp's `GetToken` does (MAP-1).
#[derive(Clone, Debug)]
pub struct Lexer<'a> {
    text: &'a [u8],
    pos: usize,
    line: u32,
    qbsp_line: u32,
    limit: TokenLimit,
}

impl<'a> Lexer<'a> {
    /// A tokenizer over `bytes` up to its first NUL (MAP-1.1), reading tokens up to `limit`.
    #[must_use]
    pub fn new(bytes: &'a [u8], limit: TokenLimit) -> Self {
        Self {
            text: map_text(bytes),
            pos: 0,
            line: 1,
            qbsp_line: 1,
            limit,
        }
    }

    /// The next token if it is on the current line, else the line break, or the end of the
    /// text, that comes first (MAP-1.7).
    ///
    /// # Errors
    ///
    /// The errors of [`Self::token`].
    pub fn same_line_token(&mut self) -> Result<Result<Token<'a>, LineBreak>, LexError> {
        match self.skip() {
            Gap::End(line_break) | Gap::Token(Some(line_break)) => Ok(Err(line_break)),
            Gap::Token(None) => Ok(Ok(self.token(None)?)),
        }
    }

    /// The text being read, without the NUL and what follows it.
    #[must_use]
    pub const fn text(&self) -> &'a [u8] {
        self.text
    }

    /// The offset just past the last token read, or the gap skipped.
    #[must_use]
    pub const fn offset(&self) -> usize {
        self.pos
    }

    fn byte(&self, offset: usize) -> Option<u8> {
        self.text.get(offset).copied()
    }

    fn here(&self) -> LineBreak {
        LineBreak {
            line: self.line,
            qbsp_line: self.qbsp_line,
        }
    }

    /// Skips whitespace and comments up to the next token or the end of the text, and says
    /// where the gap first breaks the line (MAP-1.2, MAP-1.3, MAP-1.7). qbsp checks the
    /// gap before it reads the token, so a reader that needs the token on the current line
    /// checks [`Gap`] before calling [`Lexer::token`].
    pub fn skip(&mut self) -> Gap {
        let mut gap_break = None;
        loop {
            let Some(byte) = self.byte(self.pos) else {
                return Gap::End(gap_break.unwrap_or(self.here()));
            };
            if is_map_space(byte) {
                if byte == b'\n' {
                    gap_break.get_or_insert(self.here());
                    self.line = self.line.saturating_add(1);
                    self.qbsp_line = self.qbsp_line.saturating_add(1);
                }
                self.pos = self.pos.saturating_add(1);
                continue;
            }
            if byte == b'/' && self.byte(self.pos.saturating_add(1)) == Some(b'/') {
                gap_break.get_or_insert(self.here());
                // MAP-1.9: the LF that ends a comment is consumed with it, uncounted by qbsp.
                while let Some(byte) = self.byte(self.pos) {
                    self.pos = self.pos.saturating_add(1);
                    if byte == b'\n' {
                        self.line = self.line.saturating_add(1);
                        break;
                    }
                }
                continue;
            }
            return Gap::Token(gap_break);
        }
    }

    /// The next token, or the end of the text: [`Lexer::skip`] then [`Lexer::token`].
    ///
    /// # Errors
    ///
    /// As [`Lexer::token`].
    pub fn next_token(&mut self) -> Result<Lexed<'a>, LexError> {
        match self.skip() {
            Gap::End(line_break) => Ok(Lexed::End(line_break)),
            Gap::Token(gap_break) => self.token(gap_break).map(Lexed::Token),
        }
    }

    /// Reads the token that [`Lexer::skip`] stopped at; `gap_break` is what it returned.
    ///
    /// Implements MAP-1.4 to MAP-1.6. At the end of the text it returns an empty token.
    ///
    /// # Errors
    ///
    /// [`LexError::EofInQuote`], and [`LexError::TokenTooLarge`] when the lexer was made
    /// with [`TokenLimit::Qbsp`].
    pub fn token(&mut self, gap_break: Option<LineBreak>) -> Result<Token<'a>, LexError> {
        let start = self.pos;
        let line = self.line;
        let too_large = LexError::TokenTooLarge {
            line,
            qbsp_line: self.qbsp_line,
        };
        let quoted = self.byte(start) == Some(b'"');
        let (text_start, text_end, end) = if quoted {
            let text_start = start.saturating_add(1);
            let mut pos = text_start;
            loop {
                match self.byte(pos) {
                    Some(b'"') => break,
                    None => return Err(LexError::EofInQuote { line }),
                    Some(byte) => {
                        if byte == b'\n' {
                            self.line = self.line.saturating_add(1);
                        }
                        pos = pos.saturating_add(1);
                        if self.limit == TokenLimit::Qbsp
                            && pos.saturating_sub(text_start) >= MAP_MAX_TOKEN
                        {
                            return Err(too_large);
                        }
                    }
                }
            }
            (text_start, pos, pos.saturating_add(1))
        } else {
            let mut pos = start;
            while self.byte(pos).is_some_and(|byte| !is_map_space(byte)) {
                pos = pos.saturating_add(1);
                if self.limit == TokenLimit::Qbsp && pos.saturating_sub(start) >= MAP_MAX_TOKEN {
                    return Err(too_large);
                }
            }
            (start, pos, pos)
        };
        self.pos = end;
        Ok(Token {
            start,
            end,
            text: self.text.get(text_start..text_end).unwrap_or_default(),
            quoted,
            line,
            gap_break,
        })
    }
}
