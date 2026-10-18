// SPDX-License-Identifier: GPL-3.0-or-later

//! The vanilla QuakeC lexer.
//!
//! Splits one source file into tokens with byte spans, as id's `qcc` splits it
//! (`docs/spec/language.md`, QC-1 to QC-8). Every name is a [`TokenKind::Name`]: no name is
//! reserved, and the parser decides where a word is a keyword (QC-3.2). `$` commands are
//! lexed here, as `qcc` does, but frame macro references are resolved by the preprocessor,
//! which keeps the frame list of each source file (QC-8.3). Lexical errors are reported as
//! [`Diagnostic`]s with the codes of `docs/spec/language-diagnostics.md`, and lexing always
//! runs to the end of the file.
//!
//! This crate is written from the spec, not ported: the spec's test vectors were recorded
//! against id's `qcc` and the tests here check them.

mod atof;
mod lines;

#[cfg(test)]
mod tests;

pub use atof::atof;
pub use lines::LineIndex;
// The diagnostic vocabulary lives in `qc-diag`; re-exported here so code written against
// `qc_lexer::{Code, Diagnostic, Span}` keeps compiling.
pub use qc_diag::{Code, Diagnostic, Severity, Span};

/// Longest token `qcc` holds, including its terminating NUL.
///
/// `QC_MAX_TOKEN` in QC-3, from `pr_token[2048]` in `qcc/pr_lex.c`. A token of more than
/// `QC_MAX_TOKEN - 1` bytes is an error (QC-27.4).
pub const QC_MAX_TOKEN: usize = 2048;

/// Longest frame name id's `qcc` can store, without its NUL.
///
/// `QC_MAX_FRAME_NAME` in QC-8, from `pr_framemacros[MAX_FRAMES][16]` in `qcc/pr_lex.c`.
pub const QC_MAX_FRAME_NAME: usize = 15;

/// The QuakeC dialect the lexer and parser accept.
///
/// [`Dialect::Vanilla`] is id's QuakeC as `docs/spec/language.md` records it; the lexer's
/// output for it is byte for byte what [`lex`] produces. [`Dialect::Fteqcc`] adds the
/// FTEQCC subset the Quake rerelease uses (`docs/spec/dialect-fteqcc-rerelease.md`,
/// FTE-5 to FTE-8 at the lexical level). [`Dialect::Gmqcc`] is GMQCC's QuakeC, which comes
/// later (COMP-3); at the lexical level it is treated as [`Dialect::Fteqcc`] for now.
///
/// This mirrors `qc_project::Dialect`, which deserialises it from a project file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// id's QuakeC, as id's `qcc` accepts it.
    #[default]
    Vanilla,
    /// FTEQCC's QuakeC (the rerelease subset in the MVP).
    Fteqcc,
    /// GMQCC's QuakeC.
    Gmqcc,
}

impl Dialect {
    /// Whether this dialect lexes and parses the FTEQCC extensions (FTE-1 to FTE-16).
    ///
    /// Vanilla does not; every other dialect does, since GMQCC is a superset of the FTEQCC
    /// subset the rerelease uses (COMP-3).
    #[must_use]
    pub const fn has_fteqcc_extensions(self) -> bool {
        matches!(self, Self::Fteqcc | Self::Gmqcc)
    }
}

/// A punctuation token, in the order the lexer tries them (QC-7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Punct {
    /// `&&`
    AndAnd,
    /// `||`
    OrOr,
    /// `<=`
    LessEq,
    /// `>=`
    GreaterEq,
    /// `==`
    EqEq,
    /// `!=`
    NotEq,
    /// `;`
    Semicolon,
    /// `,`
    Comma,
    /// `!`
    Not,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `-`
    Minus,
    /// `+`
    Plus,
    /// `=`
    Eq,
    /// `[`
    LBracket,
    /// `]`
    RBracket,
    /// `{`
    LBrace,
    /// `}`
    RBrace,
    /// `...`
    Ellipsis,
    /// `.`
    Dot,
    /// `<`
    Less,
    /// `>`
    Greater,
    /// `#`
    Hash,
    /// `&`
    And,
    /// `|`
    Or,
    /// `++` (FTEQCC, FTE-5).
    PlusPlus,
    /// `--` (FTEQCC, FTE-5).
    MinusMinus,
    /// `+=` (FTEQCC, FTE-6).
    PlusEq,
    /// `-=` (FTEQCC, FTE-6).
    MinusEq,
    /// `*=` (FTEQCC, FTE-6).
    StarEq,
    /// `/=` (FTEQCC, FTE-6).
    SlashEq,
    /// `%` (FTEQCC, FTE-7).
    Percent,
    /// `?` (FTEQCC, FTE-8).
    Question,
    /// `:` (FTEQCC, FTE-8).
    Colon,
}

