// SPDX-License-Identifier: GPL-3.0-or-later

//! The QuakeC preprocessor.
//!
//! Turns the source files of one `progs.src` into the token streams the parser reads: it runs
//! the directives of QC-9 (`#include`, `#define`, `#undef`, `#ifdef`, `#ifndef`, `#else`,
//! `#endif`), expands macros, and resolves frame macro references against each source file's
//! `$frame` list (QC-8). Every token produced by expansion records the macro use it came from
//! and where its text is in the definition (QC-9.7), so a diagnostic can point at either.
//!
//! id's `qcc` has no preprocessor, so for vanilla code (where no line starts with `#` and a
//! name, QC-9.1) the only change is frame macro resolution. The directive set is this
//! project's own definition; FTEQCC's pragmas come with the dialect profiles. In an FTEQCC
//! dialect the preprocessor reads FTEQCC's directives as FTEQCC's `QCC_PR_Precompiler` does,
//! as bytes, wherever FTEQCC meets one (`docs/spec/dialect-fteqcc.md`, FTEF-14.33 to FTEF-14.47,
//! `fte_directive`), and its macros as text (FTEF-14.13 to FTEF-14.32, `fte_macro`).

pub mod cond;
mod fte_directive;
mod fte_macro;
mod source;

#[cfg(test)]
mod tests;

pub use fte_directive::{FileStart, FteEvent, FteFileReader, ModuleState};
pub use source::{FileId, FileSpan, FsLoader, Loader, MemoryLoader, SourceFile, SourceMap};

use fte_directive::{Bytes, FteState, Position, Resume, TextStart};
use fte_macro::{ChunkReader, Text, ends_word};
use std::cell::Cell;
use std::collections::HashMap;
use std::io;
use std::ops::Range;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use qc_lexer::{Code, Dialect, FrameCommand, Punct, Span, Token, TokenKind, lex_with};

/// Most frames one source file may define.
///
/// `LIMIT_QCC_MAX_FRAMES` in `docs/spec/limits.md`, from `MAX_FRAMES` in `qcc/pr_lex.c`. More
/// is an error (QC-27.5).
pub const LIMIT_QCC_MAX_FRAMES: usize = 256;

/// Most frame macros an FTEQCC program may define, over all its files.
///
/// `MAX_FRAMES` in FTEQCC's `qcc_pr_lex.c`, `#define MAX_FRAMES 8192`: defining the last
/// one is "Too many frame macros defined" (FTEF-18.17).
pub const FTE_MAX_FRAMES: usize = 8192;

/// One macro expansion: which macro, where it was used, and the use it sits inside.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expansion {
    /// The macro's name.
    pub name: String,
    /// The use: the macro's name, through the closing `)` for a function-like macro.
    pub use_span: FileSpan,
    /// The macro's name in its `#define`.
    pub definition: FileSpan,
    /// The expansion the use itself came from, when the use was produced by another macro.
    pub parent: Option<ExpansionId>,
}

/// Index of an [`Expansion`] in [`Preprocessor::expansions`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExpansionId(pub usize);

/// A token after preprocessing.
#[derive(Clone, Debug, PartialEq)]
pub struct PpToken {
    /// What it is. A resolved frame macro reference is a [`TokenKind::Float`]; one that
    /// could not be resolved stays a [`TokenKind::FrameRef`], and its error is reported.
    pub kind: TokenKind,
    /// Where its text is: in the source for an ordinary token or a macro argument, in the
    /// `#define` for a token of a macro body (QC-9.7).
    pub span: FileSpan,
    /// The expansion that produced it, if any.
    pub expansion: Option<ExpansionId>,
    /// Its text when that is not the bytes at [`PpToken::span`]: a token `##` pasted, a string
    /// `#` made, or a built-in constant's value (FTEF-14.14, FTEF-14.31, FTEF-14.7).
    pub spelling: Option<String>,
    /// In an expansion, the span of the token of the same expansion that the expansion's text
    /// has right before this one with nothing between them, if any (QC-9.16). Always `None`
    /// for a token of no expansion, whose place in the source says what it touches
    /// ([`PpToken::touches`]).
    pub follows: Option<FileSpan>,
}

impl PpToken {
    /// Whether this token comes right after `last`, with nothing between them, in the text the
    /// compiler reads (QC-9.16): two tokens of no expansion that touch in the source, or two
    /// of one expansion, this one [`PpToken::follows`] `last`. A token of an expansion never
    /// touches one of another expansion or of none, nor one that came to stand before it
    /// because the tokens between them expanded to nothing.
    #[must_use]
    pub fn touches(&self, last: &Self) -> bool {
        match self.expansion {
            None => {
                last.expansion.is_none()
                    && last.span.file == self.span.file
                    && last.span.span.end == self.span.span.start
            }
            Some(_) => last.expansion == self.expansion && self.follows == Some(last.span),
        }
    }
}

/// A preprocessor or lexical error or warning.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Its code (QCD-1.1).
    pub code: Code,
    /// The message, with the names filled in.
    pub message: String,
    /// The primary span (QCD-1.3).
    pub span: FileSpan,
}

/// The tokens of one source file, its includes expanded, and the diagnostics found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileOutput {
    /// The tokens the parser reads, in order.
    pub tokens: Vec<PpToken>,
    /// Lexical and preprocessor diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// In an FTEQCC dialect, each `#pragma noref N` in the file: the index of the first token
    /// after it, and whether the definitions from there on are `noref` (FTEF-12.6).
    pub noref: Vec<(usize, bool)>,
    /// In an FTEQCC dialect, each `#pragma autoproto [N]` in the file: the index of the first
    /// token after it, whether it asks for autoprototyping, and the directive's span
    /// (FTEF-12.10).
    pub autoproto: Vec<(usize, bool, FileSpan)>,
}

#[derive(Debug)]
struct Macro {
    /// The parameters' names, as bytes, for FTEQCC compares them bytewise (FTEF-14.23).
    params: Option<Vec<Vec<u8>>>,
    /// Whether the parameter list ends in `...` (FTEF-14.5).
    variadic: Variadic,
    body: Body,
    file: FileId,
    name_span: Span,
}

/// Whether a macro's parameter list ends in `...`, taking the arguments after the named ones
/// (FTEF-14.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variadic {
    No,
    Yes,
}

/// A macro's body as each dialect keeps it.
#[derive(Debug)]
enum Body {
    /// The vanilla dialect's: the body's tokens, each with the file it is in.
    Tokens(Vec<(FileId, Token)>),
    /// FTEQCC's: the text `QCC_PR_PreProcessor_Define` stored, an `#append`ed part included
    /// (FTEF-14.39, FTEF-14.24).
    Text(std::rc::Rc<Text>),
}

impl Macro {
    /// The body's tokens, under the vanilla dialect.
    fn tokens(&self) -> &[(FileId, Token)] {
        match &self.body {
            Body::Tokens(tokens) => tokens,
            Body::Text(_) => &[],
        }
    }

    /// The body's stored text, under FTEQCC.
    fn text(&self) -> Option<&std::rc::Rc<Text>> {
        match &self.body {
            Body::Text(text) => Some(text),
            Body::Tokens(_) => None,
        }
    }
}

/// The names a token may no longer expand to: the macros whose expansion produced it.
type Hide = Option<Rc<HideNode>>;

/// One macro name's counter of the live hide-set nodes that name it, one counter per name in
/// [`Run::live`], so the counter also stands for the name. A name no live node names is in no
/// token's hide set, so [`hidden`] answers without walking the set (QC-9.17).
type Live = Rc<Cell<usize>>;

struct HideNode {
    /// The macro this node hides, as its counter.
    live: Live,
    next: Hide,
}

impl Drop for HideNode {
    fn drop(&mut self) {
        self.live.set(self.live.get().saturating_sub(1));
        // QC-9.17: the nodes only this one holds are unlinked in a loop, not by the recursive
        // drop of `next`, which would take one stack frame per nested expansion.
        let mut next = self.next.take();
        while let Some(node) = next {
            next = Rc::try_unwrap(node)
                .ok()
                .and_then(|mut node| node.next.take());
        }
    }
}

