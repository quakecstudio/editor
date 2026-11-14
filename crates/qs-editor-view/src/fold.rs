// SPDX-License-Identifier: GPL-3.0-or-later

//! Code folding (P4-32, UI-16): the regions the text can fold, as the syntax tree gives them,
//! and which of them are folded.
//!
//! The model is VS Code's and Zed's: a region is a range of lines whose first line stays shown
//! with a fold mark and whose lines after it, up to the line its region ends on, are hidden;
//! the last line, which holds a block's closing brace, stays shown. Folding at a line folds the
//! innermost region that starts there or holds it; a caret that lands on a hidden line unfolds
//! the regions that hide it.

use std::collections::BTreeSet;

/// Whether a fold region is folded or open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fold {
    /// Folded: its lines after the first are hidden.
    Folded,
    /// Open.
    Open,
}

/// One foldable region: the lines from `start` to `end`, both from 0. Folded, it hides the
/// lines after `start` and before `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FoldRegion {
    /// The line it starts on, which stays shown.
    pub start: usize,
    /// The line it ends on, which stays shown.
    pub end: usize,
}

impl FoldRegion {
    /// Whether folding it hides any line: it ends two or more lines after it starts.
    #[must_use]
    pub const fn hides_any(self) -> bool {
        self.end > self.start.saturating_add(1)
    }

    /// Whether folded it hides `line`.
    #[must_use]
    pub const fn hides(self, line: usize) -> bool {
        self.start < line && line < self.end
    }

    /// Whether `line` is in it, its first and last lines included.
    #[must_use]
    pub const fn holds(self, line: usize) -> bool {
        self.start <= line && line <= self.end
    }
}

/// The foldable regions and the folded ones.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Folds {
    /// The regions that hide a line when folded, by start and then the widest first.
    regions: Vec<FoldRegion>,
    /// The folded ones.
    folded: BTreeSet<FoldRegion>,
}

impl Folds {
    /// Replaces the foldable regions with `regions`, keeping folded those still among them.
    pub fn set_regions(&mut self, regions: impl IntoIterator<Item = FoldRegion>) {
        let mut regions: Vec<FoldRegion> = regions.into_iter().filter(|r| r.hides_any()).collect();
        regions.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
        regions.dedup();
        self.folded.retain(|folded| regions.contains(folded));
        self.regions = regions;
    }

    /// The foldable regions.
    #[must_use]
    pub fn regions(&self) -> &[FoldRegion] {
        &self.regions
    }

    /// Whether any region is folded.
    #[must_use]
    pub fn any_folded(&self) -> bool {
        !self.folded.is_empty()
    }

    /// The innermost region that starts on `line` or holds it, if any.
    fn innermost(&self, line: usize, folded: Option<bool>) -> Option<FoldRegion> {
        self.regions
            .iter()
            .filter(|r| r.holds(line))
            .filter(|r| folded.is_none_or(|f| self.folded.contains(r) == f))
            .min_by_key(|r| r.end.saturating_sub(r.start))
            .copied()
    }

    /// Folds the innermost unfolded region at `line`; false when there is none.
    pub fn fold_at(&mut self, line: usize) -> bool {
        match self.innermost(line, Some(false)) {
            Some(region) => self.folded.insert(region),
            None => false,
        }
    }

    /// Unfolds the innermost folded region at `line`; false when there is none.
    pub fn unfold_at(&mut self, line: usize) -> bool {
        match self.innermost(line, Some(true)) {
            Some(region) => self.folded.remove(&region),
            None => false,
        }
    }

    /// Folds the region that starts on `line` when it is unfolded, else unfolds it; false when
    /// no region starts there.
    pub fn toggle_start(&mut self, line: usize) -> bool {
        let Some(region) = self.regions.iter().find(|r| r.start == line).copied() else {
            return false;
        };
        if !self.folded.remove(&region) {
            self.folded.insert(region);
        }
        true
    }

    /// Folds every region.
    pub fn fold_all(&mut self) {
        self.folded = self.regions.iter().copied().collect();
    }

    /// Unfolds every region.
    pub fn unfold_all(&mut self) {
        self.folded.clear();
    }

