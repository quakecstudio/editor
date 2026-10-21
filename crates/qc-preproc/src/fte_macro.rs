// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_CheckCompConst, QCC_PR_ExpandPreProcessorMacro, QCC_PR_ExpandStrCatMarkup, QCC_PR_PreProcessor_Define, QCC_PR_IncludeChunkEx, QCC_PR_LexWhitespace, QCC_PR_SimpleGetToken, QCC_PR_SkipToEndOfLine
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_cmdlib.c, QCC_COM_Parse2

//! FTEQCC's macros (`docs/spec/dialect-fteqcc.md` FTEF-14.13 to FTEF-14.32, and
//! `docs/spec/language.md` QC-9.16), ported from FTEQCC's behaviour under ADR-0017's
//! clean-port policy. FTEQCC keeps a macro's body as the text `QCC_PR_PreProcessor_Define`
//! stored, finds a use's arguments by scanning the text after its `(`, writes the expansion as
//! text with the arguments copied in as the use wrote them (`QCC_PR_ExpandPreProcessorMacro`),
//! and lexes that text as a chunk of its own (`QCC_PR_IncludeChunkEx`), reading a macro used in
//! the chunk from the chunk's text. This module does the same: a [`Text`] is such a text with
//! where each byte came from, so every token lexed from it keeps its span in the source
//! (QC-9.7), and a [`ChunkReader`] reads the tokens of one expansion. The FTEQCC function each
//! function follows is named in its rustdoc.

use std::ops::Range;
use std::rc::Rc;

use qc_lexer::{Code, Span, TokenKind, lex_with};

use crate::fte_directive::{
    self, Bytes, Definer, FTE_MAX_MACRO_NAME, Gap, Resume, TextStart, WarningAction, byte_at,
    simple_get_token, whitespace,
};
use crate::{
    Body, Expansion, ExpansionId, FTE_MAX_EXPANDED_TOKENS, FTE_MAX_OPEN_CHUNKS, FileId, FileSpan,
    Item, Loader, Macro, Place, PpToken, Preprocessor, Run, TokenStream, Variadic, cond,
    moved_frames, quote_bytes,
};

/// The most parameters an FTEQCC macro has: `MAXCONSTANTPARAMS` in FTEQCC's `qcc.h`,
/// `#define MAXCONSTANTPARAMS 32` (FTEF-14.23).
const FTE_MAX_PARAMS: usize = 32;

/// The length a macro parameter's name stays under: `MAXCONSTANTPARAMLENGTH` in `qcc.h`,
/// `#define MAXCONSTANTPARAMLENGTH 32` (FTEF-14.23).
const FTE_MAX_PARAM_LENGTH: usize = 32;

/// The most bytes of a token `QCC_COM_Parse2` keeps: its `qcc_token` is `char qcc_token[1024]`
/// and it stores a byte only `if (len < sizeof(qcc_token)-1)` (FTEF-14.25).
const FTE_MAX_TOKEN: usize = 1023;

/// Where a run of a [`Text`]'s bytes came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Origin {
    /// Copied from a file: the run's first byte is the file's byte `at`.
    Copied { file: FileId, at: usize },
    /// Made by an expansion (a `#` string's quotes and escapes, a built-in constant's value,
    /// `__VA_COUNT__`'s number), standing for the text at this span.
    Made(FileSpan),
}

impl Origin {
    /// The origin of the byte `offset` bytes into a run that starts with this origin.
    fn advanced(self, offset: usize) -> Self {
        match self {
            Self::Copied { file, at } => Self::Copied {
                file,
                at: at.saturating_add(offset),
            },
            Self::Made(span) => Self::Made(span),
        }
    }
}

/// Text FTEQCC reads as a macro's: a body as `QCC_PR_PreProcessor_Define` stored it, an
/// argument, or an expansion's chunk, each byte with where it came from.
#[derive(Clone, Debug, Default)]
pub(crate) struct Text {
    bytes: Vec<u8>,
    /// Each run of `bytes`, by the offset it starts at, in order, and where it came from.
    runs: Vec<(usize, Origin)>,
}

impl Text {
    /// The text's bytes.
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Writes `bytes`, which came from `origin`, at the end.
    fn push(&mut self, bytes: &[u8], origin: Origin) {
        if bytes.is_empty() {
            return;
        }
        let start = self.bytes.len();
        let joins = self
            .runs
            .last()
            .is_some_and(|&(run, last)| last.advanced(start.saturating_sub(run)) == origin);
        if !joins {
            self.runs.push((start, origin));
        }
        self.bytes.extend_from_slice(bytes);
    }

    /// Writes the bytes `range` of `other` at the end, each with its origin there.
    fn push_from(&mut self, other: &Self, range: Range<usize>) {
        let mut at = range.start;
        while let Some((origin, run_end)) = other.origin(at).filter(|_| at < range.end) {
            let end = run_end.min(range.end);
            self.push(other.bytes.get(at..end).unwrap_or_default(), origin);
            at = end;
        }
    }

    /// The origin of the byte at `at` and the end of its run.
    fn origin(&self, at: usize) -> Option<(Origin, usize)> {
        if at >= self.bytes.len() {
            return None;
        }
        let index = self
            .runs
            .partition_point(|&(start, _)| start <= at)
            .checked_sub(1)?;
        let &(start, origin) = self.runs.get(index)?;
        let end = self
            .runs
            .get(index.saturating_add(1))
            .map_or(self.bytes.len(), |&(next, _)| next);
        Some((origin.advanced(at.saturating_sub(start)), end))
    }

    /// Drops the bytes from `len` on (`*bufferlen = whitestart;`).
    fn truncate(&mut self, len: usize) {
        self.bytes.truncate(len);
        while self.runs.last().is_some_and(|&(start, _)| start >= len) {
            self.runs.pop();
        }
    }

    /// Drops white space (spaces and tabs, `qcc_iswhitesameline`) at the end.
    fn trim_end(&mut self) {
        let kept = self
            .bytes
            .iter()
            .rposition(|&b| b != b' ' && b != b'\t')
            .map_or(0, |last| last.saturating_add(1));
        self.truncate(kept);
    }

    /// Where the bytes `range` stand in the source, and whether they are the source's bytes
    /// there (QC-9.7): bytes of one copied run are their span; others are at where their first
    /// byte came from, through where their last did when both were copied in order from one
    /// file. A range at the end stands where the last byte does.
    pub(crate) fn place(&self, range: Range<usize>) -> Option<(FileSpan, bool)> {
        let last_byte = self.bytes.len().checked_sub(1)?;
        let start = range.start.min(last_byte);
        let (file, at, run_end) = match self.origin(start)? {
            (Origin::Copied { file, at }, run_end) => (file, at, run_end),
            (Origin::Made(span), _) => return Some((span, false)),
        };
        let len = range.end.saturating_sub(start);
        if range.end <= run_end {
            let span = Span::new(at, at.saturating_add(len));
            return Some((FileSpan { file, span }, true));
        }
        let last = range
            .end
            .checked_sub(1)
            .and_then(|i| self.origin(i.min(last_byte)));
        let end = match last {
            Some((Origin::Copied { file: f, at: a }, _)) if f == file && a >= at => {
                a.saturating_add(1)
            }
            _ => at.saturating_add(run_end.saturating_sub(start)),
        };
        Some((
            FileSpan {
                file,
                span: Span::new(at, end),
            },
            false,
        ))
    }
}

