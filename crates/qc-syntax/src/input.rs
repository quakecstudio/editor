// SPDX-License-Identifier: GPL-3.0-or-later

//! The parser's input: the lexer's tokens, with every byte between them classified as
//! trivia, and directive lines grouped so the parser can step over them: under the vanilla
//! dialect to the first line break after the `#` (QC-9.1), under an FTEQCC dialect where the
//! preprocessor ends them (FTEF-14.51).

use qc_lexer::{Diagnostic, Dialect, Punct, Span, TokenKind, is_whitespace, lex_with};
use qc_preproc::{fte_directive_end, relex_from};

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
    /// Text a preprocessor wrote, which holds no directive lines: every `#` is a token.
    Preprocessed,
}

/// The tokens and trivia of one file.
#[derive(Debug)]
pub(crate) struct Input<'a> {
    pub(crate) source: &'a [u8],
    pub(crate) pieces: Vec<Piece>,
    pub(crate) tokens: Vec<Tok>,
    pub(crate) diagnostics: Vec<Diagnostic>,
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

impl<'a> Input<'a> {
    /// Lexes `source`, a `text`, in `dialect` and classifies everything between the tokens.
    /// A source file's directive lines are QC-9.1's under the vanilla dialect and FTEF-14.51's
    /// under an FTEQCC dialect.
    pub(crate) fn new(source: &'a [u8], dialect: Dialect, text: Text) -> Self {
        let mut lexed = lex_with(source, dialect);
        let mut input = Self {
            source,
            pieces: Vec::with_capacity(lexed.tokens.len().saturating_mul(2)),
            tokens: Vec::with_capacity(lexed.tokens.len()),
            diagnostics: Vec::new(),
        };
        let text_end = source.iter().position(|&b| b == 0).unwrap_or(source.len());
        let until_nul = source.get(..text_end).unwrap_or_default();
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
            if line.is_none() && text == Text::Source {
                if fte {
                    let hash = lexed
                        .tokens
                        .get(index)
                        .is_some_and(|t| t.kind == TokenKind::Punct(Punct::Hash));
                    let end = hash
                        .then(|| fte_directive_end(until_nul, from, span.start))
                        .flatten();
                    if let Some(end) = end {
                        // The preprocessor reads the directive's text as bytes, so the lexical
                        // errors in it are none (FTEF-14.51).
                        let directive = span.start..end;
                        lexed
                            .diagnostics
                            .retain(|d| !directive.contains(&d.span.start));
                        // The `#` ends by `end`, so it stays at `index`.
                        relex_from(&mut lexed, source, end, dialect);
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
            let column0 = token.span.start == 0
                || source
                    .get(token.span.start.wrapping_sub(1))
                    .is_some_and(|&b| b == b'\n');
            let (text, value) = match &token.kind {
                TokenKind::Name(name) => (name.clone(), None),
                TokenKind::String(bytes) => (latin1(bytes), None),
                TokenKind::Float(value) => (
                    latin1(source.get(token.span.range()).unwrap_or_default()),
                    Some(*value),
                ),
                TokenKind::Punct(punct) => (punct.as_str().to_owned(), None),
                TokenKind::Vector(_)
                | TokenKind::FrameRef(_)
                | TokenKind::Frames(_)
                | TokenKind::FrameCommand(_) => (
                    latin1(source.get(token.span.range()).unwrap_or_default()),
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
        input.diagnostics = lexed.diagnostics;
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
