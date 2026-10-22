// SPDX-License-Identifier: GPL-3.0-or-later

//! The parser's input: the lexer's tokens, with every byte between them classified as
//! trivia, and directive lines grouped so the parser can step over them: under the vanilla
//! dialect to the first line break after the `#` (QC-9.1), under an FTEQCC dialect where the
//! preprocessor ends them (FTEF-14.51), in the text FTEQCC's file loader gives (FTEF-14.56).

use std::borrow::Cow;

use qc_lexer::{
    Diagnostic, Dialect, LexFlag, LexFlags, Punct, Span, TokenKind, is_whitespace, lex_with,
};
use qc_preproc::{fte_directive_end, qcc_load_in_place, relex_from, skip_script_line};

use crate::kind::SyntaxKind;

/// One element of the input, in source order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Piece {
    /// A trivia token, or a token inside a directive line.
    Leaf(SyntaxKind, Span),
    /// The start of a directive line.
    OpenDirective,
    /// The end of a directive line.
    CloseDirective,
    /// The significant token with this index in [`Input::tokens`].
    Token(usize),
}

/// A token the parser reads.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Tok {
    /// Its kind before the parser remaps a keyword.
    pub(crate) kind: SyntaxKind,
    /// Its bytes.
    pub(crate) span: Span,
    /// Its index in [`Input::pieces`].
    pub(crate) piece: usize,
    /// Whether it is the first byte of its line: no indentation and nothing else before it.
    pub(crate) column0: bool,
    /// Whether only whitespace precedes it on its line.
    pub(crate) line_start: bool,
    /// The text `qcc` would print for it in "found X": a name or punctuation as written, a
    /// string literal's contents (QC-3.4).
    pub(crate) text: String,
    /// A float literal's value.
    pub(crate) value: Option<f32>,
}

/// What a parser's input text is, which decides whether it has directive lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Text {
    /// A source file as written: a `#` and a name first on a line open a directive line
    /// (QC-9.1).
    Source,
    /// A module's source in source form (PRJ-4.4), read as [`Self::Source`] but that, under an
    /// FTEQCC dialect, a first line that starts with `#!` is passed over (PRJ-4.10).
    ModuleSource,
    /// Text a preprocessor wrote, which holds no directive lines: every `#` is a token.
    Preprocessed,
}

impl Text {
    /// Whether the text is a source file as written, with directive lines.
    const fn is_source(self) -> bool {
        match self {
            Self::Source | Self::ModuleSource => true,
            Self::Preprocessed => false,
        }
    }
}

/// The tokens and trivia of one file.
#[derive(Debug)]
pub(crate) struct Input<'a> {
    pub(crate) source: &'a [u8],
    pub(crate) pieces: Vec<Piece>,
    pub(crate) tokens: Vec<Tok>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Every lexical error and warning of the text, a preprocessor's text's too, which
    /// [`Self::diagnostics`] leaves out: the parser reports those of the tokens it lexed with
    /// other flags than the preprocessor did (FTEF-21.2).
    pub(crate) lexical: Vec<Diagnostic>,
    /// The compiler flags the tokens were lexed with, each from the byte it holds from
    /// (FTEF-12.12).
    pub(crate) flags: Vec<(usize, LexFlags)>,
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

/// The directive line being read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Line {
    /// The vanilla dialect's, whose `#` is at this offset: it ends at the first line break
    /// after it (QC-9.1).
    Vanilla { hash: usize },
    /// An FTEQCC dialect's, which ends at this offset, where the preprocessor reads on
    /// (FTEF-14.51).
    Fte { end: usize },
}

/// What the lexer reads of a file, at the file's own offsets, so that the tree's spans are
/// the file's bytes (FTEF-14.56).
struct Lexable<'a> {
    /// The bytes the lexer reads.
    text: Cow<'a, [u8]>,
    /// Where the text a directive is read in starts: after a UTF-8 byte order mark FTEQCC's
    /// loader skips, else 0.
    start: usize,
    /// Where the text ends: the first NUL under the vanilla dialect (QC-1.1), else the end.
    end: usize,
}

