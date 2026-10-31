// SPDX-License-Identifier: GPL-3.0-or-later

//! Where an FTEQCC directive's text ends, for a reader that shows the file as written and runs
//! no directive: the syntax tree (`docs/spec/dialect-fteqcc.md`, FTEF-14.51). The ends are
//! those the preprocessor reads on from, found with its own reader (`fte_directive`, ported
//! from FTEQCC's `QCC_PR_Precompiler`), and the text after one is lexed again as the
//! preprocessor lexes it.

use qc_lexer::{Dialect, Lexed};

use crate::fte_directive::{
    Directive, IfTest, Junk, TextStart, byte_at, condition, read, simple_get_token,
    skip_to_end_of_line, starts_line,
};
use crate::fte_macro::{Reading, read_define};
use crate::{FileId, Lexing, Relexed, merge_errors};

/// Under an FTEQCC dialect, where the text of the directive whose `#` is at `hash` in a file's
/// `text` ends: the offset the preprocessor reads on from after it, with nothing run. `None`
/// when the `#` starts no directive: `QCC_PR_NewLine` does not reach it (FTEF-14.52), as when a
/// `/* */` comment or other text is before it on its line or the code read from `from` passes
/// over the line break before it with a `\` (FTEF-14.53), or its name matches no directive.
/// `from` is where the code before the `#` is read from: the end of the token before it, or
/// where reading went on after a directive.
///
/// The file is read as one being compiled (`QCC_PR_CompileFile`), so a `#` on its first line
/// after spaces and tabs starts a directive. A directive's line ends where
/// `QCC_PR_SkipToEndOfLine` ends it (FTEF-14.35), after the operand the directive reads: a
/// `#define`'s body (FTEF-14.24), an `#if` or `#elif` test (FTEF-14.33, FTEF-14.37), and so on.
/// A `#pragma` ends at its line break (FTEF-14.43), an `#if` with no white space after its name,
/// as `#iffy`, after its word (FTEF-14.57), and an `#eof`, after which FTEQCC reads nothing, at
/// its line break. Implements FTEF-14.51 and FTEF-14.53.
#[must_use]
pub fn fte_directive_end(text: &[u8], from: usize, hash: usize) -> Option<usize> {
    if text.get(hash) != Some(&b'#') || !starts_line(text, from, hash, TextStart::Compiled) {
        return None;
    }
    let line = |from: usize| skip_to_end_of_line(text, from, Junk::Ignored).at;
    let end = match read(text, hash)? {
        Directive::Else { end } => elif_test_end(text, hash).map_or(end.at, line),
        Directive::Hanging(end)
        | Directive::Undef { end, .. }
        | Directive::Endif { end }
        | Directive::Error { end, .. }
        | Directive::Warning { end, .. }
        | Directive::Build { end }
        | Directive::Output { end, .. }
        | Directive::Pack { end, .. }
        | Directive::Package { end }
        | Directive::IncludeList { end }
        | Directive::Include { end, .. } => end.at,
        Directive::If {
            test: IfTest::Expression(from),
            ..
        } => line(condition_end(text, from)),
        Directive::If { end, .. } => end.at,
        Directive::NotIf { word } => word.end,
        Directive::Define { word_end, .. } => {
            let reading = Reading::File(FileId::new(0), text);
            line(read_define(&reading, word_end).1)
        }
        Directive::Eof => text
            .get(hash..)
            .and_then(|rest| rest.iter().position(|&b| b == b'\n' || b == 0))
            .map_or(text.len(), |len| hash.saturating_add(len)),
        Directive::Pragma { end, .. } => end,
        // FTEF-14.66: the compile stops there, so the preprocessor reads no more of the text.
        Directive::Fatal { .. } => text.len(),
    };
    Some(end)
}

/// Where the preprocessor stops reading the expression of an `#if` or `#elif` that starts at
/// `from` (`fte_condition`, FTEF-14.33): after it, or at its start when it nests past the limit
/// of FTEF-14.12. Where reading stops does not depend on the macros.
fn condition_end(text: &[u8], from: usize) -> usize {
    let no_macros = |_: &str| None;
    condition(text, from, &no_macros).1
}

/// For an `#elif`, `#elifdef` or `#elifndef` at `hash`, where its test stops reading, as
/// `QCC_PR_FalsePreProcessorIf` reads it after the side before it was skipped (FTEF-14.37), the
/// way a file reaches one; `None` for an `#else`.
fn elif_test_end(text: &[u8], hash: usize) -> Option<usize> {
    let mut name = hash.saturating_add(1);
    while byte_at(text, name).is_some_and(|b| b <= b' ' || b >= 0x80) {
        name = name.saturating_add(1);
    }
    if !text.get(name..)?.starts_with(b"elif") {
        return None;
    }
    let after = name.saturating_add(4);
    let tail = text.get(after..).unwrap_or_default();
    let stop = if tail.starts_with(b"def") {
        simple_get_token(text, after.saturating_add(3)).1
    } else if tail.starts_with(b"ndef") {
        simple_get_token(text, after.saturating_add(4)).1
    } else {
        condition_end(text, after)
    };
    Some(stop)
}

/// Reading goes on at the byte `offset` of `text`, after a directive, in `lexed`, the first
/// lexing of the whole text under `dialect`, as the preprocessor reads on there
/// (`TokenStream::resume_at`, FTEF-14.22): unless only white space stands between the token
/// before `offset` and `offset`, the tokens that end after `offset` are lexed again from there,
/// only as far as a token of the first lexing that the new lexing reaches as that one did, and
/// the lexical errors between are the new lexing's. A token lexed again starts a line only where
/// only white space comes before it on its line.
pub fn relex_from(lexed: &mut Lexed, text: &[u8], offset: usize, dialect: Dialect) {
    let Some(relexed) = Relexed::from(&lexed.tokens, text, offset, Lexing::plain(dialect)) else {
        return;
    };
    let first = lexed.tokens.partition_point(|t| t.span.end <= offset);
    let replaced = lexed
        .tokens
        .get_mut(first..relexed.kept)
        .unwrap_or_default();
    if replaced.len() == relexed.tokens.len() {
        for (place, token) in replaced.iter_mut().zip(relexed.tokens) {
            *place = token;
        }
    } else {
        lexed.tokens.splice(first..relexed.kept, relexed.tokens);
    }
    merge_errors(
        &mut lexed.diagnostics,
        offset,
        relexed.resumed,
        relexed.diagnostics,
    );
}
