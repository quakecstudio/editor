// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_Precompiler, QCC_PR_SkipToEndOfLine, QCC_PR_FalsePreProcessorIf, QCC_FindBestInclude, QCC_JoinPaths, QCC_PR_AddIncludePath, QCC_PR_CloseProcessor, QCC_PR_NewLine, QCC_PR_LexWhitespace, QCC_PR_SimpleGetToken, QCC_PR_SimpleGetString, QCC_PR_LexInteger, QCC_PR_DefineName, QCC_PR_UndefineName
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_cmdlib.c, QCC_COM_Parse
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qccmain.c, QCC_SetDefaultProperties, QCC_WarningForName

//! FTEQCC's directives (`docs/spec/dialect-fteqcc.md` FTEF-14.33 to FTEF-14.47), ported from
//! FTEQCC's behaviour under ADR-0017's clean-port policy. FTEQCC reads a directive as bytes:
//! `QCC_PR_NewLine` runs `QCC_PR_Precompiler` at the first byte after the spaces and tabs that
//! follow each line ending its white-space reader passes, which matches a directive's name by
//! its first bytes and reads the rest of the line as that directive does, ending it with
//! `QCC_PR_SkipToEndOfLine`. [`read`] and the functions it calls read a directive, which needs
//! no state; `Preprocessor::fte_directive` runs it, wherever FTEQCC meets a directive: at a line
//! of a file or of a macro's expansion, among a use's arguments, in an `#includelist` and in
//! the text `#pragma DONT_COMPILE_THIS_FILE` passes over. The FTEQCC function each function
//! follows is named in its rustdoc.

mod graph;
mod load;

pub use graph::{FileStart, FteEvent, FteFileReader, ModuleState};
pub(crate) use load::first_nul;
pub use load::{qcc_load_as, qcc_load_file, qcc_load_in_place, skip_script_line};

use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

use qc_lexer::{Code, Dialect, FTEQCC_MAX_WORD, Span};

use crate::compiler_flags::{FlagEdit, PragmaDefault};
use crate::cond::{self, FteIfError, fte_white};
use crate::fte_keywords::{KeywordEdit, Keywords};
use crate::fte_macro::{Reading, Text};
use crate::fte_tables::TargetFormat;
use crate::{
    Body, ChunkReader, FileId, FileSpan, FlagSettings, Loader, Macro, Preprocessor, Run, Undef,
    Variadic, WarningSettings, fte_pragma, normalize,
};

/// The longest name `QCC_PR_DefineName` defines: `MAXCONSTANTNAMELENGTH` in FTEQCC's `qcc.h`,
/// `#define MAXCONSTANTNAMELENGTH 64`, and `if (strlen(name) >= MAXCONSTANTNAMELENGTH ||
/// !*name)` refuses one of 64 bytes or more (FTEF-14.42).
pub(crate) const FTE_MAX_MACRO_NAME: usize = 63;

/// The most bytes of a `#warning` or `#error` text FTEQCC keeps: its `char msg[1024]`, filled
/// `for (a = 0; a < 1023 && ...` (FTEF-14.38).
const FTE_MAX_MESSAGE: usize = 1023;

/// The most packs `#pack` names: "No more than 5 packs are allowed" (FTEF-14.39).
const FTE_MAX_PACKS: i32 = 5;

/// The byte at `at` of a text FTEQCC reads, `None` at its end, which a NUL is.
pub(crate) fn byte_at(text: &[u8], at: usize) -> Option<u8> {
    text.get(at).copied().filter(|&b| b != 0)
}

/// `qcc_islineending`: a LF, or a CR not before a LF.
pub(crate) fn line_ending(byte: u8, next: Option<u8>) -> bool {
    byte == b'\n' || (byte == b'\r' && next != Some(b'\n'))
}

/// `qcc_iswhitesameline`: a space or a tab.
const fn blank(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

/// The offset after the spaces and tabs from `at`.
pub(crate) fn skip_blanks(text: &[u8], mut at: usize) -> usize {
    while byte_at(text, at).is_some_and(blank) {
        at = at.saturating_add(1);
    }
    at
}

/// How a text starts, for a `#` at its start (FTEF-14.52).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextStart {
    /// A compiled file: `QCC_PR_CompileFile` calls `QCC_PR_NewLine` at its start, which skips
    /// spaces and tabs.
    Compiled,
    /// An `#include`d file: `QCC_PR_Precompiler` is called at its first byte, with no spaces
    /// skipped.
    Included,
    /// A macro's expansion, whose start is no line's.
    Chunk,
}

/// Whether `QCC_PR_NewLine`, or `QCC_PR_Precompiler` at a file's start, reads the `#` at
/// `hash` (FTEF-14.52, FTEF-14.53): only spaces and tabs stand between it and a line ending
/// before it that the code read from `from` does not pass over with a `\`, or, as `start` says,
/// the text's start. `from` is where the code before the `#` is read from: the end of the token
/// before it, or where reading went on after a directive.
pub(crate) fn starts_line(text: &[u8], from: usize, hash: usize, start: TextStart) -> bool {
    let before = text.get(..hash).unwrap_or_default();
    let line = before
        .iter()
        .rposition(|&b| !blank(b))
        .map(|at| (at, before.get(at).copied()));
    match line {
        Some((_, Some(b'\n' | b'\r'))) => !joined(text, from, hash),
        Some(_) => false,
        None => match start {
            TextStart::Compiled => true,
            TextStart::Included => hash == 0,
            TextStart::Chunk => false,
        },
    }
}

/// Whether the code read from `from` reaches the `#` at `hash` over a `\` and the line break
/// after it, with no line ending the white-space reader passes after them: in code
/// `QCC_PR_LexPunctuation` passes over both (`pr_file_p+=2;`, FTEF-13.2) and calls no
/// `QCC_PR_NewLine`, so the `#` starts no directive (FTEF-14.53). Only white space, comments
/// and such joins stand between `from` and `hash`.
fn joined(text: &[u8], from: usize, hash: usize) -> bool {
    let mut at = from;
    let mut joined = false;
    loop {
        let (Gap::Text(stop) | Gap::Unclosed(stop)) = whitespace(text, at) else {
            return false;
        };
        if stop >= hash {
            return joined && stop == hash;
        }
        let Some(after) = join_end(text, stop) else {
            return false;
        };
        joined = true;
        at = after;
    }
}

/// Where `QCC_PR_LexPunctuation` reads on after a `\` at `at` that a line break follows: after a
/// CR LF (`pr_file_p+=3;`), or after a lone CR or LF (`pr_file_p+=2;`); `None` when no `\` and
/// line break are at `at` (FTEF-13.2).
fn join_end(text: &[u8], at: usize) -> Option<usize> {
    if byte_at(text, at) != Some(b'\\') {
        return None;
    }
    let next = |by: usize| text.get(at.saturating_add(by)).copied();
    match (next(1), next(2)) {
        (Some(b'\r'), Some(b'\n')) => Some(at.saturating_add(3)),
        (Some(b'\r' | b'\n'), _) => Some(at.saturating_add(2)),
        _ => None,
    }
}

/// Where `QCC_PR_LexWhitespace(false)` stops (FTEF-14.45).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Gap {
    /// At this byte, which is no white space and starts no comment, or at the end of the text.
    Text(usize),
    /// At this `#`, the first byte after a line ending and the spaces and tabs after it, where
    /// `QCC_PR_NewLine` calls `QCC_PR_Precompiler`.
    LineStart(usize),
    /// At the end of the text, inside the `/* */` comment that starts at this offset: FTEQCC's
    /// "EOF inside comment" (FTEF-14.67).
    Unclosed(usize),
}

/// FTEQCC's error for a `/* */` comment the text ends in (FTEF-14.67).
pub(crate) const EOF_IN_COMMENT: &str = "EOF inside comment";

/// Why [`Preprocessor::fte_whitespace`] read no further than the white space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WhiteStop {
    /// A directive it ran ended the text (`#eof`, `#package`).
    Ended,
    /// The text ends in the `/* */` comment that starts at this offset, FTEQCC's "EOF inside
    /// comment" (FTEF-14.67).
    Unclosed(usize),
}

/// `QCC_PR_LexWhitespace(false)` from `at`: past white space (`qcc_iswhite`), `//` comments to
/// their line break and `/* */` comments, up to the first other byte or a `#` at a line's
/// start ([`Gap`]). A `/* */` comment that does not end runs to the end of the text, where
/// FTEQCC reports "EOF inside comment" ([`Gap::Unclosed`]).
pub(crate) fn whitespace(text: &[u8], mut at: usize) -> Gap {
    let new_line = |at: usize| {
        let at = skip_blanks(text, at);
        (byte_at(text, at) == Some(b'#')).then_some(Gap::LineStart(at))
    };
    loop {
        while let Some(b) = byte_at(text, at).filter(|&b| fte_white(b)) {
            at = at.saturating_add(1);
            if line_ending(b, text.get(at).copied()) {
                if let Some(gap) = new_line(at) {
                    return gap;
                }
                at = skip_blanks(text, at);
            }
        }
        let next = text.get(at.saturating_add(1)).copied();
        match (byte_at(text, at), next) {
            (Some(b'/'), Some(b'/')) => {
                while let Some(b) = byte_at(text, at) {
                    if line_ending(b, text.get(at.saturating_add(1)).copied()) {
                        break;
                    }
                    at = at.saturating_add(1);
                }
                if byte_at(text, at).is_some() {
                    at = at.saturating_add(1);
                }
                if let Some(gap) = new_line(at) {
                    return gap;
                }
            }
            (Some(b'/'), Some(b'*')) => {
                let open = at;
                at = at.saturating_add(1);
                loop {
                    at = at.saturating_add(1);
                    if byte_at(text, at.saturating_add(1)).is_none() {
                        return Gap::Unclosed(open);
                    }
                    if text.get(at..at.saturating_add(2)) == Some(b"*/") {
                        break;
                    }
                }
                at = at.saturating_add(2);
            }
            _ => return Gap::Text(at.min(text.len())),
        }
    }
}

