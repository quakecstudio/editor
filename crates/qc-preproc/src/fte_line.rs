// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_NewLine, QCC_PR_Precompiler, QCC_PR_SkipToEndOfLine, QCC_PR_PreProcessor_Define

//! Where an FTEQCC directive's text ends, for a reader that shows the file as written and runs
//! no directive: the syntax tree (`docs/spec/dialect-fteqcc.md`, FTEF-14.51). The ends are
//! those the preprocessor reads on from, found with its own reader (`fte_directive`).

use qc_lexer::{Dialect, Lexed, Span};

use crate::FileId;
use crate::cond::{self, FteIfError};
use crate::fte_directive::{
    Directive, IfTest, Junk, TextStart, read, skip_to_end_of_line, starts_line,
};
use crate::fte_macro::{Reading, read_define};

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

/// The tokens and lexical errors of `text` from the byte `offset` on, as the preprocessor lexes
/// them when it reads on there after a directive, their spans in `text`: a token there starts a
/// line only where only white space comes before it on its line (FTEF-14.22).
#[must_use]
pub fn lex_after(text: &[u8], offset: usize, dialect: Dialect) -> Lexed {
    let lexed = crate::lex_from(text, offset..text.len(), dialect);
    let shift = |span: Span| {
        Span::new(
            span.start.saturating_add(offset),
            span.end.saturating_add(offset),
        )
    };
    Lexed {
        tokens: lexed
            .tokens
            .into_iter()
            .map(|mut token| {
                token.span = shift(token.span);
                token.kind = crate::moved_frames(token.kind, shift);
                token
            })
            .collect(),
        diagnostics: lexed
            .diagnostics
            .into_iter()
            .map(|mut d| {
                d.span = shift(d.span);
                d
            })
            .collect(),
    }
}
