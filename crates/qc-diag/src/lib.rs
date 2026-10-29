// SPDX-License-Identifier: GPL-3.0-or-later

//! The one QuakeC diagnostic vocabulary.
//!
//! Every stage of the QuakeC front end reports the same kind of thing: an error or a warning,
//! with a code from `docs/spec/language-diagnostics.md`, a message with the names filled in,
//! and the byte span it is about. The lexer, the preprocessor, the parser (`qc-syntax`), the
//! semantic model (`qc-sema`) and, above them, the compiler, the editor, the language server
//! and the command line all produce and consume this one [`Diagnostic`] type, so an
//! as-you-type error in the editor is the same value as the one a full build reports
//! (COMP-2).
//!
//! This crate owns three things and nothing else: [`Span`], a byte range in one file;
//! [`Code`], the diagnostic codes of `docs/spec/language-diagnostics.md` with the format rule
//! QCD-1.1; and [`Diagnostic`], the code, message and primary span of QCD-1.3. It depends on
//! no other crate and sits at the bottom of the stack so every layer can share its one
//! diagnostic type.
//!
//! # Layer
//!
//! L0 formats. It depends on no other crate; the lexer, preprocessor, parser, semantic model
//! and compiler build on it.
//!
//! # Spec sections
//!
//! Implements QCD-1 (the code format and the diagnostic shape) and names the codes of QCD-2
//! to QCD-8 of `docs/spec/language-diagnostics.md`. Written from the spec, not ported.

mod code;

pub use code::{Code, Severity};

use std::ops::Range;

/// A byte range in one source file: `start` is the first byte, `end` one past the last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Span {
    /// Offset of the first byte.
    pub start: usize,
    /// Offset one past the last byte.
    pub end: usize,
}

impl Span {
    /// The span from `start` to `end`.
    #[must_use]
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The span as a range, for slicing the source with `get`.
    #[must_use]
    pub const fn range(self) -> Range<usize> {
        self.start..self.end
    }

    /// Whether `other` lies entirely inside this span.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// Whether the byte at `offset` is in this span: `[start, end)`, or exactly `start` for an
    /// empty span, so a cursor at the first byte of a zero-width name still hits.
    #[must_use]
    pub const fn covers(self, offset: usize) -> bool {
        if self.start == self.end {
            offset == self.start
        } else {
            offset >= self.start && offset < self.end
        }
    }

    /// The two spans joined: from this one's start to `last`'s end.
    #[must_use]
    pub const fn to(self, last: Self) -> Self {
        Self::new(self.start, last.end)
    }

    /// How many bytes it holds; 0 for an empty or inverted span.
    #[must_use]
    pub const fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether it holds no byte.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// An error or a warning from any stage of the QuakeC front end.
///
/// The one diagnostic type the lexer, preprocessor, parser, semantic model, compiler, editor,
/// language server and command line all share, so a diagnostic raised while typing is equal
/// to the one a full build raises (COMP-2). Its fields are public and it has no constructor;
/// build one with a struct literal.
///
/// Implements QCD-1.3: a diagnostic carries its code, its message with the names filled in,
/// and a primary span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Its code (QCD-1.1).
    pub code: Code,
    /// The message, with the names filled in.
    pub message: String,
    /// The primary span (QCD-1.3).
    pub span: Span,
}

impl Diagnostic {
    /// Whether this diagnostic is an error rather than a warning (QCD-1.1).
    #[must_use]
    pub const fn is_error(&self) -> bool {
        self.code.is_error()
    }

    /// Whether this diagnostic is an error or a warning (QCD-1.1).
    #[must_use]
    pub const fn severity(&self) -> Severity {
        self.code.severity()
    }
}

#[cfg(test)]
mod tests;