/// Where a text FTEQCC reads is: a file, or an expansion's chunk, with the use that wrote it,
/// where a byte of no place stands.
#[derive(Clone, Copy)]
pub(crate) enum Reading<'a> {
    File(FileId, &'a [u8]),
    Chunk(&'a Text, FileSpan),
}

impl Reading<'_> {
    pub(crate) fn bytes(&self) -> &[u8] {
        match self {
            Self::File(_, bytes) => bytes,
            Self::Chunk(text, _) => text.bytes(),
        }
    }

    /// Where the byte at `at` came from.
    fn origin(&self, at: usize) -> Origin {
        match self {
            Self::File(file, _) => Origin::Copied { file: *file, at },
            Self::Chunk(text, use_) => text
                .origin(at)
                .map_or(Origin::Made(*use_), |(origin, _)| origin),
        }
    }

    /// The bytes `range` as a [`Text`], each with where it came from.
    fn copy(&self, range: Range<usize>) -> Text {
        let mut out = Text::default();
        match self {
            Self::File(file, bytes) => {
                let origin = Origin::Copied {
                    file: *file,
                    at: range.start,
                };
                out.push(bytes.get(range).unwrap_or_default(), origin);
            }
            Self::Chunk(text, _) => out.push_from(text, range),
        }
        out
    }

    /// Where the bytes `range` stand in the source: their span in a file, or in a chunk where
    /// [`Text::place`] puts them, or else the use that wrote the chunk.
    pub(crate) fn place(&self, range: Range<usize>) -> FileSpan {
        match self {
            Self::File(file, _) => FileSpan {
                file: *file,
                span: Span::new(range.start, range.end),
            },
            Self::Chunk(text, use_) => text.place(range).map_or(*use_, |(span, _)| span),
        }
    }
}

/// The reading of one FTEQCC expansion: the chunk `QCC_PR_IncludeChunkEx` includes, which
/// stays open until reading goes past its last token (FTEF-14.17).
pub(crate) struct ChunkReader {
    text: Rc<Text>,
    /// The line a built-in constant read in this chunk has: `currentchunk->currentlinenumber`,
    /// the line FTEQCC was reading when it included the chunk (FTEF-14.27).
    line: i32,
    /// The name a `#pragma file` read in the chunk gave, `s_filen` until the chunk ends
    /// (FTEF-14.32, FTEF-14.46).
    file_name: Option<Vec<u8>>,
    /// The line a `#pragma line` read in the chunk gave the chunk's line starting at this
    /// offset (FTEF-14.43).
    line_base: Option<(usize, i32)>,
    id: ExpansionId,
    /// The use the chunk is the expansion of, where a token of no place stands.
    at: FileSpan,
    /// The macro the chunk is an expansion of, `chunk->cnst`.
    pub(crate) name: String,
    stream: TokenStream,
    /// The text a macro used in the chunk took as its arguments, whose lexical errors are
    /// not this chunk's (FTEF-14.22).
    consumed: Vec<Span>,
    /// The offsets of the line breaks in the chunk's text, so the line a use in it ends on is
    /// found without counting them again for each use (FTEF-14.27).
    breaks: Vec<usize>,
    /// Whether the token read last was a built-in constant, whose value is a chunk of its own,
    /// so the next token touches nothing (FTEF-14.17).
    detached: bool,
}

impl ChunkReader {
    /// The next token of the chunk, written against the one before it when nothing stands
    /// between them in the chunk's text (QC-9.16).
    pub(crate) fn next_item(&mut self) -> Option<Item> {
        let index = self.stream.pos;
        let token = self.stream.tokens.get(index)?.clone();
        self.stream.pos = index.saturating_add(1);
        let (span, verbatim) = self.place(token.span.range());
        let spelling = (!verbatim).then(|| {
            let bytes = self.text.bytes.get(token.span.range()).unwrap_or_default();
            made_spelling(&token.kind, bytes)
        });
        let detached = std::mem::take(&mut self.detached);
        let follows = index
            .checked_sub(1)
            .and_then(|before| self.stream.tokens.get(before))
            .filter(|before| !detached && before.span.end == token.span.start)
            .map(|before| self.place(before.span.range()).0);
        let kind = moved_frames(token.kind, |name| self.place(name.range()).0.span);
        Some(Item {
            tok: PpToken {
                kind,
                span,
                expansion: Some(self.id),
                spelling,
                follows,
            },
            hide: None,
            place: Place::Chunk(token.span.end),
        })
    }

    /// Where the bytes `range` of the chunk stand ([`Text::place`]), a token of the chunk
    /// always standing somewhere.
    fn place(&self, range: Range<usize>) -> (FileSpan, bool) {
        self.text.place(range).unwrap_or((self.at, false))
    }

    /// Marks the token read last as a built-in constant's (FTEF-14.17).
    pub(crate) fn detach(&mut self) {
        self.detached = true;
    }

    /// The offset of the next token when it is a `#` that starts a line of the chunk, which
    /// `QCC_PR_NewLine` reads as a directive (FTEF-14.46, FTEF-14.53).
    pub(crate) fn directive_at(&self) -> Option<usize> {
        let token = self.stream.tokens.get(self.stream.pos)?;
        let hash = token.span.start;
        (token.kind == TokenKind::Punct(qc_lexer::Punct::Hash)
            && fte_directive::starts_line(
                self.text.bytes(),
                self.stream.code_start(hash),
                hash,
                TextStart::Chunk,
            ))
        .then_some(hash)
    }

    /// The chunk's text as a directive reads it.
    pub(crate) fn bytes(&self) -> Bytes {
        Bytes::Chunk(Rc::clone(&self.text), self.at)
    }

    /// Reading of the chunk goes on at `resume`, after the directive at `hash`, whose text is
    /// read as no code.
    pub(crate) fn resume(&mut self, hash: usize, resume: Resume, dialect: qc_lexer::Dialect) {
        let end = match resume {
            Resume::At(end) => {
                self.stream.resume_at(&self.text.bytes, end, dialect);
                end
            }
            Resume::End => {
                self.stream.pos = self.stream.tokens.len();
                self.text.bytes.len()
            }
        };
        self.consumed.push(Span::new(hash, end));
    }

    /// Gives the chunk the name `name` from here on, as `#pragma file` does (FTEF-14.32).
    pub(crate) fn rename(&mut self, name: Vec<u8>) {
        self.file_name = Some(name);
    }

    /// The name `#pragma file` gave the chunk, if any.
    pub(crate) fn file_name(&self) -> Option<&[u8]> {
        self.file_name.as_deref()
    }

    /// Numbers the chunk's line starting at `from` as `line`, as `#pragma line` does
    /// (FTEF-14.43).
    pub(crate) fn renumber(&mut self, from: usize, line: i32) {
        self.line_base = Some((from, line));
    }

