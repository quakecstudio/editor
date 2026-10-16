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
//! project's own definition; FTEQCC's pragmas come with the dialect profiles.

mod source;

#[cfg(test)]
mod tests;

pub use source::{FileId, FileSpan, FsLoader, Loader, MemoryLoader, SourceFile, SourceMap};

use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use qc_lexer::{Code, Punct, Span, Token, TokenKind, lex};

/// Most frames one source file may define.
///
/// `LIMIT_QCC_MAX_FRAMES` in `docs/spec/limits.md`, from `MAX_FRAMES` in `qcc/pr_lex.c`. More
/// is an error (QC-27.5).
pub const LIMIT_QCC_MAX_FRAMES: usize = 256;

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
}

#[derive(Debug)]
struct Macro {
    params: Option<Vec<String>>,
    body: Vec<Token>,
    file: FileId,
    name_span: Span,
}

/// The names a token may no longer expand to: the macros whose expansion produced it.
type Hide = Option<Rc<HideNode>>;

#[derive(Debug)]
struct HideNode {
    name: String,
    next: Hide,
}

fn hidden(hide: &Hide, name: &str) -> bool {
    let mut node = hide.as_deref();
    while let Some(n) = node {
        if n.name == name {
            return true;
        }
        node = n.next.as_deref();
    }
    false
}

fn hide_with(hide: &Hide, name: &str) -> Hide {
    Some(Rc::new(HideNode {
        name: name.to_owned(),
        next: hide.clone(),
    }))
}

#[derive(Clone, Debug)]
struct Item {
    tok: PpToken,
    hide: Hide,
}

#[derive(Debug)]
struct Cond {
    value: bool,
    parent: bool,
    opened: Span,
}

/// One file being read: listed in `progs.src` or included.
#[derive(Debug)]
struct Cursor {
    file: FileId,
    path: PathBuf,
    tokens: Vec<Token>,
    pos: usize,
    conds: Vec<Cond>,
    lex_diagnostics: Vec<qc_lexer::Diagnostic>,
    skipped: Vec<Span>,
    skip_start: Option<usize>,
}

impl Cursor {
    fn taking(&self) -> bool {
        self.conds.last().is_none_or(|c| c.value && c.parent)
    }
}

/// The state of preprocessing one source file of `progs.src`.
#[derive(Debug, Default)]
struct Run {
    cursors: Vec<Cursor>,
    pending: Vec<Item>,
    frames: Vec<String>,
    frames_full: bool,
    out: FileOutput,
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

/// The preprocessor for one `progs.src`: macros run on from one file to the next (QC-9.8).
#[derive(Debug)]
pub struct Preprocessor<L> {
    loader: L,
    sources: SourceMap,
    macros: HashMap<String, Rc<Macro>>,
    expansions: Vec<Expansion>,
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
            sources: SourceMap::default(),
            macros: HashMap::new(),
            expansions: Vec::new(),
        }
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
    /// each call (QC-8.3), macros carry over (QC-9.8).
    ///
    /// # Errors
    ///
    /// The loader's error when the file itself cannot be read; a project reports that as
    /// QCD-5.2. Every other problem is a diagnostic in the output.
    pub fn preprocess_file(&mut self, path: &Path) -> io::Result<FileOutput> {
        let path = normalize(path);
        let bytes = self.loader.load(&path)?;
        let file = self.sources.add(path.clone(), bytes);
        let mut run = Run::default();
        let cursor = self.cursor(file, path);
        run.cursors.push(cursor);
        while let Some(item) = self.next_item(&mut run) {
            if let Some(item) = self.try_expand(&mut run, item) {
                Self::emit(&mut run, item.tok);
            }
        }
        Ok(run.out)
    }

    fn cursor(&self, file: FileId, path: PathBuf) -> Cursor {
        let text = self
            .sources
            .file(file)
            .map_or(&[][..], |f| f.text.as_slice());
        let lexed = lex(text);
        Cursor {
            file,
            path,
            tokens: lexed.tokens,
            pos: 0,
            conds: Vec::new(),
            lex_diagnostics: lexed.diagnostics,
            skipped: Vec::new(),
            skip_start: None,
        }
    }

    fn line(&self, file: FileId, offset: usize) -> u32 {
        self.sources.file(file).map_or(0, |f| f.lines.line(offset))
    }

