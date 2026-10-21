// SPDX-License-Identifier: GPL-3.0-or-later

//! The QuakeC parser, for the dialect of id's `qcc` and the FTEQCC dialects.
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
//! conditional are parsed. Directive lines (QC-9.1; under an FTEQCC dialect FTEF-14.51) and
//! `$frame` lines (QC-8.2) are kept as
//! trivia; frame macro references are [`SyntaxKind::FrameRef`] literals, resolved later.
//!
//! This crate is written from the spec, not ported.

pub mod ast;
mod binum;
mod input;
mod kind;
mod parser;
pub mod recovery;
mod tree;

#[cfg(test)]
mod tests;

pub use binum::builtin_binum;
pub use kind::SyntaxKind;
pub use parser::{FTEQCC_MAX_NAME, LIMIT_MAX_PARMS, LIMIT_QCC_MAX_NAME, QC_MAX_NESTING};
pub use qc_lexer::{Code, Diagnostic, Dialect, Span};
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
    parse_with(source, Dialect::Vanilla)
}

/// Parses one source file in `dialect`.
///
/// With [`Dialect::Vanilla`] this is [`parse`] and the tree and diagnostics are exactly what
/// the vanilla parser produces. An FTEQCC dialect ([`Dialect::has_fteqcc_extensions`]) also
/// parses the FTEQCC constructs of `docs/spec/dialect-fteqcc-rerelease.md` (FTE-2 to FTE-16);
/// under the vanilla dialect each keyword construct is reported once as `E0510` "NAME is an
/// FTEQCC extension" (QCD-6.10) and the operator and declaration extensions keep their
/// vanilla diagnostic, so the two profiles are observably different. Never fails, and the
/// tree's text is `source`, byte for byte.
#[must_use]
pub fn parse_with(source: &[u8], dialect: Dialect) -> Parse {
    parse_text(source, dialect, input::Text::Source, &[])
}

/// Parses a module's source in source form (`docs/spec/project-file.md` PRJ-4.4), in `dialect`:
/// as [`parse_with`], except that under an FTEQCC dialect a first line that starts with `#!` is
/// passed over, as FTEQCC's `QCC_main` passes over it before it reads the source (PRJ-4.10), its
/// bytes trivia the lexer skipped. Never fails, and the tree's text is `source`, byte for byte.
#[must_use]
pub fn parse_module_source(source: &[u8], dialect: Dialect) -> Parse {
    parse_text(source, dialect, input::Text::ModuleSource, &[])
}

/// Parses text a preprocessor wrote, in `dialect`: as [`parse_with`], except that the text
/// holds no directive lines, so a `#` first on its line is the token it is and the parser
/// reads it as it reads a `#` anywhere else.
///
/// The preprocessor runs every directive, so a `#` left in its output is code: one after a
/// comment at a line's start (QC-9.1, FTEF-14.52), or one a macro's expansion wrote.
/// Implements QC-9.19. Never fails, and the tree's text is `source`, byte for byte.
#[must_use]
pub fn parse_preprocessed(source: &[u8], dialect: Dialect) -> Parse {
    parse_text(source, dialect, input::Text::Preprocessed, &[])
}

/// [`parse_preprocessed`] of text whose tokens FTEQCC lexed under the compiler flags `flags`
/// gives (FTEF-12.12): each the byte the flags hold from and the flags, in order, as
/// `qc_preproc::FileOutput::lex_flags` marks them by token. The text before the first holds the
/// default flags. `macroinstrings` is not read: the preprocessor's string token already holds the
/// text a `#NAME` in it wrote, and this text has no macros to look up.
#[must_use]
pub fn parse_preprocessed_flagged(
    source: &[u8],
    dialect: Dialect,
    flags: &[(usize, qc_lexer::LexFlags)],
) -> Parse {
    parse_text(source, dialect, input::Text::Preprocessed, flags)
}

fn parse_text(
    source: &[u8],
    dialect: Dialect,
    text: input::Text,
    flags: &[(usize, qc_lexer::LexFlags)],
) -> Parse {
    let input = input::Input::new(source, dialect, text, flags);
    let mut parser = parser::Parser::new(input, dialect);
    parser.source_file();
    let (green, diagnostics) = parser.finish();
    Parse { green, diagnostics }
}