fn hidden(live: &HashMap<String, Live>, hide: &Hide, name: &str) -> bool {
    let Some(mut node) = hide.as_deref() else {
        return false;
    };
    let Some(count) = live.get(name).filter(|count| count.get() > 0) else {
        return false;
    };
    loop {
        if Rc::ptr_eq(&node.live, count) {
            return true;
        }
        let Some(next) = node.next.as_deref() else {
            return false;
        };
        node = next;
    }
}

fn hide_with(hide: &Hide, live: &Live) -> Hide {
    live.set(live.get().saturating_add(1));
    Some(Rc::new(HideNode {
        live: Rc::clone(live),
        next: hide.clone(),
    }))
}

/// A token and its hide set. It has no `Debug`: a derived one would print the hide set by
/// recursion, one frame per nested expansion (QC-9.17).
#[derive(Clone)]
struct Item {
    tok: PpToken,
    hide: Hide,
    /// Where the token was read, for the text after it (FTEF-14.13, FTEF-14.22).
    place: Place,
}

/// Where a token was read: under FTEQCC, the text a macro's name in it reads on in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    /// A file's text, or under the vanilla dialect anywhere.
    File,
    /// An expansion's chunk, the token ending at this offset in its text.
    Chunk(usize),
    /// A built-in constant's text, a chunk of its own that holds nothing a macro's name in it
    /// could read on in (FTEF-14.17, FTEF-14.32).
    Builtin,
}

#[derive(Debug)]
struct Cond {
    value: bool,
    parent: bool,
    opened: Span,
}

/// The tokens lexed from one text, a file's or an FTEQCC expansion's, and where reading
/// stands in them.
#[derive(Debug, Default)]
struct TokenStream {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<qc_lexer::Diagnostic>,
}

/// The tokens and lexical errors of the bytes `range` of `text`, their spans from the range's
/// start, as reading on at that byte reads them (FTEF-14.22): it starts a line only where only
/// white space comes before it on its line, so in the middle of a line a `#` there starts no
/// directive and a `#warning` after it no raw text. In the middle of a line the bytes are lexed
/// after a `;`, a token of its own that changes nothing after it, which is then dropped.
fn lex_from(text: &[u8], range: Range<usize>, dialect: Dialect) -> qc_lexer::Lexed {
    let part = text.get(range.clone()).unwrap_or_default();
    let line_start = text
        .get(..range.start)
        .unwrap_or_default()
        .iter()
        .rev()
        .take_while(|&&b| b != b'\n')
        .all(|&b| qc_lexer::is_whitespace(b));
    if line_start {
        return lex_with(part, dialect);
    }
    let mut after_token = Vec::with_capacity(part.len().saturating_add(1));
    after_token.push(b';');
    after_token.extend_from_slice(part);
    let lexed = lex_with(&after_token, dialect);
    let back = |span: Span| Span::new(span.start.saturating_sub(1), span.end.saturating_sub(1));
    qc_lexer::Lexed {
        tokens: lexed
            .tokens
            .into_iter()
            .skip(1)
            .map(|mut token| {
                token.span = back(token.span);
                token.kind = moved_frames(token.kind, back);
                token
            })
            .collect(),
        diagnostics: lexed
            .diagnostics
            .into_iter()
            .map(|mut d| {
                d.span = back(d.span);
                d
            })
            .collect(),
    }
}

/// `kind` with the spans of the frame names it holds, if any, moved by `place`: a token lexed
/// from part of a text stands where `place` says its bytes are (FTEF-14.22, QC-9.7).
fn moved_frames(kind: TokenKind, place: impl Fn(Span) -> Span) -> TokenKind {
    match kind {
        TokenKind::Frames(names) => TokenKind::Frames(
            names
                .into_iter()
                .map(|mut name| {
                    name.span = place(name.span);
                    name
                })
                .collect(),
        ),
        other => other,
    }
}

impl TokenStream {
    /// The tokens of `text` under `dialect`, read from the first.
    fn lex(text: &[u8], dialect: Dialect) -> Self {
        let lexed = lex_with(text, dialect);
        Self {
            tokens: lexed.tokens,
            pos: 0,
            diagnostics: lexed.diagnostics,
        }
    }

    /// Reading goes on at the byte `offset` of `text`, the text these tokens were lexed from,
    /// where FTEQCC's reading goes on after a use's arguments or a `#define` (FTEF-14.22,
    /// FTEF-14.24): from the first token at or after it, and the text from `offset` lexed
    /// again unless only white space stands between the token before it and `offset`, so
    /// that no token or comment of the text as first lexed stands across it, and its tokens
    /// and lexical errors are those of the text from there. The text is lexed again only as far
    /// as a token of the first lexing that the new lexing reaches as that one did
    /// ([`relex_window`]), so a file of many such uses is not lexed again whole for each.
    /// Returns how many bytes were lexed again.
    fn resume_at(&mut self, text: &[u8], offset: usize, dialect: Dialect) -> usize {
        let next = self.tokens.partition_point(|t| t.span.start < offset);
        let before = next.checked_sub(1).and_then(|last| self.tokens.get(last));
        let gap_start = before.map_or(0, |t| t.span.end);
        let clean = gap_start <= offset
            && text
                .get(gap_start..offset)
                .is_some_and(|gap| gap.iter().all(|&b| qc_lexer::is_whitespace(b)));
        // The tokens from `read` on are those not read yet, or read again from here.
        let read = self.pos.min(next);
        self.pos = next;
        if clean {
            return 0;
        }
        let window = (next..self.tokens.len())
            .take(RESYNC_TRIES)
            .find_map(|kept| Some(kept).zip(self.relex_window(text, offset, kept, dialect)));
        let (kept, rest) = match window {
            Some((kept, lexed)) => (kept, lexed),
            None => (
                self.tokens.len(),
                lex_from(text, offset..text.len(), dialect),
            ),
        };
        let resumed = self.tokens.get(kept).map_or(usize::MAX, |t| t.span.start);
        let shift = |span: Span| {
            Span::new(
                span.start.saturating_add(offset),
                span.end.saturating_add(offset),
            )
        };
        let tokens: Vec<Token> = rest
            .tokens
            .into_iter()
            .map(|mut token| {
                token.span = shift(token.span);
                token.kind = moved_frames(token.kind, shift);
                token
            })
            .collect();
        // The new tokens go in the places before `kept` that hold tokens read or replaced, so
        // the tokens after them do not move; places left over between hold a token of no text
        // at `offset`, which keeps the tokens in order and is never read.
        match kept.checked_sub(tokens.len()).filter(|&at| at >= read) {
            Some(at) => {
                let unread = Token {
                    kind: TokenKind::Punct(Punct::Semicolon),
                    span: Span::new(offset, offset),
                    line_start: false,
                };
                let places = self.tokens.get_mut(next..at).unwrap_or_default();
                places.fill(unread);
                let places = self.tokens.get_mut(at..kept).unwrap_or_default();
                for (place, token) in places.iter_mut().zip(tokens) {
                    *place = token;
                }
                self.pos = at;
            }
            None => {
                self.tokens.splice(next..kept, tokens);
            }
        }
        // The first lexing's errors before `offset` and from the kept token on stand; those
        // between are the new lexing's.
        let old = std::mem::take(&mut self.diagnostics);
        let after: Vec<_> = old
            .iter()
            .filter(|d| d.span.start >= resumed)
            .cloned()
            .collect();
        self.diagnostics = old.into_iter().filter(|d| d.span.start < offset).collect();
        self.diagnostics
            .extend(rest.diagnostics.into_iter().map(|mut d| {
                d.span = shift(d.span);
                d
            }));
        self.diagnostics.extend(after);
        resumed.min(text.len()).saturating_sub(offset)
    }