/// Where `QCC_PR_SkipToEndOfLine` ends a directive's line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LineEnd {
    /// The offset of the line break, or the end of the text.
    pub(crate) at: usize,
    /// Whether it was asked to check the line and found a byte that is no white space, outside
    /// a comment, before any `//`: "unexpected tokens at end of line", an error (FTEF-14.35).
    pub(crate) junk: bool,
}

impl LineEnd {
    /// Reading goes on at this end of the line of the directive whose `#` is at `hash` in
    /// `reading`; its junk is "unexpected tokens at end of line" (FTEF-14.35).
    fn resume(self, run: &mut Run, reading: &Reading<'_>, hash: usize) -> Resume {
        self.report_junk(run, reading, hash);
        Resume::At(self.at)
    }

    /// Reports this line's junk, for a directive whose reading goes on elsewhere than its end.
    fn report_junk(self, run: &mut Run, reading: &Reading<'_>, hash: usize) {
        if self.junk {
            let at = reading.place(hash..self.at.max(hash.saturating_add(1)));
            run.report(Code::E0121, JUNK, at);
        }
    }
}

/// Whether `QCC_PR_SkipToEndOfLine` is called with `errorifnonwhite` (FTEF-14.35).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Junk {
    /// The line's junk is "unexpected tokens at end of line".
    Checked,
    /// The line is read with no check.
    Ignored,
}

/// `QCC_PR_SkipToEndOfLine(errorifnonwhite)` from `at` (FTEF-14.35): to the line break or the
/// end of the text, passing over `/* */` comments, strings before any `//`, and each `\` and
/// line break, in a `//` comment too. With [`Junk::Checked`] the first byte that is no white
/// space, outside a comment and before a `//`, a string's `"` included, is junk.
pub(crate) fn skip_to_end_of_line(text: &[u8], mut at: usize, junk_is: Junk) -> LineEnd {
    let check = junk_is == Junk::Checked;
    let mut comments = true;
    let mut junk = false;
    loop {
        let Some(byte) = byte_at(text, at).filter(|&b| b != b'\n') else {
            return LineEnd {
                at: at.min(text.len()),
                junk,
            };
        };
        let rest = text.get(at..).unwrap_or_default();
        if comments && rest.starts_with(b"/*") {
            at = at.saturating_add(2);
            while byte_at(text, at).is_some() {
                if text.get(at..at.saturating_add(2)) == Some(b"*/") {
                    at = at.saturating_add(2);
                    break;
                }
                at = at.saturating_add(1);
            }
        } else if comments && rest.starts_with(b"//") {
            comments = false;
            at = at.saturating_add(2);
        } else if rest.starts_with(b"\\\r\n") {
            at = at.saturating_add(3);
        } else if comments && byte == b'"' {
            junk |= check;
            at = at.saturating_add(1);
            while let Some(b) = byte_at(text, at) {
                let next = text.get(at.saturating_add(1)).copied();
                match (b, next) {
                    (b'\n', _) => break,
                    (b'"', _) => {
                        at = at.saturating_add(1);
                        break;
                    }
                    (b'\\', Some(b'"' | b'\\')) => at = at.saturating_add(2),
                    _ => at = at.saturating_add(1),
                }
            }
        } else if rest.starts_with(b"\\\n") {
            at = at.saturating_add(2);
        } else {
            junk |= check && comments && !fte_white(byte);
            at = at.saturating_add(1);
        }
    }
}

/// FTEQCC's error for a word or string past [`FTEQCC_MAX_WORD`] bytes, which
/// `QCC_PR_SimpleGetToken` and `QCC_PR_SimpleGetString` give with `QCC_Error` (FTEF-14.66).
pub(crate) const WORD_TOO_LONG: &str = "token exceeds 1023 chars";

/// This profile's error for a `#pragma` whose name or text FTEQCC's `char msg[1024]` cannot
/// hold, which the pinned build aborts at (FTEF-14.66).
const PRAGMA_TOO_LONG: &str = "#pragma name or text longer than FTEQCC's 1023 bytes";

/// FTEQCC's error for a line break in a string `QCC_PR_SimpleGetString` reads (FTEF-14.66).
const NEW_LINE_IN_STRING: &str = "new line inside string";

/// Whether `word`, read by [`simple_get_token`], is longer than FTEQCC's readers take
/// (FTEF-14.66).
pub(crate) fn too_long(word: Option<&Range<usize>>) -> bool {
    word.is_some_and(|word| word.len() > FTEQCC_MAX_WORD)
}

/// `QCC_PR_SimpleGetToken`: the word from `at` after white space on the line, up to white
/// space, `,`, `;`, `(`, `)`, `]` or `//`; `None` at a line break, at the end of the text, or
/// before a comment, or when no byte is taken. Also where reading stopped: a `//` comment is
/// read to its line break.
pub(crate) fn simple_get_token(text: &[u8], mut at: usize) -> (Option<Range<usize>>, usize) {
    loop {
        let Some(byte) = byte_at(text, at) else {
            return (None, at.min(text.len()));
        };
        if !fte_white(byte) {
            break;
        }
        if byte == b'\n' {
            return (None, at);
        }
        at = at.saturating_add(1);
    }
    let comment = |at: usize| {
        byte_at(text, at) == Some(b'/') && text.get(at.saturating_add(1)) == Some(&b'/')
    };
    if comment(at) {
        while byte_at(text, at).is_some_and(|b| b != b'\n') {
            at = at.saturating_add(1);
        }
        return (None, at.min(text.len()));
    }
    if text.get(at..at.saturating_add(2)) == Some(b"/*") {
        return (None, at);
    }
    let start = at;
    while let Some(byte) = byte_at(text, at) {
        if fte_white(byte) || b",;()]".contains(&byte) || comment(at) {
            break;
        }
        at = at.saturating_add(1);
    }
    ((at > start).then_some(start..at), at)
}

/// `QCC_PR_SimpleGetString`: after white space on the line, the `"` string from there, read to
/// its closing quote, `pr_token`'s text (`\n`, `\r` and `\t` the bytes they name, and `\` before
/// any other byte that byte), and where reading stopped, after the quote; nothing is read, and
/// reading stays, at a line break, a comment, a byte that opens no string, or the end of the text
/// in the string. A line break in the string, and a 1024th byte, stop FTEQCC's compile: the
/// error, "new line inside string" or "token exceeds 1023 chars" (FTEF-14.66).
fn simple_get_string(text: &[u8], mut at: usize) -> Result<(Option<Vec<u8>>, usize), &'static str> {
    loop {
        let Some(byte) = byte_at(text, at) else {
            return Ok((None, at.min(text.len())));
        };
        if !fte_white(byte) {
            break;
        }
        if byte == b'\n' {
            return Ok((None, at));
        }
        at = at.saturating_add(1);
    }
    if byte_at(text, at) != Some(b'"') {
        return Ok((None, at));
    }
    let mut token = Vec::new();
    let mut end = at.saturating_add(1);
    while let Some(byte) = byte_at(text, end) {
        end = end.saturating_add(1);
        if byte == b'\n' {
            return Err(NEW_LINE_IN_STRING);
        }
        if byte != b'"' && token.len() >= FTEQCC_MAX_WORD {
            return Err(WORD_TOO_LONG);
        }
        match byte {
            b'"' => return Ok((Some(token), end)),
            b'\\' => {
                let Some(escaped) = byte_at(text, end) else {
                    break;
                };
                end = end.saturating_add(1);
                token.push(match escaped {
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    other => other,
                });
            }
            other => token.push(other),
        }
    }
    Ok((None, at))
}

/// `QCC_PR_LexInteger`: the offset after the number at `at`, which takes the byte there
/// whatever it is, then digits, `a` to `f` and `.` not before another `.`; and the number
/// `atoi` reads from those bytes into FTEQCC's `int` (FTEF-14.55).
fn lex_integer(text: &[u8], at: usize) -> (usize, i32) {
    let mut end = at;
    if text.get(at) == Some(&b'0') && matches!(text.get(at.saturating_add(1)), Some(b'x' | b'X')) {
        end = end.saturating_add(2);
    }
    end = end.saturating_add(1);
    while let Some(byte) = byte_at(text, end) {
        let more = byte.is_ascii_digit()
            || (byte == b'.' && text.get(end.saturating_add(1)) != Some(&b'.'))
            || (b'a'..=b'f').contains(&byte);
        if !more {
            break;
        }
        end = end.saturating_add(1);
    }
    let end = end.min(text.len());
    (end, cond::fte_atoi(text.get(at..end).unwrap_or_default()))
}

/// `QCC_COM_Parse`: after white space and comments from `at`, a `"` string with its escapes
/// read, one of `{`, `}`, `)`, `(`, `'`, `:` and `,`, or a word up to white space or one of
/// those or `"`; the token and where reading goes on, `None` at the end of the text. The token
/// holds at most the 1023 bytes `qcc_token` does, the bytes past them dropped (`if (len >=
/// sizeof(qcc_token)-1) ; else ...`, FTEF-14.66). Implements PRJ-4.9.
#[must_use]
pub fn com_parse(text: &[u8], mut at: usize) -> Option<(Vec<u8>, usize)> {
    loop {
        while byte_at(text, at).is_some_and(fte_white) {
            at = at.saturating_add(1);
        }
        let byte = byte_at(text, at)?;
        let next = text.get(at.saturating_add(1)).copied();
        match (byte, next) {
            (b'/', Some(b'/')) => {
                while byte_at(text, at).is_some_and(|b| b != b'\n') {
                    at = at.saturating_add(1);
                }
            }
            (b'/', Some(b'*')) => {
                while byte_at(text, at.saturating_add(1)).is_some()
                    && text.get(at..at.saturating_add(2)) != Some(b"*/")
                {
                    at = at.saturating_add(1);
                }
                at = at.saturating_add(2);
            }
            _ => break,
        }
    }
    let first = byte_at(text, at)?;
    let mut token = Vec::new();
    if first == b'"' {
        at = at.saturating_add(1);
        loop {
            let Some(byte) = byte_at(text, at) else {
                return Some((token, at));
            };
            at = at.saturating_add(1);
            let next = text.get(at).copied();
            let byte = match (byte, next) {
                (b'\\', Some(b'"' | b'\\')) => {
                    at = at.saturating_add(1);
                    next.unwrap_or(byte)
                }
                (b'\\', Some(b'n')) => {
                    at = at.saturating_add(1);
                    b'\n'
                }
                (b'\\', Some(b'r')) => {
                    at = at.saturating_add(1);
                    b'\r'
                }
                (b'\\', Some(b't')) => {
                    at = at.saturating_add(1);
                    b'\t'
                }
                (b'"', _) => return Some((token, at)),
                (b'\n' | b'\r', _) if token.last() == Some(&b'"') => return Some((token, at)),
                (byte, _) => byte,
            };
            if token.len() < FTEQCC_MAX_WORD {
                token.push(byte);
            }
        }
    }
    let single = |b: u8| matches!(b, b'{' | b'}' | b')' | b'(' | b'\'' | b':' | b',');
    if single(first) {
        return Some((vec![first], at.saturating_add(1)));
    }
    while let Some(byte) = byte_at(text, at) {
        if token.len() < FTEQCC_MAX_WORD {
            token.push(byte);
        }
        at = at.saturating_add(1);
        match byte_at(text, at) {
            Some(b) if single(b) || b == b'"' || fte_white(b) => break,
            Some(_) => {}
            None => break,
        }
    }
    Some((token, at))
}

