// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_lex.c, QCC_PR_CheckCompConst, QCC_PR_ExpandPreProcessorMacro, QCC_PR_ExpandStrCatMarkup, QCC_PR_PreProcessor_Define, QCC_PR_IncludeChunkEx
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_cmdlib.c, QCC_COM_Parse2

//! FTEQCC's macro expansion (`docs/spec/dialect-fteqcc.md` FTEF-14.13 to FTEF-14.20, and
//! `docs/spec/language.md` QC-9.16), ported from FTEQCC's behaviour under ADR-0017's
//! clean-port policy: FTEQCC expands a macro by writing its body into a text buffer, the
//! arguments copied in as text, and lexing that buffer as a chunk of its own. This module keeps
//! the preprocessor's token stream and gives each token what that text says about it: whether
//! it touches the token before it ([`PpToken::follows`]), which chunks are open where it
//! stands, and the bytes a `##` paste or a `#` string writes. The FTEQCC functions each
//! function follows are named in its rustdoc.

use std::collections::HashSet;
use std::path::Path;

use qc_lexer::{Punct, Span, Token, TokenKind, lex_with};

use crate::{
    ExpansionId, FTE_MAX_OPEN_CHUNKS, FileSpan, Hide, Item, Live, Loader, Macro, PpToken,
    Preprocessor, Run, cond,
};

/// What a `##` joins the next text to (FTEF-14.14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paste {
    /// No `##` is waiting for its right side.
    None,
    /// The next text is written right after the last.
    Join,
    /// The left side was an argument copied whole with white space after it, so the next text
    /// comes after that white space.
    AfterSpace,
}

/// The state of one expansion's text as `QCC_PR_ExpandPreProcessorMacro` writes it.
struct Writer {
    out: Vec<Item>,
    /// Whether white space stands between the text written so far and the next token; the
    /// start of a chunk touches nothing.
    gap: bool,
    paste: Paste,
    /// Whether the last thing written was an argument copied whole whose text ends in white
    /// space (FTEF-14.14).
    space_after_argument: bool,
}

impl Writer {
    /// The span the next token follows, when nothing stands between them.
    fn follows(&self) -> Option<FileSpan> {
        if self.gap {
            None
        } else {
            self.out.last().map(|item| item.tok.span)
        }
    }
}

