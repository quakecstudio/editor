// SPDX-License-Identifier: GPL-3.0-or-later

//! The line diff an editor shows inline: which lines of the text are new since a base text,
//! and which lines of the base were removed before each line. The edit script is
//! [`qs_diff::script`], Myers' diff within [`MAX_EDITS`] edits.

use std::collections::BTreeMap;

pub use qs_diff::MAX_EDITS;
use qs_diff::{Op, script};

/// What changed between a base text and the current one, line by line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    /// For each line of the current text, whether it is new.
    added: Vec<bool>,
    /// The base's removed lines, by the current line they were removed before; the line count
    /// for lines removed at the end.
    removed: BTreeMap<usize, Vec<String>>,
}

impl Diff {
    /// The diff from `base` to `current`, each split at `\n`.
    #[must_use]
    pub fn new(base: &str, current: &str) -> Self {
        let a: Vec<&str> = base.split('\n').collect();
        let b: Vec<&str> = current.split('\n').collect();
        let mut diff = Self {
            added: vec![false; b.len()],
            removed: BTreeMap::new(),
        };
        let (mut i, mut j) = (0_usize, 0_usize);
        for op in script(&a, &b) {
            match op {
                Op::Equal => {
                    i = i.saturating_add(1);
                    j = j.saturating_add(1);
                }
                Op::Delete => {
                    if let Some(line) = a.get(i) {
                        diff.removed.entry(j).or_default().push((*line).to_owned());
                    }
                    i = i.saturating_add(1);
                }
                Op::Insert => {
                    if let Some(added) = diff.added.get_mut(j) {
                        *added = true;
                    }
                    j = j.saturating_add(1);
                }
            }
        }
        diff
    }

    /// Whether line `line` of the current text is new.
    #[must_use]
    pub fn is_added(&self, line: usize) -> bool {
        self.added.get(line).copied().unwrap_or(false)
    }

    /// The base's lines removed just before line `line` of the current text, or at the end
    /// when `line` is the line count.
    #[must_use]
    pub fn removed_before(&self, line: usize) -> &[String] {
        self.removed.get(&line).map_or(&[], Vec::as_slice)
    }

    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && !self.added.contains(&true)
    }
}