/// Which side of a conditional `#elif` reads (FTEF-14.37).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElifTest {
    /// `#elifdef NAME`.
    Defined,
    /// `#elifndef NAME`.
    NotDefined,
    /// `#elif EXPR`.
    Expression,
}

/// The test of an `#if`, `#ifdef` or `#ifndef` (FTEF-14.57).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IfTest {
    /// `#ifdef NAME`: true when NAME is a macro.
    Defined,
    /// `#ifndef NAME`.
    NotDefined,
    /// `#if EXPR`, the expression from this offset.
    Expression(usize),
}

/// A pragma as `QCC_PR_Precompiler` reads it (FTEF-14.43, FTEF-14.70): its name, and the text
/// after it, from after a space or tab or from a `(`, to the end of the line, its trailing white
/// space dropped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pragma {
    pub(crate) name: Vec<u8>,
    pub(crate) message: Vec<u8>,
}

impl Pragma {
    /// Whether the pragma's name is `word` in any case (`QC_strcasecmp`).
    pub(crate) fn is(&self, word: &str) -> bool {
        self.name.eq_ignore_ascii_case(word.as_bytes())
    }
}

/// Which of `#define` and `#append` defines a macro (FTEF-14.39).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Definer {
    /// `#define`: the body replaces any the macro had.
    Define,
    /// `#append`: the body is added to the macro's, when its parameters are the same.
    Append,
}

/// A directive as `QCC_PR_Precompiler` reads it, before it is run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Directive {
    /// A `#` with only white space after it on its line: "Hanging # with no directive".
    Hanging(LineEnd),
    /// `#define` or `#append`, whose name `QCC_PR_PreProcessor_Define` reads after the word
    /// that ends at `word_end`, the directive's own (FTEF-14.24).
    Define { definer: Definer, word_end: usize },
    /// `#undef` and the name after it, possibly empty (FTEF-14.57).
    Undef { name: Range<usize>, end: LineEnd },
    /// `#if`, `#ifdef` or `#ifndef`; for a name, the name and the line's end.
    If {
        test: IfTest,
        name: Range<usize>,
        end: LineEnd,
    },
    /// `#if` and a byte that is no white space after its name: "Unknown pragma", and reading
    /// goes on after the word, as code (FTEF-14.57); the word starts after the `#`'s blanks.
    NotIf { word: Range<usize> },
    /// `#else` or `#elif` on a side that is taken (FTEF-14.37), whose line `#else` reads with
    /// [`Junk::Checked`] and `#elif` with no check.
    Else { end: LineEnd },
    /// `#endif`.
    Endif { end: LineEnd },
    /// `#eof`, which ends the text being read.
    Eof,
    /// `#error` and its text.
    Error { message: Vec<u8>, end: LineEnd },
    /// `#warning` and its text.
    Warning { message: Vec<u8>, end: LineEnd },
    /// A directive that changes only the build: `#message`, `#copyright`, `#forcecrc`,
    /// `#merge` and `#datafile` (FTEF-14.39).
    Build { end: LineEnd },
    /// `#output` and the text of the string after it, which names the module's output for the
    /// project graph (PRJ-5.6); `None` when no string follows (FTEF-14.39).
    Output { name: Option<Vec<u8>>, end: LineEnd },
    /// `#pack`, too many of them or not (FTEF-14.39).
    Pack { end: LineEnd, too_many: bool },
    /// `#package`, which ends the text being read once its line is read.
    Package { end: LineEnd },
    /// `#includelist`, whose list starts at the line's end (FTEF-14.41).
    IncludeList { end: LineEnd },
    /// `#include` and the bytes between its quotes, or FTEQCC's error (FTEF-14.40).
    Include {
        name: Result<Vec<u8>, &'static str>,
        end: LineEnd,
    },
    /// `#pragma` (FTEF-14.43, FTEF-14.70); its line ends at the break.
    Pragma { pragma: Pragma, end: usize },
    /// A word or string of the directive that stops FTEQCC's compile (`QCC_Error`), FTEQCC's
    /// error and where reading stopped (FTEF-14.66).
    Fatal { message: &'static str, at: usize },
}

/// The directive at the `#` at `hash` (FTEF-14.52), as `QCC_PR_Precompiler` reads it: after the
/// `#` every byte up to `' '` but a line break is passed over, and the name is matched by its
/// first bytes, in FTEQCC's order: `define`, `append`, `undef`, `if`, `else` and `elif`,
/// `endif`, `eof`, `error`, `warning`, `message`, `copyright`, `package`, `pack`, `forcecrc`,
/// `merge`, `includelist`, `include`, `datafile`, `output` and `pragma`. `None` when it matches
/// none of them: the `#` is read as code (FTE-23.10). A byte of 0x80 or more is passed over too,
/// as FTEQCC's signed `char` makes it less than `' '`.
pub(crate) fn read(text: &[u8], hash: usize) -> Option<Directive> {
    let mut at = hash.saturating_add(1);
    loop {
        match byte_at(text, at) {
            None => break,
            Some(b'\r' | b'\n') => {
                return Some(Directive::Hanging(skip_to_end_of_line(
                    text,
                    at,
                    Junk::Ignored,
                )));
            }
            Some(b) if b > b' ' && b < 0x80 => break,
            Some(_) => at = at.saturating_add(1),
        }
    }
    let rest = text.get(at..).unwrap_or_default();
    let after = |len: usize| at.saturating_add(len);
    let line = |from: usize, junk: Junk| skip_to_end_of_line(text, from, junk);
    let directive = if rest.starts_with(b"define") || rest.starts_with(b"append") {
        let (word, word_end) = simple_get_token(text, at);
        if too_long(word.as_ref()) {
            return Some(Directive::Fatal {
                message: WORD_TOO_LONG,
                at: word_end,
            });
        }
        let definer = if rest.starts_with(b"append") {
            Definer::Append
        } else {
            Definer::Define
        };
        Directive::Define { definer, word_end }
    } else if rest.starts_with(b"undef") {
        let (name, stop) = simple_get_token(text, skip_blanks(text, after(5)));
        if too_long(name.as_ref()) {
            return Some(Directive::Fatal {
                message: WORD_TOO_LONG,
                at: stop,
            });
        }
        Directive::Undef {
            name: name.unwrap_or(stop..stop),
            end: line(stop, Junk::Checked),
        }
    } else if rest.starts_with(b"if") {
        read_if(text, at)
    } else if rest.starts_with(b"else") || rest.starts_with(b"elif") {
        let junk = if rest.starts_with(b"elif") {
            Junk::Ignored
        } else {
            Junk::Checked
        };
        Directive::Else {
            end: line(after(4), junk),
        }
    } else if rest.starts_with(b"endif") {
        Directive::Endif {
            end: line(after(5), Junk::Checked),
        }
    } else if rest.starts_with(b"eof") {
        Directive::Eof
    } else if rest.starts_with(b"error") {
        Directive::Error {
            message: message(text, after(5), |b| b == b'\t' || b == b'\n'),
            end: line(after(5), Junk::Ignored),
        }
    } else if rest.starts_with(b"warning") {
        Directive::Warning {
            message: message(text, after(7), |b| b == b'\r' || b == b'\n'),
            end: line(after(7), Junk::Ignored),
        }
    } else if rest.starts_with(b"message") {
        Directive::Build {
            end: line(after(7), Junk::Ignored),
        }
    } else if rest.starts_with(b"copyright") {
        Directive::Build {
            end: line(after(9), Junk::Ignored),
        }
    } else if rest.starts_with(b"package") {
        Directive::Package {
            end: line(after(7), Junk::Checked),
        }
    } else if rest.starts_with(b"pack") {
        read_pack(text, after(4))
    } else if rest.starts_with(b"forcecrc") {
        let (end, _) = lex_integer(text, after(8));
        Directive::Build {
            end: line(end, Junk::Checked),
        }
    } else if rest.starts_with(b"merge") {
        match simple_get_string(text, skip_blanks(text, after(5))) {
            Ok((_, end)) => Directive::Build {
                end: line(end, Junk::Checked),
            },
            Err(message) => Directive::Fatal { message, at },
        }
    } else if rest.starts_with(b"includelist") {
        Directive::IncludeList {
            end: line(after(11), Junk::Checked),
        }
    } else if rest.starts_with(b"include") {
        read_include(text, skip_blanks(text, after(7)))
    } else if rest.starts_with(b"datafile") {
        // `pr_file_p++;` after the string, then on to the line break as bytes.
        let end = match simple_get_string(text, skip_blanks(text, after(8))) {
            Ok((_, end)) => end,
            Err(message) => return Some(Directive::Fatal { message, at }),
        };
        let end = end.saturating_add(1);
        let end = text
            .get(end..)
            .and_then(|rest| rest.iter().position(|&b| b == b'\n' || b == 0))
            .map_or(text.len(), |len| end.saturating_add(len));
        Directive::Build {
            end: LineEnd {
                at: end,
                junk: false,
            },
        }
    } else if rest.starts_with(b"output") {
        match simple_get_string(text, skip_blanks(text, after(6))) {
            Ok((name, end)) => Directive::Output {
                name,
                end: line(end, Junk::Checked),
            },
            Err(message) => Directive::Fatal { message, at },
        }
    } else if rest.starts_with(b"pragma") {
        read_pragma(text, skip_blanks(text, after(6)))
    } else {
        return None;
    };
    Some(directive)
}