impl Punct {
    /// Every vanilla punctuation token in the order the lexer tries them, id's
    /// `pr_punctuation`.
    ///
    /// Implements QC-7.1: the first entry that matches the input wins.
    pub const ALL: [Self; 27] = [
        Self::AndAnd,
        Self::OrOr,
        Self::LessEq,
        Self::GreaterEq,
        Self::EqEq,
        Self::NotEq,
        Self::Semicolon,
        Self::Comma,
        Self::Not,
        Self::Star,
        Self::Slash,
        Self::LParen,
        Self::RParen,
        Self::Minus,
        Self::Plus,
        Self::Eq,
        Self::LBracket,
        Self::RBracket,
        Self::LBrace,
        Self::RBrace,
        Self::Ellipsis,
        Self::Dot,
        Self::Less,
        Self::Greater,
        Self::Hash,
        Self::And,
        Self::Or,
    ];

    /// The FTEQCC punctuation tokens (FTE-5 to FTE-8), tried before the vanilla tokens so
    /// that the longer FTEQCC operators win the longest match (QC-7.1): `++`, `--`, `+=`,
    /// `-=`, `*=` and `/=` before `+`, `-`, `*` and `/`. The entirely new `%`, `?` and `:`
    /// bytes are vanilla "Unknown punctuation" (QC-7.2), so their order is immaterial. These
    /// are not ported from FTEQCC; they are this project's rules for the FTEQCC subset the
    /// rerelease uses (`docs/spec/dialect-fteqcc-rerelease.md`).
    pub const FTEQCC: [Self; 9] = [
        Self::PlusPlus,
        Self::MinusMinus,
        Self::PlusEq,
        Self::MinusEq,
        Self::StarEq,
        Self::SlashEq,
        Self::Percent,
        Self::Question,
        Self::Colon,
    ];

    /// The token's text.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AndAnd => "&&",
            Self::OrOr => "||",
            Self::LessEq => "<=",
            Self::GreaterEq => ">=",
            Self::EqEq => "==",
            Self::NotEq => "!=",
            Self::Semicolon => ";",
            Self::Comma => ",",
            Self::Not => "!",
            Self::Star => "*",
            Self::Slash => "/",
            Self::LParen => "(",
            Self::RParen => ")",
            Self::Minus => "-",
            Self::Plus => "+",
            Self::Eq => "=",
            Self::LBracket => "[",
            Self::RBracket => "]",
            Self::LBrace => "{",
            Self::RBrace => "}",
            Self::Ellipsis => "...",
            Self::Dot => ".",
            Self::Less => "<",
            Self::Greater => ">",
            Self::Hash => "#",
            Self::And => "&",
            Self::Or => "|",
            Self::PlusPlus => "++",
            Self::MinusMinus => "--",
            Self::PlusEq => "+=",
            Self::MinusEq => "-=",
            Self::StarEq => "*=",
            Self::SlashEq => "/=",
            Self::Percent => "%",
            Self::Question => "?",
            Self::Colon => ":",
        }
    }
}

/// One frame name on a `$frame` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameName {
    /// The name as written.
    pub name: String,
    /// Where it is.
    pub span: Span,
}

/// What a token is.
#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    /// A name (QC-3.1). Keywords are names too (QC-3.2).
    Name(String),
    /// A float literal, its value rounded to 32 bits (QC-4.1).
    Float(f32),
    /// A vector literal (QC-5.1).
    Vector([f32; 3]),
    /// A string literal's contents, escapes replaced (QC-6.2).
    String(Vec<u8>),
    /// A punctuation token (QC-7.1).
    Punct(Punct),
    /// `$word`: a frame macro reference, the word without the `$` (QC-8.5).
    FrameRef(String),
    /// `$frame` and the names after it on its line (QC-8.2).
    Frames(Vec<FrameName>),
}