    /// FTEQCC's `pr_source_line` at the offset `at` of the chunk: 1 at its start and one more
    /// for each line break before `at`, or as the last `#pragma line` before it numbers it
    /// (FTEF-14.27, FTEF-14.43).
    fn line_at(&self, at: usize) -> i32 {
        let count = |at: usize| {
            let breaks = self.breaks.partition_point(|&b| b < at);
            u32::try_from(breaks).unwrap_or(u32::MAX)
        };
        // As in a file (`Preprocessor::fte_line`), the base is set as the chunk's reading passes
        // the pragma, and no offset before it is asked for after that.
        match self.line_base {
            Some((from, line)) => line.wrapping_add_unsigned(count(at).saturating_sub(count(from))),
            None => 1_i32.wrapping_add_unsigned(count(at)),
        }
    }
}

/// The spelling of a token of the kind `kind` whose text `bytes` an expansion made (QC-9.16):
/// a string's value as a literal that reads back as it ([`quote_bytes`]), or the text.
fn made_spelling(kind: &TokenKind, bytes: &[u8]) -> String {
    match kind {
        TokenKind::String(value) => quote_bytes(value),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// The body of a macro that has no stored text: under FTEQCC every macro has one.
static NO_TEXT: Text = Text {
    bytes: Vec::new(),
    runs: Vec::new(),
};

/// The size of the buffer `QCC_PR_CheckBuiltinCompConst` writes a built-in constant's text in,
/// `char retbuf[256]`, of which `QC_snprintfz` fills at most 255 bytes (FTEF-14.32).
const FTE_BUILTIN_TEXT: usize = 256;

/// The text `QCC_PR_CheckBuiltinCompConst` writes for the built-in constant `name`, which FTEQCC
/// then reads as a chunk of its own (FTEF-14.7, FTEF-14.32), at the line `line` (FTEF-14.27) in
/// the file named `file`; `None` when `name` is none. `__FILE__` is `"%s"` of the name, kept to
/// 255 bytes and ended at its first `:` (`erk = strchr(retbuf, ':'); if (erk) { erk[0] = '\"';
/// erk[1] = 0; }`), nothing in it escaped.
pub(crate) fn builtin_text(name: &[u8], line: i32, file: &[u8]) -> Option<Vec<u8>> {
    Some(match name {
        b"__NULL__" => b"0i".to_vec(),
        b"__FILE__" => {
            let mut text = Vec::with_capacity(file.len().saturating_add(2));
            text.push(b'"');
            text.extend_from_slice(file);
            text.push(b'"');
            text.truncate(FTE_BUILTIN_TEXT.saturating_sub(1));
            if let Some(colon) = text.iter().position(|&b| b == b':') {
                text.truncate(colon);
                text.push(b'"');
            }
            text
        }
        b"__LINE__" => line.to_string().into_bytes(),
        b"__LINESTR__" => format!("\"{line}\"").into_bytes(),
        b"__FUNC__" | b"__func__" | b"__TIME__" | b"__DATE__" | b"__QCCVER__" => b"\"\"".to_vec(),
        b"__RAND__" => b"0".to_vec(),
        _ => return None,
    })
}

/// Where the skip over a string that `QCC_PR_ExpandPreProcessorMacro` makes from the `"` at `at`
/// stops: `do { pr_file_p++; } while( (pr_file_p[-1] == '\\' || pr_file_p[0] != '\"') &&
/// *pr_file_p && *pr_file_p != '\n' );`, at the next `"` with no `\` right before it (a `\\`
/// before it included), at a line break, or at the end of the text (FTEF-14.22).
fn string_end(text: &[u8], mut at: usize) -> usize {
    loop {
        let before = text.get(at).copied();
        at = at.saturating_add(1);
        match byte_at(text, at) {
            None | Some(b'\n') => return at,
            Some(b'"') if before != Some(b'\\') => return at,
            Some(_) => {}
        }
    }
}

/// The end of the string at `at` as `QCC_PR_ExpandPreProcessorMacro` copies it, its closing
/// `"` included when [`string_end`] stopped at one.
fn copied_string_end(text: &[u8], at: usize) -> usize {
    let end = string_end(text, at);
    if byte_at(text, end) == Some(b'"') {
        end.saturating_add(1)
    } else {
        end.min(text.len())
    }
}

/// A token as `QCC_COM_Parse2` reads it.
struct Word {
    /// `qcc_token`: the token's bytes, a string's with its escapes read and no quotes.
    token: Vec<u8>,
    /// Where the token starts.
    start: usize,
    /// Where reading goes on.
    next: usize,
    /// Whether `token` is the text's bytes from `start`: every token but a string.
    verbatim: bool,
}

/// `QCC_COM_Parse2`: after white space and `//` comments from `at`, a string (its escapes
/// `\"`, `\\` and `\n` read, reading going on one byte past its closing `"`), a number (digits
/// and `.`, or `0x` and hex digits and `.`), a word (a letter or `_`, then letters, digits and
/// `_`), or one byte; each token kept to its first 1023 bytes (FTEF-14.25). `None` at the end of
/// the text.
fn parse2(text: &[u8], mut at: usize) -> Option<Word> {
    loop {
        while byte_at(text, at).is_some_and(cond::fte_white) {
            at = at.saturating_add(1);
        }
        let byte = byte_at(text, at)?;
        if byte != b'/' || text.get(at.saturating_add(1)) != Some(&b'/') {
            break;
        }
        while byte_at(text, at).is_some_and(|b| b != b'\n') {
            at = at.saturating_add(1);
        }
    }
    let start = at;
    let first = byte_at(text, at)?;
    let mut token = Vec::new();
    let keep = |token: &mut Vec<u8>, byte: u8| {
        if token.len() < FTE_MAX_TOKEN {
            token.push(byte);
        }
    };
    let continues: fn(u8) -> bool = if first == b'"' {
        at = at.saturating_add(1);
        loop {
            let byte = byte_at(text, at).unwrap_or(0);
            at = at.saturating_add(1);
            let byte = match (byte, text.get(at).copied()) {
                (b'\\', Some(escaped @ (b'"' | b'\\'))) => {
                    at = at.saturating_add(1);
                    escaped
                }
                (b'\\', Some(b'n')) => {
                    at = at.saturating_add(1);
                    b'\n'
                }
                (b'"' | 0, _) => break,
                (byte, _) => byte,
            };
            keep(&mut token, byte);
        }
        return Some(Word {
            token,
            start,
            next: at.saturating_add(1),
            verbatim: false,
        });
    } else if first == b'0' && text.get(at.saturating_add(1)) == Some(&b'x') {
        keep(&mut token, b'0');
        at = at.saturating_add(1);
        |b: u8| b.is_ascii_hexdigit() || b == b'.'
    } else if first.is_ascii_digit() {
        |b: u8| b.is_ascii_digit() || b == b'.'
    } else if first.is_ascii_alphabetic() || first == b'_' {
        |b: u8| b.is_ascii_alphanumeric() || b == b'_'
    } else {
        token.push(first);
        return Some(Word {
            token,
            start,
            next: at.saturating_add(1),
            verbatim: true,
        });
    };
    // The byte at `at` is the token's first, or the `x` of `0x`.
    let mut byte = byte_at(text, at).unwrap_or(first);
    loop {
        keep(&mut token, byte);
        at = at.saturating_add(1);
        match byte_at(text, at).filter(|&next| continues(next)) {
            Some(next) => byte = next,
            None => break,
        }
    }
    Some(Word {
        token,
        start,
        next: at,
        verbatim: true,
    })
}

/// The offset after the `*/` that ends a `/* */` comment whose text starts at `from`, or the
/// end of the text when none does.
fn block_comment_end(text: &[u8], from: usize) -> usize {
    text.get(from..)
        .and_then(|rest| rest.windows(2).position(|pair| pair == b"*/"))
        .map_or(text.len(), |at| from.saturating_add(at).saturating_add(2))
}

/// A `#define` or `#append` as `QCC_PR_PreProcessor_Define` reads it.
pub(crate) struct Definition {
    /// The macro's name.
    pub(crate) name: Range<usize>,
    /// The parameters of a function-like macro, as written.
    pub(crate) params: Option<Vec<Vec<u8>>>,
    /// Whether the last parameter is `...`.
    pub(crate) variadic: Variadic,
    /// The body as stored.
    pub(crate) value: Text,
    /// The first `#` that starts a continued line of the body, "preprocessor directive within
    /// preprocessor macro" (FTEF-14.46).
    evil: Option<usize>,
}

/// Why a `#define` defines nothing.
pub(crate) enum DefineError {
    /// "No name defined for compiler constant".
    NoName,
    /// A name of 64 bytes or more, "Compiler constant name length is too long or short"
    /// (FTEF-14.42).
    NameTooLong,
    /// A parameter list FTEQCC refuses (FTEF-14.23), of the macro named by these bytes.
    BadParams(Range<usize>),
}

/// The parameter list of a `#define` whose `(` is at `open`, as `QCC_PR_PreProcessor_Define`
/// reads it (FTEF-14.23): each parameter runs from after the spaces and tabs that follow the
/// `(` or a `,` to the next `,` or `)`, its trailing spaces and tabs dropped; the first byte of
/// every parameter after the first is passed over before a delimiter is looked for. The
/// parameters, whether the last is `...`, and the offset after the `)`; `Err` with where reading
/// stopped for a list FTEQCC refuses.
fn read_params(text: &[u8], open: usize) -> Result<(Vec<Vec<u8>>, Variadic, usize), usize> {
    let same_line = |b: u8| b == b' ' || b == b'\t';
    let skip = |mut at: usize| {
        while byte_at(text, at).is_some_and(same_line) {
            at = at.saturating_add(1);
        }
        at
    };
    let mut params: Vec<Vec<u8>> = Vec::new();
    let mut variadic = Variadic::No;
    let mut at = skip(open.saturating_add(1));
    let mut start = at;
    loop {
        let byte = byte_at(text, at);
        if let Some(delimiter @ (b',' | b')')) = byte {
            let mut param = text.get(start..at).unwrap_or_default().to_vec();
            while param.last().copied().is_some_and(same_line) {
                param.pop();
            }
            // "May not have more than %i parameters to a macro", "parameter name is too long".
            if params.len() >= FTE_MAX_PARAMS || param.len() >= FTE_MAX_PARAM_LENGTH {
                return Err(at);
            }
            if param == b"..." {
                // "varadic argument must be last"
                if delimiter != b')' {
                    return Err(at);
                }
                variadic = Variadic::Yes;
            } else if params.contains(&param) {
                // "duplicate macro paramter name '%s'"
                return Err(at);
            } else {
                params.push(param);
            }
            at = at.saturating_add(1);
            if delimiter == b')' {
                return Ok((params, variadic, at));
            }
            at = skip(at);
            start = at;
        }
        // `if(!*pr_file_p++)`: "missing ) in macro parameter list".
        if byte_at(text, at).is_none() {
            return Err(at);
        }
        at = at.saturating_add(1);
    }
}

/// `QCC_PR_PreProcessor_Define`: the `#define` whose name starts after `at` in `reading`, and
/// the offset where it stopped reading, from which `QCC_PR_SkipToEndOfLine` reads to the end
/// of its line. The name is a word `QCC_PR_SimpleGetToken` reads, of at most 63 bytes
/// (`QCC_PR_DefineName`, FTEF-14.42). The body runs from after the spaces and tabs that follow
/// the name or the `)` to a line break, to the end of the text, or to a `//` outside a quote
/// (the quote flag flipping at every `"`, an escaped one included); each `/* */` comment outside
/// a quote and each `\` before a line break is dropped, the line break with it, and the spaces
/// and tabs at its end are dropped (FTEF-14.24).
pub(crate) fn read_define(
    reading: &Reading<'_>,
    at: usize,
) -> (Result<Definition, DefineError>, usize) {
    let text = reading.bytes();
    let (name, stop) = simple_get_token(text, at);
    let Some(name) = name else {
        return (Err(DefineError::NoName), stop);
    };
    if name.len() > FTE_MAX_MACRO_NAME {
        let end = name.end;
        return (Err(DefineError::NameTooLong), end);
    }
    let mut at = name.end;
    let mut params = None;
    let mut variadic = Variadic::No;
    if text.get(at) == Some(&b'(') {
        match read_params(text, at) {
            Ok((list, dots, after)) => {
                params = Some(list);
                variadic = dots;
                at = after;
            }
            Err(stop) => return (Err(DefineError::BadParams(name)), stop),
        }
    }
    let (value, end, evil) = read_body(reading, at);
    let definition = Definition {
        name,
        params,
        variadic,
        value,
        evil,
    };
    (Ok(definition), end)
}

/// The body of a `#define` from `at`, as `QCC_PR_PreProcessor_Define` stores it, where it
/// stopped reading, and the first `#` that starts a continued line (FTEF-14.24, FTEF-14.46).
/// After a `\` and a line break, a line that starts with `#` (after spaces and tabs) keeps the
/// line break, and so does the next continued line: "#define hacks however, require that it
/// becomes foo\nbar\nmoo".
fn read_body(reading: &Reading<'_>, mut at: usize) -> (Text, usize, Option<usize>) {
    let text = reading.bytes();
    while matches!(text.get(at), Some(b' ' | b'\t')) {
        at = at.saturating_add(1);
    }
    let mut value = Text::default();
    let mut quote = false;
    let mut directive_lines = false;
    let mut evil = None;
    loop {
        let next = text.get(at.saturating_add(1)).copied();
        match byte_at(text, at) {
            Some(b'\\') if matches!(next, Some(b'\n' | b'\r')) => {
                at = at.saturating_add(1);
                if text.get(at..at.saturating_add(2)) == Some(b"\r\n") {
                    at = at.saturating_add(1);
                }
                let line_break = at;
                at = at.saturating_add(1);
                let mut first = at;
                while matches!(text.get(first), Some(b' ' | b'\t')) {
                    first = first.saturating_add(1);
                }
                let kept = if text.get(first) == Some(&b'#') {
                    directive_lines = true;
                    evil.get_or_insert(first);
                    true
                } else {
                    std::mem::take(&mut directive_lines)
                };
                if kept {
                    value.push(b"\n", reading.origin(line_break));
                }
            }
            None | Some(b'\r' | b'\n') => break,
            Some(_) => {}
        }
        // The byte after a continued line is read without looking for the body's end first.
        let Some(byte) = byte_at(text, at) else {
            break;
        };
        let rest = text.get(at..).unwrap_or_default();
        if !quote && rest.starts_with(b"//") {
            break;
        }
        if !quote && rest.starts_with(b"/*") {
            at = block_comment_end(text, at.saturating_add(2));
            continue;
        }
        if byte == b'"' {
            quote = !quote;
        }
        value.push(&[byte], reading.origin(at));
        at = at.saturating_add(1);
    }
    value.trim_end();
    (value, at, evil)
}

/// A use's arguments as `QCC_PR_ExpandPreProcessorMacro` finds them in the text after its `(`
/// (FTEF-14.22).
struct Scanned {
    /// Each argument's bytes, `paramoffset[p]`: from after the white space and comments that
    /// follow its `(` or `,` to its `,` or `)`.
    args: Vec<Range<usize>>,
    /// `extraparam`: the commas of a variadic macro's arguments after its named ones.
    extra: usize,
    /// Whether a comma came after more arguments than FTEQCC takes, "Too many parameters in
    /// macro call" (FTEF-14.9).
    too_many: bool,
    /// The offset of the `)`.
    close: usize,
}

/// A function-like macro's use, read from the text its name stands in.
struct Call {
    /// Each argument's text; the one after the named ones is the variadic argument.
    args: Vec<Text>,
    /// `extraparam` ([`Scanned::extra`]).
    extra: usize,
    /// [`Scanned::too_many`].
    too_many: bool,
    /// The offset after the `)` in the text the name stands in.
    after: usize,
}

/// Where a use's name is read and where reading goes on after it, with the lines FTEQCC gives
/// a built-in constant there (FTEF-14.27).
struct Use {
    call: Option<Call>,
    /// The use, from its name to its `)`.
    span: FileSpan,
    /// The line a built-in constant an argument copies has: the chunk the use stands in's own,
    /// or for a use in a file the line its `)` is on.
    argument_line: i32,
    /// The line of the expansion's chunk: where reading was when the use ended.
    chunk_line: i32,
    /// Where reading goes on if the use is refused.
    refused: Refused,
}

/// Why a use of a function-like macro is not expanded.
enum CallError {
    /// No `(` follows the name after spaces and tabs (FTEF-14.29).
    NoCall,
    /// The text ends before the `)`, "EOF on macro call" (FTEF-14.29); `refused` is where
    /// reading goes on.
    Unterminated { refused: Refused },
}

/// Where reading goes on after a use of a function-like macro that is refused, an error where
/// FTEQCC's compile stops (FTEF-14.45).
#[derive(Clone, Copy)]
enum Refused {
    /// After the name: the use is read again as it is written.
    AsWritten,
    /// At this offset, the end of the text its arguments were scanned in, as directives ran
    /// there, and none may run twice.
    After(usize),
}

/// The text of an expansion as `QCC_PR_ExpandPreProcessorMacro` writes it.
struct Writer<'a> {
    body: &'a Text,
    params: &'a [Vec<u8>],
    variadic: Variadic,
    call: &'a Call,
    /// The value of a built-in constant by its name, `QCC_PR_CheckBuiltinCompConst`.
    builtin: &'a dyn Fn(&[u8]) -> Option<Vec<u8>>,
    /// The most bytes the expansion may write (FTEF-14.28).
    limit: usize,
    /// The use, where text that stands nowhere else stands.
    at: FileSpan,
}

impl Writer<'_> {
    /// The argument of the named parameter `token`, `!STRCMP(qcc_token, c->params[p])`.
    fn argument_of(&self, token: &[u8]) -> Option<&Text> {
        let p = self.params.iter().position(|p| p.as_slice() == token)?;
        self.call.args.get(p)
    }

    /// Where the bytes `range` of `text`, a body or an argument, stand ([`Text::place`]).
    fn place(&self, text: &Text, range: Range<usize>) -> FileSpan {
        text.place(range).map_or(self.at, |(span, _)| span)
    }

    /// The expansion's text (FTEF-14.14 to FTEF-14.16, FTEF-14.20, FTEF-14.26), or `None` past
    /// the limit. For each piece of the body: its white space and comments are copied; a string
    /// is copied as [`string_end`] reads it; `##` drops the white space just copied and copies
    /// the next argument whole (`noargexpand`); `#` and a named parameter write the argument's
    /// text as a string, and `#` and any other token `#` and that token; a named parameter is
    /// its argument, whole before a `##` and otherwise a token at a time ([`Self::argument`]);
    /// `__VA_ARGS__` is the variadic argument, or with none after `##` takes away a `,` written
    /// right before; `__VA_COUNT__` is FTEQCC's count; every other token is copied.
    fn write(&self) -> Option<Text> {
        let bytes = self.body.bytes();
        let named = self.params.len();
        let mut out = Text::default();
        let mut at = 0_usize;
        loop {
            let whitestart = out.bytes.len();
            // `QCC_PR_LexWhitespace(true)` runs no directive, so it stops at a line's `#` too.
            let (Gap::Text(white_end) | Gap::LineStart(white_end)) = whitespace(bytes, at);
            out.push_from(self.body, at..white_end);
            at = white_end;
            // Everything written so far, the pieces before this one included.
            if out.bytes.len() > self.limit {
                return None;
            }
            let mut noargexpand = false;
            match byte_at(bytes, at) {
                Some(b'"') => {
                    let end = copied_string_end(bytes, at);
                    out.push_from(self.body, at..end);
                    at = end;
                    continue;
                }
                Some(b'#') if byte_at(bytes, at.saturating_add(1)) == Some(b'#') => {
                    out.truncate(whitestart);
                    at = at.saturating_add(2);
                    noargexpand = true;
                }
                Some(b'#') => {
                    let hash = at;
                    let Some(word) = parse2(bytes, at.saturating_add(1)) else {
                        break;
                    };
                    at = word.next;
                    match self.argument_of(&word.token) {
                        Some(arg) => {
                            let span = self.place(self.body, hash..word.next);
                            out.push(&markup(arg.bytes()), Origin::Made(span));
                        }
                        None => {
                            out.push_from(self.body, hash..hash.saturating_add(1));
                            self.copy_token(&word, &mut out);
                        }
                    }
                    continue;
                }
                _ => {}
            }
            let Some(word) = parse2(bytes, at) else {
                break;
            };
            at = word.next;
            if let Some(arg) = self.argument_of(&word.token) {
                let mut after = at;
                while byte_at(bytes, after).is_some_and(cond::fte_white) {
                    after = after.saturating_add(1);
                }
                let before_paste = bytes.get(after..after.saturating_add(2)) == Some(b"##");
                if noargexpand || before_paste {
                    out.push_from(arg, 0..arg.bytes.len());
                } else {
                    self.argument(arg, &mut out);
                }
            } else if self.variadic == Variadic::Yes && word.token == b"__VA_ARGS__" {
                // `if (param-1 == c->numparams)`: a variadic argument was given.
                if let Some(arg) = self
                    .call
                    .args
                    .get(named)
                    .filter(|_| self.call.args.len() == named.saturating_add(1))
                {
                    out.push_from(arg, 0..arg.bytes.len());
                } else if noargexpand && out.bytes.last() == Some(&b',') {
                    out.truncate(out.bytes.len().saturating_sub(1));
                }
            } else if self.variadic == Variadic::Yes && word.token == b"__VA_COUNT__" {
                // `QC_snprintfz(tmp, sizeof(tmp), "%i", param-1+extraparam);`
                let count = self
                    .call
                    .args
                    .len()
                    .saturating_sub(1)
                    .saturating_add(self.call.extra);
                let span = self.place(self.body, word.start..word.next);
                out.push(count.to_string().as_bytes(), Origin::Made(span));
            } else {
                self.copy_token(&word, &mut out);
            }
        }
        Some(out)
    }

    /// Writes `qcc_token` of `word`, read from the body: its bytes there, or for a string's
    /// token the text `QCC_COM_Parse2` made of it.
    fn copy_token(&self, word: &Word, out: &mut Text) {
        if word.verbatim {
            let end = word.start.saturating_add(word.token.len());
            out.push_from(self.body, word.start..end);
        } else {
            let span = self.place(self.body, word.start..word.next);
            out.push(&word.token, Origin::Made(span));
        }
    }

    /// Writes the argument `arg` a token at a time, as `QCC_PR_ExpandPreProcessorMacro` does
    /// for one not beside `##` (FTEF-14.20, FTEF-14.26): each token with the white space and
    /// `//` comments before it, a string as [`string_end`] reads it, a built-in constant's
    /// value in place of it and what stands before it, and nothing after the last token.
    fn argument(&self, arg: &Text, out: &mut Text) {
        let bytes = arg.bytes();
        let mut start = 0_usize;
        loop {
            let mut end = start;
            while byte_at(bytes, end).is_some_and(cond::fte_white) {
                end = end.saturating_add(1);
            }
            let value = if byte_at(bytes, end) == Some(b'"') {
                end = copied_string_end(bytes, end);
                None
            } else {
                let Some(word) = parse2(bytes, end) else {
                    break;
                };
                end = word.next.min(bytes.len());
                (self.builtin)(&word.token)
            };
            match value {
                Some(value) => out.push(&value, Origin::Made(self.place(arg, start..end))),
                None => out.push_from(arg, start..end),
            }
            start = end;
        }
    }
}

