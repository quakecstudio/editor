// SPDX-License-Identifier: GPL-3.0-or-later

//! The vanilla QuakeC parser.
//!
//! [`parse`] turns one source file into a lossless concrete syntax tree: every byte of the
//! file is in exactly one token, comments, whitespace, directive lines and bytes the lexer
//! skipped included, so the tree gives back the file unchanged. The parser is hand-written
//! recursive descent over the grammar of id's `qcc` (`docs/spec/language.md`, QC-10 to
//! QC-15), and it never fails: an error becomes a diagnostic and an error node, and parsing
//! resumes at the next statement or declaration (QCD-1.4). The tree is in the two layers of
//! rust-analyzer's `rowan` ([`GreenNode`], [`SyntaxNode`]), with a typed view in [`ast`].
//!
//! The tree is of the file as written: macros are not expanded and both sides of a
//! conditional are parsed. Directive lines (QC-9.1) and `$frame` lines (QC-8.2) are kept as
//! trivia; frame macro references are [`SyntaxKind::FrameRef`] literals, resolved later.
//!
//! This crate is written from the spec, not ported.

pub mod ast;
mod input;
mod kind;
mod parser;
pub mod recovery;
mod tree;

#[cfg(test)]
mod tests;

pub use kind::SyntaxKind;
pub use parser::{LIMIT_MAX_PARMS, LIMIT_QCC_MAX_NAME, QC_MAX_NESTING};
pub use qc_lexer::{Code, Diagnostic, Span};
pub use tree::{GreenElement, GreenNode, GreenToken, SyntaxElement, SyntaxNode, SyntaxToken};

use ast::AstNode as _;

/// The result of parsing one file: its tree and every diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parse {
    green: GreenNode,
    diagnostics: Vec<Diagnostic>,
}

impl Parse {
    /// The root of the tree, a [`SyntaxKind::SourceFile`] node.
    #[must_use]
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// The typed view of the root.
    #[must_use]
    pub fn tree(&self) -> Option<ast::SourceFile> {
        ast::SourceFile::cast(self.syntax())
    }

    /// The green tree.
    #[must_use]
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// Lexical and syntax diagnostics, in source order.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// The diagnostics that are errors.
    pub fn errors(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter().filter(|d| d.code.is_error())
    }
}

/// Parses one source file.
///
/// Implements the syntax of QC-10, QC-11, QC-13, QC-14 and QC-15, with the divergences
/// QC-27.3, QC-27.7, QC-27.8, QC-27.9, QC-27.10 and QC-27.15, and reports the syntax errors
/// of QCD-3 and QCD-6. Never fails, and the tree's text is `source`, byte for byte.
#[must_use]
pub fn parse(source: &[u8]) -> Parse {
    let mut parser = parser::Parser::new(input::Input::new(source));
    parser.source_file();
    let (green, diagnostics) = parser.finish();
    Parse { green, diagnostics }
}