/// The text of a `#warning` or `#error` from `at`: its bytes up to a NUL or a byte `stop` names,
/// at most 1023 (FTEF-14.38).
fn message(text: &[u8], at: usize, stop: impl Fn(u8) -> bool) -> Vec<u8> {
    text.get(at..)
        .unwrap_or_default()
        .iter()
        .take(FTE_MAX_MESSAGE)
        .take_while(|&&b| b != 0 && !stop(b))
        .copied()
        .collect()
}

/// `#if`, `#ifdef` or `#ifndef` whose name starts at `at` (FTEF-14.57).
fn read_if(text: &[u8], at: usize) -> Directive {
    let rest = text.get(at..).unwrap_or_default();
    let (test, from) = if rest.starts_with(b"ifdef") {
        (IfTest::Defined, at.saturating_add(5))
    } else if rest.starts_with(b"ifndef") {
        (IfTest::NotDefined, at.saturating_add(6))
    } else {
        let from = at.saturating_add(2);
        (IfTest::Expression(from), from)
    };
    if !byte_at(text, from).is_some_and(fte_white) {
        let (word, word_end) = simple_get_token(text, at);
        if too_long(word.as_ref()) {
            return Directive::Fatal {
                message: WORD_TOO_LONG,
                at: word_end,
            };
        }
        return Directive::NotIf {
            word: word.unwrap_or(word_end..word_end),
        };
    }
    if matches!(test, IfTest::Expression(_)) {
        return Directive::If {
            test,
            name: from..from,
            end: LineEnd {
                at: from,
                junk: false,
            },
        };
    }
    let (name, stop) = simple_get_token(text, from);
    if too_long(name.as_ref()) {
        return Directive::Fatal {
            message: WORD_TOO_LONG,
            at: stop,
        };
    }
    Directive::If {
        test,
        name: name.unwrap_or(stop..stop),
        end: skip_to_end_of_line(text, stop, Junk::Checked),
    }
}

/// `#pack`, read from `at` after its name: `id` and one byte more passed over, or else a
/// number read with `QCC_PR_LexInteger` and one byte more; a number above 5 is too many
/// (FTEF-14.39).
fn read_pack(text: &[u8], at: usize) -> Directive {
    let (end, too_many) = if text.get(at..).is_some_and(|rest| rest.starts_with(b"id")) {
        (at.saturating_add(3), false)
    } else {
        let (end, value) = lex_integer(text, at);
        let packs = if value == 0 { 1 } else { value };
        (end.saturating_add(1), packs > FTE_MAX_PACKS)
    };
    Directive::Pack {
        end: skip_to_end_of_line(text, end.min(text.len()), Junk::Checked),
        too_many,
    }
}

/// `#include` whose operand starts at `at` (FTEF-14.40): the bytes between `"` and `"`, or `<`
/// and `>`, on one line.
fn read_include(text: &[u8], at: usize) -> Directive {
    let close = match byte_at(text, at) {
        Some(b'"') => b'"',
        Some(b'<') => b'>',
        _ => {
            return Directive::Include {
                name: Err("Not a string literal (on a #include)"),
                end: skip_to_end_of_line(text, at, Junk::Ignored),
            };
        }
    };
    let start = at.saturating_add(1);
    let mut end = start;
    loop {
        match byte_at(text, end) {
            Some(b) if b == close => break,
            Some(b'\n') | None => {
                return Directive::Include {
                    name: Err("#include continued over line boundry"),
                    end: skip_to_end_of_line(text, end, Junk::Ignored),
                };
            }
            Some(_) => end = end.saturating_add(1),
        }
    }
    Directive::Include {
        name: Ok(text.get(start..end).unwrap_or_default().to_vec()),
        end: skip_to_end_of_line(text, end.saturating_add(1), Junk::Checked),
    }
}

/// `#pragma` whose name starts at `at` (FTEF-14.43): the bytes to the line break, split at the
/// first space, tab or `(` met while the name read so far is empty, into the name before it
/// and the text after it (from the `(`, or after the space or tab); the text's white space at
/// its end dropped, and with no split the whole line, so dropped, the name.
fn read_pragma(text: &[u8], at: usize) -> Directive {
    let line = text.get(at..).unwrap_or_default();
    let len = line
        .iter()
        .position(|&b| b == b'\n' || b == 0)
        .unwrap_or(line.len());
    let mut name = Vec::new();
    let mut message = Vec::new();
    for &byte in line.get(..len).unwrap_or_default() {
        if (blank(byte) || byte == b'(') && name.is_empty() {
            name = std::mem::take(&mut message);
            if byte != b'(' {
                continue;
            }
        }
        // FTEF-14.66: `msg[a++] = *pr_file_p;` into `char msg[1024]` with no bound, and
        // `msg[a] = '\0';` after: a name or text of 1024 bytes runs past it.
        if message.len() >= FTEQCC_MAX_WORD {
            return Directive::Fatal {
                message: PRAGMA_TOO_LONG,
                at: at.saturating_add(len),
            };
        }
        message.push(byte);
    }
    while message.last().copied().is_some_and(fte_white) {
        message.pop();
    }
    if name.is_empty() {
        name = std::mem::take(&mut message);
    }
    Directive::Pragma {
        pragma: Pragma { name, message },
        end: at.saturating_add(len),
    }
}

/// The `#elif`, `#else` or `#endif` that `QCC_PR_FalsePreProcessorIf` stops at (FTEF-14.37).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    /// An `#else` whose `#` is at `hash`, at the block's level, its line read with junk
    /// checked, while no side was taken: the side after it is taken.
    Else { hash: usize, end: LineEnd },
    /// An `#elif`, `#elifdef` or `#elifndef` whose `#` is at `hash`, at the block's level while
    /// no side was taken, its test from `at`, the first byte after `elif`, `elifdef` or
    /// `elifndef`; `malformed` when that byte is no space or tab ("malformed #elif").
    Elif {
        hash: usize,
        test: ElifTest,
        at: usize,
        malformed: bool,
    },
    /// The block's `#endif`; the line break of its line.
    Endif(usize),
    /// The end of the text: "#if with no endif".
    Eof,
}

/// Whether a side of the block being skipped was taken before, `QCC_PR_FalsePreProcessorIf`'s
/// `hadtrue` (FTEF-14.37).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Earlier {
    /// No side was taken: an `#else` or a true `#elif` takes the next.
    NoSideTaken,
    /// A side was taken: the skip runs to the block's `#endif`.
    SideTaken,
}

/// `QCC_PR_FalsePreProcessorIf` from `at`, the end of a directive's line, inside a block that
/// is `level` deep in a skipped side: each line after spaces and tabs, a `#` and spaces and tabs
/// starting `endif` closes a level, one starting `if` opens one, and at the block's own level,
/// while no side was taken ([`Earlier::NoSideTaken`]), one starting `else` or `elif` is a side to
/// try. Each line ends where `QCC_PR_SkipToEndOfLine` ends it. Returns what it stopped at and the
/// block's level there, for an `#elif` whose test is false to go on from.
pub(crate) fn false_side(text: &[u8], mut at: usize, earlier: Earlier, level: &mut usize) -> Side {
    let open = earlier == Earlier::NoSideTaken;
    loop {
        at = skip_blanks(text, at);
        if byte_at(text, at).is_none() {
            return Side::Eof;
        }
        if text.get(at) == Some(&b'#') {
            let name = skip_blanks(text, at.saturating_add(1));
            let rest = text.get(name..).unwrap_or_default();
            if rest.starts_with(b"endif") {
                *level = level.saturating_sub(1);
            }
            if rest.starts_with(b"if") {
                *level = level.saturating_add(1);
            }
            if open && *level == 1 && rest.starts_with(b"else") {
                let end = skip_to_end_of_line(text, name.saturating_add(4), Junk::Checked);
                return Side::Else { hash: at, end };
            }
            if open && *level == 1 && rest.starts_with(b"elif") {
                let after = name.saturating_add(4);
                let tail = text.get(after..).unwrap_or_default();
                let (test, from) = if tail.starts_with(b"def") {
                    (ElifTest::Defined, after.saturating_add(3))
                } else if tail.starts_with(b"ndef") {
                    (ElifTest::NotDefined, after.saturating_add(4))
                } else {
                    (ElifTest::Expression, after)
                };
                let malformed = !byte_at(text, from).is_some_and(blank);
                return Side::Elif {
                    hash: at,
                    test,
                    at: from,
                    malformed,
                };
            }
            at = name;
        }
        let end = skip_to_end_of_line(text, at, Junk::Ignored).at;
        if *level == 0 {
            return Side::Endif(end);
        }
        if byte_at(text, end).is_none() {
            return Side::Eof;
        }
        at = end.saturating_add(1);
    }
}

/// `QCC_PR_FalsePreProcessorIf` going on after an `#elif` whose test was false, from `at`,
/// where its test stopped reading.
pub(crate) fn false_side_after(
    text: &[u8],
    at: usize,
    earlier: Earlier,
    level: &mut usize,
) -> Side {
    let end = skip_to_end_of_line(text, at, Junk::Ignored).at;
    if byte_at(text, end).is_none() {
        return Side::Eof;
    }
    false_side(text, end.saturating_add(1), earlier, level)
}