/// `QCC_PR_ExpandStrCatMarkup`: `bytes` with `\` before each line break, `\` and `"` (a NUL ends
/// an argument's text, so none is in it), and
/// in `"` quotes, the string `#param` writes (FTEF-14.31).
fn markup(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len().saturating_add(2));
    out.push(b'"');
    for &byte in bytes {
        match byte {
            b'\n' => out.extend_from_slice(b"\\\n"),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"\\\""),
            _ => out.push(byte),
        }
    }
    out.push(b'"');
    out
}

impl<L: Loader> Preprocessor<L> {
    /// `#define` and `#append` under FTEQCC (FTEF-14.23, FTEF-14.24, FTEF-14.39, FTEF-14.42), as
    /// `QCC_PR_PreProcessor_Define` reads them in `bytes` from after `word_end`, the end of the
    /// word the directive's `#` at `hash` starts: the macro is stored with its body's text, and
    /// the offset where reading stopped is returned, from which the directive's line is read to
    /// its end, as `Err` when FTEQCC refuses the definition and stops; the text is never lexed
    /// as code. An `#append` whose parameters differ from the macro's defines it again with the
    /// warning W0102. A body with a continued line that starts with `#` is reported as
    /// `#pragma warning` sets F303 (FTEF-14.46).
    pub(crate) fn fte_define(
        &mut self,
        run: &mut Run,
        bytes: &Bytes,
        (hash, word_end): (usize, usize),
        definer: Definer,
    ) -> Result<usize, usize> {
        let reading = bytes.reading();
        let text = reading.bytes();
        let (read, end) = read_define(&reading, word_end);
        let whole = reading.place(hash..end);
        let definition = match read {
            Ok(definition) => definition,
            Err(DefineError::NoName) => {
                run.report(Code::E0114, "#define expects a macro name", whole);
                return Err(end);
            }
            Err(DefineError::NameTooLong) => {
                let message = "Compiler constant name length is too long or short";
                run.report(Code::E0121, message, whole);
                return Err(end);
            }
            Err(DefineError::BadParams(name)) => {
                let name = String::from_utf8_lossy(text.get(name).unwrap_or_default());
                let message = format!("bad parameter list for macro {name}");
                run.report(Code::E0115, message, whole);
                return Err(end);
            }
        };
        let name_bytes = text.get(definition.name.clone()).unwrap_or_default();
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        let at_name = reading.place(definition.name.clone());
        if let Some(evil) = definition.evil {
            // `QCC_PR_ParseWarning(WARN_EVILPREPROCESSOR, "preprocessor directive within
            // preprocessor macro %s", cnst->name);`, an error unless a pragma says otherwise.
            let message = format!("preprocessor directive within preprocessor macro {name}");
            let at = reading.place(evil..evil.saturating_add(1));
            match self.fte.evil {
                WarningAction::Ignore => {}
                WarningAction::Warn => run.report(Code::W0109, message, at),
                WarningAction::Error => run.report(Code::E0122, message, at),
            }
        }
        let params = definition.params;
        let variadic = definition.variadic;
        let mut value = definition.value;
        let old = self.macros.get(&name).cloned();
        let same_params = old
            .as_ref()
            .is_some_and(|old| old.params == params && old.variadic == variadic);
        match old.as_deref().and_then(Macro::text) {
            // "append with no previous define is treated as just a regular define".
            Some(old_value) if definer == Definer::Append && same_params => {
                let mut joined = Text::clone(old_value);
                joined.push(b" ", Origin::Made(at_name));
                joined.push_from(&value, 0..value.bytes.len());
                joined.trim_end();
                value = joined;
            }
            Some(old_value) if !same_params || old_value.bytes() != value.bytes() => {
                run.report(Code::W0102, format!("macro {name} redefined"), at_name);
            }
            Some(_) | None => {}
        }
        let mac = Macro {
            params,
            variadic,
            body: Body::Text(Rc::new(value)),
            file: at_name.file,
            name_span: at_name.span,
        };
        self.macros.insert(name, Rc::new(mac));
        Ok(end)
    }

