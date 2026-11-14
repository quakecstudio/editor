// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 1997       Greg Lewis
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: qbsp/brushbsp.cc, BrushGE, BrushesDisjoint, SubtractBrush, ChopBrushes

//! Removing what brushes overlap, ericw-tools' `ChopBrushes` (QBSP-15.30): each pair of solid
//! brushes that may intersect has the one allowed to bite take its part from the other, as long
//! as that leaves fewer pieces.

use super::brush::BspBrush;
use super::tree::Cutter;

/// Whether a bite may leave more than one piece of each brush, `-chopfragment`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fragmentation {
    /// Only bites that leave one of the two whole or in one piece, the default.
    Avoided,
    /// Any bite.
    Allowed,
}

/// What subtracting one brush from another leaves, `SubtractBrush`.
#[derive(Clone, Debug, PartialEq)]
enum Subtracted {
    /// They do not really intersect: the brush is left as it was.
    Untouched,
    /// The pieces outside the other, none when it swallows the brush.
    Pieces(Vec<BspBrush>),
}

impl BspBrush {
    /// Whether this brush may bite `other`, `BrushGE` (QBSP-15.30): not when it comes earlier
    /// in chop order, nor when detail and `other` structural; only when both are solid.
    fn may_bite(&self, other: &Self) -> bool {
        if self.sort_key < other.sort_key {
            return false;
        }
        if self.contents.is_any_detail() && !other.contents.is_any_detail() {
            return false;
        }
        self.contents.is_any_solid() && other.contents.is_any_solid()
    }

    /// Whether the two surely do not intersect, `BrushesDisjoint`: their boxes at most touch,
    /// or a side of one is on the opposite plane of a side of the other.
    fn is_disjoint(&self, other: &Self) -> bool {
        let apart = self
            .bounds
            .mins
            .iter()
            .zip(self.bounds.maxs)
            .zip(other.bounds.mins.iter().zip(other.bounds.maxs))
            .any(|((lo, hi), (olo, ohi))| hi <= *olo || *lo >= ohi);
        apart
            || self.sides.iter().any(|a| {
                other
                    .sides
                    .iter()
                    .any(|b| a.planenum == b.planenum.opposite())
            })
    }
}

impl Cutter<'_> {
    /// What is left of `a` after `b` is taken from it, `SubtractBrush` (QBSP-15.30): `a` cut by
    /// each of `b`'s planes in turn, each part in front kept and the part behind cut further;
    /// untouched when a cut leaves nothing behind. The pieces come last cut first.
    fn subtract(&self, a: &BspBrush, b: &BspBrush) -> Subtracted {
        let mut out = Vec::new();
        let mut inside = a.clone();
        for side in &b.sides {
            let [front, back] = self.split(inside, side.planenum);
            if let Some(front) = front {
                out.push(front);
            }
            match back {
                Some(back) => inside = back,
                None => return Subtracted::Untouched,
            }
        }
        out.reverse();
        Subtracted::Pieces(out)
    }

    /// `brushes` with their overlaps removed, `ChopBrushes` (QBSP-15.30). Each brush is tested
    /// against every one after it: where they may intersect, each that may bite the other is
    /// subtracted from it; a brush swallowed is removed; otherwise the bite leaving fewer pieces
    /// is taken, the second brush's on a tie, its pieces put where the brush was, unless both
    /// leave several and [`Fragmentation::Avoided`]. After each change the test goes on from the
    /// first brush that has not been tested whole.
    pub(crate) fn chop(
        &self,
        brushes: Vec<BspBrush>,
        fragmentation: Fragmentation,
    ) -> Vec<BspBrush> {
        let mut list = brushes;
        let mut b1 = 0_usize;
        'newlist: loop {
            while let Some(first) = list.get(b1) {
                let mut b2 = b1.saturating_add(1);
                while let Some(second) = list.get(b2) {
                    if first.is_disjoint(second) {
                        b2 = b2.saturating_add(1);
                        continue;
                    }
                    let mut sub = None;
                    let mut sub2 = None;
                    if second.may_bite(first) {
                        match self.subtract(first, second) {
                            Subtracted::Untouched => {
                                b2 = b2.saturating_add(1);
                                continue;
                            }
                            Subtracted::Pieces(p) if p.is_empty() => {
                                list.remove(b1);
                                continue 'newlist;
                            }
                            Subtracted::Pieces(p) => sub = Some(p),
                        }
                    }
                    if first.may_bite(second) {
                        match self.subtract(second, first) {
                            Subtracted::Untouched => {
                                b2 = b2.saturating_add(1);
                                continue;
                            }
                            Subtracted::Pieces(p) if p.is_empty() => {
                                list.remove(b2);
                                continue 'newlist;
                            }
                            Subtracted::Pieces(p) => sub2 = Some(p),
                        }
                    }
                    let c1 = sub.as_ref().map_or(usize::MAX, Vec::len);
                    let c2 = sub2.as_ref().map_or(usize::MAX, Vec::len);
                    if sub.is_none() && sub2.is_none() {
                        b2 = b2.saturating_add(1);
                        continue;
                    }
                    if fragmentation == Fragmentation::Avoided && c1 > 1 && c2 > 1 {
                        b2 = b2.saturating_add(1);
                        continue;
                    }
                    if c1 < c2 {
                        let pieces = sub.unwrap_or_default();
                        let count = pieces.len();
                        list.splice(b1..=b1, pieces);
                        b1 = b1.saturating_add(count);
                    } else {
                        list.splice(b2..=b2, sub2.unwrap_or_default());
                    }
                    continue 'newlist;
                }
                b1 = b1.saturating_add(1);
            }
            break;
        }
        list
    }
}

#[cfg(test)]
mod tests;