    /// The text from `offset` to the token `kept` of the first lexing, lexed again, when lexing
    /// the whole text from `offset` would reach that token as the first lexing did, so the
    /// tokens from it on stand: the new tokens end before it with only white space after them,
    /// none is a string (which may join the next, QC-13) or a `#` that may start a directive
    /// (whose `#warning` text is read raw, FTEF-14.38), nor is the kept token a string. A
    /// lexical error then needs no check of its own: one in a token ends with the token, and
    /// one outside any token (an unclosed comment, a stray byte) is no white space after the
    /// last token, so none runs on into the kept token.
    fn relex_window(
        &self,
        text: &[u8],
        offset: usize,
        kept: usize,
        dialect: Dialect,
    ) -> Option<qc_lexer::Lexed> {
        let resume = self
            .tokens
            .get(kept)
            .filter(|t| !matches!(t.kind, TokenKind::String(_)))?
            .span
            .start;
        let window = text.get(offset..resume)?;
        let lexed = lex_from(text, offset..resume, dialect);
        let last_end = lexed.tokens.last().map_or(0, |t| t.span.end);
        let lands = lexed.tokens.last().is_none_or(|last| {
            !matches!(last.kind, TokenKind::String(_)) && last.span.end < window.len()
        }) && window
            .get(last_end..)
            .is_some_and(|after| after.iter().all(|&b| qc_lexer::is_whitespace(b)))
            && !lexed
                .tokens
                .iter()
                .any(|t| t.kind == TokenKind::Punct(Punct::Hash) && t.line_start);
        lands.then_some(lexed)
    }
}

/// One file being read: listed in `progs.src` or included.
#[derive(Debug)]
struct Cursor {
    file: FileId,
    path: PathBuf,
    /// Under FTEQCC, the file's name as FTEQCC has it, `s_filen`, which `__FILE__` writes
    /// (FTEF-14.32): as its `progs.src` or `#include` wrote it, or as a `#pragma file` gave it.
    name: Vec<u8>,
    /// The file's name as it was read, which no `#pragma file` changes: what a GMQCC
    /// `#include` and `#pragma` in it are joined to (PRJ-5.12).
    read_as: Vec<u8>,
    stream: TokenStream,
    /// Under the vanilla dialect, the conditionals open (QC-9.6); FTEQCC skips a side at once.
    conds: Vec<Cond>,
    skipped: Vec<Span>,
    skip_start: Option<usize>,
    /// How the file starts, for a directive on its first line (FTEF-14.34).
    start: TextStart,
    /// Under FTEQCC, the line a `#pragma line` gave the file's line starting at this offset
    /// (FTEF-14.43).
    line_base: Option<(usize, i32)>,
}

impl Cursor {
    fn taking(&self) -> bool {
        self.conds.last().is_none_or(|c| c.value && c.parent)
    }
}

/// The state of preprocessing one source file of `progs.src`.
#[derive(Default)]
struct Run {
    /// The name of the source file of `progs.src` being preprocessed, as the caller gave it:
    /// FTEQCC's `compilingfile`, which an `#include`d file's name is joined to (FTEF-14.32).
    unit: Vec<u8>,
    cursors: Vec<Cursor>,
    pending: Vec<Item>,
    frames: Vec<String>,
    frames_full: bool,
    /// In an FTEQCC dialect, the number the next frame name gets, FTEQCC's `pr_macrovalue`,
    /// 0 at the start of each file (FTEF-18.17).
    frame_value: i32,
    /// For each macro name, how many live hide-set nodes name it (QC-9.17).
    live: HashMap<String, Live>,
    /// In an FTEQCC dialect, the chunks open where reading stands, outermost first: the
    /// expansion being read and those it was used in (FTEF-14.17).
    chunks: Vec<ChunkReader>,
    /// How many of [`Run::chunks`] each macro has, FTEQCC's `c->inside`.
    open_names: HashMap<String, usize>,
    /// The macro expansions this file has made (FTEF-14.28, QC-9.18).
    expanded: Expanded,
    /// Under FTEQCC, how many directives are being run inside another reading (FTEF-14.47).
    nested_directives: usize,
    /// Under FTEQCC, how many directives have been run inside another reading, so that a
    /// macro use's scan knows whether its arguments ran any (FTEF-14.45).
    directives_run: usize,
    out: FileOutput,
}

/// The work a file's macro expansions have done, against the bound of FTEF-14.28 and QC-9.18.
#[derive(Debug, Default)]
struct Expanded {
    expansions: usize,
    tokens: usize,
    /// `E0120` was reported, which happens once a file.
    reported: bool,
}

impl Expanded {
    fn over_bound(&self) -> bool {
        self.expansions >= FTE_MAX_EXPANSIONS || self.tokens >= FTE_MAX_EXPANDED_TOKENS
    }

    /// Reports `E0120` at the use `span` (QCD-8.13), the first time in the file.
    fn report(&mut self, span: FileSpan, diagnostics: &mut Vec<Diagnostic>) {
        if !std::mem::replace(&mut self.reported, true) {
            diagnostics.push(Diagnostic {
                code: Code::E0120,
                message: "macro expansion too large".to_owned(),
                span,
            });
        }
    }
}

impl Run {
    fn report(&mut self, code: Code, message: impl Into<String>, span: FileSpan) {
        self.out.diagnostics.push(Diagnostic {
            code,
            message: message.into(),
            span,
        });
    }
}

/// The longest frame macro name FTEQCC keeps: its `pr_framemacro[].name` is `char name[64]`,
/// and `QCC_PR_MacroFrame` reports a name `if (strlen(name)+1 > sizeof(pr_framemacro[0].name))`
/// (FTEF-18.29).
const FTE_MAX_FRAME_NAME: usize = 63;

/// The most chunks of one macro FTEQCC has open at once, `if (c->inside >= 8)` in
/// `QCC_PR_CheckCompConst` (FTEF-14.17).
const FTE_MAX_OPEN_CHUNKS: usize = 8;

/// The tokens of the first lexing [`TokenStream::resume_at`] tries, one after another, to lex
/// again only as far as, before it lexes the rest of the text again (FTEF-14.22).
const RESYNC_TRIES: usize = 8;

/// The most macro expansions the preprocessor makes in one file, under either dialect
/// (FTEF-14.28, QC-9.18).
const FTE_MAX_EXPANSIONS: usize = 1 << 20;

/// The most tokens the preprocessor's macro expansions write in one file, under either dialect
/// (FTEF-14.28, QC-9.18).
const FTE_MAX_EXPANDED_TOKENS: usize = 1 << 22;

/// What FTEQCC's `s_filen` is while a frame macro is defined or used: one reading of a source
/// file, a new one for each compiled file and each `#include` of a file, or one macro
/// expansion, which `QCC_PR_IncludeChunkEx` gives a name of its own (FTEF-18.29).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameFile {
    file: FileId,
    expansion: Option<ExpansionId>,
}

impl FrameFile {
    const fn of(token: &PpToken) -> Self {
        Self {
            file: token.span.file,
            expansion: token.expansion,
        }
    }
}

/// A frame macro FTEQCC keeps from file to file (FTE-22.4): its value and the reading of a
/// file that last defined it.
#[derive(Debug)]
struct CarriedFrame {
    value: i32,
    file: FrameFile,
    /// Where it stands in FTEQCC's `pr_framemacro` table, which is searched from the last.
    order: usize,
}

/// Where an FTEQCC frame macro is defined or used: the span to report at and the reading of a
/// file it is in.
type Here = (FileSpan, FrameFile);

