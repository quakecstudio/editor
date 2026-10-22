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
//! This crate is written from the spec, not ported, but for [`c_scalar`], the loop over C
//! type words of FTEQCC's `QCC_PR_ParseType`, which is ported so the parser and the semantic
//! model read those words as FTEQCC does.

pub mod ast;
mod binum;
pub mod c_scalar;
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
pub use qc_preproc::{Keyword, KeywordEdit, Keywords};
pub use tree::{GreenElement, GreenNode, GreenToken, SyntaxElement, SyntaxNode, SyntaxToken};

use ast::AstNode as _;

/// The result of parsing one file: its tree and every diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parse {
    green: GreenNode,
    diagnostics: Vec<Diagnostic>,
    /// The compiler flags the tokens were lexed with, each from the byte it holds from
    /// (FTEF-12.12); empty for the defaults.
    lex_flags: Vec<(usize, qc_lexer::LexFlags)>,
    /// The keywords the parser read each token with, each from the byte it holds from
    /// (FTEF-20.1): the preprocessor's marks and the changes the definitions made.
    keywords: Vec<(usize, Keywords)>,
}

impl Parse {
    /// The compiler flags the tokens were lexed with (FTEF-12.12): each the byte it holds from
    /// and the flags, in order, as [`parse_preprocessed_flagged`] was given them; the text before
    /// the first holds the defaults.
    #[must_use]
    pub fn lex_flags(&self) -> &[(usize, qc_lexer::LexFlags)] {
        &self.lex_flags
    }

    /// The compiler flags the token at byte `at` was lexed with (FTEF-12.12).
    #[must_use]
    pub fn lex_flags_at(&self, at: usize) -> qc_lexer::LexFlags {
        qc_lexer::LexFlags::at(&self.lex_flags, at)
    }

    /// The keywords the parser read the tokens with (FTEF-20.1): each the byte they hold from
    /// and the keywords, in order; the text before the first holds FTEQCC's defaults.
    #[must_use]
    pub fn keywords(&self) -> &[(usize, Keywords)] {
        &self.keywords
    }

    /// The keywords the parser read the token at byte `at` with (FTEF-20.1): FTEQCC's defaults
    /// unless a mark [`parse_preprocessed_flagged`] was given, or a definition named as a
    /// keyword, changed them before it.
    #[must_use]
    pub fn keywords_at(&self, at: usize) -> Keywords {
        Keywords::at(&self.keywords, at)
    }

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
    parse_text(source, dialect, input::Text::Source, &[], &[])
}

/// Parses a module's source in source form (`docs/spec/project-file.md` PRJ-4.4), in `dialect`:
/// as [`parse_with`], except that under an FTEQCC dialect a first line that starts with `#!` is
/// passed over, as FTEQCC's `QCC_main` passes over it before it reads the source (PRJ-4.10), its
/// bytes trivia the lexer skipped. Never fails, and the tree's text is `source`, byte for byte.
#[must_use]
pub fn parse_module_source(source: &[u8], dialect: Dialect) -> Parse {
    parse_text(source, dialect, input::Text::ModuleSource, &[], &[])
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
    parse_text(source, dialect, input::Text::Preprocessed, &[], &[])
}

/// [`parse_preprocessed`] of text whose tokens FTEQCC lexed under the compiler flags `flags`
/// gives (FTEF-12.12): each the byte the flags hold from and the flags, in order, as
/// `qc_preproc::FileOutput::lex_flags` marks them by token. The text before the first holds the
/// default flags. `macroinstrings` is not read: this text has no macros to look up, and the
/// preprocessor writes a string a `#NAME` wrote to as its bytes (`PpToken::spelling`). Under an
/// FTEQCC dialect the parser reads each keyword with the keywords `keywords` changes, each the
/// byte the change holds from and the change, in order, as `qc_preproc::FileOutput::keywords`
/// marks them by token, starting from FTEQCC's defaults (FTEF-20.1).
#[must_use]
pub fn parse_preprocessed_flagged(
    source: &[u8],
    dialect: Dialect,
    flags: &[(usize, qc_lexer::LexFlags)],
    keywords: &[(usize, KeywordEdit)],
) -> Parse {
    parse_text(source, dialect, input::Text::Preprocessed, flags, keywords)
}

fn parse_text(
    source: &[u8],
    dialect: Dialect,
    text: input::Text,
    flags: &[(usize, qc_lexer::LexFlags)],
    keywords: &[(usize, KeywordEdit)],
) -> Parse {
    let input = input::Input::new(source, dialect, text, flags);
    let mut parser = parser::Parser::new(input, dialect, keywords);
    parser.source_file();
    let (green, diagnostics, keywords) = parser.finish();
    Parse {
        green,
        diagnostics,
        lex_flags: flags.to_vec(),
        keywords,
    }
}
