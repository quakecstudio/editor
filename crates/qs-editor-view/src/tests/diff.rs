// SPDX-License-Identifier: GPL-3.0-or-later

//! The inline line diff.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "test code: counts of a few lines"
)]

use proptest::prelude::*;

use crate::diff::{Diff, MAX_EDITS};

/// The base text the diff of `current` gives back: each removed run before its line, and each
/// line that is not new.
fn base_of(diff: &Diff, current: &str) -> String {
    let lines: Vec<&str> = current.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        out.extend(diff.removed_before(index).iter().cloned());
        if !diff.is_added(index) {
            out.push((*line).to_owned());
        }
    }
    out.extend(diff.removed_before(lines.len()).iter().cloned());
    out.join("\n")
}

/// The number of lines added and removed.
fn edits(diff: &Diff, current: &str) -> usize {
    let lines = current.split('\n').count();
    let added = (0..lines).filter(|&l| diff.is_added(l)).count();
    let removed: usize = (0..=lines).map(|l| diff.removed_before(l).len()).sum();
    added + removed
}

/// The length of the longest common subsequence of the lines, by dynamic programming.
fn lcs(a: &str, b: &str) -> usize {
    let a: Vec<&str> = a.split('\n').collect();
    let b: Vec<&str> = b.split('\n').collect();
    let mut row = vec![0_usize; b.len() + 1];
    for x in &a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = if x == y { diagonal + 1 } else { up.max(row[j]) };
            diagonal = up;
        }
    }
    row[b.len()]
}

#[test]
fn an_unchanged_text_has_no_diff() {
    let diff = Diff::new("a\nb", "a\nb");
    assert!(diff.is_empty());
    assert!(!diff.is_added(0));
    assert!(diff.removed_before(0).is_empty());
    assert!(!diff.is_added(99), "past the end");
}

#[test]
fn added_changed_and_removed_lines_are_placed() {
    let diff = Diff::new("a\nb\nc\nd", "a\nB\nc\nd\ne");
    assert!(!diff.is_empty());
    assert!(diff.is_added(1));
    assert_eq!(diff.removed_before(1), ["b"]);
    assert!(diff.is_added(4));
    assert!(!diff.is_added(0) && !diff.is_added(2) && !diff.is_added(3));
    let removed_at_end = Diff::new("a\nb", "a");
    assert_eq!(removed_at_end.removed_before(1), ["b"]);
    let only_removed = Diff::new("a\nb\nc", "a\nc");
    assert_eq!(only_removed.removed_before(1), ["b"]);
    assert!(!only_removed.is_added(1));
}

#[test]
fn past_the_edit_limit_the_middle_is_shown_replaced() {
    let base: Vec<String> = (0..=MAX_EDITS).map(|i| format!("a{i}")).collect();
    let current: Vec<String> = (0..=MAX_EDITS).map(|i| format!("b{i}")).collect();
    let (base, current) = (base.join("\n"), current.join("\n"));
    let diff = Diff::new(&base, &current);
    assert_eq!(base_of(&diff, &current), base);
    assert!((0..=MAX_EDITS).all(|l| diff.is_added(l)));
    assert_eq!(diff.removed_before(0).len(), MAX_EDITS + 1);
}

proptest! {
    // The diff gives the base back, and is a minimal one: as many edits as the lines the two
    // texts do not share.
    #[test]
    fn the_diff_is_minimal_and_gives_the_base_back(
        base in proptest::collection::vec("[abc]", 0..12),
        current in proptest::collection::vec("[abc]", 0..12),
    ) {
        let (base, current) = (base.join("\n"), current.join("\n"));
        let diff = Diff::new(&base, &current);
        prop_assert_eq!(base_of(&diff, &current), base.clone());
        let shared = lcs(&base, &current);
        let expected = base.split('\n').count() + current.split('\n').count() - 2 * shared;
        prop_assert_eq!(edits(&diff, &current), expected);
    }
}