impl<L: Loader> Preprocessor<L> {
    /// The body of the macro `mac` with its parameters replaced by `args` under FTEQCC, as
    /// `QCC_PR_ExpandPreProcessorMacro` writes it, or as `QCC_PR_IncludeChunkEx` includes an
    /// object-like macro's stored body. `bounds` holds the `(` and each argument's `,` or `)`.
    ///
    /// A body token touches the one before it when only what `QCC_PR_PreProcessor_Define`
    /// dropped stands between them ([`dropped_in_body`]), and a parameter that touches a word
    /// that way is part of the word, as `QCC_COM_Parse2` reads it. An argument's first token
    /// touches as its parameter does; its last token touches the next body token unless a
    /// `/* */` comment, or for an argument copied whole any white space, follows it
    /// ([`Self::nothing_copied_after`]); an argument not beside `##` has its built-in constants
    /// written as their values (FTEF-14.20). Only a function-like macro reads `#` and `##`
    /// (FTEF-14.16): `#param` writes the argument's text as a string ([`Self::stringified`],
    /// FTEF-14.15), a `#` before what is no named parameter writes the next word against it,
    /// one at the end writes nothing, and `##` joins the text either side ([`Self::paste`],
    /// FTEF-14.14). Implements FTEF-14.14 to FTEF-14.16, FTEF-14.20 and QC-9.16.
    pub(crate) fn fte_substitute(
        &self,
        mac: &Macro,
        args: &[Vec<Item>],
        bounds: &[PpToken],
        id: ExpansionId,
        (hide, live): (&Hide, &Live),
        (quiet, path): (&mut Vec<FileSpan>, &Path),
    ) -> Vec<Item> {
        let params = mac.params.as_deref();
        let named = params.unwrap_or_default();
        let param_of = |token: &Token| -> Option<usize> {
            let TokenKind::Name(n) = &token.kind else {
                return None;
            };
            params?;
            if let Some(i) = named.iter().position(|p| p == n) {
                return Some(i);
            }
            (mac.variadic && n == "__VA_ARGS__").then_some(named.len())
        };
        let hash_at = |at: usize| {
            mac.body
                .get(at)
                .is_some_and(|(_, t)| t.kind == TokenKind::Punct(Punct::Hash))
        };
        let span_at = |at: usize| {
            mac.body.get(at).map(|(file, t)| FileSpan {
                file: *file,
                span: t.span,
            })
        };
        // `##` is two `#` that touch where the body was stored.
        let paste_at = |at: usize| {
            params.is_some()
                && hash_at(at)
                && hash_at(at.saturating_add(1))
                && span_at(at)
                    .zip(span_at(at.saturating_add(1)))
                    .is_some_and(|(a, b)| self.touch_in_body(a, b))
        };
        // The arguments some `#` turns into a string and none writes as code: only those
        // have their source text taken out of lexical checking.
        let mut as_code = HashSet::new();
        let mut quiet_candidates = Vec::new();
        let mut w = Writer {
            out: Vec::new(),
            gap: true,
            paste: Paste::None,
            space_after_argument: false,
        };
        let mut last_in_body: Option<(FileSpan, &Token)> = None;
        let mut literal_next = false;
        let mut index = 0_usize;
        while let Some((file, token)) = mac.body.get(index) {
            let at = index;
            index = index.saturating_add(1);
            let here = FileSpan {
                file: *file,
                span: token.span,
            };
            let touches_last = last_in_body.is_some_and(|(last, _)| self.touch_in_body(last, here));
            w.gap |= !touches_last;
            if literal_next {
                w.gap = false;
            }
            let after_name =
                last_in_body.is_some_and(|(_, t)| matches!(t.kind, TokenKind::Name(_)));
            let in_word = touches_last && after_name || self.word_goes_on(here, span_at(index));
            last_in_body = Some((here, token));
            if params.is_some() && token.kind == TokenKind::Punct(Punct::Hash) && !literal_next {
                if paste_at(at) {
                    // `*bufferlen = whitestart;`: the body's white space before `##` goes.
                    index = index.saturating_add(1);
                    last_in_body = mac
                        .body
                        .get(at.saturating_add(1))
                        .zip(span_at(at.saturating_add(1)))
                        .map(|((_, t), span)| (span, t));
                    w.paste = if w.space_after_argument {
                        Paste::AfterSpace
                    } else {
                        Paste::Join
                    };
                    w.gap = false;
                    continue;
                }
                let next = mac.body.get(index);
                // `for (p = 0; p < param; p++) if (!STRCMP(qcc_token, c->params[p]))`: only a
                // named parameter; `...` has no name.
                if let Some(((_, param), arg)) = next
                    .and_then(|next| Some((next, param_of(&next.1)?)))
                    .filter(|(_, arg)| *arg < named.len())
                {
                    index = index.saturating_add(1);
                    last_in_body = span_at(at.saturating_add(1)).map(|span| (span, param));
                    let items = args.get(arg).map(Vec::as_slice).unwrap_or_default();
                    let text = self.stringified(
                        items,
                        (bounds.get(arg), bounds.get(arg.saturating_add(1))),
                        &mut quiet_candidates,
                        arg,
                    );
                    let spelling = Some(crate::quote_bytes(&text));
                    let kind = TokenKind::String(text);
                    self.write_token(&mut w, (kind, spelling), here, (id, hide));
                    continue;
                }
                // `if (!pr_file_p) break;`: a `#` at the end of the body writes nothing.
                if next.is_none() {
                    continue;
                }
                // `#` and then the next word, `qcc_token`, with nothing between.
                self.write_token(&mut w, (token.kind.clone(), None), here, (id, hide));
                literal_next = true;
                continue;
            }
            let param = param_of(token).filter(|_| !in_word && !literal_next);
            literal_next = false;
            let Some(arg) = param else {
                self.write_token(&mut w, (token.kind.clone(), None), here, (id, hide));
                continue;
            };
            as_code.insert(arg);
            let items = args.get(arg).map(Vec::as_slice).unwrap_or_default();
            // An argument beside `##`, and the variadic one, are copied whole
            // (`noargexpand`, `strlen(paramoffset[p])`).
            let whole = w.paste != Paste::None || paste_at(index) || arg == named.len();
            let end = bounds.get(arg.saturating_add(1));
            self.write_argument(&mut w, items, (end, whole), (id, live, path));
        }
        for (arg, span) in quiet_candidates {
            if !as_code.contains(&arg) {
                quiet.push(span);
            }
        }
        w.out
    }