impl<'a> Lexable<'a> {
    /// `source`, a `text`, as `dialect` reads it: a source file under an FTEQCC dialect in the
    /// loader's text in place (`qc_preproc::qcc_load_in_place`, FTEF-14.56); under the vanilla
    /// dialect, a UTF-16 or UTF-32 file, whose decoded text is at other offsets, and a
    /// preprocessor's text, which no loader read, its bytes to the first NUL (QC-1.1).
    fn new(source: &'a [u8], dialect: Dialect, text: Text) -> Self {
        let mut lexable = Self::loaded(source, dialect, text);
        // PRJ-4.10: `QCC_main` passes over the line, which the tree keeps as bytes the lexer
        // skipped.
        let script = dialect.has_fteqcc_extensions()
            && text == Text::ModuleSource
            && lexable
                .text
                .get(lexable.start..)
                .is_some_and(|rest| rest.starts_with(b"#!"));
        if script {
            let mut owned = lexable.text.into_owned();
            if let Some(rest) = owned.get_mut(lexable.start..) {
                skip_script_line(rest);
            }
            lexable.text = Cow::Owned(owned);
        }
        lexable
    }

    /// [`Self::new`] before a module source's `#!` line is passed over.
    fn loaded(source: &'a [u8], dialect: Dialect, text: Text) -> Self {
        let loaded = (dialect.has_fteqcc_extensions() && text.is_source())
            .then(|| qcc_load_in_place(source))
            .flatten();
        match loaded {
            // The mark the loader skips is no text the lexer reads: under FTEQCC its bytes would
            // start a name (FTEF-13.16), so they are read as the spaces they stand for. The
            // loader's text is its own copy, so this writes no further one.
            Some((text, start)) => {
                let mut text = text.into_owned();
                for byte in text.iter_mut().take(start) {
                    *byte = b' ';
                }
                Self {
                    text: Cow::Owned(text),
                    start,
                    end: source.len(),
                }
            }
            None => Self {
                text: Cow::Borrowed(source),
                start: 0,
                end: source.iter().position(|&b| b == 0).unwrap_or(source.len()),
            },
        }
    }

    /// The text a directive is read in: from [`Self::start`], the loader's line feed after the
    /// file's bytes included (FTEF-14.64).
    fn directive_text(&self) -> &[u8] {
        self.text.get(self.start..).unwrap_or_default()
    }

    /// Where the preprocessor ends the directive whose `#` is at `hash`, the code before it
    /// read from `from`, all at the file's offsets (FTEF-14.51).
    fn directive_end(&self, from: usize, hash: usize) -> Option<usize> {
        let at = |offset: usize| offset.saturating_sub(self.start);
        let end = fte_directive_end(self.directive_text(), at(from), at(hash))?;
        Some(end.saturating_add(self.start).min(self.end))
    }
}

/// The tokens and lexical errors of `text`, lexed in one pass under the flags `flags` marks from
/// each byte (FTEF-12.12), `macroinstrings` aside, the text before the first under the default
/// flags.
fn lex_marked(text: &[u8], dialect: Dialect, flags: &[(usize, LexFlags)]) -> qc_lexer::Lexed {
    if flags.is_empty() {
        return lex_with(text, dialect);
    }
    let marks: Vec<(usize, LexFlags)> = flags
        .iter()
        .map(|&(at, flags)| (at, flags.with(LexFlag::MacroInStrings, false)))
        .collect();
    qc_lexer::lex_marked(text, dialect, &marks, &|_| None)
}

/// Ends at `len`, the file's end, the tokens and lexical errors that reach past it: the loader's
/// line feed after the file's bytes is read, and is no byte of the tree (FTEF-14.64). Only the
/// last of either, which stand in order, can reach it.
fn clamp_to(lexed: &mut qc_lexer::Lexed, len: usize) {
    let clamp = |span: &mut Span| {
        span.start = span.start.min(len);
        span.end = span.end.min(len);
    };
    for token in lexed.tokens.iter_mut().rev() {
        if token.span.end <= len {
            break;
        }
        clamp(&mut token.span);
    }
    for d in &mut lexed.diagnostics {
        clamp(&mut d.span);
    }
}