    /// Expands `item` under FTEQCC if it is the name of a macro `QCC_PR_CheckCompConst`
    /// expands there, or of a built-in constant (FTEF-14.7); returns the item when it is not
    /// replaced. The name's word must end with it (FTEF-14.13), the macro must not be stopped
    /// by FTEQCC's recursion rule (FTEF-14.17), and a function-like macro needs a `(` after
    /// spaces and tabs in the text it stands in, whose arguments are found there
    /// ([`scan_arguments`]); the expansion's text is written ([`Writer::write`]) and read as a
    /// chunk of its own, and reading the text the use stood in goes on after its `)`
    /// (FTEF-14.22).
    pub(crate) fn fte_try_expand(&mut self, run: &mut Run, item: Item) -> Option<Item> {
        let TokenKind::Name(name) = &item.tok.kind else {
            return Some(item);
        };
        if !self.word_ends_after(run, &item) {
            return Some(item);
        }
        let Some(mac) = self.macros.get(name).cloned() else {
            let replaced = self.fte_builtin(run, &item, name);
            return (!replaced).then_some(item);
        };
        if self.fte_recursion_stops(run, &item, (name, &mac)) {
            return Some(item);
        }
        let name = name.clone();
        let stored = mac.text();
        let body: &Text = stored.map_or(&NO_TEXT, |text| text);
        let use_ = match self.read_use(run, &item, &mac) {
            Ok(use_) => use_,
            Err(CallError::NoCall) => return Some(item),
            Err(CallError::Unterminated { refused }) => {
                let message = format!("unterminated argument list for macro {name}");
                run.report(Code::E0117, message, item.tok.span);
                return self.refuse(run, item, refused);
            }
        };
        let named = mac.params.as_ref().map_or(0, Vec::len);
        if let Some(call) = &use_.call
            && (call.too_many || call.args.len() < named)
        {
            let found = call.args.len();
            let message = format!("macro {name} expects {named} arguments, found {found}");
            run.report(Code::E0116, message, use_.span);
            return self.refuse(run, item, use_.refused);
        }
        if run.expanded.over_bound() {
            run.expanded.report(use_.span, &mut run.out.diagnostics);
            return self.refuse(run, item, use_.refused);
        }
        let limit = FTE_MAX_EXPANDED_TOKENS.saturating_sub(run.expanded.tokens);
        let written = match (&use_.call, &mac.params) {
            (Some(call), Some(params)) => {
                let file = Self::fte_file_name(run).to_vec();
                let line = use_.argument_line;
                let builtin = move |word: &[u8]| builtin_text(word, line, &file);
                let writer = Writer {
                    body,
                    params,
                    variadic: mac.variadic,
                    call,
                    builtin: &builtin,
                    limit,
                    at: use_.span,
                };
                writer.write().map(Rc::new)
            }
            // An object-like macro's chunk is its stored text, shared, not copied.
            _ => (body.bytes.len() <= limit).then(|| stored.map_or_else(Rc::default, Rc::clone)),
        };
        let Some(text) = written else {
            run.expanded.tokens = FTE_MAX_EXPANDED_TOKENS;
            run.expanded.report(use_.span, &mut run.out.diagnostics);
            return self.refuse(run, item, use_.refused);
        };
        run.expanded.expansions = run.expanded.expansions.saturating_add(1);
        if let Some(call) = &use_.call {
            self.resume_after(run, &item, call.after);
        }
        let id = ExpansionId(self.expansions.len());
        self.expansions.push(Expansion {
            name: name.clone(),
            use_span: use_.span,
            definition: FileSpan {
                file: mac.file,
                span: mac.name_span,
            },
            parent: item.tok.expansion,
        });
        // `if (*c->value) QCC_PR_IncludeChunkEx(...)`: an empty expansion includes nothing.
        if text.bytes.is_empty() {
            return None;
        }
        let stream = TokenStream::lex(text.bytes(), self.dialect);
        let breaks = (0..text.bytes.len())
            .filter(|&at| text.bytes.get(at) == Some(&b'\n'))
            .collect();
        run.expanded.tokens = run.expanded.tokens.saturating_add(stream.tokens.len());
        let open = run.open_names.entry(name.clone()).or_default();
        *open = open.saturating_add(1);
        run.chunks.push(ChunkReader {
            text,
            line: use_.chunk_line,
            file_name: None,
            line_base: None,
            id,
            at: use_.span,
            name,
            stream,
            consumed: Vec::new(),
            breaks,
            detached: false,
        });
        None
    }