    /// Writes the token `kind` (with its `spelling`, if made) at the body's place `here` to
    /// `w`, after the last written when nothing stands between them.
    fn write_token(
        &self,
        w: &mut Writer,
        (kind, spelling): (TokenKind, Option<String>),
        here: FileSpan,
        (id, hide): (ExpansionId, &Hide),
    ) {
        let tok = PpToken {
            kind,
            span: here,
            expansion: Some(id),
            spelling,
            follows: w.follows(),
        };
        self.write(
            w,
            vec![Item {
                tok,
                hide: hide.clone(),
            }],
        );
    }

    /// Whether the body token at `here` goes on into a word as FTEQCC stored the body: the
    /// next body token touches it ([`Self::touch_in_body`]) and starts with a byte
    /// `QCC_COM_Parse2` reads as part of a word, a letter, digit or `_`.
    fn word_goes_on(&self, here: FileSpan, next: Option<FileSpan>) -> bool {
        next.is_some_and(|next| {
            self.touch_in_body(here, next)
                && self
                    .sources
                    .text(next)
                    .and_then(|t| t.first())
                    .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_')
        })
    }

    /// Writes the argument `items`, whose `,` or `)` is `end`, to `w`: copied `whole`
    /// (`noargexpand`) or a token at a time with its built-in constants written as their
    /// values (`end = QCC_PR_CheckBuiltinCompConst(qcc_token, ...)`, FTEF-14.20), and leaves
    /// `w.gap` as the white space after it stands (QC-9.16).
    fn write_argument(
        &self,
        w: &mut Writer,
        items: &[Item],
        (end, whole): (Option<&PpToken>, bool),
        (id, live, path): (ExpansionId, &Live, &Path),
    ) {
        let mut pieces = Self::argument_pieces(items, w.follows(), id, live);
        if !whole {
            for piece in &mut pieces {
                let value = match &piece.tok.kind {
                    TokenKind::Name(name) if piece.tok.spelling.is_none() => {
                        self.builtin(path, &piece.tok, name)
                    }
                    _ => None,
                };
                if let Some(value) = value {
                    piece.tok.kind = value.kind;
                    piece.tok.spelling = value.spelling;
                }
            }
        }
        let space_after = items.last().zip(end).is_some_and(|(last, end)| {
            if whole {
                !end.touches(&last.tok)
            } else {
                !self.nothing_copied_after(&last.tok, end)
            }
        });
        if pieces.is_empty() {
            return;
        }
        self.write(w, pieces);
        w.gap = space_after;
        w.space_after_argument = whole && space_after;
    }

    /// Writes `items` to `w`, joining the first to the last written when a `##` waits
    /// (FTEF-14.14).
    fn write(&self, w: &mut Writer, items: Vec<Item>) {
        let mut items = items.into_iter();
        let Some(mut first) = items.next() else {
            return;
        };
        match std::mem::replace(&mut w.paste, Paste::None) {
            Paste::None => w.out.push(first),
            Paste::AfterSpace => {
                first.tok.follows = None;
                w.out.push(first);
            }
            Paste::Join => match w.out.pop() {
                Some(last) => {
                    let first_span = first.tok.span;
                    let joined = self.paste(last, first);
                    let joined_span = joined.last().map(|item| item.tok.span);
                    w.out.extend(joined);
                    // The argument's next token touched the token the paste used up.
                    let mut rest: Vec<Item> = items.collect();
                    if let Some(next) = rest.first_mut()
                        && next.tok.follows == Some(first_span)
                    {
                        next.tok.follows = joined_span;
                    }
                    w.out.extend(rest);
                    w.gap = false;
                    w.space_after_argument = false;
                    return;
                }
                None => w.out.push(first),
            },
        }
        w.out.extend(items);
        w.gap = false;
        w.space_after_argument = false;
    }