/// The preprocessor for one `progs.src`: macros run on from one file to the next (QC-9.8).
#[derive(Debug)]
pub struct Preprocessor<L> {
    loader: L,
    dialect: Dialect,
    sources: SourceMap,
    macros: HashMap<String, Rc<Macro>>,
    expansions: Vec<Expansion>,
    /// In an FTEQCC dialect, every frame macro defined so far (FTE-22.4).
    carried_frames: HashMap<String, CarriedFrame>,
    /// In an FTEQCC dialect, the name of the last `$modelname`, FTEQCC's
    /// `pr_framemodelname` (FTEF-18.16).
    model_name: String,
    /// Whether FTEQCC's frame macro table is full (FTEF-18.17), so the error is given once.
    frames_full: bool,
    /// In an FTEQCC dialect, what FTEQCC's directives keep from file to file.
    fte: FteState,
    /// The module's include directories, FTEQCC's `-I` options (FTEF-14.49), as FTEQCC keeps
    /// them, as bytes.
    command_line_dirs: Vec<Vec<u8>>,
    /// The same directories as given, where the vanilla dialect looks for an `#include`
    /// (QC-9.19).
    include_dirs: Vec<PathBuf>,
}

/// The path with `.` removed and `..` folded into the component before it, so one file has
/// one name on the include stack.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let parent_is_name =
                    matches!(out.components().next_back(), Some(Component::Normal(_)));
                if parent_is_name {
                    out.pop();
                } else {
                    out.push(component);
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                out.push(component);
            }
        }
    }
    out
}

impl<L: Loader> Preprocessor<L> {
    /// A preprocessor that reads files through `loader`, with no macros defined.
    pub fn new(loader: L) -> Self {
        Self {
            loader,
            dialect: Dialect::Vanilla,
            sources: SourceMap::default(),
            macros: HashMap::new(),
            expansions: Vec::new(),
            carried_frames: HashMap::new(),
            model_name: String::new(),
            frames_full: false,
            fte: FteState::default(),
            command_line_dirs: Vec::new(),
            include_dirs: Vec::new(),
        }
    }

    /// Starts FTEQCC's second autoprototyping pass, `-Fautoproto` (FTE-22.3, FTE-22.4): every
    /// `#define` is forgotten, as FTEQCC's `QCC_SetDefaultProperties` makes a new macro table
    /// between the passes, and the frame macros of the first pass are kept, since nothing
    /// between the passes clears them. In an FTEQCC dialect it also defines `FTEQCC` and
    /// `__FTEQCC__` again, clears the include directories but the module's, the `#pragma once`
    /// marks and F303's setting (`QCC_PR_CloseProcessor`), keeping the count of open sides
    /// (FTEF-14.48, FTEF-14.49).
    /// The files are then preprocessed again in order.
    pub fn restart(&mut self) {
        self.macros.clear();
        if self.dialect.has_fteqcc_extensions() {
            self.fte_default_properties();
        }
    }

    /// Sets the dialect this preprocessor accepts (PRJ-2.8). In an FTEQCC dialect the
    /// FTEQCC `#pragma` directive (FTE-16) is accepted rather than reported as an unknown
    /// directive (QC-9.9), and `FTEQCC` and `__FTEQCC__` are defined (FTEF-14.48). Vanilla is
    /// the default.
    #[must_use]
    pub fn with_dialect(mut self, dialect: Dialect) -> Self {
        self.dialect = dialect;
        if dialect.has_fteqcc_extensions() {
            self.fte_default_properties();
        }
        self
    }

    /// Gives the module's include directories, FTEQCC's `-I` options (PRJ-5.2). In an FTEQCC
    /// dialect they are the first include directories, before any `#pragma includedir` one,
    /// and are given again before the second autoprototyping pass (FTEF-14.49); in the vanilla
    /// dialect an `#include` not found beside the file holding it is looked for in them, in
    /// order (QC-9.19).
    #[must_use]
    pub fn with_include_dirs(mut self, dirs: &[PathBuf]) -> Self {
        self.include_dirs = dirs.to_vec();
        self.command_line_dirs = dirs
            .iter()
            .map(|dir| dir.as_os_str().as_encoded_bytes().to_vec())
            .collect();
        if self.dialect.has_fteqcc_extensions() {
            for dir in &self.command_line_dirs {
                self.fte.add_include_dir(dir);
            }
        }
        self
    }

    /// Every file read so far.
    #[must_use]
    pub const fn sources(&self) -> &SourceMap {
        &self.sources
    }

    /// Every expansion made so far, indexed by [`ExpansionId`].
    #[must_use]
    pub fn expansions(&self) -> &[Expansion] {
        &self.expansions
    }

    /// Whether `name` is a defined macro now.
    #[must_use]
    pub fn is_defined(&self, name: &str) -> bool {
        self.macros.contains_key(name)
    }

    /// Where a token stands in the file being compiled: its own span for an ordinary token,
    /// the outermost macro use for a token produced by expansion (QC-9.7).
    #[must_use]
    pub fn origin(&self, token: &PpToken) -> FileSpan {
        let mut span = token.span;
        let mut next = token.expansion;
        while let Some(ExpansionId(id)) = next {
            let Some(expansion) = self.expansions.get(id) else {
                break;
            };
            span = expansion.use_span;
            next = expansion.parent;
        }
        span
    }

    /// Preprocesses one source file of `progs.src`, after the ones before it (QC-23.3).
    ///
    /// Implements QC-9 and the frame macro rules of QC-8: the frame list starts empty for
    /// each call (QC-8.3), macros carry over (QC-9.8). In an FTEQCC dialect the frame macros
    /// carry over too (FTE-22.4).
    ///
    /// # Errors
    ///
    /// The loader's error when the file itself cannot be read; a project reports that as
    /// QCD-5.2. Every other problem is a diagnostic in the output.
    ///
    /// Under FTEQCC the file's name, which `__FILE__` writes (FTEF-14.32), is `path` as given,
    /// so a caller gives it as `progs.src` wrote it.
    pub fn preprocess_file(&mut self, path: &Path) -> io::Result<FileOutput> {
        let name = path.as_os_str().as_encoded_bytes().to_vec();
        let path = normalize(path);
        let bytes = self.loader.load(&path)?;
        let file = self.sources.add(path.clone(), bytes);
        let mut run = Run {
            unit: name.clone(),
            ..Run::default()
        };
        let cursor = self.cursor(file, path, name, TextStart::Compiled);
        run.cursors.push(cursor);
        while let Some(item) = self.next_item(&mut run) {
            let Some(item) = self.explicit_use(&mut run, item) else {
                continue;
            };
            if let Some(item) = self.try_expand(&mut run, item) {
                self.emit(&mut run, item.tok);
            }
        }
        Ok(run.out)
    }

    /// In an FTEQCC dialect, a `#` right before the name of a macro it may expand is that
    /// macro's expansion with no `#` (FTE-23.10), as `QCC_PR_Lex` skips the `#` and
    /// `QCC_PR_CheckCompConst` expands the word after it. Returns `item` when it is not such a
    /// `#`, which the parser then reads as it is; `None` when the `#` is gone, or when it went
    /// out with the name after it. The name is the one that touches the `#` in the text FTEQCC
    /// reads, a macro's body's as FTEQCC stored it included (QC-9.16).
    fn explicit_use(&mut self, run: &mut Run, item: Item) -> Option<Item> {
        if !self.dialect.has_fteqcc_extensions() || item.tok.kind != TokenKind::Punct(Punct::Hash) {
            return Some(item);
        }
        // In the source a `#` before white space or a stop byte names nothing, and nothing
        // needs reading after it.
        let ends_at_hash = item.tok.expansion.is_none()
            && self
                .sources
                .file(item.tok.span.file)
                .and_then(|file| file.text.get(item.tok.span.span.end))
                .is_none_or(|&b| ends_word(b));
        if ends_at_hash {
            return Some(item);
        }
        let Some(next) = self.next_item(run) else {
            return Some(item);
        };
        // FTEF-14.13: the name touches the `#` in the text FTEQCC reads, and `try_expand` sees
        // that its word ends with it.
        let names_word =
            matches!(&next.tok.kind, TokenKind::Name(_)) && next.tok.touches(&item.tok);
        if !names_word {
            run.pending.push(next);
            return Some(item);
        }
        // A use `try_expand` refused, a function-like macro's with no or a bad argument list,
        // goes out as it is: tried again, it would report its error again.
        let next = self.try_expand(run, next)?;
        // FTEF-14.34: a `#` first on its line before a word that names no macro is FTEQCC's
        // "Explicit precompiler usage when not defined NAME". The parser reports such a `#` in
        // the rest of a line as E0209 (FTE-23.10); one that starts a line it would read as a
        // directive's, which a profile's text no longer holds, and drop.
        if let TokenKind::Name(name) = &next.tok.kind
            && !self.macros.contains_key(name)
            && self.starts_output_line(run, &item.tok)
        {
            let message = format!("Explicit precompiler usage when not defined {name}");
            run.report(Code::E0121, message, item.tok.span);
        }
        self.emit(run, item.tok);
        self.emit(run, next.tok);
        None
    }