impl<'a> Input<'a> {
    /// Lexes `source`, a `text`, in `dialect` and classifies everything between the tokens.
    /// A source file's directive lines are QC-9.1's under the vanilla dialect and FTEF-14.51's
    /// under an FTEQCC dialect, read in the text FTEQCC's file loader gives (FTEF-14.56).
    pub(crate) fn new(
        source: &'a [u8],
        dialect: Dialect,
        text: Text,
        flags: &[(usize, LexFlags)],
    ) -> Self {
        let lexable = Lexable::new(source, dialect, text);
        let mut lexed = lex_marked(&lexable.text, dialect, flags);
        clamp_to(&mut lexed, source.len());
        let mut input = Self {
            source,
            pieces: Vec::with_capacity(lexed.tokens.len().saturating_mul(2)),
            tokens: Vec::with_capacity(lexed.tokens.len()),
            diagnostics: Vec::new(),
            lexical: Vec::new(),
            flags: flags.to_vec(),
        };
        let text_end = lexable.end;
        let fte = dialect.has_fteqcc_extensions();
        let mut at = 0_usize;
        let mut line: Option<Line> = None;
        let mut index = 0_usize;
        while let Some(span) = lexed.tokens.get(index).map(|t| t.span) {
            match line {
                Some(Line::Vanilla { hash }) => {
                    let line_break = source
                        .get(hash..span.start)
                        .is_some_and(|text| text.contains(&b'\n'));
                    if line_break {
                        input.pieces.push(Piece::CloseDirective);
                        line = None;
                    }
                }
                Some(Line::Fte { end }) if span.start >= end => {
                    input.gap(at, end);
                    input.pieces.push(Piece::CloseDirective);
                    at = at.max(end);
                    line = None;
                }
                Some(Line::Fte { .. }) | None => {}
            }
            input.gap(at, span.start);
            // Where the code before the token is read from: after the token or directive before
            // it (FTEF-14.53).
            let from = at;
            at = span.end;
            if line.is_none() && text.is_source() {
                if fte {
                    let hash = lexed
                        .tokens
                        .get(index)
                        .is_some_and(|t| t.kind == TokenKind::Punct(Punct::Hash));
                    let end = hash
                        .then(|| lexable.directive_end(from, span.start))
                        .flatten();
                    if let Some(end) = end {
                        // The preprocessor reads the directive's text as bytes, so the lexical
                        // errors in it are none (FTEF-14.51).
                        let directive = span.start..end;
                        lexed
                            .diagnostics
                            .retain(|d| !directive.contains(&d.span.start));
                        // The `#` ends by `end`, so it stays at `index`.
                        relex_from(&mut lexed, &lexable.text, end, dialect);
                        clamp_to(&mut lexed, source.len());
                        input.pieces.push(Piece::OpenDirective);
                        line = Some(Line::Fte { end });
                    }
                } else if starts_directive(source, &lexed.tokens, index) {
                    input.pieces.push(Piece::OpenDirective);
                    line = Some(Line::Vanilla { hash: span.start });
                }
            }
            let Some(token) = lexed.tokens.get(index) else {
                break;
            };
            index = index.saturating_add(1);
            let kind = token_kind(&token.kind);
            if line.is_some() || kind.is_trivia() {
                input.pieces.push(Piece::Leaf(kind, token.span));
                continue;
            }
            let column0 = token.span.start == lexable.start
                || source
                    .get(token.span.start.wrapping_sub(1))
                    .is_some_and(|&b| b == b'\n');
            let (text, value) = match &token.kind {
                TokenKind::Name(name) => (name.clone(), None),
                TokenKind::String(bytes) => (latin1(bytes), None),
                TokenKind::Float(value) => (
                    latin1(lexable.text.get(token.span.range()).unwrap_or_default()),
                    Some(*value),
                ),
                TokenKind::Punct(punct) => (punct.as_str().to_owned(), None),
                TokenKind::Vector(_)
                | TokenKind::FrameRef(_)
                | TokenKind::Frames(_)
                | TokenKind::FrameCommand(_) => (
                    latin1(lexable.text.get(token.span.range()).unwrap_or_default()),
                    None,
                ),
            };
            input.pieces.push(Piece::Token(input.tokens.len()));
            input.tokens.push(Tok {
                kind,
                span: token.span,
                piece: input.pieces.len().saturating_sub(1),
                column0,
                line_start: token.line_start,
                text,
                value,
            });
        }
        match line {
            Some(Line::Vanilla { .. }) => {
                let line_break = source
                    .get(at..text_end)
                    .is_some_and(|gap| gap.contains(&b'\n'));
                if line_break {
                    input.pieces.push(Piece::CloseDirective);
                    input.gap(at, text_end);
                } else {
                    input.gap(at, text_end);
                    input.pieces.push(Piece::CloseDirective);
                }
            }
            Some(Line::Fte { end }) => {
                let end = end.clamp(at, text_end);
                input.gap(at, end);
                input.pieces.push(Piece::CloseDirective);
                input.gap(end, text_end);
            }
            None => input.gap(at, text_end),
        }
        // A preprocessor's text is the tokens the preprocessor lexed from the files and their
        // expansions, which reported their lexical errors and warnings there, so lexing it again
        // reports none (QC-9.21).
        if text.is_source() {
            input.diagnostics = lexed.diagnostics;
        } else {
            input.lexical = lexed.diagnostics;
        }
        if text_end < source.len() {
            input.pieces.push(Piece::Leaf(
                SyntaxKind::Skipped,
                Span::new(text_end, source.len()),
            ));
        }
        input
    }