/// An `#if` or `#elif` expression from `from` in `text`, read as `ParsePrecompilerIf` reads it
/// (FTEF-14.33), each macro's text being what `macros` gives: whether it is true, or FTEQCC's
/// error, and where reading it stopped, which for an expression nested too deeply (FTEF-14.12)
/// is `from`.
pub(crate) fn condition(
    text: &[u8],
    from: usize,
    macros: &dyn Fn(&str) -> Option<String>,
) -> (Result<bool, FteIfError>, usize) {
    let (value, read) = cond::fte_if(text.get(from..).unwrap_or_default(), macros);
    match value {
        Ok(value) => (Ok(value != 0), from.saturating_add(read)),
        Err(FteIfError::TooDeep) => (Err(FteIfError::TooDeep), from),
        Err(error) => (Err(error), from.saturating_add(read)),
    }
}

/// An `#elif` that `QCC_PR_FalsePreProcessorIf` tests (FTEF-14.37): the `#` at `hash`, which
/// test it reads from `at`, the first byte after `elif`, `elifdef` or `elifndef`, and whether
/// that byte is no space or tab ("malformed #elif").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Elif {
    pub(crate) hash: usize,
    pub(crate) test: ElifTest,
    pub(crate) at: usize,
    pub(crate) malformed: bool,
}

/// Where a skipped side ends (FTEF-14.37).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Skipped {
    /// At the side the `#else` or `#elif` whose `#` is at `hash` takes, whose line ends at `end`.
    Taken { hash: usize, end: LineEnd },
    /// At the line break of the block's `#endif`.
    Endif(usize),
    /// At the end of the text: "#if with no endif".
    Eof,
}

/// `QCC_PR_FalsePreProcessorIf` from `from`, the end of the line of a conditional whose side is
/// skipped (FTEF-14.37), to the side an `#else` takes, or the first `#elif` that `test` finds
/// true, or to the block's `#endif`. `test` gives an `#elif`'s value, where reading its test
/// stopped and how the rest of its line is read.
pub(crate) fn skip_side(
    text: &[u8],
    from: usize,
    earlier: Earlier,
    mut test: impl FnMut(Elif) -> (bool, usize, Junk),
) -> Skipped {
    let mut level = 1_usize;
    let mut side = false_side(text, from, earlier, &mut level);
    loop {
        match side {
            Side::Else { hash, end } => return Skipped::Taken { hash, end },
            Side::Elif {
                hash,
                test: kind,
                at,
                malformed,
            } => {
                let (value, stop, junk) = test(Elif {
                    hash,
                    test: kind,
                    at,
                    malformed,
                });
                if value {
                    let end = skip_to_end_of_line(text, stop, junk);
                    return Skipped::Taken { hash, end };
                }
                side = false_side_after(text, stop, earlier, &mut level);
            }
            Side::Endif(end) => return Skipped::Endif(end),
            Side::Eof => return Skipped::Eof,
        }
    }
}

/// Whether the name `QCC_PR_SimpleGetToken` reads from `at` in `text` is a macro as
/// `is_defined` says, as `#elifdef` (`Defined`) or `#elifndef` asks, and where reading it
/// stopped (FTEF-14.37).
pub(crate) fn elif_defined(
    text: &[u8],
    at: usize,
    test: ElifTest,
    is_defined: impl Fn(&str) -> bool,
) -> (Result<bool, &'static str>, usize) {
    let (name, stop) = simple_get_token(text, at);
    if too_long(name.as_ref()) {
        return (Err(WORD_TOO_LONG), stop);
    }
    let name = lossy(name.and_then(|n| text.get(n)).unwrap_or_default());
    (Ok(is_defined(&name) == (test == ElifTest::Defined)), stop)
}

/// `QCC_JoinPaths(fullname, size, name, base)`: `name` after the last `/` or `\` of `base`, or
/// `name` itself when it starts with one (FTEF-14.40).
pub(crate) fn join_paths(name: &[u8], base: &[u8]) -> Vec<u8> {
    if name.first().is_some_and(|&b| b == b'/' || b == b'\\') {
        return name.to_vec();
    }
    let dir = base
        .iter()
        .rposition(|&b| b == b'/' || b == b'\\')
        .map_or(0, |last| last.saturating_add(1));
    let mut joined = base.get(..dir).unwrap_or_default().to_vec();
    joined.extend_from_slice(name);
    joined
}

/// `QCC_FindBestInclude`'s names for `name`, in the order it tries them: joined to `compiling`,
/// the name of the file being compiled, and then to each include directory (FTEF-14.40).
pub(crate) fn include_candidates(name: &[u8], compiling: &[u8], dirs: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut names = vec![join_paths(name, compiling)];
    names.extend(dirs.iter().map(|dir| join_paths(name, dir)));
    names
}

/// The most include directories FTEQCC keeps: `#define MAXINCLUDEDIRS 8` in `qcc_pr_lex.c`;
/// one more is "Too many include dirs. Ignoring and hoping the stars align." (FTEF-14.70).
const FTE_MAX_INCLUDE_DIRS: usize = 8;

/// `sizeof(qccincludedir)`, the size `QCC_PR_AddIncludePath` copies a name into: the whole
/// table, `char qccincludedir[MAXINCLUDEDIRS][256]` (FTEF-14.68).
const FTE_INCLUDE_TABLE_BYTES: usize = 2048;

impl FteState {
    /// `QCC_PR_AddIncludePath(new)`: `new` added after the include directories, `.` for an empty
    /// one and a `/` after it unless it ends in `/` or `\`, unless it is there already or there
    /// are 8 (FTEF-14.70).
    ///
    /// Returns FTEQCC's warning, if any (FTEF-14.68): "Too many include dirs. Ignoring and hoping
    /// the stars align." for a ninth, and "Include path too long." for a name that with its `/`
    /// does not fit `sizeof(qccincludedir)`, which is the whole table's 2048 bytes; the name is
    /// then not kept.
    pub(crate) fn add_include_dir(&mut self, new: &[u8]) -> Option<&'static str> {
        let dirs = &mut self.include_dirs;
        let given: &[u8] = if new.is_empty() { b"." } else { new };
        if dirs.iter().any(|dir| dir.as_slice() == given) {
            return None;
        }
        if dirs.len() >= FTE_MAX_INCLUDE_DIRS {
            return Some("Too many include dirs. Ignoring and hoping the stars align.");
        }
        let mut dir = given.to_vec();
        if dir.last().is_some_and(|&b| b != b'/' && b != b'\\') {
            dir.push(b'/');
        }
        if dir.len() >= FTE_INCLUDE_TABLE_BYTES {
            return Some("Include path too long.");
        }
        dirs.push(dir);
        None
    }
}

impl Pragma {
    /// The include directory `#pragma includedir` adds, in a file being compiled as `compiling`
    /// (FTEF-14.70): the text's first token read with `QCC_COM_Parse`, with a `/` after it unless
    /// it ends in `/` or `\`, joined to `compiling`.
    pub(crate) fn include_dir(&self, compiling: &[u8]) -> Vec<u8> {
        let mut dir = com_parse(&self.message, 0)
            .map(|(token, _)| token)
            .unwrap_or_default();
        if dir.last().is_some_and(|&b| b != b'/' && b != b'\\') {
            dir.push(b'/');
        }
        join_paths(&dir, compiling)
    }

    /// The name `#pragma file` gives (FTEF-14.32): the text, or within the `(` it starts with and
    /// the first `)` after it.
    pub(crate) fn file_name(&self) -> Vec<u8> {
        match self.message.strip_prefix(b"(") {
            Some(inside) => inside
                .iter()
                .position(|&b| b == b')')
                .and_then(|close| inside.get(..close))
                .unwrap_or(inside)
                .to_vec(),
            None => self.message.clone(),
        }
    }

    /// The line `#pragma line` gives the line after it (FTEF-14.43): `pr_source_line =
    /// strtoul(m, &m, 0)-1;`, `m` the text after a `(` it starts with, and the line break after
    /// the pragma counting one, the number `strtoul` reads kept to the 32 bits of FTEQCC's `int`.
    pub(crate) fn line(&self) -> i32 {
        let text = self.message.strip_prefix(b"(").unwrap_or(&self.message);
        let value = strtoul(text);
        // `(int)(value - 1) + 1`: the low 32 bits of `value`, as two's complement.
        i32::from_le_bytes(
            value
                .to_le_bytes()
                .first_chunk::<4>()
                .copied()
                .unwrap_or_default(),
        )
    }
}

/// C's `strtoul(text, &end, 0)` on a 64-bit `unsigned long`: white space, a sign, then `0x` and
/// hex digits, `0` and octal digits, or decimal digits; a value too large is `ULONG_MAX`, and a
/// `-` negates the value modulo 2^64.
fn strtoul(text: &[u8]) -> u64 {
    let mut at = text
        .iter()
        .take_while(|&&b| fte_white(b) || b == 0x0c)
        .count();
    let negative = match text.get(at) {
        Some(b'-') => {
            at = at.saturating_add(1);
            true
        }
        Some(b'+') => {
            at = at.saturating_add(1);
            false
        }
        _ => false,
    };
    let rest = text.get(at..).unwrap_or_default();
    let hex = matches!(rest, [b'0', b'x' | b'X', digit, ..] if digit.is_ascii_hexdigit());
    let (radix, digits) = if hex {
        (16, rest.get(2..).unwrap_or_default())
    } else if rest.first() == Some(&b'0') {
        (8, rest)
    } else {
        (10, rest)
    };
    let mut value = 0_u64;
    let mut overflow = false;
    for &byte in digits {
        let Some(digit) = char::from(byte).to_digit(radix) else {
            break;
        };
        match value
            .checked_mul(u64::from(radix))
            .and_then(|v| v.checked_add(u64::from(digit)))
        {
            Some(next) => value = next,
            None => overflow = true,
        }
    }
    if overflow {
        return u64::MAX;
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// The text a directive is read in: a file's, or a chunk's with the use that wrote it.
#[derive(Clone)]
pub(crate) enum Bytes {
    File(FileId, Rc<[u8]>),
    Chunk(Rc<Text>, FileSpan),
}

impl Bytes {
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            Self::File(_, text) => text,
            Self::Chunk(text, _) => text.bytes(),
        }
    }

    pub(crate) fn reading(&self) -> Reading<'_> {
        match self {
            Self::File(file, text) => Reading::File(*file, text),
            Self::Chunk(text, at) => Reading::Chunk(text, *at),
        }
    }
}