    /// Whether `token`, a token of no expansion, stands first on its line of the output: no
    /// token was written before it, or the last one written stands on another line or file.
    fn starts_output_line(&self, run: &Run, token: &PpToken) -> bool {
        if token.expansion.is_some() {
            return false;
        }
        let Some(last) = run.out.tokens.last() else {
            return true;
        };
        let last = self.origin(last);
        last.file != token.span.file
            || self.line(last.file, last.span.start)
                != self.line(token.span.file, token.span.span.start)
    }

    fn cursor(&self, file: FileId, path: PathBuf, name: Vec<u8>, start: TextStart) -> Cursor {
        let text = self.sources.file(file).map_or(&[][..], |f| &f.text[..]);
        Cursor {
            file,
            path,
            read_as: name.clone(),
            name,
            stream: TokenStream::lex(text, self.dialect),
            conds: Vec::new(),
            skipped: Vec::new(),
            skip_start: None,
            start,
            line_base: None,
        }
    }

    fn line(&self, file: FileId, offset: usize) -> u32 {
        self.sources.file(file).map_or(0, |f| f.lines.line(offset))
    }

    /// The next token after directives and skipped blocks: from the pushed-back tokens first,
    /// then under FTEQCC from the chunk being read, each chunk closed when reading goes past
    /// its last token (FTEF-14.17), then from the file. Under FTEQCC a `#` that starts a line
    /// of the chunk or the file is a directive there (FTEF-14.34, FTEF-14.46).
    fn next_item(&mut self, run: &mut Run) -> Option<Item> {
        if let Some(item) = run.pending.pop() {
            return Some(item);
        }
        let fte = self.dialect.has_fteqcc_extensions();
        while let Some(mut chunk) = run.chunks.pop() {
            if fte && let Some(hash) = chunk.directive_at() {
                let bytes = chunk.bytes();
                run.chunks.push(chunk);
                let resume = self.fte_directive(run, &bytes, hash, Position::Line);
                chunk = run.chunks.pop()?;
                if let Some(resume) = resume {
                    chunk.resume(hash, resume, self.dialect);
                    run.chunks.push(chunk);
                    continue;
                }
            }
            if let Some(item) = chunk.next_item() {
                run.chunks.push(chunk);
                return Some(item);
            }
            Self::close_chunk(run, chunk);
        }
        loop {
            let cursor = run.cursors.last_mut()?;
            let Some(token) = cursor.stream.tokens.get(cursor.stream.pos).cloned() else {
                self.finish_cursor(run);
                continue;
            };
            let file = cursor.file;
            if fte {
                // An `#include` reads the file it names next, in a cursor of its own above
                // this one, so this one is found by its place.
                let index = run.cursors.len().saturating_sub(1);
                if let Some(resume) = self.fte_file_directive(run, file, &token) {
                    self.resume_cursor(run, index, token.span.start, resume);
                    continue;
                }
                let cursor = run.cursors.last_mut()?;
                cursor.stream.pos = cursor.stream.pos.saturating_add(1);
                return Some(Self::file_item(file, token));
            }
            if let Some(end) = self.directive_end(cursor) {
                let stream = &mut cursor.stream;
                let line: Vec<Token> = stream
                    .tokens
                    .get(stream.pos..end)
                    .unwrap_or_default()
                    .to_vec();
                stream.pos = end;
                self.directive(run, &line);
                continue;
            }
            cursor.stream.pos = cursor.stream.pos.saturating_add(1);
            if !cursor.taking() {
                continue;
            }
            return Some(Self::file_item(file, token));
        }
    }

    /// The token `token` of the file `file`, as the file's text has it.
    fn file_item(file: FileId, token: Token) -> Item {
        Item {
            tok: PpToken {
                kind: token.kind,
                span: FileSpan {
                    file,
                    span: token.span,
                },
                expansion: None,
                spelling: None,
                follows: None,
            },
            hide: None,
            place: Place::File,
        }
    }

    /// Under FTEQCC, the directive `token` starts in the file `file` the top cursor reads, when
    /// it is a `#` that starts a line there (FTEF-14.34), run; where reading goes on after it.
    fn fte_file_directive(&mut self, run: &mut Run, file: FileId, token: &Token) -> Option<Resume> {
        if token.kind != TokenKind::Punct(Punct::Hash) {
            return None;
        }
        let start = run.cursors.last()?.start;
        let text = Rc::clone(&self.sources.file(file)?.text);
        let hash = token.span.start;
        if !fte_directive::starts_line(&text, hash, start) {
            return None;
        }
        self.fte_directive(run, &Bytes::File(file, text), hash, Position::Line)
    }

    /// Reading of the file of the cursor `index` goes on at `resume`, after the directive whose
    /// `#` is at `hash`; the directive's text is read as no code.
    fn resume_cursor(&self, run: &mut Run, index: usize, hash: usize, resume: Resume) {
        let Some(cursor) = run.cursors.get_mut(index) else {
            return;
        };
        let Some(text) = self.sources.file(cursor.file).map(|f| Rc::clone(&f.text)) else {
            return;
        };
        let end = match resume {
            Resume::At(end) => {
                cursor.stream.resume_at(&text, end, self.dialect);
                end
            }
            Resume::End => {
                cursor.stream.pos = cursor.stream.tokens.len();
                text.len()
            }
        };
        cursor.skipped.push(Span::new(hash, end));
    }

    /// If the cursor is at a directive (QC-9.1), the index one past its last token: the
    /// vanilla dialect's directives end with their line.
    fn directive_end(&self, cursor: &Cursor) -> Option<usize> {
        let stream = &cursor.stream;
        let hash = stream.tokens.get(stream.pos)?;
        if hash.kind != TokenKind::Punct(Punct::Hash) || !hash.line_start {
            return None;
        }
        let line = self.line(cursor.file, hash.span.start);
        let name = stream.tokens.get(stream.pos.checked_add(1)?)?;
        if !matches!(name.kind, TokenKind::Name(_))
            || self.line(cursor.file, name.span.start) != line
        {
            return None;
        }
        let len = stream
            .tokens
            .get(stream.pos..)?
            .iter()
            .take_while(|t| self.line(cursor.file, t.span.start) == line)
            .count();
        stream.pos.checked_add(len)
    }

    /// Ends the innermost file: open blocks are reported and closed (QC-9.6, QC-9.12), and
    /// its lexical diagnostics outside skipped blocks are reported.
    fn finish_cursor(&self, run: &mut Run) {
        let Some(mut cursor) = run.cursors.pop() else {
            return;
        };
        let len = self.sources.file(cursor.file).map_or(0, |f| f.text.len());
        if let Some(start) = cursor.skip_start.take() {
            cursor.skipped.push(Span::new(start, len));
        }
        for d in cursor.stream.diagnostics {
            let skipped = cursor
                .skipped
                .iter()
                .any(|s| s.start <= d.span.start && d.span.start < s.end);
            if !skipped {
                run.report(
                    d.code,
                    d.message,
                    FileSpan {
                        file: cursor.file,
                        span: d.span,
                    },
                );
            }
        }
        for cond in cursor.conds {
            let span = FileSpan {
                file: cursor.file,
                span: cond.opened,
            };
            run.report(Code::E0110, "#ifdef or #ifndef without #endif", span);
        }
    }

