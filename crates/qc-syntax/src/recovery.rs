// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks that the parser's error recovery is local (LANG-2).
//!
//! [`check_deletions`] deletes each token of a file in turn, parses what is left and checks
//! that the damage stays where the token was: the tree still gives back the text, every
//! declaration before the one that lost the token parses as before, every declaration from
//! two after it onwards parses as before, and every error is between them. The tests run it
//! on hand-written sources and `cargo xtask parse-corpus` on the corpus.

use crate::ast::{AstNode as _, SourceFile};
use crate::{GreenNode, Parse, Span, SyntaxKind, SyntaxNode, parse};

/// What [`check_deletions`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeletionReport {
    /// How many deletions were tried.
    pub deletions: usize,
    /// How many left a program with no error at all.
    pub silent: usize,
    /// One line for each deletion whose damage was not local.
    pub failures: Vec<String>,
}

/// A top-level item: a declaration or a run of tokens that is not one.
fn items(root: &SyntaxNode) -> Vec<SyntaxNode> {
    root.children()
        .filter(|n| matches!(n.kind(), SyntaxKind::Declaration | SyntaxKind::Error))
        .collect()
}

fn greens(items: &[SyntaxNode]) -> Vec<GreenNode> {
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
    let original = parse(source);
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
        match check_one(&edited, &before_greens, item) {
            Ok(0) => report.silent = report.silent.saturating_add(1),
            Ok(_) => {}
            Err(failure) => report.failures.push(format!(
                "deleting {}..{} {:?}: {failure}",
                span.start,
                span.end,
                String::from_utf8_lossy(source.get(span.range()).unwrap_or_default())
            )),
        }
    }
    report
}

/// Checks one edited source whose deleted token was in item `damaged` of the original, and
/// returns how many errors it has.
fn check_one(edited: &[u8], before: &[GreenNode], damaged: usize) -> Result<usize, String> {
    let parsed: Parse = parse(edited);
    let root = parsed.syntax();
    if root.text() != edited {
        return Err("the tree does not give back the text".to_owned());
    }
    if SourceFile::cast(root.clone()).is_none() {
        return Err("the root is not a source file".to_owned());
    }
    let after_nodes = items(&root);
    let after = greens(&after_nodes);
    let prefix = before
        .iter()
        .zip(&after)
        .take_while(|(a, b)| a == b)
        .count();
    if prefix < damaged {
        return Err(format!("declaration {prefix} before the deletion changed"));
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
        return Err(format!(
            "declaration {changed} changed, more than one after the deletion"
        ));
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
            return Err(format!(
                "error {} {} at {} is outside {window_start}..{window_end}",
                error.code.as_str(),
                error.message,
                error.span.start
            ));
        }
    }
    Ok(count)
}
