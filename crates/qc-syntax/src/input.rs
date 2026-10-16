// SPDX-License-Identifier: GPL-3.0-or-later

//! The parser's input: the lexer's tokens, with every byte between them classified as
//! trivia, and directive lines grouped so the parser can step over them.

use qc_lexer::{Diagnostic, Punct, Span, TokenKind, is_whitespace, lex};

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

impl<'a> Input<'a> {
    /// Lexes `source` and classifies everything between the tokens.
    pub(crate) fn new(source: &'a [u8]) -> Self {
        let lexed = lex(source);
        let mut input = Self {
            source,
            pieces: Vec::with_capacity(lexed.tokens.len().saturating_mul(2)),
            tokens: Vec::with_capacity(lexed.tokens.len()),
            diagnostics: lexed.diagnostics,
        };
        let text_end = source.iter().position(|&b| b == 0).unwrap_or(source.len());
        let mut at = 0_usize;
        // The offset of the `#` of the directive line being read.
        let mut directive_line: Option<usize> = None;
        let lexed_tokens = lexed.tokens;
        for (index, token) in lexed_tokens.iter().enumerate() {
            let line_break = directive_line.is_some_and(|hash| {
                source
                    .get(hash..token.span.start)
                    .is_some_and(|text| text.contains(&b'\n'))
            });
            if line_break {
                input.pieces.push(Piece::CloseDirective);
                directive_line = None;
            }
            input.gap(at, token.span.start);
            at = token.span.end;
            if directive_line.is_none() && starts_directive(source, &lexed_tokens, index) {
                input.pieces.push(Piece::OpenDirective);
                directive_line = Some(token.span.start);
            }
            let kind = token_kind(&token.kind);
            if directive_line.is_some() || kind.is_trivia() {
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
                TokenKind::Vector(_) | TokenKind::FrameRef(_) | TokenKind::Frames(_) => (
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
        let line_break = source
            .get(at..text_end)
            .is_some_and(|gap| gap.contains(&b'\n'));
        if line_break && directive_line.is_some() {
            input.pieces.push(Piece::CloseDirective);
            directive_line = None;
        }
        input.gap(at, text_end);
        if directive_line.is_some() {
            input.pieces.push(Piece::CloseDirective);
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
        TokenKind::Frames(_) => SyntaxKind::Frames,
    }
}