    /// Runs one directive line; `line[0]` is the `#` and `line[1]` the name.
    fn directive(&mut self, run: &mut Run, line: &[Token]) {
        let (
            Some(hash),
            Some(Token {
                kind: TokenKind::Name(name),
                span: name_span,
                ..
            }),
        ) = (line.first(), line.get(1))
        else {
            return;
        };
        let Some(cursor) = run.cursors.last_mut() else {
            return;
        };
        let file = cursor.file;
        let whole = Span::new(
            hash.span.start,
            line.last().map_or(name_span.end, |t| t.span.end),
        );
        let at = |span: Span| FileSpan { file, span };
        let was_taking = cursor.taking();
        let operand = match line.get(2) {
            Some(Token {
                kind: TokenKind::Name(n),
                ..
            }) => Some(n.as_str()),
            _ => None,
        };
        match name.as_str() {
            "ifdef" | "ifndef" => {
                let value =
                    operand.is_some_and(|n| self.macros.contains_key(n) == (name == "ifdef"));
                cursor.conds.push(Cond {
                    value,
                    parent: was_taking,
                    opened: whole,
                });
                if was_taking && operand.is_none() {
                    run.report(
                        Code::E0114,
                        format!("#{name} expects a macro name"),
                        at(whole),
                    );
                }
            }
            "else" | "endif" => {
                let closed = if name == "else" {
                    cursor
                        .conds
                        .last_mut()
                        .map(|c| c.value = !c.value)
                        .is_some()
                } else {
                    cursor.conds.pop().is_some()
                };
                if !closed {
                    run.report(
                        Code::E0109,
                        format!("#{name} without #ifdef or #ifndef"),
                        at(whole),
                    );
                }
            }
            _ if !was_taking => {}
            "define" => self.define(run, file, line),
            "undef" => {
                if let Some(n) = operand {
                    self.macros.remove(n);
                } else {
                    run.report(Code::E0114, "#undef expects a macro name", at(whole));
                }
            }
            "include" => self.include(run, line, whole),
            _ => run.report(Code::E0108, format!("unknown directive #{name}"), at(whole)),
        }
        let Some(cursor) = run.cursors.iter_mut().rev().find(|c| c.file == file) else {
            return;
        };
        let taking = cursor.taking();
        match (was_taking, taking) {
            (true, false) => cursor.skip_start = Some(whole.end),
            (false, true) => {
                let start = cursor.skip_start.take().unwrap_or(whole.start);
                cursor.skipped.push(Span::new(start, whole.start));
            }
            (true, true) | (false, false) => {}
        }
    }

    /// `#define` in the vanilla dialect (QC-9.3), with the operand errors of QC-9.9; under
    /// FTEQCC [`Self::fte_define`] reads it.
    fn define(&mut self, run: &mut Run, file: FileId, line: &[Token]) {
        let at = |span: Span| FileSpan { file, span };
        let whole = match (line.first(), line.last()) {
            (Some(first), Some(last)) => Span::new(first.span.start, last.span.end),
            _ => return,
        };
        let Some(Token {
            kind: TokenKind::Name(name),
            span: name_span,
            ..
        }) = line.get(2)
        else {
            run.report(Code::E0114, "#define expects a macro name", at(whole));
            return;
        };
        let mut body_start = 3;
        let mut params = None;
        if let Some(open) = line.get(3)
            && open.kind == TokenKind::Punct(Punct::LParen)
            && open.span.start == name_span.end
        {
            let Some((list, after)) = parse_params(line, 4) else {
                run.report(
                    Code::E0115,
                    format!("bad parameter list for macro {name}"),
                    at(whole),
                );
                return;
            };
            params = Some(list);
            body_start = after;
        }
        let body: Vec<(FileId, Token)> = line
            .get(body_start..)
            .unwrap_or_default()
            .iter()
            .map(|t| (file, t.clone()))
            .collect();
        if let Some(old) = self.macros.get(name) {
            let old_body = old.tokens();
            let same_body = old_body.len() == body.len()
                && old_body
                    .iter()
                    .zip(&body)
                    .all(|(a, b)| a.1.kind == b.1.kind);
            if old.params != params || !same_body {
                run.report(
                    Code::W0102,
                    format!("macro {name} redefined"),
                    at(*name_span),
                );
            }
        }
        let mac = Macro {
            params,
            variadic: Variadic::No,
            body: Body::Tokens(body),
            file,
            name_span: *name_span,
        };
        self.macros.insert(name.clone(), Rc::new(mac));
    }

    /// `#include` in the vanilla dialect (QC-9.2, QC-9.19), with the errors of QC-9.9 and
    /// QC-9.11; under FTEQCC `fte_directive` reads it (FTEF-14.40).
    fn include(&mut self, run: &mut Run, line: &[Token], whole: Span) {
        let Some(current) = run.cursors.last() else {
            return;
        };
        let file = current.file;
        let Some(Token {
            kind: TokenKind::String(bytes),
            ..
        }) = line.get(2)
        else {
            run.report(
                Code::E0113,
                "#include expects a file name in quotes",
                FileSpan { file, span: whole },
            );
            return;
        };
        let relative = PathBuf::from(String::from_utf8_lossy(bytes).into_owned());
        let beside = current
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        // QC-9.19: beside the including file, then in each include directory in order.
        let mut candidates = vec![normalize(&beside.join(&relative))];
        if relative.is_relative() {
            for dir in &self.include_dirs {
                candidates.push(normalize(&dir.join(&relative)));
            }
        }
        let mut first_error = None;
        let mut found = None;
        for candidate in &candidates {
            match self.loader.load(candidate) {
                Ok(bytes) => {
                    found = Some((candidate.clone(), bytes));
                    break;
                }
                Err(err) => {
                    first_error.get_or_insert(err);
                }
            }
        }
        let Some((path, bytes)) = found else {
            let shown = candidates.first().cloned().unwrap_or_default();
            let reason = first_error.map(|e| e.to_string()).unwrap_or_default();
            let message = format!("cannot include {}: {reason}", shown.display());
            run.report(Code::E0111, message, FileSpan { file, span: whole });
            return;
        };
        if run.cursors.iter().any(|c| c.path == path) {
            let message = format!("recursive #include of {}", path.display());
            run.report(Code::E0112, message, FileSpan { file, span: whole });
            return;
        }
        let included = self.sources.add(path.clone(), bytes);
        let name = path.as_os_str().as_encoded_bytes().to_vec();
        let cursor = self.cursor(included, path, name, TextStart::Included);
        run.cursors.push(cursor);
    }

