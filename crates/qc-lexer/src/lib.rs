// SPDX-License-Identifier: GPL-3.0-or-later

//! The QuakeC lexer, for the dialect of id's `qcc` and the FTEQCC dialects.
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
mod fte_string;
mod lines;

#[cfg(test)]
mod tests;

pub use atof::atof;
pub use fte_string::fteqcc_string_bytes;
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
    /// Whether this dialect lexes and parses the FTEQCC extensions (FTE-1 to FTE-22).
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
    /// `->` (FTEQCC, FTE-19.2): member access, read as `.`.
    Arrow,
    /// `~` (FTEQCC, FTE-19.4): bitwise not.
    Tilde,
    /// `&=` (FTEQCC, FTE-19.1).
    AndEq,
    /// `|=` (FTEQCC, FTE-19.1).
    OrEq,
    /// `&~=` (FTEQCC, FTE-19.1): clear the bits of the right side.
    AndNotEq,
    /// `(+)` (FTEQCC, FTE-19.2): read as `|=`.
    SetBits,
    /// `(-)` (FTEQCC, FTE-19.2): read as `&~=`.
    ClearBits,
    /// `^` (FTEQCC, FTE-19.1): bitwise exclusive or.
    Caret,
    /// `^=` (FTEQCC, FTE-19.1).
    CaretEq,
    /// `>>` (FTEQCC, FTE-19.1): shift right.
    Shr,
    /// `>>=` (FTEQCC, FTE-19.1).
    ShrEq,
    /// `<<` (FTEQCC, FTE-19.1): shift left.
    Shl,
    /// `<<=` (FTEQCC, FTE-19.1).
    ShlEq,
    /// `::` (FTEQCC, FTEF-13.1): a class member's scope, `Class::member`.
    ColonColon,
    /// `..` (FTEQCC, FTEF-18.4, FTEF-18.10): a range, `[++ a .. b]` and `case a .. b:`.
    DotDot,
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

    /// The FTEQCC punctuation tokens (FTE-5 to FTE-8, FTE-19.1), tried before the vanilla
    /// tokens so that the longer FTEQCC operators win the longest match (QC-7.1): `++`, `--`,
    /// `+=`, `-=`, `*=` and `/=` before `+`, `-`, `*` and `/`; `(+)` and `(-)` before `(`;
    /// `&~=` and `&=` before `&`; `|=` before `|`; `->` before `-`; and `<<=`, `<<`, `>>=`
    /// and `>>` before `<` and `>`. FTEQCC's own table (`pr_punctuation` in `qcc_pr_lex.c`)
    /// keeps the same rule: "longer symbols must be before a shorter partial match". The
    /// entirely new `%`, `?`, `:`, `^` and `~` bytes are vanilla "Unknown punctuation"
    /// (QC-7.2), so their order among themselves is immaterial. These are not ported from
    /// FTEQCC; they are this project's rules for the FTEQCC subset the rerelease uses
    /// (`docs/spec/dialect-fteqcc-rerelease.md`).
    pub const FTEQCC: [Self; 25] = [
        Self::PlusPlus,
        Self::MinusMinus,
        Self::PlusEq,
        Self::MinusEq,
        Self::StarEq,
        Self::SlashEq,
        Self::SetBits,
        Self::ClearBits,
        Self::OrEq,
        Self::AndNotEq,
        Self::AndEq,
        Self::Arrow,
        Self::CaretEq,
        Self::Caret,
        Self::ShlEq,
        Self::Shl,
        Self::ShrEq,
        Self::Shr,
        Self::Tilde,
        Self::Percent,
        Self::Question,
        Self::ColonColon,
        Self::Colon,
        // FTEQCC's `pr_punctuation` lists `"...", "..", "."`, longest first.
        Self::Ellipsis,
        Self::DotDot,
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
            Self::Arrow => "->",
            Self::Tilde => "~",
            Self::AndEq => "&=",
            Self::OrEq => "|=",
            Self::AndNotEq => "&~=",
            Self::SetBits => "(+)",
            Self::ClearBits => "(-)",
            Self::Caret => "^",
            Self::CaretEq => "^=",
            Self::Shr => ">>",
            Self::ShrEq => ">>=",
            Self::Shl => "<<",
            Self::ShlEq => "<<=",
            Self::ColonColon => "::",
            Self::DotDot => "..",
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
    /// `$frame` and the names after it on its line (QC-8.2), or FTEQCC's `$framesave`
    /// (FTEF-18.14).
    Frames(Vec<FrameName>),
    /// One of FTEQCC's further `$` commands, which change the frame numbering (FTEF-18.16).
    FrameCommand(FrameCommand),
}

