// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "test code: counts and indices of a few lines"
)]

use proptest::prelude::*;

use super::{Hunk, MAX_EDITS, Op, hunks, script};

/// `b`, rebuilt from `a` and a script from `a` to `b`.
fn apply(a: &[u8], b: &[u8], ops: &[Op]) -> Vec<u8> {
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    for op in ops {
        match op {
            Op::Equal => {
                assert_eq!(a[i], b[j], "an equal step pairs equal lines");
                out.push(a[i]);
                i += 1;
                j += 1;
            }
            Op::Delete => i += 1,
            Op::Insert => {
                out.push(b[j]);
                j += 1;
            }
        }
    }
    assert_eq!(
        (i, j),
        (a.len(), b.len()),
        "the script reads both lists to the end"
    );
    out
}

/// The length of the longest common subsequence, by dynamic programming.
fn lcs(a: &[u8], b: &[u8]) -> usize {
    let mut row = vec![0_usize; b.len() + 1];
    for x in a {
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
fn equal_lists_are_all_equal_steps_and_no_hunks() {
    let ops = script(b"abc", b"abc");
    assert_eq!(ops, [Op::Equal; 3]);
    assert!(hunks(&ops).is_empty());
    assert!(script::<u8>(&[], &[]).is_empty());
}

#[test]
fn hunks_are_the_runs_of_changes() {
    // a b c d e -> a B c e f: b replaced, d deleted, f inserted at the end.
    let ops = script(b"abcde", b"aBcef");
    assert_eq!(
        hunks(&ops),
        [
            Hunk {
                old: 1..2,
                new: 1..2
            },
            Hunk {
                old: 3..4,
                new: 3..3
            },
            Hunk {
                old: 5..5,
                new: 4..5
            },
        ]
    );
    assert_eq!(
        hunks(&script(b"", b"xy")),
        [Hunk {
            old: 0..0,
            new: 0..2
        }]
    );
    assert_eq!(
        hunks(&script(b"xy", b"")),
        [Hunk {
            old: 0..2,
            new: 0..0
        }]
    );
}

#[test]
fn past_max_edits_the_middle_is_replaced_whole() {
    let a: Vec<u32> = (0..=MAX_EDITS)
        .map(|n| u32::try_from(n).unwrap_or(0))
        .collect();
    let b: Vec<u32> = a.iter().map(|n| n + 1_000_000).collect();
    let ops = script(&a, &b);
    assert_eq!(ops.len(), 2 * (MAX_EDITS + 1));
    assert_eq!(
        hunks(&ops),
        [Hunk {
            old: 0..MAX_EDITS + 1,
            new: 0..MAX_EDITS + 1
        }]
    );
}

proptest! {
    // The script turns a into b, and is as short as any: its equal steps are a longest common
    // subsequence.
    #[test]
    fn the_script_is_a_shortest_one(
        a in proptest::collection::vec(0_u8..4, 0..40),
        b in proptest::collection::vec(0_u8..4, 0..40),
    ) {
        let ops = script(&a, &b);
        prop_assert_eq!(apply(&a, &b, &ops), b.clone());
        let equal = ops.iter().filter(|op| **op == Op::Equal).count();
        prop_assert_eq!(equal, lcs(&a, &b));
    }

    // The hunks are in order, never empty on both sides, never touching, and replacing each
    // one's old lines by its new lines turns a into b.
    #[test]
    fn the_hunks_rebuild_the_second_list(
        a in proptest::collection::vec(0_u8..4, 0..40),
        b in proptest::collection::vec(0_u8..4, 0..40),
    ) {
        let found = hunks(&script(&a, &b));
        let mut out = Vec::new();
        let mut at = 0;
        let mut last_new_end = 0;
        for (index, hunk) in found.iter().enumerate() {
            prop_assert!(!hunk.old.is_empty() || !hunk.new.is_empty());
            prop_assert!(index == 0 || hunk.old.start > at, "hunks never touch");
            prop_assert_eq!(hunk.old.start - at, hunk.new.start - last_new_end);
            out.extend_from_slice(&a[at..hunk.old.start]);
            out.extend_from_slice(&b[hunk.new.clone()]);
            at = hunk.old.end;
            last_new_end = hunk.new.end;
        }
        out.extend_from_slice(&a[at..]);
        prop_assert_eq!(out, b);
    }
}