    /// Expands `item` if it names a macro it may expand to (QC-9.4, QC-9.10), or in an FTEQCC
    /// dialect a built-in constant (FTEF-14.7), which [`Self::fte_try_expand`] does. Returns
    /// the item when it is not replaced. A use past the bound on a file's expansions (QC-9.18)
    /// is left as written, with `E0120` the first time.
    fn try_expand(&mut self, run: &mut Run, item: Item) -> Option<Item> {
        if self.dialect.has_fteqcc_extensions() {
            return self.fte_try_expand(run, item);
        }
        let TokenKind::Name(name) = &item.tok.kind else {
            return Some(item);
        };
        if hidden(&run.live, &item.hide, name) {
            return Some(item);
        }
        let Some(mac) = self.macros.get(name).cloned() else {
            return Some(item);
        };
        let name = name.clone();
        let mut use_span = item.tok.span;
        let mut args = Vec::new();
        // What a use that is not expanded after all gives back, its arguments read.
        let mut collected_back = Vec::new();
        if let Some(params) = &mac.params {
            let open = match self.next_item(run) {
                Some(next) if next.tok.kind == TokenKind::Punct(Punct::LParen) => next,
                Some(next) => {
                    run.pending.push(next);
                    return Some(item);
                }
                None => return Some(item),
            };
            let mut collected = vec![open];
            let Some(close) = self.collect_args(run, &mut collected, &mut args) else {
                run.report(
                    Code::E0117,
                    format!("unterminated argument list for macro {name}"),
                    item.tok.span,
                );
                run.pending.extend(collected.into_iter().rev());
                return Some(item);
            };
            if params.is_empty() && args.len() == 1 && args.first().is_some_and(Vec::is_empty) {
                args.clear();
            }
            if close.tok.span.file == use_span.file
                && close.tok.span.span.end >= use_span.span.start
            {
                use_span.span.end = close.tok.span.span.end;
            }
            collected.push(close);
            if args.len() != params.len() {
                let message = format!(
                    "macro {name} expects {} arguments, found {}",
                    params.len(),
                    args.len()
                );
                run.report(Code::E0116, message, use_span);
                run.pending.extend(collected.into_iter().rev());
                return Some(item);
            }
            collected_back = collected;
        }
        let live = if let Some(live) = run.live.get(&name) {
            Rc::clone(live)
        } else {
            let live = Live::default();
            run.live.insert(name.clone(), Rc::clone(&live));
            live
        };
        let hide = hide_with(&item.hide, &live);
        let id = ExpansionId(self.expansions.len());
        let limit = FTE_MAX_EXPANDED_TOKENS.saturating_sub(run.expanded.tokens);
        let result = (!run.expanded.over_bound())
            .then(|| Self::substitute(&mac, &args, id, (&hide, &live), limit))
            .flatten();
        let Some(result) = result else {
            run.expanded.tokens = FTE_MAX_EXPANDED_TOKENS;
            run.expanded.report(use_span, &mut run.out.diagnostics);
            run.pending.extend(collected_back.into_iter().rev());
            return Some(item);
        };
        run.expanded.expansions = run.expanded.expansions.saturating_add(1);
        run.expanded.tokens = run.expanded.tokens.saturating_add(result.len());
        self.expansions.push(Expansion {
            name,
            use_span,
            definition: FileSpan {
                file: mac.file,
                span: mac.name_span,
            },
            parent: item.tok.expansion,
        });
        run.pending.extend(result.into_iter().rev());
        None
    }

    /// The tokens of an argument, `items`, as the expansion `id` writes them, each as it
    /// touches the one before it (QC-9.16), with a hide set made from `live`.
    fn argument_pieces(items: &[Item], id: ExpansionId, live: &Live) -> Vec<Item> {
        let mut pieces: Vec<Item> = Vec::with_capacity(items.len());
        let mut before: Option<&Item> = None;
        for a in items {
            let follows = before
                .filter(|before| a.tok.touches(&before.tok))
                .and_then(|_| pieces.last().map(|item| item.tok.span));
            pieces.push(Item {
                tok: PpToken {
                    kind: a.tok.kind.clone(),
                    span: a.tok.span,
                    expansion: Some(id),
                    spelling: a.tok.spelling.clone(),
                    follows,
                },
                hide: hide_with(&a.hide, live),
                place: Place::File,
            });
            before = Some(a);
        }
        pieces
    }

    /// The body of `mac` with its parameters replaced by `args` (QC-9.4) in the vanilla
    /// dialect: a body token follows the one before it where the two touch in the body, an
    /// argument's tokens follow each other as they touch, and an argument stays apart from the
    /// body (QC-9.16). `None`, with nothing written, when it would hold more than `limit`
    /// tokens (QC-9.18): each body token is one and each parameter its argument's. Under
    /// FTEQCC [`Self::fte_try_expand`] writes the expansion.
    fn substitute(
        mac: &Macro,
        args: &[Vec<Item>],
        id: ExpansionId,
        (hide, live): (&Hide, &Live),
        limit: usize,
    ) -> Option<Vec<Item>> {
        let params = mac.params.as_deref().unwrap_or_default();
        let param_of = |token: &Token| match &token.kind {
            TokenKind::Name(n) => params.iter().position(|p| p.as_slice() == n.as_bytes()),
            _ => None,
        };
        let size = mac.tokens().iter().fold(0_usize, |size, (_, token)| {
            let written = param_of(token).map_or(1, |arg| args.get(arg).map_or(0, Vec::len));
            size.saturating_add(written)
        });
        if size > limit {
            return None;
        }
        let mut result: Vec<Item> = Vec::with_capacity(size);
        let mut last_in_body: Option<FileSpan> = None;
        let mut after_argument = false;
        for (file, token) in mac.tokens() {
            let here = FileSpan {
                file: *file,
                span: token.span,
            };
            let touches = !after_argument
                && last_in_body
                    .is_some_and(|last| last.file == here.file && last.span.end == here.span.start);
            last_in_body = Some(here);
            if let Some(arg) = param_of(token) {
                let items = args.get(arg).map(Vec::as_slice).unwrap_or_default();
                result.extend(Self::argument_pieces(items, id, live));
                after_argument = true;
                continue;
            }
            after_argument = false;
            let follows = touches
                .then(|| result.last().map(|item| item.tok.span))
                .flatten();
            result.push(Item {
                tok: PpToken {
                    kind: token.kind.clone(),
                    span: here,
                    expansion: Some(id),
                    spelling: None,
                    follows,
                },
                hide: hide.clone(),
                place: Place::File,
            });
        }
        Some(result)
    }

    /// Reads a vanilla function-like macro's arguments after its `(`, which is `collected[0]`,
    /// up to the matching `)`, which is returned. Every token read but the `)` goes to
    /// `collected`, and each argument to `args` (QC-9.10).
    fn collect_args(
        &mut self,
        run: &mut Run,
        collected: &mut Vec<Item>,
        args: &mut Vec<Vec<Item>>,
    ) -> Option<Item> {
        let mut depth = 0_usize;
        let mut current = Vec::new();
        loop {
            let item = self.next_item(run)?;
            match item.tok.kind {
                TokenKind::Punct(Punct::RParen) if depth == 0 => {
                    args.push(current);
                    return Some(item);
                }
                TokenKind::Punct(Punct::Comma) if depth == 0 => {
                    args.push(std::mem::take(&mut current));
                }
                TokenKind::Punct(Punct::LParen) => {
                    depth = depth.saturating_add(1);
                    current.push(item.clone());
                }
                TokenKind::Punct(Punct::RParen) => {
                    depth = depth.saturating_sub(1);
                    current.push(item.clone());
                }
                _ => current.push(item.clone()),
            }
            collected.push(item);
        }
    }

    /// Records an FTEQCC frame macro (FTE-22.4), as `QCC_PR_MacroFrame` does: a name another
    /// reading of a file defined takes the new value, and a name this one already defined
    /// keeps its first value (QC-8.7) with the warning "Duplicate macro defined (NAME). Rename
    /// it." unless `force`d (FTEF-18.29). A new name of more than 63 bytes is the error "Name
    /// for frame macro NAME is too long" and is not defined (FTEF-18.29); a new name past
    /// `FTE_MAX_FRAMES` is an error (FTEF-18.17).
    fn carry_frame(&mut self, run: &mut Run, name: &str, value: i32, force: bool, at: Here) {
        let (at, here) = at;
        match self.carried_frames.get_mut(name) {
            Some(frame) if frame.file == here && !force => {
                // `if (pr_framemacro[i].file == s_filen && !force) QCC_PR_ParseWarning(
                // WARN_DUPLICATEMACRO, "Duplicate macro defined (%s). Rename it.", pr_token);`
                run.report(
                    Code::W0106,
                    format!("Duplicate macro defined ({name}). Rename it."),
                    at,
                );
            }
            Some(frame) => {
                frame.value = value;
                frame.file = here;
            }
            None if name.len() > FTE_MAX_FRAME_NAME => {
                // `QCC_PR_ParseWarning(ERR_TOOMANYFRAMEMACROS, "Name for frame macro %s is
                // too long", name);`, an error code, which FTEQCC prints as an error.
                run.report(
                    Code::E0119,
                    format!("Name for frame macro {name} is too long"),
                    at,
                );
            }
            None => {
                let frame = CarriedFrame {
                    value,
                    file: here,
                    order: self.carried_frames.len(),
                };
                self.carried_frames.insert(name.to_owned(), frame);
                // `if (pr_nummacros >= MAX_FRAMES) QCC_PR_ParseError(ERR_TOOMANYFRAMEMACROS,
                // "Too many frame macros defined");`
                if self.carried_frames.len() >= FTE_MAX_FRAMES && !self.frames_full {
                    self.frames_full = true;
                    run.report(Code::E0504, "too many frames", at);
                }
            }
        }
    }