    /// The next token after directives and skipped blocks, from the pushed-back tokens first.
    fn next_item(&mut self, run: &mut Run) -> Option<Item> {
        if let Some(item) = run.pending.pop() {
            return Some(item);
        }
        loop {
            let cursor = run.cursors.last_mut()?;
            let Some(token) = cursor.tokens.get(cursor.pos).cloned() else {
                self.finish_cursor(run);
                continue;
            };
            let file = cursor.file;
            if let Some(end) = self.directive_end(cursor) {
                let line: Vec<Token> = cursor
                    .tokens
                    .get(cursor.pos..end)
                    .unwrap_or_default()
                    .to_vec();
                cursor.pos = end;
                self.directive(run, &line);
                continue;
            }
            cursor.pos = cursor.pos.saturating_add(1);
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
                },
                hide: None,
            });
        }
    }

    /// If the cursor is at a directive (QC-9.1), the index one past its last token.
    fn directive_end(&self, cursor: &Cursor) -> Option<usize> {
        let hash = cursor.tokens.get(cursor.pos)?;
        if hash.kind != TokenKind::Punct(Punct::Hash) || !hash.line_start {
            return None;
        }
        let line = self.line(cursor.file, hash.span.start);
        let name = cursor.tokens.get(cursor.pos.checked_add(1)?)?;
        if !matches!(name.kind, TokenKind::Name(_))
            || self.line(cursor.file, name.span.start) != line
        {
            return None;
        }
        let rest = cursor.tokens.get(cursor.pos..)?;
        let len = rest
            .iter()
            .take_while(|t| self.line(cursor.file, t.span.start) == line)
            .count();
        cursor.pos.checked_add(len)
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
        for d in cursor.lex_diagnostics {
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

    /// `#define` (QC-9.3), with the operand errors of QC-9.9.
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
        let body = line.get(body_start..).unwrap_or_default().to_vec();
        if let Some(old) = self.macros.get(name) {
            let same_body = old.body.len() == body.len()
                && old.body.iter().zip(&body).all(|(a, b)| a.kind == b.kind);
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
            body,
            file,
            name_span: *name_span,
        };
        self.macros.insert(name.clone(), Rc::new(mac));
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

    /// Expands `item` if it names a macro it may expand to (QC-9.4, QC-9.10). Returns the item
    /// when it is not replaced.
    fn try_expand(&mut self, run: &mut Run, item: Item) -> Option<Item> {
        let TokenKind::Name(name) = &item.tok.kind else {
            return Some(item);
        };
        if hidden(&item.hide, name) {
            return Some(item);
        }
        let Some(mac) = self.macros.get(name).cloned() else {
            return Some(item);
        };
        let name = name.clone();
        let mut use_span = item.tok.span;
        let mut args = Vec::new();
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
            if args.len() != params.len() {
                let message = format!(
                    "macro {name} expects {} arguments, found {}",
                    params.len(),
                    args.len()
                );
                run.report(Code::E0116, message, use_span);
                collected.push(close);
                run.pending.extend(collected.into_iter().rev());
                return Some(item);
            }
        }
        let id = ExpansionId(self.expansions.len());
        self.expansions.push(Expansion {
            name: name.clone(),
            use_span,
            definition: FileSpan {
                file: mac.file,
                span: mac.name_span,
            },
            parent: item.tok.expansion,
        });
        let hide = hide_with(&item.hide, &name);
        let mut result = Vec::new();
        for token in &mac.body {
            let param = match (&token.kind, &mac.params) {
                (TokenKind::Name(n), Some(params)) => params.iter().position(|p| p == n),
                _ => None,
            };
            if let Some(arg) = param.and_then(|i| args.get(i)) {
                for a in arg {
                    let tok = PpToken {
                        kind: a.tok.kind.clone(),
                        span: a.tok.span,
                        expansion: Some(id),
                    };
                    result.push(Item {
                        tok,
                        hide: hide_with(&a.hide, &name),
                    });
                }
            } else {
                let span = FileSpan {
                    file: mac.file,
                    span: token.span,
                };
                let tok = PpToken {
                    kind: token.kind.clone(),
                    span,
                    expansion: Some(id),
                };
                result.push(Item {
                    tok,
                    hide: hide.clone(),
                });
            }
        }
        run.pending.extend(result.into_iter().rev());
        None
    }

    /// Reads a function-like macro's arguments after its `(`, which is `collected[0]`, up to
    /// the matching `)`, which is returned. Every token read but the `)` goes to `collected`.
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

    /// Adds a token to the output, applying the frame macros of QC-8.
    fn emit(run: &mut Run, mut tok: PpToken) {
        match &tok.kind {
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
            TokenKind::FrameRef(word) => {
                if let Some(index) = run.frames.iter().position(|f| f == word) {
                    let index = u16::try_from(index).unwrap_or(u16::MAX);
                    tok.kind = TokenKind::Float(f32::from(index));
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
        let TokenKind::Name(name) = &line.get(i)?.kind else {
            return None;
        };
        params.push(name.clone());
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
            | TokenKind::Frames(_) => return None,
        }
    }
}
