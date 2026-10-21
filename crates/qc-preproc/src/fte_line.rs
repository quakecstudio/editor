// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_NewLine, QCC_PR_Precompiler, QCC_PR_SkipToEndOfLine, QCC_PR_PreProcessor_Define

//! Where an FTEQCC directive's text ends, for a reader that shows the file as written and runs
//! no directive: the syntax tree (`docs/spec/dialect-fteqcc.md`, FTEF-14.51). The ends are
//! those the preprocessor reads on from, found with its own reader (`fte_directive`), and the
//! text after one is lexed again as the preprocessor lexes it.

use qc_lexer::{Dialect, Lexed, Span, is_whitespace};

use crate::cond::{self, FteIfError};
use crate::fte_directive::{
    Directive, IfTest, Junk, TextStart, read, skip_to_end_of_line, starts_line,
};
use crate::fte_macro::{Reading, read_define};
use crate::{FileId, RESYNC_TRIES, TokenStream};

/// Under an FTEQCC dialect, where the text of the directive whose `#` is at `hash` in a file's
/// `text` ends: the offset the preprocessor reads on from after it, with nothing run. `None`
/// when the `#` starts no directive: `QCC_PR_NewLine` does not reach it (FTEF-14.34), as when a
/// `/* */` comment or other text is before it on its line, or its name matches no directive.
///
/// The file is read as one being compiled (`QCC_PR_CompileFile`), so a `#` on its first line
/// after spaces and tabs starts a directive. A directive's line ends where
/// `QCC_PR_SkipToEndOfLine` ends it (FTEF-14.35), after the operand the directive reads: a
/// `#define`'s body (FTEF-14.24), an `#if` expression (FTEF-14.33), and so on. A `#pragma` ends
/// at its line break (FTEF-14.43), an `#if` that is no `#if`, as `#iffy`, after its word
/// (FTEF-14.36), and an `#eof`, after which FTEQCC reads nothing, at its line break.
/// Implements FTEF-14.51.
#[must_use]
pub fn fte_directive_end(text: &[u8], hash: usize) -> Option<usize> {
    if text.get(hash) != Some(&b'#') || !starts_line(text, hash, TextStart::Compiled) {
        return None;
    }
    let line = |from: usize| skip_to_end_of_line(text, from, Junk::Ignored).at;
    let end = match read(text, hash)? {
        Directive::Hanging(end)
        | Directive::Undef { end, .. }
        | Directive::Else { end }
        | Directive::Endif { end }
        | Directive::Error { end, .. }
        | Directive::Warning { end, .. }
        | Directive::Build { end }
        | Directive::Pack { end, .. }
        | Directive::Package { end }
        | Directive::IncludeList { end }
        | Directive::Include { end, .. } => end.at,
        Directive::If {
            test: IfTest::Expression(from),
            ..
        } => {
            let no_macros = |_: &str| None;
            let (value, read) = cond::fte_if(text.get(from..).unwrap_or_default(), &no_macros);
            // Where the preprocessor reads on from (`fte_condition`): an expression nested past
            // the limit of FTEF-14.12 leaves reading at its start.
            let stop = match value {
                Err(FteIfError::TooDeep) => from,
                Ok(_) | Err(FteIfError::Reported(_)) => from.saturating_add(read),
            };
            line(stop)
        }
        Directive::If { end, .. } => end.at,
        Directive::NotIf { word_end } => word_end,
        Directive::Define { word_end, .. } => {
            let reading = Reading::File(FileId(0), text);
            line(read_define(&reading, word_end).1)
        }
        Directive::Eof => text
            .get(hash..)
            .and_then(|rest| rest.iter().position(|&b| b == b'\n' || b == 0))
            .map_or(text.len(), |len| hash.saturating_add(len)),
        Directive::Pragma { end, .. } => end,
    };
    Some(end)
}

/// Reading goes on at the byte `offset` of `text`, after a directive, in `lexed`, the first
/// lexing of the whole text under `dialect`, as the preprocessor reads on there
/// (`TokenStream::resume_at`, FTEF-14.22): unless only white space stands between the token
/// before `offset` and `offset`, the tokens that end after `offset` are lexed again from there,
/// only as far as a token of the first lexing that the new lexing reaches as that one did, and
/// the lexical errors between are the new lexing's. A token lexed again starts a line only where
/// only white space comes before it on its line.
pub fn relex_from(lexed: &mut Lexed, text: &[u8], offset: usize, dialect: Dialect) {
    let tokens = &mut lexed.tokens;
    let next = tokens.partition_point(|t| t.span.start < offset);
    let before = next.checked_sub(1).and_then(|last| tokens.get(last));
    let gap_start = before.map_or(0, |t| t.span.end);
    let clean = gap_start <= offset
        && text
            .get(gap_start..offset)
            .is_some_and(|gap| gap.iter().all(|&b| is_whitespace(b)));
    if clean {
        return;
    }
    let first = tokens.partition_point(|t| t.span.end <= offset);
    let window = (next..tokens.len()).take(RESYNC_TRIES).find_map(|kept| {
        Some(kept).zip(TokenStream::relex_window(
            tokens, text, offset, kept, dialect,
        ))
    });
    let (kept, rest) = window.unwrap_or_else(|| {
        (
            tokens.len(),
            crate::lex_from(text, offset..text.len(), dialect),
        )
    });
    let resumed = tokens.get(kept).map_or(usize::MAX, |t| t.span.start);
    let shift = |span: Span| {
        Span::new(
            span.start.saturating_add(offset),
            span.end.saturating_add(offset),
        )
    };
    let again = rest.tokens.into_iter().map(|mut token| {
        token.span = shift(token.span);
        token.kind = crate::moved_frames(token.kind, shift);
        token
    });
    tokens.splice(first..kept, again);
    // The first lexing's errors before `offset` and from the kept token on stand; those
    // between are the new lexing's.
    let mut later = Vec::new();
    for d in std::mem::take(&mut lexed.diagnostics) {
        if d.span.start < offset {
            lexed.diagnostics.push(d);
        } else if d.span.start >= resumed {
            later.push(d);
        }
    }
    lexed
        .diagnostics
        .extend(rest.diagnostics.into_iter().map(|mut d| {
            d.span = shift(d.span);
            d
        }));
    lexed.diagnostics.extend(later);
}