/// Where reading goes on after a directive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Resume {
    /// At this offset of the text the directive was in.
    At(usize),
    /// Past the end of that text: `#eof`, `#package`, `#pragma DONT_COMPILE_THIS_FILE` or a
    /// skipped side that does not end.
    End,
}

/// Where a directive is met (FTEF-14.40, FTEF-14.47).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Position {
    /// At a line of a file or a chunk, where reading the text goes on after it.
    Line,
    /// Inside another reading that goes on after it: a use's arguments, an `#includelist` or
    /// the text `#pragma DONT_COMPILE_THIS_FILE` passes over.
    Nested,
}

/// What FTEQCC keeps from file to file while it compiles a module (FTEF-14.37, FTEF-14.40,
/// FTEF-14.70): its `static int ifs`, `qccincludedir`, `qccincludeonced` and
/// `qccwarningaction[WARN_EVILPREPROCESSOR]`.
#[derive(Debug, Default)]
pub(crate) struct FteState {
    /// How many conditionals are open on a side that is taken, in any file.
    pub(crate) ifs: usize,
    /// The include directories, each ending in `/` or `\`.
    pub(crate) include_dirs: Vec<Vec<u8>>,
    /// The names of the compiled files `#pragma once` marked.
    pub(crate) once: Vec<Vec<u8>>,
    /// `qccwarningaction`, the warning settings (FTEF-14.63).
    pub(crate) warnings: WarningSettings,
    /// The compiler flags and keywords on (FTEF-12.11).
    pub(crate) flags: FlagSettings,
    /// `qcc_targetformat`: the command line's, then each `#pragma TARGET`'s (FTEF-20.4).
    pub(crate) target: TargetFormat,
}

impl FteState {
    /// The keywords the parser reads from here (FTEF-20.1): the flags', under the target.
    pub(crate) fn keywords(&self) -> Keywords {
        self.flags
            .keywords()
            .with_hexen2_target(self.target.keeps_float())
    }
}

/// The most directives one inside another's reading this profile runs: a deliberate divergence,
/// as FTEQCC's recursion has no bound (FTEF-14.47).
const FTE_MAX_NESTED_DIRECTIVES: usize = 64;

/// "unexpected tokens at end of line" (FTEF-14.35).
const JUNK: &str = "unexpected tokens at end of line";

impl<L: Loader> Preprocessor<L> {
    /// `QCC_SetDefaultProperties`'s part in the directives (FTEF-14.48): no include directory,
    /// no `#pragma once` mark and F303 an error (`QCC_PR_CloseProcessor`,
    /// `qccwarningaction[WARN_EVILPREPROCESSOR] = WA_ERROR;`), the count of open sides kept, and
    /// `FTEQCC` and `__FTEQCC__` defined with empty bodies (`QCC_PR_DefineName("FTEQCC",
    /// NULL);`), which have no place in a file, so their definition is at no file's byte. Then
    /// `QCC_PR_CommandLinePrecompilerOptions` adds the module's include directories, its `-I`
    /// options, with `QCC_PR_AddIncludePath` (FTEF-14.49). Under GMQCC, whose `ftepp_create`
    /// defines neither, as the C preprocessor Xonotic is read with does not, the two are not
    /// defined (PRJ-5.15).
    pub(crate) fn fte_default_properties(&mut self) {
        // `QCC_SetDefaultProperties` sets no compiler flag: `QCC_main` set them once, before the
        // first pass, so a `#pragma flag` holds into the second (FTEF-12.11).
        // FTEF-14.72: then `QCC_PR_CommandLinePrecompilerOptions` reads the command line again.
        let defaults = FteState::default();
        // FTEF-21.4: `QCC_PrioritiseOpcodes` runs once, before the first pass, so the priorities
        // a pragma set hold into the second too.
        let priorities = self.fte.flags.priorities();
        // FTEF-21.14: `defaultstatic = 0;`, while `defaultnoref` and `defaultnosave` hold.
        self.fte.flags.set_default(PragmaDefault::Static, false);
        self.fte = FteState {
            ifs: self.fte.ifs,
            warnings: self.command_line.warnings(defaults.warnings),
            flags: self
                .command_line
                .flags(self.fte.flags)
                .with_priorities(priorities),
            target: self.command_line.target(),
            ..defaults
        };
        // FTEF-14.68: FTEQCC prints a `-I` option's warning against no file, "cmdline".
        for dir in &self.command_line_dirs {
            let _ = self.fte.add_include_dir(dir);
        }
        let names: &[&str] = match self.dialect {
            Dialect::Fteqcc => &["FTEQCC", "__FTEQCC__"],
            Dialect::Gmqcc | Dialect::Vanilla => &[],
        };
        for &name in names {
            let mac = Macro {
                params: None,
                variadic: Variadic::No,
                body: Body::Text(Rc::default()),
                file: FileId(u32::MAX),
                name_span: Span::new(0, 0),
                evil: false,
            };
            self.macros.insert_builtin(name, Rc::new(mac));
            self.macros_changed();
        }
    }

    /// FTEQCC's `pr_source_line` at `at` in the file `file` (FTEF-14.43): the line it is on, or
    /// as the last `#pragma line` before it in the file numbers it.
    pub(crate) fn fte_line(&self, run: &Run, file: FileId, at: usize) -> i32 {
        let line = |at: usize| self.line(file, at);
        let base = run
            .cursors
            .iter()
            .rev()
            .find(|c| c.file == file)
            .and_then(|c| c.line_base);
        // A file's base is set as its reading passes the pragma, and no offset before it is
        // asked for after that: tokens are read in order, and a use whose scan ran a directive
        // is refused with reading going on after its arguments, none read again (FTEF-14.45).
        match base {
            Some((from, value)) => value.wrapping_add_unsigned(line(at).saturating_sub(line(from))),
            None => i32::try_from(line(at)).unwrap_or(i32::MAX),
        }
    }

    /// `s_filen` as `__FILE__` writes it (FTEF-14.32): the name the innermost `#pragma file`
    /// of an open chunk gave, or else the name of the file being read.
    pub(crate) fn fte_file_name(run: &Run) -> &[u8] {
        run.chunks
            .iter()
            .rev()
            .find_map(ChunkReader::file_name)
            .or_else(|| run.cursors.last().map(|c| c.name.as_slice()))
            .unwrap_or_default()
    }

    /// The text of the macro `name` as an `#if` reads it, `c->value`: the body FTEQCC stored.
    fn fte_macro_value(&self, name: &str) -> Option<String> {
        self.macros.get(name).map(|m| {
            m.text()
                .map_or_else(String::new, |text| lossy(text.bytes()))
        })
    }

    /// `QCC_PR_LexWhitespace(false)` from `at` in `bytes` (FTEF-14.45): the offset of the first
    /// byte that is no white space and in no comment, running each directive at a line's start
    /// on the way; [`WhiteStop`] when one ends the text or the text ends in a comment
    /// (FTEF-14.67), which the caller reports.
    pub(crate) fn fte_whitespace(
        &mut self,
        run: &mut Run,
        bytes: &Bytes,
        mut at: usize,
    ) -> Result<usize, WhiteStop> {
        loop {
            match whitespace(bytes.bytes(), at) {
                Gap::Text(at) => return Ok(at),
                Gap::Unclosed(open) => return Err(WhiteStop::Unclosed(open)),
                Gap::LineStart(hash) => {
                    match self.fte_directive(run, bytes, hash, Position::Nested) {
                        Some(Resume::At(end)) => at = end,
                        Some(Resume::End) => return Err(WhiteStop::Ended),
                        None => return Ok(hash),
                    }
                }
            }
        }
    }

    /// `QCC_PR_Precompiler` at the `#` at `hash` in `bytes`, met at `position` (FTEF-14.52 to
    /// FTEF-14.47): runs the directive and says where reading goes on, or `None` when the `#`
    /// starts no directive and is read as code.
    pub(crate) fn fte_directive(
        &mut self,
        run: &mut Run,
        bytes: &Bytes,
        hash: usize,
        position: Position,
    ) -> Option<Resume> {
        let directive = read(bytes.bytes(), hash)?;
        if position == Position::Line {
            return Some(self.fte_run(run, bytes, hash, directive, position));
        }
        run.directives_run = run.directives_run.wrapping_add(1);
        if run.nested_directives >= FTE_MAX_NESTED_DIRECTIVES {
            let at = bytes.reading().place(hash..hash.saturating_add(1));
            run.report(Code::E0121, "directives nested too deeply", at);
            return Some(Resume::End);
        }
        run.nested_directives = run.nested_directives.saturating_add(1);
        let resume = self.fte_run(run, bytes, hash, directive, position);
        run.nested_directives = run.nested_directives.saturating_sub(1);
        Some(resume)
    }