/// An FTEQCC `$` command that changes how the next frame names are numbered (FTEF-18.16):
/// `QCC_PR_LexGrab`'s cases beyond `$frame` and the ignored model commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameCommand {
    /// `$flush`: the next frame name is numbered 0 again.
    Flush,
    /// `$frame_reset`: every frame macro is forgotten and the next is numbered 0.
    Reset,
    /// `$framevalue N`: the next frame name is numbered `N`, the word read with `atoi`.
    Value(String),
    /// `$framerestore NAME`: the next frame name is numbered as the macro `NAME` is.
    Restore(String),
    /// `$modelname NAME`: a new model, whose name becomes a frame macro of its own.
    ModelName(String),
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
/// punctuation of [`Punct::FTEQCC`] (`docs/spec/dialect-fteqcc-rerelease.md`, FTE-5 to
/// FTE-8, FTE-19.1), a number that starts with `.` or ends with an `f` suffix (FTE-20.1,
/// FTE-20.2), and the further string escapes of FTE-20.4; and the full dialect's tokens of
/// `docs/spec/dialect-fteqcc.md`: `::`, line continuations, character constants and
/// two-component vectors, every escape, and FTEQCC's number forms (FTEF-3, FTEF-13). Every
/// other rule is the same.
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

/// `(float)atof("0x<whole>.<fraction>")` for an FTEQCC hex number with a fraction (FTE-23.2):
/// C's `atof` reads `0x` as a hexadecimal float, so each decimal digit `QCC_PR_LexNumber` took
/// after the `.` is a hex digit worth a sixteenth of the one before; `whole` is the hex digits
/// before the `.`, read in full, as `atof` reads them. The value is exact up to 2^53, where
/// `atof` rounds once and this rounds at each digit; no builtin number is that large.
fn hex_float(whole: &[u8], fraction: &[u8]) -> f32 {
    let mut value = 0.0_f64;
    for digit in whole.iter().filter_map(|&b| char::from(b).to_digit(16)) {
        value = value * 16.0 + f64::from(digit);
    }
    let mut scale = 1.0_f64;
    for digit in fraction {
        scale /= 16.0;
        value += f64::from(digit.wrapping_sub(b'0')) * scale;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec FTE-23.2: `(float)atof(...)` rounds the double to a float"
    )]
    let value = value as f32;
    value
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

    /// Whether the byte at `at` is on a line that starts, after white space, with `#`: a
    /// directive's line (QC-9.1).
    fn on_directive_line(&self, at: usize) -> bool {
        let before = self.text.get(..at).unwrap_or_default();
        let line = before
            .iter()
            .rposition(|&b| b == b'\n')
            .and_then(|nl| before.get(nl.saturating_add(1)..))
            .unwrap_or(before);
        line.iter().find(|&&b| !is_whitespace(b)) == Some(&b'#')
    }

    fn run(&mut self) {
        loop {
            self.skip_trivia();
            let Some(c) = self.peek(0) else { return };
            let start = self.pos;
            // FTEF-13.7: an `L` prefix is read only after another part of a string, which a
            // directive's string (an `#include` file, a `#warning` text) is not.
            let joins = self.out.tokens.last().is_some_and(|last| {
                let same_line = !self.bytes(Span::new(last.span.end, start)).contains(&b'\n');
                matches!(last.kind, TokenKind::String(_))
                    && (same_line || !self.on_directive_line(last.span.start))
            });
            let fte_string = self
                .dialect
                .has_fteqcc_extensions()
                .then(|| self.fte_string_prefix(joins))
                .flatten();
            if let Some(prefix) = fte_string {
                self.fte_string(prefix);
            } else if c == b'"' {
                self.string();
            } else if c == b'\'' {
                self.vector();
            } else if c.is_ascii_digit()
                || (c == b'-' && self.peek(1).is_some_and(|d| d.is_ascii_digit()))
                // FTE-20.1: in FTEQCC a `.` before a digit starts a number, `.5`.
                || (self.dialect.has_fteqcc_extensions()
                    && c == b'.'
                    && self.peek(1).is_some_and(|d| d.is_ascii_digit()))
            {
                let value = if self.dialect.has_fteqcc_extensions() {
                    self.fteqcc_number()
                } else {
                    self.number()
                };
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
                // FTEF-13.2: in FTEQCC a `\` before a line break joins the lines. FTEQCC skips
                // the LF of a CR LF with them; here it is whitespace, skipped by the next turn.
                (Some(b'\\'), Some(b'\n' | b'\r')) if self.dialect.has_fteqcc_extensions() => {
                    self.bump(2);
                }
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

    /// A vanilla string literal (QC-6); an FTEQCC one is [`Self::fte_string`]'s.
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
    /// dots, converted with `atof` (QC-4.1, QC-5.1). In an FTEQCC dialect, where only a vector
    /// component is read here, a trailing lowercase `f`, as `QCC_PR_LexFloat` skips it, is part
    /// of the token but not of the value (FTEF-13.6).
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
        let value = atof(self.bytes(span));
        if self.dialect.has_fteqcc_extensions() && self.peek(0) == Some(b'f') {
            self.bump(1);
        }
        self.check_length(self.pos.saturating_sub(start), span);
        value
    }

    /// A number as FTEQCC's `QCC_PR_LexNumber` reads it (FTEF-3.1, FTEF-3.2): an optional
    /// `-`, then `0x` for base 16 or a leading `0` for base 8, the digits of the base, and a
    /// type suffix (`i`, `u`, `l` in their combinations, `f` or `d`) that is part of the token
    /// but not of the value; a `.` not followed by another `.` starts a fraction, read with
    /// `atof` and ended by an optional `f` or `d`. The value is kept as a float: the literal's
    /// type is the compiler's (FTEF-3.5).
    fn fteqcc_number(&mut self) -> f32 {
        let start = self.pos;
        let negative = self.peek(0) == Some(b'-');
        if negative {
            self.bump(1);
        }
        let base: u64 = match (self.peek(0), self.peek(1)) {
            (Some(b'0'), Some(b'x' | b'X')) => {
                self.bump(2);
                16
            }
            (Some(b'0'), _) => {
                self.bump(1);
                8
            }
            _ => 10,
        };
        let digits = self.pos;
        let mut num: u64 = 0;
        while let Some(c) = self.peek(0) {
            let digit = match c {
                b'0'..=b'9' => Some(c.wrapping_sub(b'0')),
                b'a'..=b'z' => Some(c.wrapping_sub(b'a').wrapping_add(10)),
                b'A'..=b'Z' => Some(c.wrapping_sub(b'A').wrapping_add(10)),
                _ => None,
            }
            .filter(|d| u64::from(*d) < base);
            if let Some(digit) = digit {
                num = num.wrapping_mul(base).wrapping_add(u64::from(digit));
                self.bump(1);
                continue;
            }
            if c == b'.' && self.peek(1) != Some(b'.') {
                let whole = Span::new(digits, self.pos);
                self.bump(1);
                let fraction = self.pos;
                while self.peek(0).is_some_and(|b| b.is_ascii_digit()) {
                    self.bump(1);
                }
                let value = if base == 16 {
                    // FTE-23.2: C's `atof` reads `0x` and a fraction as a hexadecimal float.
                    let fraction = self.bytes(Span::new(fraction, self.pos));
                    let value = hex_float(self.bytes(whole), fraction);
                    if negative { -value } else { value }
                } else {
                    atof(self.bytes(Span::new(start, self.pos)))
                };
                if matches!(self.peek(0), Some(b'f' | b'F' | b'd' | b'D')) {
                    self.bump(1);
                }
                self.check_length(self.pos.saturating_sub(start), Span::new(start, self.pos));
                return value;
            }
            match c {
                b'f' | b'F' | b'd' | b'D' => self.bump(1),
                b'i' | b'I' | b'u' | b'U' | b'l' | b'L' => self.integer_suffix(c),
                _ => {}
            }
            break;
        }
        self.check_length(self.pos.saturating_sub(start), Span::new(start, self.pos));
        // `(float)(num*sign)`: the whole number rounded to the nearest float.
        let value = num.to_string().parse::<f32>().unwrap_or(f32::MAX);
        if negative { -value } else { value }
    }

    /// The integer suffix of an FTEQCC number, whose first byte `c` is next: `i` or `u`, then
    /// an optional `l` or `ll`; or `l` or `ll`, then an optional `i` or `u` (FTEF-3.1).
    fn integer_suffix(&mut self, c: u8) {
        self.bump(1);
        if matches!(c, b'l' | b'L') {
            if self.peek(0) == Some(c) {
                self.bump(1);
            }
            if matches!(self.peek(0), Some(b'i' | b'I' | b'u' | b'U')) {
                self.bump(1);
            }
        } else if let Some(l @ (b'l' | b'L')) = self.peek(0) {
            self.bump(1);
            if self.peek(0) == Some(l) {
                self.bump(1);
            }
        }
    }

    /// A vector literal (QC-5).
    ///
    /// In an FTEQCC dialect a quote also holds a character constant, `'a'` or `'\n'`, whose
    /// value is the character's code, and a vector may have two components, `'8 8'`, the
    /// third 0 (FTEF-13.3).
    fn vector(&mut self) {
        let start = self.pos;
        if self.dialect.has_fteqcc_extensions() && self.character() {
            return;
        }
        self.bump(1);
        let mut value = [0.0_f32; 3];
        let fteqcc = self.dialect.has_fteqcc_extensions();
        for (index, component) in value.iter_mut().enumerate() {
            if fteqcc && index == 2 && self.peek(0) == Some(b'\'') {
                break;
            }
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

    /// An FTEQCC character constant at a `'` (FTEF-13.3), as `QCC_PR_LexVector` reads one:
    /// `'\c'`, an escape read as a string's is, errors included (FTEF-13.13); one byte and a
    /// closing `'`; or a UTF-8 sequence. Pushes
    /// it as a float token holding the character's code and returns true; false, consuming
    /// nothing, when the quote holds a vector.
    fn character(&mut self) -> bool {
        let start = self.pos;
        let (Some(first), second) = (self.peek(1), self.peek(2)) else {
            return false;
        };
        let code = if first == b'\\' {
            let escape = start.saturating_add(1);
            self.bump(2);
            // FTEQCC stops at "EOF inside quote", before it looks for the closing `'`.
            let reported = self.out.diagnostics.len();
            let code = self.fte_escape(start, escape);
            let ended = self
                .out
                .diagnostics
                .get(reported..)
                .is_some_and(|new| new.iter().any(|d| d.code == Code::E0101));
            if self.peek(0) == Some(b'\'') {
                self.bump(1);
            } else if !ended {
                self.error(
                    Code::E0104,
                    "Bad character constant",
                    Span::new(start, self.pos),
                );
            }
            code
        } else if first >= 0x80 {
            // FTEF-13.14: `pr_file_p+=b+1;` skips the sequence and one byte more, unread; a
            // byte that starts no sequence is `0xFFFD` and is skipped alone.
            let rest = self.text.get(start.saturating_add(1)..).unwrap_or_default();
            let (code, skip) = match fte_string::utf8_check(rest) {
                Some((code, len)) => (code, len.saturating_add(2)),
                None => (0xfffd, 2),
            };
            self.bump(skip);
            code
        } else if second == Some(b'\'') {
            self.bump(3);
            u32::from(first)
        } else {
            return false;
        };
        let value = code.to_string().parse::<f32>().unwrap_or(f32::MAX);
        self.push(TokenKind::Float(value), start);
        true
    }

    /// A name (QC-3.1).
    fn name(&mut self) {
        let start = self.pos;
        while self.peek(0).is_some_and(is_name_char) {
            self.bump(1);
        }
        let span = Span::new(start, self.pos);
        self.check_length(span.end.saturating_sub(start), span);
        let name: String = self.bytes(span).iter().copied().map(char::from).collect();
        let message = self.dialect.has_fteqcc_extensions()
            && matches!(name.as_str(), "warning" | "error" | "message")
            && self.out.tokens.last().is_some_and(|t| {
                t.kind == TokenKind::Punct(Punct::Hash)
                    && t.line_start
                    && !self.bytes(Span::new(t.span.end, start)).contains(&b'\n')
            });
        self.push(TokenKind::Name(name), start);
        if message {
            self.message_text();
        }
    }

    /// The rest of a `#warning`, `#error` or `#message` line, read raw as FTEQCC reads it
    /// (FTEF-14.3): a string token of its bytes, blanks before it skipped, up to the line
    /// break. Nothing when the line ends at once.
    fn message_text(&mut self) {
        while self.peek(0).is_some_and(|b| b == b' ' || b == b'\t') {
            self.bump(1);
        }
        let start = self.pos;
        while self.peek(0).is_some_and(|b| b != b'\n' && b != b'\r') {
            self.bump(1);
        }
        if self.pos > start {
            let text = self.bytes(Span::new(start, self.pos)).to_vec();
            self.push(TokenKind::String(text), start);
        }
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

    /// The next frame macro word on the line, as FTEQCC's `QCC_PR_LexMacroName` reads it
    /// (FTEF-18.13): whitespace skipped, then the bytes above `' '` (as a signed `char`) up to
    /// `,`, `;`, `&`, `|`, `(`, `)`, `]` or `..`. `None`, taking nothing more, at the end of the
    /// line, at a comment, or when no byte is taken.
    fn macro_name(&mut self) -> Option<Span> {
        while let Some(b) = self.peek(0) {
            if b == b'\n' {
                return None;
            }
            if !matches!(b, b' ' | b'\r' | b'\t' | 0x0b) {
                break;
            }
            self.bump(1);
        }
        // The comment is left for the main loop, which skips it as trivia (QC-2).
        if self.peek(0) == Some(b'/') && matches!(self.peek(1), Some(b'/' | b'*')) {
            return None;
        }
        let start = self.pos;
        while let Some(b) = self.peek(0) {
            let stop = !(0x21..0x80).contains(&b)
                || matches!(b, b',' | b';' | b'&' | b'|' | b')' | b'(' | b']')
                || (b == b'.' && self.peek(1) == Some(b'.'));
            if stop {
                break;
            }
            self.bump(1);
        }
        (self.pos > start).then(|| Span::new(start, self.pos))
    }

    /// A `$` command or frame macro reference (QC-8).
    ///
    /// In an FTEQCC dialect the word after the `$` is read as `QCC_PR_LexGrab` reads it: it
    /// must follow the `$` at once, and it ends where FTEQCC's frame macro names end
    /// (FTEF-18.13); `$framesave` is `$frame` (FTEF-18.14), the further commands of FTEF-18.16
    /// are tokens of their own, and an ignored command takes only the words after it.
    fn dollar(&mut self) {
        let start = self.pos;
        self.bump(1);
        let fteqcc = self.dialect.has_fteqcc_extensions();
        let word = if fteqcc {
            // `if (qcc_iswhite(*pr_file_p)) QCC_PR_ParseError (ERR_BADFRAMEMACRO, "hanging $");`
            let white = matches!(self.peek(0), Some(b' ' | b'\r' | b'\n' | b'\t' | 0x0b));
            if white { None } else { self.macro_name() }
        } else {
            self.word()
        };
        let Some(word) = word else {
            self.error(Code::E0107, "hanging $", Span::new(start, self.pos));
            return;
        };
        self.check_length(word.end.saturating_sub(word.start), word);
        let command = self.word_text(word);
        match command.as_str() {
            "frame" => self.frames(start),
            "framesave" if fteqcc => self.frames(start),
            word if IGNORED_COMMANDS.contains(&word) => {
                if fteqcc {
                    // `while (QCC_PR_LexMacroName ()) ;`: the words, up to the first that
                    // ends at once.
                    while self.macro_name().is_some() {}
                } else {
                    // QC-27.14: the rest of the line goes, `,` and `;` included.
                    while self.peek(0).is_some_and(|b| b != b'\n') {
                        self.bump(1);
                    }
                }
            }
            "flush" | "frame_reset" if fteqcc => {
                while self.macro_name().is_some() {}
                let command = if command == "flush" {
                    FrameCommand::Flush
                } else {
                    FrameCommand::Reset
                };
                self.push(TokenKind::FrameCommand(command), start);
            }
            "framevalue" | "framerestore" | "modelname" if fteqcc => {
                let operand = self
                    .macro_name()
                    .map(|span| self.word_text(span))
                    .unwrap_or_default();
                let command = match command.as_str() {
                    "framevalue" => FrameCommand::Value(operand),
                    "framerestore" => FrameCommand::Restore(operand),
                    _ => FrameCommand::ModelName(operand),
                };
                self.push(TokenKind::FrameCommand(command), start);
            }
            _ => self.push(TokenKind::FrameRef(command), start),
        }
    }

    /// The next name of a `$frame` line: `PR_SimpleGetToken`'s word (QC-8.1), or in an FTEQCC
    /// dialect `QCC_PR_LexMacroName`'s (FTEF-18.13), where a `,` or `;` is an empty word.
    fn frame_word(&mut self) -> Option<Span> {
        if !self.dialect.has_fteqcc_extensions() {
            return self.word();
        }
        if let Some(span) = self.macro_name() {
            return Some(span);
        }
        matches!(self.peek(0), Some(b',' | b';')).then(|| Span::new(self.pos, self.pos))
    }

    /// The names of a `$frame` line (QC-8.2), with the errors of QC-27.5 and the warning of
    /// QC-27.6.
    fn frames(&mut self, start: usize) {
        let mut names = Vec::new();
        while let Some(span) = self.frame_word() {
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