    /// The built-in constant the name `item` is, when no macro has its name (FTEF-14.7): the text
    /// `QCC_PR_CheckBuiltinCompConst` writes for it ([`builtin_text`]), read as a chunk of its
    /// own (`QCC_PR_IncludeChunkEx(tok, true, NULL, NULL);`) before the text after the name, so
    /// its tokens touch nothing beside them (FTEF-14.17), and its lexical errors reported at the
    /// name (FTEF-14.32). Whether `name`, the name of `item`, is one, and was replaced.
    fn fte_builtin(&self, run: &mut Run, item: &Item, name: &str) -> bool {
        // `if (constname[0] != '_' || constname[1] != '_') return NULL;`: most names are none,
        // and are known before their line is looked for.
        if !name.starts_with("__") {
            return false;
        }
        let line = match item.place {
            Place::Chunk(_) => run.chunks.last().map_or(0, |chunk| chunk.line),
            Place::File | Place::Builtin => {
                self.fte_line(run, item.tok.span.file, item.tok.span.span.start)
            }
        };
        let Some(text) = builtin_text(name.as_bytes(), line, Self::fte_file_name(run)) else {
            return false;
        };
        if let Some(chunk) = run
            .chunks
            .last_mut()
            .filter(|_| matches!(item.place, Place::Chunk(_)))
        {
            chunk.detach();
        }
        let lexed = lex_with(&text, self.dialect);
        for d in lexed.diagnostics {
            run.report(d.code, d.message, item.tok.span);
        }
        let mut items = Vec::with_capacity(lexed.tokens.len());
        let mut end = None;
        for token in lexed.tokens {
            let bytes = text.get(token.span.range()).unwrap_or_default();
            let follows = (item.tok.expansion.is_some() && end == Some(token.span.start))
                .then_some(item.tok.span);
            end = Some(token.span.end);
            items.push(Item {
                tok: PpToken {
                    spelling: Some(made_spelling(&token.kind, bytes)),
                    kind: token.kind,
                    span: item.tok.span,
                    expansion: item.tok.expansion,
                    follows,
                },
                hide: item.hide.clone(),
                place: Place::Builtin,
            });
        }
        run.pending.extend(items.into_iter().rev());
        true
    }