    /// Runs `directive`, read at the `#` at `hash` in `bytes`.
    fn fte_run(
        &mut self,
        run: &mut Run,
        bytes: &Bytes,
        hash: usize,
        directive: Directive,
        position: Position,
    ) -> Resume {
        let text = bytes.bytes();
        let reading = bytes.reading();
        let at = |end: usize| reading.place(hash..end.max(hash.saturating_add(1)));
        match directive {
            Directive::Hanging(end) => {
                run.report(Code::E0121, "Hanging # with no directive", at(end.at));
                Resume::At(end.at)
            }
            // FTEF-14.66: `QCC_Error` stops the compile, so nothing more of the text is read.
            Directive::Fatal { message, at: stop } => {
                run.report(Code::E0121, message, at(stop));
                Resume::End
            }
            Directive::Define { definer, word_end } => {
                match self.fte_define(run, bytes, (hash, word_end), definer) {
                    Ok(stop) => {
                        let end = skip_to_end_of_line(text, stop, Junk::Checked);
                        end.resume(run, &reading, hash)
                    }
                    // FTEQCC's compile stops at the error, so the line's rest is no error too.
                    Err(stop) => Resume::At(skip_to_end_of_line(text, stop, Junk::Ignored).at),
                }
            }
            Directive::Undef { name, end } => {
                let name = lossy(text.get(name).unwrap_or_default());
                // FTEF-14.57: `QCC_PR_ParseWarning(WARN_UNDEFNOTDEFINED, "Precompiler constant %s
                // was not defined", name);`
                match self.macros.remove(&name) {
                    Undef::Removed => self.macros_changed(),
                    Undef::Kept => {}
                    Undef::Absent => {
                        let message = format!("Precompiler constant {name} was not defined");
                        run.report(Code::W0114, message, at(end.at));
                    }
                }
                end.resume(run, &reading, hash)
            }
            Directive::If { test, name, end } => {
                let (value, end) = match test {
                    IfTest::Expression(from) => {
                        let (value, stop, junk) = self.fte_condition(run, &reading, hash, from);
                        (value, skip_to_end_of_line(text, stop, junk))
                    }
                    IfTest::Defined | IfTest::NotDefined => {
                        let name = lossy(text.get(name).unwrap_or_default());
                        let defined = self.macros.contains_key(&name);
                        (defined == (test == IfTest::Defined), end)
                    }
                };
                let resume = end.resume(run, &reading, hash);
                if value {
                    self.fte.ifs = self.fte.ifs.saturating_add(1);
                    return resume;
                }
                self.fte_false_side(run, bytes, hash, end.at, Earlier::NoSideTaken)
            }
            Directive::NotIf { word } => {
                // FTEF-14.57: FTEQCC names its last `QCC_COM_Parse` word; this profile the word.
                let name = lossy(text.get(word.clone()).unwrap_or_default());
                run.report(
                    Code::W0113,
                    format!("Unknown pragma '{name}'"),
                    at(word.end),
                );
                Resume::At(word.end)
            }
            Directive::Else { end } => {
                if self.fte.ifs == 0 {
                    // FTEQCC's compile stops at the error, so the line's rest is no error too.
                    run.report(Code::E0109, "#else outside of #if", at(end.at));
                    return Resume::At(end.at);
                }
                self.fte.ifs = self.fte.ifs.saturating_sub(1);
                end.report_junk(run, &reading, hash);
                self.fte_false_side(run, bytes, hash, end.at, Earlier::SideTaken)
            }
            Directive::Endif { end } => {
                let resume = end.resume(run, &reading, hash);
                if self.fte.ifs == 0 {
                    run.report(Code::E0109, "unmatched #endif", at(end.at));
                }
                self.fte.ifs = self.fte.ifs.saturating_sub(1);
                resume
            }
            Directive::Eof => Resume::End,
            Directive::Error { message, end } => {
                let message = format!("#Error: {}", lossy(&message));
                run.report(Code::E0118, message, at(end.at));
                Resume::At(end.at)
            }
            Directive::Warning { message, end } => {
                let message = format!("#warning: {}", lossy(&message));
                run.report(Code::W0103, message, at(end.at));
                Resume::At(end.at)
            }
            Directive::Build { end } | Directive::Output { end, .. } => {
                end.resume(run, &reading, hash)
            }
            Directive::Pack { end, too_many } => {
                if too_many {
                    let message = "No more than 5 packs are allowed";
                    run.report(Code::E0121, message, at(end.at));
                }
                end.resume(run, &reading, hash)
            }
            Directive::Package { end } => {
                end.report_junk(run, &reading, hash);
                Resume::End
            }
            Directive::IncludeList { end } => {
                end.report_junk(run, &reading, hash);
                self.fte_include_list(run, bytes, hash, end.at)
            }
            Directive::Include { name, end } => {
                let name = match name {
                    Ok(name) => name,
                    Err(message) => {
                        run.report(Code::E0113, message, at(end.at));
                        return Resume::At(end.at);
                    }
                };
                let resume = end.resume(run, &reading, hash);
                // `if (!*newfile) return;`: an empty name includes nothing, anywhere.
                if name.is_empty() {
                    return resume;
                }
                if position == Position::Line && matches!(bytes, Bytes::File(..)) {
                    self.fte_include(run, &name, at(end.at));
                } else {
                    let message = "#include where this profile does not follow it";
                    run.report(Code::E0121, message, at(end.at));
                }
                resume
            }
            Directive::Pragma { pragma, end } => self.fte_pragma(run, bytes, hash, &pragma, end),
        }
    }

    /// An `#if` or `#elif` expression from `from` in `reading`, whose directive's `#` is at
    /// `hash` (FTEF-14.33): its value, an error FTEQCC reports being reported and false, where
    /// reading it stopped, and whether the rest of its line is checked for junk, which it is not
    /// after an error, where FTEQCC's compile stops.
    fn fte_condition(
        &self,
        run: &mut Run,
        reading: &Reading<'_>,
        hash: usize,
        from: usize,
    ) -> (bool, usize, Junk) {
        let macros = |name: &str| self.fte_macro_value(name);
        let (value, stop) = condition(reading.bytes(), from, &macros);
        match value {
            Ok(value) => (value, stop, Junk::Checked),
            Err(FteIfError::Reported(message)) => {
                run.report(Code::E0121, message, reading.place(hash..stop));
                (false, stop, Junk::Ignored)
            }
            // FTEF-14.12: FTEQCC would read on, so what is left of the line is no junk.
            Err(FteIfError::TooDeep) => (false, stop, Junk::Ignored),
        }
    }

    /// `QCC_PR_FalsePreProcessorIf(hadtrue, ...)` from `from`, the end of the line of the
    /// conditional whose `#` is at `hash` (FTEF-14.37): the side skipped to the `#else` or the
    /// first true `#elif` after it, which open a taken side, or to the block's `#endif`; at the
    /// end of the text, "#if with no endif" at the conditional.
    fn fte_false_side(
        &mut self,
        run: &mut Run,
        bytes: &Bytes,
        hash: usize,
        from: usize,
        earlier: Earlier,
    ) -> Resume {
        let text = bytes.bytes();
        let reading = bytes.reading();
        let skipped = skip_side(text, from, earlier, |elif| {
            if elif.malformed {
                let other = elif.hash;
                let span = reading.place(other..elif.at.max(other.saturating_add(1)));
                run.report(Code::E0121, "malformed #elif", span);
            }
            match elif.test {
                ElifTest::Expression => self.fte_condition(run, &reading, elif.hash, elif.at),
                ElifTest::Defined | ElifTest::NotDefined => {
                    let defined = |name: &str| self.macros.contains_key(name);
                    match elif_defined(text, elif.at, elif.test, defined) {
                        (Ok(value), stop) => (value, stop, Junk::Checked),
                        (Err(message), stop) => {
                            run.report(Code::E0121, message, reading.place(elif.hash..stop));
                            (false, stop, Junk::Ignored)
                        }
                    }
                }
            }
        });
        match skipped {
            Skipped::Taken { hash: other, end } => {
                self.fte.ifs = self.fte.ifs.saturating_add(1);
                end.resume(run, &reading, other)
            }
            Skipped::Endif(end) => Resume::At(end),
            Skipped::Eof => {
                let at = reading.place(hash..from.max(hash.saturating_add(1)));
                run.report(Code::E0110, "#if with no endif", at);
                Resume::End
            }
        }
    }

    /// An `#includelist` whose `#` is at `hash` and whose list starts at `from`, as
    /// `QCC_PR_Precompiler` reads it (FTEF-14.41): words, each after white space read as
    /// `QCC_PR_LexWhitespace(false)` reads it, which runs the directives at the starts of its
    /// lines, up to the word `#endlist`, whose line ends the list. The words name files the
    /// project graph compiles (PRJ-5.3). At the end of the text, reached or made by a directive
    /// in the list, "#includelist without #endlist".
    fn fte_include_list(
        &mut self,
        run: &mut Run,
        bytes: &Bytes,
        hash: usize,
        from: usize,
    ) -> Resume {
        let text = bytes.bytes();
        let reading = bytes.reading();
        let mut at = from;
        loop {
            let next = match self.fte_whitespace(run, bytes, at) {
                Ok(next) => Some(next),
                Err(WhiteStop::Ended) => None,
                // FTEF-14.67: the compile's error is the comment's, and the list ends with it.
                Err(WhiteStop::Unclosed(open)) => {
                    let span = reading.place(open..open.saturating_add(2));
                    run.report(Code::E0501, EOF_IN_COMMENT, span);
                    return Resume::End;
                }
            };
            let word = next.map(|next| (next, simple_get_token(text, next)));
            match word {
                Some((next, (Some(word), stop))) if text.get(word.clone()) == Some(b"#endlist") => {
                    let end = skip_to_end_of_line(text, stop, Junk::Checked);
                    return end.resume(run, &reading, next);
                }
                Some((next, (Some(word), stop))) if too_long(Some(&word)) => {
                    let span = reading.place(next..stop);
                    run.report(Code::E0121, WORD_TOO_LONG, span);
                    return Resume::End;
                }
                Some((_, (Some(_), stop))) => {
                    at = stop;
                    if text.get(at) == Some(&b'\r') {
                        at = at.saturating_add(1);
                    }
                }
                Some((_, (None, stop))) if byte_at(text, stop).is_some() => {
                    at = stop.saturating_add(1);
                }
                // "eof in includelist".
                Some((_, (None, _))) | None => {
                    let span = reading.place(hash..from.max(hash.saturating_add(1)));
                    run.report(Code::E0406, "#includelist without #endlist", span);
                    return Resume::End;
                }
            }
        }
    }

    /// The rest of the text from `from`, which `#pragma DONT_COMPILE_THIS_FILE` passes over a
    /// byte at a time after white space read as `QCC_PR_LexWhitespace(false)` reads it, which
    /// runs the directives at the starts of its lines (FTEF-14.70).
    fn fte_dont_compile(&mut self, run: &mut Run, bytes: &Bytes, from: usize) -> Resume {
        let mut at = from;
        loop {
            let next = match self.fte_whitespace(run, bytes, at) {
                Ok(next) => next,
                Err(WhiteStop::Ended) => return Resume::End,
                // FTEF-14.67.
                Err(WhiteStop::Unclosed(open)) => {
                    let span = bytes.reading().place(open..open.saturating_add(2));
                    run.report(Code::E0501, EOF_IN_COMMENT, span);
                    return Resume::End;
                }
            };
            if byte_at(bytes.bytes(), next).is_none() {
                return Resume::End;
            }
            at = next.saturating_add(1);
        }
    }