    /// `a ## b` (FTEF-14.14): `a`'s text and then `b`'s, which FTEQCC lexes again with the
    /// rest of the chunk. When that is one token it is that token; when it is none (a comment,
    /// `//` or `/*`, which runs to the end of the chunk) neither side is written; otherwise `a`
    /// stays and `b` is written against it.
    fn paste(&self, left: Item, mut right: Item) -> Vec<Item> {
        let text = format!(
            "{}{}",
            self.item_text(&left.tok),
            self.item_text(&right.tok)
        );
        let lexed = lex_with(text.as_bytes(), self.dialect);
        match lexed.tokens.as_slice() {
            [] => Vec::new(),
            [one] => vec![Item {
                tok: PpToken {
                    kind: one.kind.clone(),
                    span: left.tok.span,
                    expansion: left.tok.expansion,
                    spelling: Some(text),
                    follows: left.tok.follows,
                },
                hide: left.hide,
            }],
            _ => {
                right.tok.follows = Some(left.tok.span);
                vec![left, right]
            }
        }
    }

    /// The bytes `#param` writes for the argument `items` between the delimiters `bounds`, its
    /// `(` or `,` and its `,` or `)` (FTEF-14.15): `QCC_PR_ExpandPreProcessorMacro` copies
    /// `paramoffset[p]`, the argument's text from after the white space and comments that
    /// follow its `(` or `,` (`QCC_PR_LexWhitespace(false)`) up to its `,` or `)`, every byte as
    /// it is, and `QCC_PR_ExpandStrCatMarkup` escapes it into the string. Where the argument is
    /// not text of the source, its tokens are written as they touch. The source text becomes a
    /// string's, so the span of the argument `arg` is offered to `quiet`.
    fn stringified(
        &self,
        items: &[Item],
        bounds: (Option<&PpToken>, Option<&PpToken>),
        quiet: &mut Vec<(usize, FileSpan)>,
        arg: usize,
    ) -> Vec<u8> {
        if let (Some(start), Some(end)) = bounds
            && start.expansion.is_none()
            && end.expansion.is_none()
            && items.iter().all(|item| item.tok.expansion.is_none())
            && start.span.file == end.span.file
            && let Some(text) = self
                .sources
                .file(end.span.file)
                .and_then(|file| file.text.get(start.span.span.end..end.span.span.start))
        {
            let skipped = text.len().saturating_sub(after_lex_whitespace(text).len());
            let from = start.span.span.end.saturating_add(skipped);
            quiet.push((
                arg,
                FileSpan {
                    file: end.span.file,
                    span: Span::new(from, end.span.span.start),
                },
            ));
            return after_lex_whitespace(text).to_vec();
        }
        let mut text = Vec::new();
        let mut before: Option<&PpToken> = None;
        for item in items {
            if before.is_some_and(|before| !item.tok.touches(before)) {
                text.push(b' ');
            }
            text.extend_from_slice(self.item_text(&item.tok).as_bytes());
            before = Some(&item.tok);
        }
        text
    }

    /// Whether the word `QCC_PR_CheckCompConst` reads at the name `tok` ends with the name
    /// (FTEF-14.13): the byte after it in the text FTEQCC reads [`ends_word`]. For a token of
    /// no expansion that byte is the source's; for a token of an expansion it is the first
    /// byte of the next token when that one [`PpToken::touches`] it, and otherwise white
    /// space or the end of the expansion, which ends the word too.
    pub(crate) fn word_ends_after(&self, run: &Run, tok: &PpToken) -> bool {
        let next = match tok.expansion {
            None => self
                .sources
                .file(tok.span.file)
                .and_then(|file| file.text.get(tok.span.span.end).copied()),
            Some(_) => run
                .pending
                .last()
                .filter(|next| next.tok.touches(tok))
                .and_then(|next| {
                    let text = match &next.tok.spelling {
                        Some(spelling) => Some(spelling.as_bytes()),
                        None => self.sources.text(next.tok.span),
                    };
                    text?.first().copied()
                }),
        };
        next.is_none_or(ends_word)
    }

