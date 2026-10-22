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
//! model read those words as FTEQCC does, and `flags`, the flags an `extern "C"` block of
//! FTEQCC's `QCC_PR_ParseDefs` saves, sets and restores (FTEF-21.2, FTEF-21.3).

pub mod ast;
mod binum;
pub mod c_scalar;
mod flags;
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
pub use qc_preproc::{CompilerFlags, FlagEdit, Keyword, KeywordEdit, Keywords, Priorities};
pub use tree::{GreenElement, GreenNode, GreenToken, SyntaxElement, SyntaxNode, SyntaxToken};

use ast::AstNode as _;

/// The result of parsing one file: its tree and every diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parse {
    green: GreenNode,
    diagnostics: Vec<Diagnostic>,
    /// The compiler flags the tokens were lexed and read with, each from the byte it holds from
    /// (FTEF-12.12, FTEF-21.1), the first at byte 0.
    flags: Vec<(usize, CompilerFlags)>,
    /// The keywords the parser read each token with, each from the byte it holds from
    /// (FTEF-20.1): the preprocessor's marks and the changes the definitions made.
    keywords: Vec<(usize, Keywords)>,
    /// The tokens of a preprocessor's text the parser lexed with other flags than the
    /// preprocessor did, those of an `extern` block (FTEF-21.2).
    relexed: Vec<Span>,
}

impl Parse {
    /// The compiler flags the tokens were lexed and read with (FTEF-12.12, FTEF-21.1): each the
    /// byte it holds from and the flags, in order, the first at byte 0: the changes
    /// [`parse_preprocessed_flagged`] was given and those of the `extern` blocks the parser read.
    #[must_use]
    pub fn flags(&self) -> &[(usize, CompilerFlags)] {
        &self.flags
    }

    /// The compiler flags in force at the token at byte `at` (FTEF-21.1).
    #[must_use]
    pub fn flags_at(&self, at: usize) -> CompilerFlags {
        CompilerFlags::at(&self.flags, at)
    }

    /// The flags the lexer read the token at byte `at` with (FTEF-12.12).
    #[must_use]
    pub fn lex_flags_at(&self, at: usize) -> qc_lexer::LexFlags {
        self.flags_at(at).lex()
    }

    /// The spans of the tokens of a preprocessor's text that the parser lexed with other flags
    /// than the preprocessor's marks give, those of an `extern` block (FTEF-21.2), in order; their
    /// lexical errors and warnings are in [`Self::diagnostics`], and the preprocessor's for them
    /// are not FTEQCC's. Empty for any other text.
    #[must_use]
    pub fn relexed(&self) -> &[Span] {
        &self.relexed
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

/// [`parse_preprocessed`] of text whose compiler flags `flags` changes (FTEF-12.12, FTEF-21.1):
/// each the byte the change holds from and the change, in order, as
/// `qc_preproc::FileOutput::flags` marks them by token, starting from FTEQCC's defaults. The
/// parser lexes the text with them and with the changes its own `extern` blocks make
/// (FTEF-21.2). `macroinstrings` is not read: this text has no macros to look up, and the
/// preprocessor writes a string a `#NAME` wrote to as its bytes (`PpToken::spelling`). Under an
/// FTEQCC dialect the parser reads each keyword with the keywords `keywords` changes, each the
/// byte the change holds from and the change, in order, as `qc_preproc::FileOutput::keywords`
/// marks them by token, starting from FTEQCC's defaults (FTEF-20.1).
#[must_use]
pub fn parse_preprocessed_flagged(
    source: &[u8],
    dialect: Dialect,
    flags: &[(usize, FlagEdit)],
    keywords: &[(usize, KeywordEdit)],
) -> Parse {
    parse_text(source, dialect, input::Text::Preprocessed, flags, keywords)
}

/// How many times a text is parsed at most to find the `extern` blocks it holds (FTEF-21.2).
/// A pass lexes the text with the blocks the pass before it read. The flags a block sets change
/// how its strings and numbers read, not where a token ends, so the second pass reads the
/// blocks the first did; the limit stops a text whose blocks would move from pass to pass,
/// which no flag a block sets makes, and the last pass is kept.
const PASSES: usize = 4;

fn parse_text(
    source: &[u8],
    dialect: Dialect,
    text: input::Text,
    edits: &[(usize, FlagEdit)],
    keywords: &[(usize, KeywordEdit)],
) -> Parse {
    let mut blocks = Vec::new();
    let mut pass = 0_usize;
    loop {
        let marks = flags::marks(edits, &blocks);
        let lex: Vec<(usize, qc_lexer::LexFlags)> =
            marks.iter().map(|&(at, flags)| (at, flags.lex())).collect();
        let input = input::Input::new(source, dialect, text, &lex);
        let mut parser = parser::Parser::new(input, dialect, keywords, &marks);
        parser.source_file();
        pass = pass.saturating_add(1);
        if parser.blocks() == blocks.as_slice() || pass >= PASSES {
            let relexed = if text == input::Text::Preprocessed && !blocks.is_empty() {
                parser.relexed(&flags::marks(edits, &[]))
            } else {
                Vec::new()
            };
            let (green, diagnostics, keywords) = parser.finish();
            return Parse {
                green,
                diagnostics,
                flags: marks,
                keywords,
                relexed,
            };
        }
        blocks = parser.blocks().to_vec();
    }
}