    /// The text the name `item` stands in and the offset after the name: the innermost chunk
    /// for a name read from a chunk, which is the chunk being read, and otherwise the file the
    /// name is in.
    fn use_text(&self, run: &Run, item: &Item) -> Option<(Bytes, usize)> {
        match item.place {
            Place::Chunk(end) => Some((run.chunks.last()?.bytes(), end)),
            Place::File => {
                let file = item.tok.span.file;
                let text = Rc::clone(&self.sources.file(file)?.text);
                Some((Bytes::File(file, text), item.tok.span.span.end))
            }
            Place::Builtin => None,
        }
    }

    /// The use of the macro `mac` whose name is `item`: for a function-like macro its `(`,
    /// found after spaces and tabs (`while(*pr_file_p == ' ' || *pr_file_p == '\t')`), and its
    /// arguments ([`Self::fte_scan_arguments`]), with the lines FTEQCC gives a built-in constant
    /// (FTEF-14.27).
    fn read_use(&mut self, run: &mut Run, item: &Item, mac: &Macro) -> Result<Use, CallError> {
        let (bytes, name_end) = self.use_text(run, item).ok_or(CallError::NoCall)?;
        let text = bytes.bytes();
        let mut call = None;
        let mut end = name_end;
        let mut refused = Refused::AsWritten;
        if let Some(params) = &mac.params {
            let mut open = name_end;
            while matches!(text.get(open), Some(b' ' | b'\t')) {
                open = open.saturating_add(1);
            }
            if text.get(open) != Some(&b'(') {
                return Err(CallError::NoCall);
            }
            let before = run.directives_run;
            let scanned = self.fte_scan_arguments(run, &bytes, open, (params.len(), mac.variadic));
            let ran = run.directives_run != before;
            let Some(scanned) = scanned else {
                let refused = if ran {
                    Refused::After(text.len())
                } else {
                    Refused::AsWritten
                };
                return Err(CallError::Unterminated { refused });
            };
            end = scanned.close.saturating_add(1);
            if ran {
                refused = Refused::After(end);
            }
            let reading = bytes.reading();
            call = Some(Call {
                args: scanned
                    .args
                    .iter()
                    .map(|range| reading.copy(range.clone()))
                    .collect(),
                extra: scanned.extra,
                too_many: scanned.too_many,
                after: end,
            });
        }
        let reading = bytes.reading();
        let mut span = item.tok.span;
        if let Some(close) = end
            .checked_sub(1)
            .map(|at| reading.place(at..at.saturating_add(1)))
            && close.file == span.file
            && close.span.end >= span.span.start
        {
            span.span.end = close.span.end;
        }
        let (argument_line, chunk_line) = match &bytes {
            Bytes::File(file, _) => {
                let close = end.saturating_sub(1).max(item.tok.span.span.start);
                let line = self.fte_line(run, *file, close);
                (line, line)
            }
            Bytes::Chunk(..) => run
                .chunks
                .last()
                .map_or((0, 1), |chunk| (chunk.line, chunk.line_at(end))),
        };
        Ok(Use {
            call,
            span,
            argument_line,
            chunk_line,
            refused,
        })
    }

