// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2004-2025 FTE's team and its contributors
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from fte-team/fteqw@f937b9d88f71fc4429db5fe56c6a98d922711b2e: engine/qclib/qcc_pr_comp.c, QCC_PR_ParseDefs

//! The compiler flags in force at each token of a file (`docs/spec/dialect-fteqcc.md` FTEF-21):
//! the preprocessor's, which each `#pragma flag` changes, and the changes the parser's own
//! `extern "C"` and `extern "QC"` blocks make, which the preprocessor cannot know of.
//!
//! `QCC_PR_ParseDefs` reads `extern` and a string as a block, and for `"C"` or `"QC"` saves
//! six flags, sets some of them and lexes the next token, then parses the block and restores
//! them (`//restore stuff to original mode.`). The tokens are lexed as the parser asks for them,
//! so a flag holds from the token after the string to the token after the block's last, which
//! the parser had lexed before it restored them. [`marks`] replays both kinds of change in
//! order, from which the parser lexes the text again.

use qc_preproc::{CompilerFlags, FlagEdit, FteFlag};

/// The flags an `extern` block saves and restores: `pbool old_qcfuncs = flag_qcfuncs; pbool
/// old_cpriority = flag_cpriority; pbool old_assumeint = flag_assume_integer; pbool
/// old_assumef64 = flag_assume_double; pbool old_assumevar = flag_assumevar; pbool old_subscope
/// = pr_subscopedlocals;` (FTEF-21.17).
const SAVED: [FteFlag; 6] = [
    FteFlag::Qcfuncs,
    FteFlag::Cpriority,
    FteFlag::AssumeInteger,
    FteFlag::AssumeDouble,
    FteFlag::Assumevar,
    FteFlag::PrSubscopedlocals,
];

/// What an `extern` block's string asks for (FTEF-21.17), compared in any case
/// (`strcasecmp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Linkage {
    /// `"C"`: `qcfuncs` off and `cpriority`, `assumeint`, `assumevar` and `subscope` on.
    C,
    /// `"QC"`: `qcfuncs` on.
    Qc,
    /// Any other string, which sets nothing but is saved and restored all the same.
    Other,
}

impl Linkage {
    /// The linkage the string `text` names, compared as `strcasecmp(pr_token, ...)` compares
    /// it: up to the first NUL, where `pr_token` ends.
    pub(crate) fn of(text: &[u8]) -> Self {
        let end = text.iter().position(|&b| b == 0).unwrap_or(text.len());
        let text = text.get(..end).unwrap_or(text);
        if text.eq_ignore_ascii_case(b"C") {
            Self::C
        } else if text.eq_ignore_ascii_case(b"QC") {
            Self::Qc
        } else {
            Self::Other
        }
    }

    /// `flags` once the block starts.
    fn start(self, flags: CompilerFlags) -> CompilerFlags {
        match self {
            // `flag_qcfuncs = false; flag_cpriority = true; flag_assume_integer = true;
            // flag_assumevar = true; pr_subscopedlocals = true;`, `assumedouble` left as it is.
            Self::C => flags
                .with(FteFlag::Qcfuncs, false)
                .with(FteFlag::Cpriority, true)
                .with(FteFlag::AssumeInteger, true)
                .with(FteFlag::Assumevar, true)
                .with(FteFlag::PrSubscopedlocals, true),
            // `flag_qcfuncs = true;`, the others "refrain from changing".
            Self::Qc => flags.with(FteFlag::Qcfuncs, true),
            Self::Other => flags,
        }
    }
}

/// One `extern` block the parser read: the byte of the token after its string, where its flags
/// start; the byte of the token that followed its last, which FTEQCC lexed with them and looks at
/// once it has restored them; the byte of the token after that, the first lexed with the flags
/// restored (the end of the text where there is none); and what it asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) start: usize,
    pub(crate) lookahead: usize,
    pub(crate) end: usize,
    pub(crate) linkage: Linkage,
}

/// Which flags [`marks`] gives: those each token was lexed with, or those the parser reads while
/// it looks at the token (FTEF-21.1). They differ only at the token after a block, which FTEQCC
/// lexed before it restored the flags and reads after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reading {
    /// The flags `QCC_PR_Lex` lexed each token with.
    Lexed,
    /// The flags the parser and the definitions read at each token.
    Parsed,
}

/// What happens to the flags at a byte, in the order FTEQCC does it there: for the lexer, a
/// block's restore before the token is lexed; a block's start; the `#pragma flag`s in the white
/// space before the token, which `QCC_PR_Lex` runs as it lexes it; and for the parser, a block's
/// restore once the token after it is lexed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Event {
    /// The block that started at this byte ends before the token is lexed.
    LexedEnd { started: std::cmp::Reverse<usize> },
    /// A block starts.
    Start,
    /// The preprocessor's change with this index.
    Edit(usize),
    /// The block that started at this byte ends once the token is lexed.
    ParsedEnd { started: std::cmp::Reverse<usize> },
}

/// The flags in force from each byte of a text on, as `reading` says, in order, the first at byte
/// 0 (FTEF-21.1): the preprocessor's changes `edits`, each from the byte it holds from, with the
/// `blocks` the parser read, each saving the six flags of [`SAVED`] where it starts and restoring
/// them where it ends. A text with no change holds FTEQCC's defaults.
pub(crate) fn marks(
    edits: &[(usize, FlagEdit)],
    blocks: &[Block],
    reading: Reading,
) -> Vec<(usize, CompilerFlags)> {
    let mut events: Vec<(usize, Event, usize)> = Vec::new();
    for (index, &(at, _)) in edits.iter().enumerate() {
        events.push((at, Event::Edit(index), index));
    }
    for (index, block) in blocks.iter().enumerate() {
        events.push((block.start, Event::Start, index));
        let started = std::cmp::Reverse(block.start);
        events.push(match reading {
            Reading::Lexed => (block.end, Event::LexedEnd { started }, index),
            Reading::Parsed => (block.lookahead, Event::ParsedEnd { started }, index),
        });
    }
    events.sort_unstable();
    let mut flags = CompilerFlags::default();
    let mut saved: Vec<(usize, CompilerFlags)> = Vec::new();
    let mut out = vec![(0, flags)];
    for (at, event, index) in events {
        match event {
            Event::Edit(_) => {
                if let Some(&(_, edit)) = edits.get(index) {
                    flags = edit.apply(flags);
                }
            }
            Event::Start => {
                if let Some(block) = blocks.get(index) {
                    saved.push((index, flags));
                    flags = block.linkage.start(flags);
                }
            }
            Event::LexedEnd { .. } | Event::ParsedEnd { .. } => {
                if let Some(at) = saved.iter().rposition(|&(block, _)| block == index) {
                    let (_, before) = saved.remove(at);
                    flags = SAVED
                        .iter()
                        .fold(flags, |now, &flag| now.with(flag, before.has(flag)));
                }
            }
        }
        match out.last_mut() {
            Some(last) if last.0 == at => last.1 = flags,
            _ => out.push((at, flags)),
        }
    }
    out.dedup_by(|later, earlier| later.1 == earlier.1);
    out
}

#[cfg(test)]
mod tests;
