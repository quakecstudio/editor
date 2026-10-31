// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks that the parser's error recovery is local (LANG-2).
//!
//! [`check_deletions`] deletes each token of a file in turn, parses what is left and checks
//! that the damage stays where the token was: the tree still gives back the text, every
//! declaration before the one that lost the token parses as before, every declaration from
//! two after it onwards parses as before, and every error is between them. The tests run it
//! on hand-written sources and `cargo xtask parse-corpus` on the corpus, and
//! [`check_deletions_with`] in a chosen dialect.

use std::fmt;

use crate::ast::{AstNode as _, SourceFile};
use crate::{Code, Dialect, GreenNode, Parse, Span, SyntaxKind, SyntaxNode, parse_with};

/// What [`check_deletions`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeletionReport {
    /// How many deletions were tried.
    pub deletions: usize,
    /// How many left a program with no error at all.
    pub silent: usize,
    /// Each deletion whose damage was not local.
    pub failures: Vec<DeletionFailure>,
}

/// A deletion whose damage was not local (LANG-2): the token deleted and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeletionFailure {
    /// The deleted token's span in the original source.
    pub span: Span,
    /// Its text.
    pub token: String,
    /// What went wrong.
    pub fault: RecoveryFault,
}

impl fmt::Display for DeletionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "deleting {}..{} {:?}: {}",
            self.span.start, self.span.end, self.token, self.fault
        )
    }
}

/// Why one edited source's damage was not local (LANG-2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecoveryFault {
    /// The tree does not give back the text.
    NotLossless,
    /// The root is not a source file.
    NotSourceFile,
    /// The item at this index, before the damaged one, changed.
    EarlierChanged(usize),
    /// The item at this index, more than one after the damaged one, changed.
    LaterChanged(usize),
    /// An error is outside the damaged items' span.
    ErrorOutside {
        /// Its code.
        code: Code,
        /// Its message.
        message: String,
        /// Where it starts.
        at: usize,
        /// The damaged items' span, from and to.
        window: (usize, usize),
    },
}

impl fmt::Display for RecoveryFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotLossless => f.write_str("the tree does not give back the text"),
            Self::NotSourceFile => f.write_str("the root is not a source file"),
            Self::EarlierChanged(item) => {
                write!(f, "declaration {item} before the deletion changed")
            }
            Self::LaterChanged(item) => write!(
                f,
                "declaration {item} changed, more than one after the deletion"
            ),
            Self::ErrorOutside {
                code,
                message,
                at,
                window: (start, end),
            } => write!(
                f,
                "error {} {message} at {at} is outside {start}..{end}",
                code.as_str()
            ),
        }
    }
}

/// A top-level item: a declaration, one of the FTEQCC dialect's file-level constructs, or a
/// run of tokens that is not one.
pub(crate) fn items(root: &SyntaxNode) -> Vec<SyntaxNode> {
    SourceFile::cast(root.clone())
        .map(|file| file.items())
        .unwrap_or_default()
        .into_iter()
        .filter(|n| {
            matches!(
                n.kind(),
                SyntaxKind::Declaration
                    | SyntaxKind::Error
                    | SyntaxKind::TypedefDecl
                    | SyntaxKind::EnumDecl
                    | SyntaxKind::StructDecl
                    | SyntaxKind::ClassDecl
                    | SyntaxKind::AccessorDecl
            )
        })
        .collect()
}

pub(crate) fn greens(items: &[SyntaxNode]) -> Vec<GreenNode> {
    items.iter().map(|n| n.green().clone()).collect()
}

/// The spans of the tokens a deletion may remove: every token the parser read, so not
/// trivia and not the inside of a directive line.
fn deletable(root: &SyntaxNode) -> Vec<Span> {
    root.tokens()
        .filter(|t| !t.kind().is_trivia())
        .filter(|t| {
            !t.parent()
                .ancestors()
                .chain(std::iter::once(t.parent()))
                .any(|n| n.kind() == SyntaxKind::Directive)
        })
        .map(|t| t.span())
        .collect()
}

/// Deletes each token of `source` in turn and checks that the damage is local.
#[must_use]
pub fn check_deletions(source: &[u8]) -> DeletionReport {
    check_deletions_with(source, Dialect::Vanilla)
}

/// [`check_deletions`] with `source` and each edited copy parsed in `dialect`.
#[must_use]
pub fn check_deletions_with(source: &[u8], dialect: Dialect) -> DeletionReport {
    let original = parse_with(source, dialect);
    let root = original.syntax();
    let before = items(&root);
    let before_greens = greens(&before);
    let mut report = DeletionReport::default();
    for span in deletable(&root) {
        let Some(item) = before.iter().position(|n| n.span().contains(span)) else {
            continue;
        };
        let mut edited = Vec::with_capacity(source.len());
        edited.extend_from_slice(source.get(..span.start).unwrap_or_default());
        edited.extend_from_slice(source.get(span.end..).unwrap_or_default());
        report.deletions = report.deletions.saturating_add(1);
        match check_one(&edited, dialect, &before_greens, item) {
            Ok(0) => report.silent = report.silent.saturating_add(1),
            Ok(_) => {}
            Err(fault) => report.failures.push(DeletionFailure {
                span,
                token: String::from_utf8_lossy(source.get(span.range()).unwrap_or_default())
                    .into_owned(),
                fault,
            }),
        }
    }
    report
}

/// Checks one edited source whose deleted token was in item `damaged` of the original, and
/// returns how many errors it has.
pub(crate) fn check_one(
    edited: &[u8],
    dialect: Dialect,
    before: &[GreenNode],
    damaged: usize,
) -> Result<usize, RecoveryFault> {
    let parsed: Parse = parse_with(edited, dialect);
    let root = parsed.syntax();
    if root.text() != edited {
        return Err(RecoveryFault::NotLossless);
    }
    if SourceFile::cast(root.clone()).is_none() {
        return Err(RecoveryFault::NotSourceFile);
    }
    let after_nodes = items(&root);
    let after = greens(&after_nodes);
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    if prefix < damaged {
        return Err(RecoveryFault::EarlierChanged(prefix));
    }
    let suffix = before
        .iter()
        .rev()
        .zip(after.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let kept_from = damaged.saturating_add(2);
    if before.len().saturating_sub(suffix) > kept_from {
        let changed = before.len().saturating_sub(suffix).saturating_sub(1);
        return Err(RecoveryFault::LaterChanged(changed));
    }
    let window_start = after_nodes
        .get(damaged.min(prefix))
        .map_or(0, |n| n.span().start);
    let window_end = after_nodes
        .len()
        .checked_sub(suffix)
        .and_then(|first_kept| after_nodes.get(first_kept))
        .map_or(edited.len(), |n| n.span().start);
    let mut count = 0_usize;
    for error in parsed.errors() {
        count = count.saturating_add(1);
        if error.span.start < window_start || error.span.start > window_end {
            return Err(RecoveryFault::ErrorOutside {
                code: error.code,
                message: error.message.clone(),
                at: error.span.start,
                window: (window_start, window_end),
            });
        }
    }
    Ok(count)
}