    /// Whether FTEQCC leaves the name `tok` of the macro `name` as it is because the macro is
    /// already being expanded (FTEF-14.17): `QCC_PR_CheckCompConst` expands no macro in the
    /// chunk of its own expansion (`currentchunk->cnst != c`), no object-like macro with 8
    /// chunks of it open (`if (c->inside >= 8)`) and no function-like one with more than 8
    /// (`if (c->inside>8) return false;`). The open chunks are the expansion `tok` came from
    /// and those it was used in, back to the source ([`Self::open_chunks_at`]).
    pub(crate) fn fte_recursion_stops(
        &self,
        run: &mut Run,
        tok: &PpToken,
        name: &str,
        function_like: bool,
    ) -> bool {
        self.open_chunks_at(run, tok.expansion);
        if run
            .open
            .last()
            .and_then(|id| self.expansions.get(id.0))
            .is_some_and(|e| e.name == name)
        {
            return true;
        }
        let open = run.open_names.get(name).copied().unwrap_or(0);
        if function_like {
            open > FTE_MAX_OPEN_CHUNKS
        } else {
            open >= FTE_MAX_OPEN_CHUNKS
        }
    }

    /// Makes [`Run::open`] the chunks open at a token of `expansion`, as
    /// `QCC_PR_IncludeChunkEx` pushes them and the lexer pops them at their ends: tokens are
    /// read in order, so the expansion is the last open one, a new one used in it, or one of
    /// those open, the ones after it having ended.
    fn open_chunks_at(&self, run: &mut Run, expansion: Option<ExpansionId>) {
        loop {
            let top = run.open.last().copied();
            if top == expansion {
                return;
            }
            if let Some(id) = expansion
                && self.expansions.get(id.0).is_some_and(|e| e.parent == top)
            {
                self.open_chunk(run, id);
                return;
            }
            let Some(ended) = run.open.pop() else {
                // Not reached in reading order; take the expansion's chain as it is.
                let mut chain = Vec::new();
                let mut at = expansion;
                while let Some(id) = at {
                    chain.push(id);
                    at = self.expansions.get(id.0).and_then(|e| e.parent);
                }
                for id in chain.into_iter().rev() {
                    self.open_chunk(run, id);
                }
                return;
            };
            if let Some(e) = self.expansions.get(ended.0)
                && let Some(count) = run.open_names.get_mut(&e.name)
            {
                *count = count.saturating_sub(1);
            }
        }
    }

    /// Opens the chunk of the expansion `id`: `QCC_PR_IncludeChunkEx` and its `c->inside++`.
    fn open_chunk(&self, run: &mut Run, id: ExpansionId) {
        run.open.push(id);
        if let Some(e) = self.expansions.get(id.0) {
            let count = run.open_names.entry(e.name.clone()).or_default();
            *count = count.saturating_add(1);
        }
    }

    /// Whether the `(` `open` after the name `name` of a function-like macro opens its
    /// arguments under FTEQCC (FTEF-14.18): `QCC_PR_CheckCompConst` skips only spaces and tabs
    /// after the name (`while(*pr_file_p == ' ' || *pr_file_p == '\t')`) and then needs the
    /// `(` in the same chunk (`if (*pr_file_p == '(')`), so a line break or a comment before
    /// it, or the end of the expansion the name is in, leaves the name as it is.
    pub(crate) fn fte_opens_arguments(&self, name: &PpToken, open: &PpToken) -> bool {
        if name.expansion.is_some() || open.expansion.is_some() {
            return name.expansion == open.expansion;
        }
        name.span.file == open.span.file
            && self
                .sources
                .file(open.span.file)
                .and_then(|file| file.text.get(name.span.span.end..open.span.span.start))
                .is_some_and(|between| between.iter().all(|&b| b == b' ' || b == b'\t'))
    }