    /// The value of the FTEQCC frame macro `name`, as `QCC_PR_FindMacro` finds it: the macro
    /// of that name, or else the last one defined whose name differs only in case, with the
    /// warning "Case insensitive frame macro (using NAME)" at `at` (FTEF-18.21). A macro
    /// another reading of a file defined is the warning "Stale macro used (NAME, defined in
    /// FILE)" (FTEF-18.29).
    fn find_frame(&self, run: &mut Run, name: &str, at: Here) -> Option<i32> {
        let (at, here) = at;
        let frame = if let Some(frame) = self.carried_frames.get(name) {
            frame
        } else {
            let (found, frame) = self
                .carried_frames
                .iter()
                .filter(|(other, _)| other.eq_ignore_ascii_case(name))
                .max_by_key(|(_, frame)| frame.order)?;
            run.report(
                Code::W0104,
                format!("Case insensitive frame macro (using {found})"),
                at,
            );
            frame
        };
        // `if (pr_framemacro[i].file != s_filen) QCC_PR_ParseWarning(WARN_STALEMACRO,
        // "Stale macro used (%s, defined in %s)", pr_token, pr_framemacro[i].file);`
        if frame.file != here {
            let defined = self
                .sources
                .file(frame.file.file)
                .map(|f| f.path.display().to_string())
                .unwrap_or_default();
            run.report(
                Code::W0105,
                format!("Stale macro used ({name}, defined in {defined})"),
                at,
            );
        }
        Some(frame.value)
    }

    /// Applies an FTEQCC `$` command to the frame numbering (FTEF-18.16).
    fn frame_command(&mut self, run: &mut Run, command: &FrameCommand, at: Here) {
        match command {
            FrameCommand::Flush => run.frame_value = 0,
            FrameCommand::Reset => {
                self.carried_frames.clear();
                run.frame_value = 0;
            }
            FrameCommand::Value(word) => {
                // A number past an `int` is held at the nearest one.
                let value = cond::atoi(word).clamp(i64::from(i32::MIN), i64::from(i32::MAX));
                run.frame_value = i32::try_from(value).unwrap_or_default();
            }
            FrameCommand::Restore(name) => match self.find_frame(run, name, at) {
                Some(value) => run.frame_value = value,
                None => run.report(Code::E0106, format!("Unknown frame macro ${name}"), at.0),
            },
            FrameCommand::ModelName(name) => {
                if !self.model_name.is_empty() {
                    let previous = std::mem::take(&mut self.model_name);
                    self.carry_frame(run, &previous, run.frame_value, true, at);
                }
                // `if (!QC_strlcpy(pr_framemodelname, pr_token, sizeof(pr_framemodelname)))
                // QCC_PR_ParseWarning (WARN_STRINGTOOLONG, "$modelname name too long");`: the
                // name is cut to the 63 bytes `char pr_framemodelname[64]` holds (FTEF-18.29).
                let kept = name.floor_char_boundary(FTE_MAX_FRAME_NAME);
                if kept < name.len() {
                    run.report(Code::W0107, "$modelname name too long", at.0);
                }
                let name = name.get(..kept).unwrap_or_default();
                name.clone_into(&mut self.model_name);
                // `i = QCC_PR_FindMacro(pr_framemodelname); if (i) pr_macrovalue = i;`, where
                // a name with no macro is -1.
                let found = self.find_frame(run, name, at).unwrap_or(-1);
                if found != 0 {
                    run.frame_value = found;
                }
            }
        }
    }

    /// Adds a token to the output, applying the frame macros of QC-8, and in an FTEQCC
    /// dialect those carried from earlier files (FTE-22.4) and its further `$` commands
    /// (FTEF-18.16, FTEF-18.17).
    fn emit(&mut self, run: &mut Run, mut tok: PpToken) {
        let carry = self.dialect.has_fteqcc_extensions();
        match &tok.kind {
            TokenKind::Frames(names) if carry => {
                let here = FrameFile::of(&tok);
                for frame in names {
                    let at = FileSpan {
                        file: tok.span.file,
                        span: frame.span,
                    };
                    let value = run.frame_value;
                    run.frame_value = run.frame_value.wrapping_add(1);
                    self.carry_frame(run, &frame.name, value, false, (at, here));
                }
                return;
            }
            TokenKind::Frames(names) => {
                for frame in names {
                    if run.frames.len() < LIMIT_QCC_MAX_FRAMES {
                        run.frames.push(frame.name.clone());
                    } else if !run.frames_full {
                        run.frames_full = true;
                        let span = FileSpan {
                            file: tok.span.file,
                            span: frame.span,
                        };
                        run.report(Code::E0504, "too many frames", span);
                    }
                }
                return;
            }
            TokenKind::FrameCommand(command) => {
                let command = command.clone();
                self.frame_command(run, &command, (tok.span, FrameFile::of(&tok)));
                return;
            }
            TokenKind::FrameRef(word) => {
                let value = if carry {
                    self.find_frame(run, word, (tok.span, FrameFile::of(&tok)))
                } else {
                    let index = run.frames.iter().position(|f| f == word);
                    index.map(|i| i32::try_from(i).unwrap_or(i32::MAX))
                };
                if let Some(value) = value {
                    tok.kind = TokenKind::Float(frame_float(value));
                } else {
                    run.report(
                        Code::E0106,
                        format!("Unknown frame macro ${word}"),
                        tok.span,
                    );
                }
            }
            TokenKind::Name(_)
            | TokenKind::Float(_)
            | TokenKind::Vector(_)
            | TokenKind::String(_)
            | TokenKind::Punct(_) => {}
        }
        run.out.tokens.push(tok);
    }
}

/// A frame number as the float it expands to: `pr_immediate._float = (float)i;`, the integer
/// rounded to the nearest float.
fn frame_float(value: i32) -> f32 {
    value.to_string().parse::<f32>().unwrap_or(0.0)
}

/// Parses `name, name, ...)` from `line[start..]`; returns the names and the index after `)`.
fn parse_params(line: &[Token], start: usize) -> Option<(Vec<Vec<u8>>, usize)> {
    let mut params = Vec::new();
    let mut i = start;
    if line
        .get(i)
        .is_some_and(|t| t.kind == TokenKind::Punct(Punct::RParen))
    {
        return Some((params, i.checked_add(1)?));
    }
    loop {
        match &line.get(i)?.kind {
            TokenKind::Name(name) => params.push(name.clone().into_bytes()),
            // FTEF-14.5: the vanilla dialect has no `...` parameter.
            _ => return None,
        }
        i = i.checked_add(1)?;
        match &line.get(i)?.kind {
            TokenKind::Punct(Punct::Comma) => i = i.checked_add(1)?,
            TokenKind::Punct(Punct::RParen) => return Some((params, i.checked_add(1)?)),
            TokenKind::Name(_)
            | TokenKind::Float(_)
            | TokenKind::Vector(_)
            | TokenKind::String(_)
            | TokenKind::Punct(_)
            | TokenKind::FrameRef(_)
            | TokenKind::Frames(_)
            | TokenKind::FrameCommand(_) => return None,
        }
    }
}

/// `bytes` as a QuakeC string literal, the spelling of a string an FTEQCC expansion made
/// (QC-9.16, FTEF-14.31, FTEF-14.32): `"`, `\`, a line break and a carriage return escaped,
/// and a byte of 0x80 or more written as `\x` and its two hex digits, which an FTEQCC string
/// reads back as that byte (FTEF-13.11), so the spelling is UTF-8 and reads back as `bytes`.
fn quote_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_add(2));
    out.push('"');
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            0x80..=0xff => out.push_str(&format!("\\x{b:02x}")),
            _ => out.push(char::from(b)),
        }
    }
    out.push('"');
    out
}
