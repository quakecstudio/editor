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
//! dialect the preprocessor also reads FTEQCC's directives and macros
//! (`docs/spec/dialect-fteqcc.md`, FTEF-14): `#if` and `#elif`, continued lines, `#warning` and
//! `#error`, `#includelist`, variadic macros, `#` and `##`, and the built-in constants.

pub mod cond;
mod fte_macro;
mod source;

#[cfg(test)]
mod tests;

pub use source::{FileId, FileSpan, FsLoader, Loader, MemoryLoader, SourceFile, SourceMap};

use fte_macro::{ChunkReader, Text, ends_word};
use std::cell::Cell;
use std::collections::HashMap;
use std::io;
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
    params: Option<Vec<String>>,
    /// Whether the parameter list ends in `...` (FTEF-14.5).
    variadic: bool,
    body: Body,
    file: FileId,
    name_span: Span,
}

/// A macro's body as each dialect keeps it.
#[derive(Debug)]
enum Body {
    /// The vanilla dialect's: the body's tokens, each with the file it is in.
    Tokens(Vec<(FileId, Token)>),
    /// FTEQCC's: the text `QCC_PR_PreProcessor_Define` stored, an `#append`ed part included
    /// (FTEF-14.8, FTEF-14.24).
    Text(Text),
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
    fn text(&self) -> Option<&Text> {
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
    /// Under FTEQCC, for a token read from an expansion's chunk, the offset in the chunk's
    /// text where it ends (FTEF-14.13, FTEF-14.22).
    chunk_end: Option<usize>,
}

#[derive(Debug)]
struct Cond {
    value: bool,
    parent: bool,
    opened: Span,
    /// Whether a side of the block was taken, so a later `#elif` or `#else` is not
    /// (FTEF-14.1).
    taken: bool,
}

/// The tokens lexed from one text, a file's or an FTEQCC expansion's, and where reading
/// stands in them.
#[derive(Debug, Default)]
struct TokenStream {
    tokens: Vec<Token>,
    pos: usize,
    diagnostics: Vec<qc_lexer::Diagnostic>,
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
    /// and lexical errors are those of the text from there.
    fn resume_at(&mut self, text: &[u8], offset: usize, dialect: Dialect) {
        let next = self.tokens.partition_point(|t| t.span.start < offset);
        let before = next.checked_sub(1).and_then(|last| self.tokens.get(last));
        let gap_start = before.map_or(0, |t| t.span.end);
        let clean = gap_start <= offset
            && text
                .get(gap_start..offset)
                .is_some_and(|gap| gap.iter().all(|&b| qc_lexer::is_whitespace(b)));
        if !clean {
            let rest = lex_with(text.get(offset..).unwrap_or_default(), dialect);
            // Only white space before `offset` on its line: a token there may start a line.
            let line_start = text
                .get(..offset)
                .unwrap_or_default()
                .iter()
                .rev()
                .take_while(|&&b| b != b'\n')
                .all(|&b| qc_lexer::is_whitespace(b));
            let shift = |span: Span| {
                Span::new(
                    span.start.saturating_add(offset),
                    span.end.saturating_add(offset),
                )
            };
            self.tokens.truncate(next);
            let mut first_line = true;
            let mut end = offset;
            for mut token in rest.tokens {
                token.span = shift(token.span);
                first_line &= !text
                    .get(end..token.span.start)
                    .unwrap_or_default()
                    .contains(&b'\n');
                if first_line {
                    token.line_start &= line_start;
                }
                end = token.span.end;
                self.tokens.push(token);
            }
            self.diagnostics.retain(|d| d.span.start < offset);
            self.diagnostics
                .extend(rest.diagnostics.into_iter().map(|mut d| {
                    d.span = shift(d.span);
                    d
                }));
        }
        self.pos = next;
    }
}

/// One file being read: listed in `progs.src` or included.
#[derive(Debug)]
struct Cursor {
    file: FileId,
    path: PathBuf,
    stream: TokenStream,
    conds: Vec<Cond>,
    skipped: Vec<Span>,
    skip_start: Option<usize>,
    /// The end of an `#includelist` line whose `#endlist` has not been reached (FTEF-14.4).
    list_start: Option<usize>,
}

impl Cursor {
    fn taking(&self) -> bool {
        self.conds.last().is_none_or(|c| c.value && c.parent)
    }
}

/// The state of preprocessing one source file of `progs.src`.
#[derive(Default)]
struct Run {
    /// The source file of `progs.src` being preprocessed.
    path: PathBuf,
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
/// (FTEF-18.22).
const FTE_MAX_FRAME_NAME: usize = 63;

/// The most chunks of one macro FTEQCC has open at once, `if (c->inside >= 8)` in
/// `QCC_PR_CheckCompConst` (FTEF-14.17).
const FTE_MAX_OPEN_CHUNKS: usize = 8;

/// The most macro expansions the preprocessor makes in one file, under either dialect
/// (FTEF-14.28, QC-9.18).
const FTE_MAX_EXPANSIONS: usize = 1 << 20;

/// The most tokens the preprocessor's macro expansions write in one file, under either dialect
/// (FTEF-14.28, QC-9.18).
const FTE_MAX_EXPANDED_TOKENS: usize = 1 << 22;

/// What FTEQCC's `s_filen` is while a frame macro is defined or used: one reading of a source
/// file, a new one for each compiled file and each `#include` of a file, or one macro
/// expansion, which `QCC_PR_IncludeChunkEx` gives a name of its own (FTEF-18.22).
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
        }
    }

    /// Starts FTEQCC's second autoprototyping pass, `-Fautoproto` (FTE-22.3, FTE-22.4): every
    /// `#define` is forgotten, as FTEQCC's `QCC_SetDefaultProperties` makes a new macro table
    /// between the passes, and the frame macros of the first pass are kept, since nothing
    /// between the passes clears them. The files are then preprocessed again in order.
    pub fn restart(&mut self) {
        self.macros.clear();
    }

    /// Sets the dialect this preprocessor accepts (PRJ-2.8). In an FTEQCC dialect the
    /// FTEQCC `#pragma` directive (FTE-16) is accepted rather than reported as an unknown
    /// directive (QC-9.9). Vanilla is the default.
    #[must_use]
    pub fn with_dialect(mut self, dialect: Dialect) -> Self {
        self.dialect = dialect;
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
    pub fn preprocess_file(&mut self, path: &Path) -> io::Result<FileOutput> {
        let path = normalize(path);
        let bytes = self.loader.load(&path)?;
        let file = self.sources.add(path.clone(), bytes);
        let mut run = Run {
            path: path.clone(),
            ..Run::default()
        };
        let cursor = self.cursor(file, path);
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
        if !self.dialect.has_fteqcc_extensions()
            || item.tok.kind != TokenKind::Punct(Punct::Hash)
            || item.tok.spelling.as_deref().is_some_and(|text| text != "#")
        {
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
        self.emit(run, item.tok);
        self.emit(run, next.tok);
        None
    }

    fn cursor(&self, file: FileId, path: PathBuf) -> Cursor {
        let text = self
            .sources
            .file(file)
            .map_or(&[][..], |f| f.text.as_slice());
        Cursor {
            file,
            path,
            stream: TokenStream::lex(text, self.dialect),
            conds: Vec::new(),
            skipped: Vec::new(),
            skip_start: None,
            list_start: None,
        }
    }

    fn line(&self, file: FileId, offset: usize) -> u32 {
        self.sources.file(file).map_or(0, |f| f.lines.line(offset))
    }

    /// The next token after directives and skipped blocks: from the pushed-back tokens first,
    /// then under FTEQCC from the chunk being read, each chunk closed when reading goes past
    /// its last token (FTEF-14.17), then from the file.
    fn next_item(&mut self, run: &mut Run) -> Option<Item> {
        if let Some(item) = run.pending.pop() {
            return Some(item);
        }
        while let Some(chunk) = run.chunks.last_mut() {
            if let Some(item) = chunk.next_item() {
                return Some(item);
            }
            if let Some(chunk) = run.chunks.pop() {
                Self::close_chunk(run, chunk);
            }
        }
        loop {
            let cursor = run.cursors.last_mut()?;
            let Some(token) = cursor.stream.tokens.get(cursor.stream.pos).cloned() else {
                self.finish_cursor(run);
                continue;
            };
            let file = cursor.file;
            if let Some(start) = cursor.list_start {
                // FTEF-14.4: the lines of an `#includelist` produce no tokens, up to `#endlist`.
                let end = self.directive_end(cursor);
                let stream = &mut cursor.stream;
                let name = stream.tokens.get(stream.pos.saturating_add(1));
                let ends = end.is_some()
                    && name.is_some_and(|n| n.kind == TokenKind::Name("endlist".to_owned()));
                if ends && let Some(end) = end {
                    cursor.skipped.push(Span::new(start, token.span.start));
                    cursor.list_start = None;
                    stream.pos = end;
                } else {
                    stream.pos = stream.pos.saturating_add(1);
                }
                continue;
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
            let span = FileSpan {
                file,
                span: token.span,
            };
            return Some(Item {
                tok: PpToken {
                    kind: token.kind,
                    span,
                    expansion: None,
                    spelling: None,
                    follows: None,
                },
                hide: None,
                chunk_end: None,
            });
        }
    }

    /// If the cursor is at a directive (QC-9.1), the index one past its last token.
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
        let rest = stream.tokens.get(stream.pos..)?;
        let mut len = 1_usize;
        if self.dialect.has_fteqcc_extensions() {
            // FTEF-14.30: a line that ends in `\` before its break goes on.
            let text = self
                .sources
                .file(cursor.file)
                .map_or(&[][..], |f| f.text.as_slice());
            for pair in rest.windows(2) {
                let [a, b] = pair else { break };
                if !continues(text, a.span.end, b.span.start) {
                    break;
                }
                len = len.saturating_add(1);
            }
        } else {
            len = rest
                .iter()
                .take_while(|t| self.line(cursor.file, t.span.start) == line)
                .count();
        }
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
        if let Some(start) = cursor.list_start.take() {
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
        let fte = self.dialect.has_fteqcc_extensions();
        for cond in cursor.conds {
            // FTEF-14.10: FTEQCC reports a block left open only while it skips a side of it
            // ("#if with no endif" in `QCC_PR_FalsePreProcessorIf`); a taken side open at the
            // end of the file is closed without a word.
            if fte && cond.value && cond.parent {
                continue;
            }
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
        let fte = self.dialect.has_fteqcc_extensions();
        match name.as_str() {
            "ifdef" | "ifndef" => {
                let value =
                    operand.is_some_and(|n| self.macros.contains_key(n) == (name == "ifdef"));
                cursor.conds.push(Cond {
                    value,
                    parent: was_taking,
                    opened: whole,
                    taken: value,
                });
                if was_taking && operand.is_none() {
                    run.report(
                        Code::E0114,
                        format!("#{name} expects a macro name"),
                        at(whole),
                    );
                }
            }
            "if" | "elif" | "elifdef" | "elifndef" if fte => {
                self.fteqcc_conditional(run, file, name, line, was_taking);
            }
            "else" | "endif" => {
                let closed = if name == "else" {
                    cursor
                        .conds
                        .last_mut()
                        .map(|c| {
                            // FTEF-14.1: after an `#elif` the `#else` is taken only when no side
                            // was; in vanilla it flips the block (QC-9.6).
                            if fte {
                                c.value = !c.taken;
                                c.taken = true;
                            } else {
                                c.value = !c.value;
                            }
                        })
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
            "define" | "append" if fte => self.fte_define(run, file, line),
            "define" => self.define(run, file, line),
            "undef" => {
                if let Some(n) = operand {
                    self.macros.remove(n);
                } else {
                    run.report(Code::E0114, "#undef expects a macro name", at(whole));
                }
            }
            "include" => self.include(run, line, whole),
            // FTEQCC `#pragma` (FTE-16): a compiler directive that produces no tokens and no
            // progs output (FTE-16.3). In an FTEQCC dialect it is accepted and ignored,
            // including the rerelease's `#pragma warning disable/enable F317` (FTE-16.1); in
            // the vanilla dialect it stays an unknown directive (QC-9.9).
            "pragma" if fte => {
                // FTEQCC compares a pragma's name with `QC_strcasecmp`.
                let pragma = |word: &str| operand.is_some_and(|o| o.eq_ignore_ascii_case(word));
                // FTEF-12.6: `#pragma noref N` sets whether the definitions after it are
                // `noref`.
                if pragma("noref")
                    && let Some(Token {
                        kind: TokenKind::Float(value),
                        ..
                    }) = line.get(3)
                {
                    let at = run.out.tokens.len();
                    run.out.noref.push((at, *value != 0.0));
                }
                // FTEF-12.10: `#pragma autoproto [N]` asks for autoprototyping, or not for a 0:
                // `autoprototype = *msg?!!atoi(msg):true;`.
                if pragma("autoproto") {
                    let on = match line.get(3) {
                        None => true,
                        Some(token) => cond::atoi(&self.token_text(file, token)) != 0,
                    };
                    let index = run.out.tokens.len();
                    run.out.autoproto.push((index, on, at(whole)));
                }
            }
            _ => {
                if !(fte && self.fteqcc_directive(run, file, name, line, whole)) {
                    run.report(Code::E0108, format!("unknown directive #{name}"), at(whole));
                }
            }
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

    /// FTEQCC's `#if expr`, `#elif expr`, `#elifdef NAME` and `#elifndef NAME` (FTEF-14.1):
    /// a new block, or the next side of the open one, taken only when no earlier side was.
    fn fteqcc_conditional(
        &self,
        run: &mut Run,
        file: FileId,
        name: &str,
        line: &[Token],
        was_taking: bool,
    ) {
        let whole = directive_span(line);
        let operand = match line.get(2) {
            Some(Token {
                kind: TokenKind::Name(n),
                ..
            }) => Some(n.as_str()),
            _ => None,
        };
        let expr = line.get(2..).unwrap_or_default();
        if name == "if" {
            let value = was_taking && self.condition(file, expr);
            if let Some(cursor) = run.cursors.last_mut() {
                cursor.conds.push(Cond {
                    value,
                    parent: was_taking,
                    opened: whole,
                    taken: value,
                });
            }
            return;
        }
        let taken = run
            .cursors
            .last()
            .and_then(|c| c.conds.last())
            .map(|c| c.taken);
        let value = match taken {
            None => {
                run.report(
                    Code::E0109,
                    format!("#{name} without #ifdef or #ifndef"),
                    FileSpan { file, span: whole },
                );
                return;
            }
            Some(true) => false,
            Some(false) if name == "elif" => self.condition(file, expr),
            Some(false) => {
                operand.is_some_and(|n| self.macros.contains_key(n) == (name == "elifdef"))
            }
        };
        if let Some(c) = run.cursors.last_mut().and_then(|c| c.conds.last_mut()) {
            c.value = value;
            c.taken |= value;
        }
    }

    /// FTEQCC's other directives on a taken side (FTEF-14.3, FTEF-14.4, FTEF-14.8); false
    /// for a name that is none of them.
    fn fteqcc_directive(
        &mut self,
        run: &mut Run,
        file: FileId,
        name: &str,
        line: &[Token],
        whole: Span,
    ) -> bool {
        let at = |span: Span| FileSpan { file, span };
        match name {
            // FTEF-14.3: `#warning`, `#error` and `#message`, with the rest of the line.
            "warning" | "error" | "message" => {
                let text = self.line_text(file, line.get(2..).unwrap_or_default());
                if name == "warning" {
                    run.report(Code::W0103, format!("#warning: {text}"), at(whole));
                } else if name == "error" {
                    run.report(Code::E0118, format!("#Error: {text}"), at(whole));
                }
            }
            // FTEF-14.4: the list's files are the module graph's to compile.
            "includelist" => {
                if let Some(cursor) = run.cursors.last_mut() {
                    cursor.list_start = Some(whole.end);
                }
            }
            // FTEF-14.8: `#eof` ends the file, and the build directives produce no tokens;
            // `directive` gives `#append` to `fte_define`.
            "eof" => {
                if let Some(cursor) = run.cursors.last_mut() {
                    cursor.skip_start.get_or_insert(whole.end);
                    cursor.stream.pos = cursor.stream.tokens.len();
                }
            }
            "endlist" | "output" | "forcecrc" | "copyright" | "pack" | "package" | "merge"
            | "datafile" => {}
            _ => return false,
        }
        true
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
            // The vanilla dialect has no `...` parameter (FTEF-14.5).
            let list = parse_params(line, 4).filter(|(list, _)| !list.iter().any(|p| p == "..."));
            let Some((list, after)) = list else {
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
            variadic: false,
            body: Body::Tokens(body),
            file,
            name_span: *name_span,
        };
        self.macros.insert(name.clone(), Rc::new(mac));
    }

    /// The value of an `#if` or `#elif` expression (FTEF-14.1), evaluated as the project graph
    /// evaluates it in this dialect (PRJ-5.5, FTEF-14.11), a macro's value being its body's
    /// text: its tokens' text, or under FTEQCC the text it stored (`c->value`). The expression
    /// is its tokens' text; under an FTEQCC profile it is the bytes as written, since FTEQCC
    /// reads words up to whitespace (FTEF-14.11).
    fn condition(&self, file: FileId, tokens: &[Token]) -> bool {
        let expr = if self.dialect.has_fteqcc_extensions() {
            self.source_text(file, tokens)
        } else {
            self.line_text(file, tokens)
        };
        let value = |name: &str| {
            self.macros.get(name).map(|m| match &m.body {
                Body::Text(text) => String::from_utf8_lossy(text.bytes()).into_owned(),
                Body::Tokens(tokens) => {
                    let parts: Vec<String> =
                        tokens.iter().map(|(f, t)| self.token_text(*f, t)).collect();
                    parts.join(" ")
                }
            })
        };
        cond::eval(&expr, &value, self.dialect)
    }

    /// The bytes of `file` from the first of `tokens` to the end of the last, a line
    /// continuation (FTEF-14.30) read as a space.
    fn source_text(&self, file: FileId, tokens: &[Token]) -> String {
        let (Some(first), Some(last)) = (tokens.first(), tokens.last()) else {
            return String::new();
        };
        let span = FileSpan {
            file,
            span: Span::new(first.span.start, last.span.end),
        };
        let text = String::from_utf8_lossy(self.sources.text(span).unwrap_or_default());
        text.replace("\\\r\n", " ").replace("\\\n", " ")
    }

    /// The text of directive tokens of `file`, each token's bytes, joined by a space.
    fn line_text(&self, file: FileId, tokens: &[Token]) -> String {
        let parts: Vec<String> = tokens.iter().map(|t| self.token_text(file, t)).collect();
        parts.join(" ")
    }

    /// One token's bytes in `file`, as text.
    fn token_text(&self, file: FileId, token: &Token) -> String {
        let span = FileSpan {
            file,
            span: token.span,
        };
        String::from_utf8_lossy(self.sources.text(span).unwrap_or_default()).into_owned()
    }

    /// `#include` (QC-9.2), with the errors of QC-9.9 and QC-9.11.
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
        let base = current
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let path = normalize(&base.join(relative));
        if run.cursors.iter().any(|c| c.path == path) {
            let message = format!("recursive #include of {}", path.display());
            run.report(Code::E0112, message, FileSpan { file, span: whole });
            return;
        }
        match self.loader.load(&path) {
            Ok(bytes) => {
                let included = self.sources.add(path.clone(), bytes);
                let cursor = self.cursor(included, path);
                run.cursors.push(cursor);
            }
            Err(err) => {
                let message = format!("cannot include {}: {err}", path.display());
                run.report(Code::E0111, message, FileSpan { file, span: whole });
            }
        }
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
                chunk_end: None,
            });
            before = Some(a);
        }
        pieces
    }

    /// The body of `mac` with its parameters replaced by `args` (QC-9.4) in the vanilla
    /// dialect: a body token follows the one before it where the two touch in the body, an
    /// argument's tokens follow each other as they touch, and an argument stays apart from the
    /// body (QC-9.16). `None` once it would hold more than `limit` tokens (QC-9.18). Under
    /// FTEQCC [`Self::fte_try_expand`] writes the expansion.
    fn substitute(
        mac: &Macro,
        args: &[Vec<Item>],
        id: ExpansionId,
        (hide, live): (&Hide, &Live),
        limit: usize,
    ) -> Option<Vec<Item>> {
        let params = mac.params.as_deref().unwrap_or_default();
        let mut result: Vec<Item> = Vec::new();
        let mut last_in_body: Option<FileSpan> = None;
        let mut after_argument = false;
        for (file, token) in mac.tokens() {
            if result.len() > limit {
                return None;
            }
            let here = FileSpan {
                file: *file,
                span: token.span,
            };
            let touches = !after_argument
                && last_in_body
                    .is_some_and(|last| last.file == here.file && last.span.end == here.span.start);
            last_in_body = Some(here);
            let param = match &token.kind {
                TokenKind::Name(n) => params.iter().position(|p| p == n),
                _ => None,
            };
            if let Some(arg) = param {
                let items = args.get(arg).map(Vec::as_slice).unwrap_or_default();
                // QC-9.18: the bound holds before an argument is copied, not only after.
                if result.len().saturating_add(items.len()) > limit {
                    return None;
                }
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
                chunk_end: None,
            });
        }
        (result.len() <= limit).then_some(result)
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
    /// it." unless `force`d (FTEF-18.22). A new name of more than 63 bytes is the error "Name
    /// for frame macro NAME is too long" and is not defined (FTEF-18.22); a new name past
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
    /// FILE)" (FTEF-18.22).
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
                // name is cut to the 63 bytes `char pr_framemodelname[64]` holds (FTEF-18.22).
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
fn parse_params(line: &[Token], start: usize) -> Option<(Vec<String>, usize)> {
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
            TokenKind::Name(name) => params.push(name.clone()),
            // FTEF-14.5: `...` ends a variadic list.
            TokenKind::Punct(Punct::Ellipsis) => {
                params.push("...".to_owned());
                return match &line.get(i.checked_add(1)?)?.kind {
                    TokenKind::Punct(Punct::RParen) => Some((params, i.checked_add(2)?)),
                    _ => None,
                };
            }
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

/// Whether a directive goes on from the token ending at `from` to the one starting at `to`
/// (FTEF-14.30): on the same line, or every line break between them is right after a `\` or
/// inside a `/* */` comment. A `//` comment ends the line.
fn continues(text: &[u8], from: usize, to: usize) -> bool {
    let gap = text.get(from..to).unwrap_or_default();
    let mut i = 0_usize;
    let mut in_block = false;
    while let Some(&b) = gap.get(i) {
        let next = gap.get(i.saturating_add(1)).copied();
        if in_block {
            if b == b'*' && next == Some(b'/') {
                in_block = false;
                i = i.saturating_add(1);
            }
        } else if b == b'/' && next == Some(b'*') {
            in_block = true;
            i = i.saturating_add(1);
        } else if b == b'/' && next == Some(b'/') {
            return !gap.get(i..).unwrap_or_default().contains(&b'\n');
        } else if b == b'\n' {
            let before = i.checked_sub(1).and_then(|j| gap.get(j)).copied();
            let before2 = i.checked_sub(2).and_then(|j| gap.get(j)).copied();
            let escaped =
                before == Some(b'\\') || (before == Some(b'\r') && before2 == Some(b'\\'));
            if !escaped {
                return false;
            }
        }
        i = i.saturating_add(1);
    }
    true
}

/// The span of a directive line, from its `#` to its last token.
fn directive_span(line: &[Token]) -> Span {
    match (line.first(), line.last()) {
        (Some(first), Some(last)) => Span::new(first.span.start, last.span.end),
        _ => Span::new(0, 0),
    }
}

/// The token a built-in constant `name` expands to and its text, `QCC_PR_CheckBuiltinCompConst`
/// (FTEF-14.7), in the file `path` at the line `line` FTEQCC gives it there (FTEF-14.27); `None`
/// when `name` is none. A string's text escapes its bytes, so it reads back as the token's
/// (FTEF-14.21).
fn builtin_value(name: &str, line: u32, path: &Path) -> Option<(TokenKind, String)> {
    let string = |text: &str| (TokenKind::String(text.as_bytes().to_vec()), quote(text));
    Some(match name {
        "__NULL__" => (TokenKind::Float(0.0), "0i".to_owned()),
        "__FILE__" => string(&path.display().to_string()),
        "__LINE__" => (
            TokenKind::Float(f32::from(u16::try_from(line).unwrap_or(u16::MAX))),
            line.to_string(),
        ),
        "__LINESTR__" => string(&line.to_string()),
        "__FUNC__" | "__func__" | "__TIME__" | "__DATE__" | "__QCCVER__" => string(""),
        "__RAND__" => (TokenKind::Float(0.0), "0".to_owned()),
        _ => return None,
    })
}

/// `text` as a QuakeC string literal, `"`, `\` and a line break escaped (FTEF-14.21).
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_add(2));
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `bytes` as a QuakeC string literal (FTEF-14.31), as `QCC_PR_ExpandStrCatMarkup` escapes
/// them: `"`, `\` and a line break escaped; a byte of 0x80 or more is written as `\x`
/// and its two hex digits, which an FTEQCC string reads back as that byte (FTEF-13.11), so
/// the literal's text stays UTF-8.
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