/// A token and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    /// What it is.
    pub kind: TokenKind,
    /// Its bytes in the source, delimiters and the `$` included.
    pub span: Span,
    /// Whether only whitespace stands between the start of its line and the token, so a `#`
    /// token may start a directive (QC-9.1).
    pub line_start: bool,
}

/// The tokens of one file and the diagnostics found while lexing it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lexed {
    /// The tokens in source order. Spans are increasing and do not overlap.
    pub tokens: Vec<Token>,
    /// Diagnostics in source order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Splits `source` into tokens, in the vanilla dialect.
///
/// Implements QC-1, QC-2, QC-3.1, QC-4, QC-5, QC-6, QC-7 and the lexical part of QC-8, with
/// the divergences QC-27.2, QC-27.4, QC-27.5, QC-27.6 and QC-27.14. Never fails: an error is
/// reported and lexing carries on after it. Equivalent to
/// [`lex_with`]`(source, Dialect::Vanilla)`.
#[must_use]
pub fn lex(source: &[u8]) -> Lexed {
    lex_with(source, Dialect::Vanilla)
}

/// Splits `source` into tokens in `dialect`.
///
/// With [`Dialect::Vanilla`] the output is byte for byte what [`lex`] produces. A dialect
/// with FTEQCC extensions ([`Dialect::has_fteqcc_extensions`]) also lexes the FTEQCC
/// punctuation `++`, `--`, `+=`, `-=`, `*=`, `/=`, `%`, `?` and `:`
/// (`docs/spec/dialect-fteqcc-rerelease.md`, FTE-5 to FTE-8); every other rule is the same.
#[must_use]
pub fn lex_with(source: &[u8], dialect: Dialect) -> Lexed {
    let text = source
        .iter()
        .position(|&b| b == 0)
        .and_then(|nul| source.get(..nul))
        .unwrap_or(source);
    let mut lexer = Lexer {
        text,
        pos: 0,
        dialect,
        out: Lexed::default(),
    };
    lexer.run();
    lexer.out
}

/// Whitespace as `qcc` sees it: every byte `<= ' '` as a signed `char`, so 0x01 to 0x20 and
/// 0x80 to 0xFF (QC-1.3, QC-2.1).
#[must_use]
pub const fn is_whitespace(b: u8) -> bool {
    b <= b' ' || b >= 0x80
}

const fn is_name_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

const fn is_name_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The `$` commands `qcc` skips with the rest of their line (QC-8.4).
const IGNORED_COMMANDS: [&str; 6] = ["cd", "origin", "base", "flags", "scale", "skin"];

struct Lexer<'a> {
    text: &'a [u8],
    pos: usize,
    dialect: Dialect,
    out: Lexed,
}