    /// Unfolds every region that hides `line`, so it shows; true when one did.
    pub fn reveal(&mut self, line: usize) -> bool {
        let before = self.folded.len();
        self.folded.retain(|r| !r.hides(line));
        self.folded.len() != before
    }

    /// Whether a folded region hides `line`.
    #[must_use]
    pub fn is_hidden(&self, line: usize) -> bool {
        self.folded.iter().any(|r| r.hides(line))
    }

    /// Whether a region starts on `line`, and if so whether it is folded.
    #[must_use]
    pub fn start_at(&self, line: usize) -> Option<Fold> {
        self.regions.iter().find(|r| r.start == line).map(|r| {
            if self.folded.contains(r) {
                Fold::Folded
            } else {
                Fold::Open
            }
        })
    }

    /// The first line at or after `line` that no fold hides: the end of the widest folded
    /// region hiding it.
    #[must_use]
    pub fn shown_at_or_after(&self, line: usize) -> usize {
        self.folded
            .iter()
            .filter(|r| r.hides(line))
            .map(|r| r.end)
            .max()
            .unwrap_or(line)
    }

    /// The first line at or before `line` that no fold hides: the start of the widest folded
    /// region hiding it.
    #[must_use]
    pub fn shown_at_or_before(&self, line: usize) -> usize {
        self.folded
            .iter()
            .filter(|r| r.hides(line))
            .map(|r| r.start)
            .min()
            .unwrap_or(line)
    }
}

#[cfg(test)]
mod tests {
    use super::{Fold, FoldRegion, Folds};

    fn region(start: usize, end: usize) -> FoldRegion {
        FoldRegion { start, end }
    }

    /// A function on lines 1 to 9 holding a block on lines 3 to 6, and a two-line comment.
    fn folds() -> Folds {
        let mut folds = Folds::default();
        folds.set_regions([region(1, 9), region(3, 6), region(11, 12)]);
        folds
    }

    // No spec statement: P4-32: a region that would hide no line is not foldable; folding
    // hides only the lines between its first and last
    #[test]
    fn a_folded_region_hides_the_lines_between_its_ends() {
        let mut folds = folds();
        assert_eq!(folds.regions(), [region(1, 9), region(3, 6)]);
        assert!(folds.fold_at(4));
        assert!(!folds.is_hidden(3) && folds.is_hidden(4) && folds.is_hidden(5));
        assert!(!folds.is_hidden(6));
        assert_eq!(folds.start_at(3), Some(Fold::Folded));
        assert_eq!(folds.start_at(1), Some(Fold::Open));
        assert_eq!(folds.start_at(2), None);
        assert!(folds.fold_at(4), "then the region around it");
        assert!(folds.is_hidden(2) && folds.is_hidden(8) && !folds.is_hidden(9));
        assert!(!folds.fold_at(4), "nothing more to fold");
        assert_eq!(folds.shown_at_or_after(4), 9);
        assert_eq!(folds.shown_at_or_before(4), 1);
        assert!(folds.unfold_at(1));
        assert!(
            folds.is_hidden(4) && !folds.is_hidden(2),
            "the inner one stays folded"
        );
    }

    // No spec statement: P4-32: toggling, folding and unfolding all, revealing a hidden line,
    // and new regions keeping the folds still among them
    #[test]
    fn toggles_reveal_and_new_regions() {
        let mut folds = folds();
        assert!(folds.toggle_start(3));
        assert!(folds.is_hidden(5));
        assert!(folds.toggle_start(3));
        assert!(!folds.any_folded());
        assert!(!folds.toggle_start(4));
        folds.fold_all();
        assert!(folds.is_hidden(2) && folds.is_hidden(5));
        assert!(folds.reveal(5));
        assert!(!folds.is_hidden(5) && !folds.is_hidden(2));
        assert!(!folds.reveal(5));
        folds.fold_all();
        folds.unfold_all();
        assert!(!folds.any_folded());
        folds.fold_at(4);
        folds.set_regions([region(3, 6), region(20, 30)]);
        assert!(folds.is_hidden(4), "a region still there stays folded");
        folds.set_regions([region(3, 7)]);
        assert!(!folds.any_folded(), "a changed region unfolds");
    }
}