    /// `#include` of `name`, at `at` (FTEF-14.40): `QCC_FindBestInclude(msg, compilingfile,
    /// false)` names the first file that loads of the name joined to the file being compiled and
    /// then to each include directory, and the file is read next, unless a `#pragma once`
    /// marked that name. `name` is not empty. Under GMQCC the name is joined first to the file
    /// the directive is in, as `ftepp_include_find` tries `ftepp->includename` before
    /// `ftepp->itemname` (PRJ-5.12).
    fn fte_include(&mut self, run: &mut Run, name: &[u8], at: FileSpan) {
        let mut names = include_candidates(name, &run.unit, &self.fte.include_dirs);
        if self.dialect == Dialect::Gmqcc
            && let Some(cursor) = run.cursors.iter().rev().find(|c| c.file == at.file)
        {
            let here = join_paths(name, &cursor.read_as);
            let path = normalize(&PathBuf::from(lossy(&here)));
            let known = names
                .iter()
                .any(|name| normalize(&PathBuf::from(lossy(name))) == path);
            if !known {
                names.insert(0, here);
            }
        }
        let mut first_error = None;
        let mut found = None;
        for candidate in &names {
            let path = normalize(&PathBuf::from(lossy(candidate)));
            match self.loader.load(&path) {
                Ok(text) => {
                    found = Some((candidate.clone(), path, text));
                    break;
                }
                Err(err) => {
                    first_error.get_or_insert(err);
                }
            }
        }
        let Some((full, path, text)) = found else {
            let shown = lossy(names.first().map(Vec::as_slice).unwrap_or_default());
            let reason = first_error.map(|e| e.to_string()).unwrap_or_default();
            run.report(Code::E0111, format!("cannot include {shown}: {reason}"), at);
            return;
        };
        if self.fte.once.contains(&full) {
            return;
        }
        if run.cursors.iter().any(|c| c.path == path) {
            let message = format!("recursive #include of {}", path.display());
            run.report(Code::E0112, message, at);
            return;
        }
        let included = self.add_source(run, path.clone(), text, FileStart::Included);
        let cursor = self.cursor(included, path, full, TextStart::Included);
        run.cursors.push(cursor);
    }

    /// `#pragma` (FTEF-14.43, FTEF-14.70), whose `#` is at `hash` and whose line ends at `end`
    /// in `bytes`: `DONT_COMPILE_THIS_FILE`, `once`, `file`, `line`, `includedir`, `noref`,
    /// `nosave`, `defaultstatic`, `autoproto`, `flag`, `keyword`, `TARGET` and `warning` change
    /// what is read; the others change only the build.
    fn fte_pragma(
        &mut self,
        run: &mut Run,
        bytes: &Bytes,
        hash: usize,
        pragma: &Pragma,
        end: usize,
    ) -> Resume {
        let message = pragma.message.as_slice();
        if pragma.is("DONT_COMPILE_THIS_FILE") {
            return self.fte_dont_compile(run, bytes, end);
        }
        if pragma.is("once") {
            let marked = self.pragma_base(run, bytes);
            self.fte.once.push(marked);
        } else if pragma.is("file") && !self.fte.flags.no_file_line() {
            let name = pragma.file_name();
            match bytes {
                Bytes::File(file, _) => {
                    if let Some(cursor) = run.cursors.iter_mut().rev().find(|c| c.file == *file) {
                        cursor.name = name;
                    }
                }
                Bytes::Chunk(..) => {
                    if let Some(chunk) = run.chunks.last_mut() {
                        chunk.rename(name);
                    }
                }
            }
        } else if pragma.is("line") && !self.fte.flags.no_file_line() {
            let line = pragma.line();
            let from = end.saturating_add(1);
            match bytes {
                Bytes::File(file, _) => {
                    if let Some(cursor) = run.cursors.iter_mut().rev().find(|c| c.file == *file) {
                        cursor.line_base = Some((from, line));
                    }
                }
                Bytes::Chunk(..) => {
                    if let Some(chunk) = run.chunks.last_mut() {
                        chunk.renumber(from, line);
                    }
                }
            }
        } else if pragma.is("includedir") {
            let base = self.pragma_base(run, bytes);
            let dir = pragma.include_dir(&base);
            if let Some(warning) = self.fte.add_include_dir(&dir) {
                let at = bytes.reading().place(hash..end.max(hash.saturating_add(1)));
                run.report(Code::W0117, warning, at);
            }
        } else if let Some(default) = [
            ("noref", PragmaDefault::Noref),
            ("nosave", PragmaDefault::Nosave),
            ("defaultstatic", PragmaDefault::Static),
        ]
        .into_iter()
        .find_map(|(name, default)| pragma.is(name).then_some(default))
        {
            // FTEF-12.6, FTEF-21.14: `defaultnoref = !!atoi(msg);`, and so `defaultnosave` and
            // `defaultstatic`.
            let on = cond::fte_atoi(message) != 0;
            self.fte.flags.set_default(default, on);
            run.out
                .flags
                .push((run.out.tokens.len(), FlagEdit::of_default(default, on)));
        } else if pragma.is("autoproto") {
            // FTEF-12.10: `autoprototype = *msg?!!atoi(msg):true;`
            let on = message.is_empty() || cond::fte_atoi(message) != 0;
            let index = run.out.tokens.len();
            let at = bytes.reading().place(hash..end.max(hash.saturating_add(1)));
            run.out.autoproto.push((index, on, at));
        } else if pragma.is("keyword") || pragma.is("flag") {
            let at = bytes.reading().place(hash..end.max(hash.saturating_add(1)));
            let (warnings, set) = self.fte.flags.apply_pragma(message);
            for warning in warnings {
                run.report(Code::W0113, warning, at);
            }
            // FTEF-21.1: the flags it names, which an `extern "C"` block may have changed since.
            let edit = FlagEdit::of_flags(&set, self.fte.flags.compiler_flags());
            if !edit.is_empty() {
                run.out.flags.push((run.out.tokens.len(), edit));
            }
            // FTEF-20.1: the keywords it names, which a definition may have turned off since.
            let edit = KeywordEdit::of_flags(&set, self.fte.keywords());
            if !edit.is_empty() {
                run.out.keywords.push((run.out.tokens.len(), edit));
            }
        } else if pragma.is("optimise") || pragma.is("optimize") {
            let at = bytes.reading().place(hash..end.max(hash.saturating_add(1)));
            for warning in fte_pragma::optimise_warnings(&pragma.name, message) {
                run.report(Code::W0113, warning, at);
            }
        } else if pragma.is("opcode") {
            let at = bytes.reading().place(hash..end.max(hash.saturating_add(1)));
            for warning in fte_pragma::opcode_warnings(message) {
                run.report(Code::W0113, warning, at);
            }
        } else if pragma.is("TARGET") {
            let at = bytes.reading().place(hash..end.max(hash.saturating_add(1)));
            let (target, warnings) = fte_pragma::target_warnings(message);
            for warning in warnings {
                run.report(Code::W0117, warning, at);
            }
            // FTEF-20.4: `QCC_OPCodeSetTargetName` sets `qcc_targetformat`, which a definition
            // named `float` reads.
            if let Some(target) = target {
                let was = self.fte.target.keeps_float();
                self.fte.target = target;
                if target.keeps_float() != was {
                    let edit = KeywordEdit::hexen2_target(target.keeps_float());
                    run.out.keywords.push((run.out.tokens.len(), edit));
                }
            }
        } else if !fte_pragma::is_known(&pragma.name) {
            let at = bytes.reading().place(hash..end.max(hash.saturating_add(1)));
            let name = lossy(&pragma.name);
            run.report(Code::W0113, format!("Unknown pragma '{name}'"), at);
        } else if pragma.is("warning") {
            let (settings, warnings) = self.fte.warnings.after_pragma(message);
            let at = bytes.reading().place(hash..end.max(hash.saturating_add(1)));
            for (text, now) in warnings {
                run.report_under(Some(&now), Code::W0113, text, at);
            }
            self.set_warnings(run, bytes, end, settings);
        }
        Resume::At(end)
    }

    /// Makes `settings` FTEQCC's warning settings from the end `end` of a `#pragma warning` in
    /// `bytes` on (FTEF-14.63): for the rest of the module, for the warnings of the files being
    /// read from where each reads on, and for the tokens the parser reads after it.
    fn set_warnings(
        &mut self,
        run: &mut Run,
        bytes: &Bytes,
        end: usize,
        settings: WarningSettings,
    ) {
        self.fte.warnings = settings;
        run.warnings = Some(settings);
        let index = run.out.tokens.len();
        run.out.warnings.push((index, settings));
        let holding = match bytes {
            Bytes::File(file, _) => Some(*file),
            Bytes::Chunk(..) => None,
        };
        let innermost = run.cursors.len().saturating_sub(1);
        for (at, cursor) in run.cursors.iter_mut().enumerate() {
            let from = if at == innermost && holding == Some(cursor.file) {
                end
            } else {
                cursor.stream.read_offset()
            };
            cursor.warnings.push((from, settings));
        }
    }
}

impl<L: Loader> Preprocessor<L> {
    /// The name `#pragma once` marks and `#pragma includedir` joins its directory to: the file
    /// being compiled (FTEF-14.70), or under GMQCC the file holding the pragma (PRJ-5.12).
    fn pragma_base(&self, run: &Run, bytes: &Bytes) -> Vec<u8> {
        let holding = match bytes {
            Bytes::File(file, _) if self.dialect == Dialect::Gmqcc => {
                run.cursors.iter().rev().find(|c| c.file == *file)
            }
            Bytes::File(..) | Bytes::Chunk(..) => None,
        };
        holding.map_or_else(|| run.unit.clone(), |cursor| cursor.read_as.clone())
    }
}

/// `bytes` as text, each byte that is no UTF-8 replaced.
fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