    /// The arguments of a use of a macro with `named` named parameters, `variadic` or not, whose
    /// `(` is at `open` in `bytes` (FTEF-14.22): a `"` string is passed over as [`string_end`]
    /// passes over it, `(` and `)` nest, and a `,` or `)` outside them ends an argument, every
    /// other byte (a character constant's, a comment's) being part of it. The white space after
    /// the `(` and each `,` is read as `QCC_PR_LexWhitespace(false)` reads it, which runs the
    /// directives at the starts of its lines (FTEF-14.45). `None` when the text ends first, "EOF
    /// on macro call", or a directive ends it.
    fn fte_scan_arguments(
        &mut self,
        run: &mut Run,
        bytes: &Bytes,
        open: usize,
        (named, variadic): (usize, Variadic),
    ) -> Option<Scanned> {
        let text = bytes.bytes();
        let mut at = self.fte_whitespace(run, bytes, open.saturating_add(1))?;
        let mut start = at;
        let mut args = Vec::new();
        let mut extra = 0_usize;
        let mut too_many = false;
        let mut level = 0_usize;
        loop {
            let mut byte = byte_at(text, at)?;
            if byte == b'"' {
                at = string_end(text, at);
                byte = byte_at(text, at)?;
            }
            match byte {
                b'(' => level = level.saturating_add(1),
                b',' | b')' if level == 0 => {
                    if byte == b',' && variadic == Variadic::Yes && args.len() >= named {
                        extra = extra.saturating_add(1);
                    } else {
                        args.push(start..at);
                        if byte == b')' {
                            return Some(Scanned {
                                args,
                                extra,
                                too_many,
                                close: at,
                            });
                        }
                        at = self.fte_whitespace(run, bytes, at.saturating_add(1))?;
                        start = at;
                        too_many |= args.len() == FTE_MAX_PARAMS || args.len() > named;
                        continue;
                    }
                }
                b')' => level = level.saturating_sub(1),
                _ => {}
            }
            at = at.saturating_add(1);
        }
    }

    /// The name `item` of a refused use, read as itself, reading going on where `refused` says.
    fn refuse(&self, run: &mut Run, item: Item, refused: Refused) -> Option<Item> {
        if let Refused::After(at) = refused {
            self.resume_after(run, &item, at);
        }
        Some(item)
    }

    /// Reading of the text the name `item` stood in goes on at `after`, the byte after its use's
    /// `)`, where `QCC_PR_ExpandPreProcessorMacro` leaves `pr_file_p`; the lexical errors of
    /// the text its arguments took are not reported there (FTEF-14.22).
    fn resume_after(&self, run: &mut Run, item: &Item, after: usize) {
        match item.place {
            Place::Chunk(end) => {
                if let Some(chunk) = run.chunks.last_mut() {
                    chunk
                        .stream
                        .resume_at(&chunk.text.bytes, after, self.dialect);
                    chunk.consumed.push(Span::new(end, after));
                }
            }
            Place::Builtin => {}
            Place::File => {
                let file = self.sources.file(item.tok.span.file);
                if let (Some(cursor), Some(file)) = (run.cursors.last_mut(), file) {
                    cursor.stream.resume_at(&file.text, after, self.dialect);
                    let consumed = Span::new(item.tok.span.span.end, after);
                    cursor.skipped.push(consumed);
                }
            }
        }
    }

    /// Closes the chunk `chunk`, whose last token has been read (`QCC_PR_UnInclude`, and its
    /// `c->inside--`), and reports its lexical errors outside the text a macro used in it took
    /// as arguments, each where its first byte came from.
    pub(crate) fn close_chunk(run: &mut Run, chunk: ChunkReader) {
        let open = run.open_names.entry(chunk.name.clone()).or_default();
        *open = open.saturating_sub(1);
        for d in &chunk.stream.diagnostics {
            let consumed = chunk
                .consumed
                .iter()
                .any(|s| s.start <= d.span.start && d.span.start < s.end);
            if !consumed {
                let (span, _) = chunk.place(d.span.range());
                run.report(d.code, d.message.clone(), span);
            }
        }
    }

    /// Whether the word `QCC_PR_CheckCompConst` reads at the name `item` ends with the name
    /// (FTEF-14.13): the byte after it in the text FTEQCC reads, a file's or a chunk's, is one
    /// [`ends_word`] names, or there is none.
    pub(crate) fn word_ends_after(&self, run: &Run, item: &Item) -> bool {
        let next = match item.place {
            Place::Chunk(end) => run
                .chunks
                .last()
                .and_then(|chunk| chunk.text.bytes.get(end).copied()),
            Place::Builtin => None,
            Place::File => self
                .sources
                .file(item.tok.span.file)
                .and_then(|file| file.text.get(item.tok.span.span.end).copied()),
        };
        next.is_none_or(ends_word)
    }

    /// Whether FTEQCC leaves the name `item` of the macro `name` as it is because the macro is
    /// already being expanded (FTEF-14.17): `QCC_PR_CheckCompConst` expands no macro in the
    /// chunk of its own expansion (`currentchunk->cnst != c`), no object-like macro with 8
    /// chunks of it open (`if (c->inside >= 8)`) and no function-like one with more than 8
    /// (`if (c->inside>8) return false;`).
    fn fte_recursion_stops(&self, run: &Run, item: &Item, (name, mac): (&str, &Macro)) -> bool {
        let own_chunk = matches!(item.place, Place::Chunk(_))
            && run.chunks.last().is_some_and(|chunk| chunk.name == name);
        let open = run.open_names.get(name).copied().unwrap_or(0);
        own_chunk
            || if mac.params.is_some() {
                open > FTE_MAX_OPEN_CHUNKS
            } else {
                open >= FTE_MAX_OPEN_CHUNKS
            }
    }
}

/// Whether `byte` ends the word `QCC_PR_CheckCompConst` reads as a macro's name
/// (FTEF-14.13): a NUL, white space (`qcc_iswhite`: space, CR, LF, tab and vertical tab) or
/// one of `)(+-*/|&=^~[]"{};:<>,.#`.
pub(crate) fn ends_word(byte: u8) -> bool {
    const STOPS: &[u8] = b")(+-*/|&=^~[]\"{};:<>,.#";
    byte == 0 || cond::fte_white(byte) || STOPS.contains(&byte)
}