    /// Splits the bytes the lexer skipped between `start` and `end` into trivia tokens.
    fn gap(&mut self, start: usize, end: usize) {
        let mut at = start;
        while at < end {
            let rest = self.source.get(at..end).unwrap_or_default();
            let (kind, len) = classify(rest);
            let len = len.max(1);
            let next = at.saturating_add(len).min(end);
            self.pieces.push(Piece::Leaf(kind, Span::new(at, next)));
            at = next;
        }
    }
}

/// The kind and length of the trivia token at the start of `rest`, bytes the lexer skipped.
fn classify(rest: &[u8]) -> (SyntaxKind, usize) {
    let run = |pred: &dyn Fn(u8) -> bool| rest.iter().take_while(|&&b| pred(b)).count();
    match rest {
        [b'/', b'/', ..] => (SyntaxKind::LineComment, run(&|b| b != b'\n')),
        [b'/', b'*', ..] => {
            // QC-2.3: the search for `*/` starts at the `*` of the `/*`.
            let close = rest
                .get(1..)
                .unwrap_or_default()
                .windows(2)
                .position(|w| w == b"*/");
            let len = close.map_or(rest.len(), |at| at.saturating_add(3));
            (SyntaxKind::BlockComment, len)
        }
        [b'$', ..] => (SyntaxKind::Skipped, run(&|b| b != b'\n')),
        [b, ..] if is_whitespace(*b) => (SyntaxKind::Whitespace, run(&is_whitespace)),
        _ => {
            let len = rest
                .iter()
                .enumerate()
                .take_while(|&(i, &b)| {
                    let comment =
                        b == b'/' && matches!(rest.get(i.saturating_add(1)), Some(b'/' | b'*'));
                    i == 0 || !(is_whitespace(b) || b == b'$' || comment)
                })
                .count();
            (SyntaxKind::Skipped, len)
        }
    }
}

/// Whether the token at `index` is the `#` of a directive line: `#` first on its line and a
/// name after it on the same line (QC-9.1).
fn starts_directive(source: &[u8], tokens: &[qc_lexer::Token], index: usize) -> bool {
    let Some(hash) = tokens.get(index) else {
        return false;
    };
    if hash.kind != TokenKind::Punct(Punct::Hash) || !hash.line_start {
        return false;
    }
    let Some(name) = tokens.get(index.saturating_add(1)) else {
        return false;
    };
    matches!(name.kind, TokenKind::Name(_))
        && source
            .get(hash.span.end..name.span.start)
            .is_some_and(|gap| !gap.contains(&b'\n'))
}

/// The tree kind of a lexer token.
fn token_kind(kind: &TokenKind) -> SyntaxKind {
    match kind {
        TokenKind::Name(_) => SyntaxKind::Name,
        TokenKind::Float(_) => SyntaxKind::FloatLit,
        TokenKind::Vector(_) => SyntaxKind::VectorLit,
        TokenKind::String(_) => SyntaxKind::StringLit,
        TokenKind::Punct(punct) => SyntaxKind::from_punct(*punct),
        TokenKind::FrameRef(_) => SyntaxKind::FrameRef,
        TokenKind::Frames(_) | TokenKind::FrameCommand(_) => SyntaxKind::Frames,
    }
}