impl Lexer<'_> {
    fn peek(&self, ahead: usize) -> Option<u8> {
        self.text.get(self.pos.checked_add(ahead)?).copied()
    }

    fn bump(&mut self, n: usize) {
        self.pos = self.pos.saturating_add(n).min(self.text.len());
    }

    fn bytes(&self, span: Span) -> &[u8] {
        self.text.get(span.range()).unwrap_or_default()
    }

    fn error(&mut self, code: Code, message: impl Into<String>, span: Span) {
        self.out.diagnostics.push(Diagnostic {
            code,
            message: message.into(),
            span,
        });
    }

    fn check_length(&mut self, len: usize, span: Span) {
        if len > QC_MAX_TOKEN.saturating_sub(1) {
            self.error(Code::E0502, "token too long", span);
        }
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        let span = Span::new(start, self.pos);
        let line_start = self
            .text
            .get(..start)
            .unwrap_or_default()
            .iter()
            .rev()
            .take_while(|&&b| b != b'\n')
            .all(|&b| is_whitespace(b));
        self.out.tokens.push(Token {
            kind,
            span,
            line_start,
        });
    }

    fn run(&mut self) {
        loop {
            self.skip_trivia();
            let Some(c) = self.peek(0) else { return };
            let start = self.pos;
            if c == b'"' {
                self.string();
            } else if c == b'\'' {
                self.vector();
            } else if c.is_ascii_digit()
                || (c == b'-' && self.peek(1).is_some_and(|d| d.is_ascii_digit()))
            {
                let value = self.number();
                self.push(TokenKind::Float(value), start);
            } else if is_name_start(c) {
                self.name();
            } else if c == b'$' {
                self.dollar();
            } else {
                self.punctuation();
            }
        }
    }

    /// Skips whitespace and comments (QC-2).
    fn skip_trivia(&mut self) {
        loop {
            while self.peek(0).is_some_and(is_whitespace) {
                self.bump(1);
            }
            match (self.peek(0), self.peek(1)) {
                (Some(b'/'), Some(b'/')) => {
                    while self.peek(0).is_some_and(|b| b != b'\n') {
                        self.bump(1);
                    }
                }
                (Some(b'/'), Some(b'*')) => self.block_comment(),
                _ => return,
            }
        }
    }

    /// A `/*` comment ends at the first `*/` found from the `*` of the `/*` (QC-2.3), so `/*/`
    /// is a whole comment. One with no end is an error at the `/*` (QC-27.2).
    fn block_comment(&mut self) {
        let start = self.pos;
        let body = self.pos.saturating_add(1);
        let close = self
            .text
            .get(body..)
            .unwrap_or_default()
            .windows(2)
            .position(|w| w == b"*/");
        if let Some(at) = close {
            self.pos = body.saturating_add(at).saturating_add(2);
        } else {
            self.pos = self.text.len();
            self.error(
                Code::E0501,
                "unterminated comment",
                Span::new(start, start.saturating_add(2)),
            );
        }
    }

    /// A string literal (QC-6).
    fn string(&mut self) {
        let start = self.pos;
        self.bump(1);
        let mut contents = Vec::new();
        loop {
            match self.peek(0) {
                None => {
                    self.error(Code::E0101, "EOF inside quote", Span::new(start, self.pos));
                    break;
                }
                Some(b'\n') => {
                    self.error(
                        Code::E0102,
                        "newline inside quote",
                        Span::new(start, self.pos),
                    );
                    break;
                }
                Some(b'"') => {
                    self.bump(1);
                    break;
                }
                Some(b'\\') => {
                    let escape = self.pos;
                    self.bump(1);
                    match self.peek(0) {
                        None => {
                            self.error(Code::E0101, "EOF inside quote", Span::new(start, self.pos));
                            break;
                        }
                        Some(b'n') => {
                            contents.push(b'\n');
                            self.bump(1);
                        }
                        Some(b'"') => {
                            contents.push(b'"');
                            self.bump(1);
                        }
                        // The line break is left for the next turn, which reports it.
                        Some(b'\n') => self.error(
                            Code::E0103,
                            "Unknown escape char",
                            Span::new(escape, self.pos),
                        ),
                        Some(_) => {
                            self.bump(1);
                            self.error(
                                Code::E0103,
                                "Unknown escape char",
                                Span::new(escape, self.pos),
                            );
                        }
                    }
                }
                Some(b) => {
                    contents.push(b);
                    self.bump(1);
                }
            }
        }
        self.check_length(contents.len(), Span::new(start, self.pos));
        self.push(TokenKind::String(contents), start);
    }

    /// A number as `PR_LexNumber` reads it: the first byte whatever it is, then digits and
    /// dots, converted with `atof` (QC-4.1, QC-5.1).
    fn number(&mut self) -> f32 {
        let start = self.pos;
        self.bump(1);
        while self
            .peek(0)
            .is_some_and(|b| b.is_ascii_digit() || b == b'.')
        {
            self.bump(1);
        }
        let span = Span::new(start, self.pos);
        self.check_length(self.pos.saturating_sub(start), span);
        atof(self.bytes(span))
    }

    /// A vector literal (QC-5).
    fn vector(&mut self) {
        let start = self.pos;
        self.bump(1);
        let mut value = [0.0_f32; 3];
        for component in &mut value {
            if self.peek(0).is_some() {
                *component = self.number();
            }
            self.skip_trivia();
        }
        if self.peek(0) == Some(b'\'') {
            self.bump(1);
        } else {
            self.error(Code::E0104, "Bad vector", Span::new(start, self.pos));
        }
        self.push(TokenKind::Vector(value), start);
    }

    /// A name (QC-3.1).
    fn name(&mut self) {
        let start = self.pos;
        while self.peek(0).is_some_and(is_name_char) {
            self.bump(1);
        }
        let span = Span::new(start, self.pos);
        self.check_length(span.end.saturating_sub(start), span);
        let name = self.bytes(span).iter().copied().map(char::from).collect();
        self.push(TokenKind::Name(name), start);
    }

    /// The next word on the line, as `PR_SimpleGetToken` reads it: blanks skipped, then bytes
    /// up to whitespace, `,` or `;`. `None` at the end of the line (QC-8.1).
    fn word(&mut self) -> Option<Span> {
        while let Some(b) = self.peek(0) {
            if b == b'\n' {
                return None;
            }
            if !is_whitespace(b) {
                break;
            }
            self.bump(1);
        }
        self.peek(0)?;
        let start = self.pos;
        while self
            .peek(0)
            .is_some_and(|b| !is_whitespace(b) && b != b',' && b != b';')
        {
            self.bump(1);
        }
        Some(Span::new(start, self.pos))
    }

    fn word_text(&self, span: Span) -> String {
        self.bytes(span).iter().copied().map(char::from).collect()
    }

    /// A `$` command or frame macro reference (QC-8).
    fn dollar(&mut self) {
        let start = self.pos;
        self.bump(1);
        let Some(word) = self.word() else {
            self.error(Code::E0107, "hanging $", Span::new(start, self.pos));
            return;
        };
        self.check_length(word.end.saturating_sub(word.start), word);
        let command = self.word_text(word);
        if command == "frame" {
            self.frames(start);
        } else if IGNORED_COMMANDS.contains(&command.as_str()) {
            // QC-27.14: the rest of the line goes, `,` and `;` included.
            while self.peek(0).is_some_and(|b| b != b'\n') {
                self.bump(1);
            }
        } else {
            self.push(TokenKind::FrameRef(command), start);
        }
    }

    /// The names of a `$frame` line (QC-8.2), with the errors of QC-27.5 and the warning of
    /// QC-27.6.
    fn frames(&mut self, start: usize) {
        let mut names = Vec::new();
        while let Some(span) = self.word() {
            if span.start == span.end {
                self.bump(1);
                self.error(
                    Code::E0503,
                    "`,` or `;` in a $frame list",
                    Span::new(span.start, self.pos),
                );
                continue;
            }
            self.check_length(span.end.saturating_sub(span.start), span);
            let name = self.word_text(span);
            if name.len() > QC_MAX_FRAME_NAME {
                self.error(
                    Code::W0101,
                    "frame name longer than 15 bytes; id's qcc cannot look it up",
                    span,
                );
            }
            names.push(FrameName { name, span });
        }
        self.push(TokenKind::Frames(names), start);
    }

    /// Punctuation (QC-7.1), or an unknown byte (QC-7.2), which is skipped.
    ///
    /// In an FTEQCC dialect the FTEQCC punctuators ([`Punct::FTEQCC`]) are tried before the
    /// vanilla ones, so `++`, `+=` and the rest win the longest match over `+`, `-`, `*` and
    /// `/` (FTE-5 to FTE-8).
    fn punctuation(&mut self) {
        let start = self.pos;
        let rest = self.text.get(start..).unwrap_or_default();
        let fteqcc = self.dialect.has_fteqcc_extensions();
        let found = fteqcc
            .then(|| {
                Punct::FTEQCC
                    .into_iter()
                    .find(|p| rest.starts_with(p.as_str().as_bytes()))
            })
            .flatten()
            .or_else(|| {
                Punct::ALL
                    .into_iter()
                    .find(|p| rest.starts_with(p.as_str().as_bytes()))
            });
        if let Some(punct) = found {
            self.bump(punct.as_str().len());
            self.push(TokenKind::Punct(punct), start);
        } else {
            self.bump(1);
            self.error(
                Code::E0105,
                "Unknown punctuation",
                Span::new(start, self.pos),
            );
        }
    }
}