    /// Whether FTEQCC copies nothing after `last`, the last token of an argument other than
    /// one copied whole, whose `,` or `)` is `end` (QC-9.16): it copies such an argument a
    /// token at a time, each with what stands before it, up to the last token `QCC_COM_Parse2`
    /// finds, which skips white space and `//` comments but not a `/* */` one. A token of an
    /// expansion has only white space after it, since FTEQCC dropped the body's comments, and
    /// its `end` is in the same expansion (FTEF-14.18).
    fn nothing_copied_after(&self, last: &PpToken, end: &PpToken) -> bool {
        if last.expansion.is_some() {
            return true;
        }
        let between = self
            .sources
            .file(end.span.file)
            .filter(|_| last.span.file == end.span.file)
            .and_then(|file| file.text.get(last.span.span.end..end.span.span.start));
        between.is_some_and(white_and_line_comments)
    }

    /// Whether the body token at `next` touches the one at `last` before it (QC-9.16): nothing
    /// stands between them where they are written, or only what `QCC_PR_PreProcessor_Define`
    /// drops from a body.
    fn touch_in_body(&self, last: FileSpan, next: FileSpan) -> bool {
        if last.file != next.file {
            return false;
        }
        self.sources
            .file(next.file)
            .and_then(|file| file.text.get(last.span.end..next.span.start))
            .is_some_and(|between| between.is_empty() || dropped_in_body(between))
    }
}

/// Whether `between`, the bytes between two tokens of a macro's body, is only what
/// `QCC_PR_PreProcessor_Define` drops as it stores the body (QC-9.16): each `/* */` comment
/// ("multi-line c style comments become part of the define itself", skipped to its `*/`) and
/// each `\` before a line break ("read over a newline if necessary").
fn dropped_in_body(mut between: &[u8]) -> bool {
    loop {
        if between.is_empty() {
            return true;
        }
        if let Some(comment) = between.strip_prefix(b"/*") {
            let Some(end) = comment.windows(2).position(|pair| pair == b"*/") else {
                return false;
            };
            between = comment.get(end.saturating_add(2)..).unwrap_or_default();
        } else if let Some(rest) = [&b"\\\r\n"[..], b"\\\n", b"\\\r"]
            .iter()
            .find_map(|line_break| between.strip_prefix(*line_break))
        {
            between = rest;
        } else {
            return false;
        }
    }
}

/// `text` after the white space and comments `QCC_PR_LexWhitespace` skips: `qcc_iswhite`
/// bytes, `//` comments to the line break and `/* */` comments (FTEF-14.15).
fn after_lex_whitespace(mut text: &[u8]) -> &[u8] {
    loop {
        let white = text.iter().take_while(|&&b| cond::fte_white(b)).count();
        text = text.get(white..).unwrap_or_default();
        if let Some(comment) = text.strip_prefix(b"//") {
            let end = comment
                .iter()
                .position(|&b| b == b'\n')
                .unwrap_or(comment.len());
            text = comment.get(end..).unwrap_or_default();
        } else if let Some(comment) = text.strip_prefix(b"/*") {
            let end = comment
                .windows(2)
                .position(|pair| pair == b"*/")
                .map_or(comment.len(), |end| end.saturating_add(2));
            text = comment.get(end..).unwrap_or_default();
        } else {
            return text;
        }
    }
}

/// Whether `text` is only white space (`qcc_iswhite`) and `//` comments, which
/// `QCC_COM_Parse2` skips (QC-9.16).
fn white_and_line_comments(mut text: &[u8]) -> bool {
    loop {
        let white = text.iter().take_while(|&&b| cond::fte_white(b)).count();
        text = text.get(white..).unwrap_or_default();
        let Some(comment) = text.strip_prefix(b"//") else {
            return text.is_empty();
        };
        let end = comment
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(comment.len());
        text = comment.get(end..).unwrap_or_default();
    }
}

/// Whether `byte` ends the word `QCC_PR_CheckCompConst` reads as a macro's name
/// (FTEF-14.13): a NUL, white space (`qcc_iswhite`: space, CR, LF, tab and vertical tab) or
/// one of `)(+-*/|&=^~[]"{};:<>,.#`.
pub(crate) fn ends_word(byte: u8) -> bool {
    const STOPS: &[u8] = b")(+-*/|&=^~[]\"{};:<>,.#";
    byte == 0 || cond::fte_white(byte) || STOPS.contains(&byte)
}
