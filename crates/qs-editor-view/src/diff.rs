// SPDX-License-Identifier: GPL-3.0-or-later

//! The line diff an editor shows inline: which lines of the text are new since a base text,
//! and which lines of the base were removed before each line.
//!
//! The diff is Myers' (E. W. Myers, "An O(ND) Difference Algorithm and Its Variations",
//! Algorithmica 1, 1986), the one git and most editors use, run on the lines between the
//! common first and last lines. Past [`MAX_EDITS`] edits it gives up on a minimal script and
//! shows the whole middle as removed and added, so a frame never waits on a huge diff.

use std::collections::BTreeMap;

/// The most line insertions and deletions the minimal search tries before showing the middle
/// as replaced.
pub const MAX_EDITS: usize = 2_000;

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
        let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let rest_a = a.get(prefix..).unwrap_or_default();
        let rest_b = b.get(prefix..).unwrap_or_default();
        let suffix = rest_a
            .iter()
            .rev()
            .zip(rest_b.iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        let mid_a = rest_a
            .get(..rest_a.len().saturating_sub(suffix))
            .unwrap_or_default();
        let mid_b = rest_b
            .get(..rest_b.len().saturating_sub(suffix))
            .unwrap_or_default();
        let ops = script(mid_a, mid_b).unwrap_or_else(|| {
            let mut ops = vec![Op::Delete; mid_a.len()];
            ops.extend(std::iter::repeat_n(Op::Insert, mid_b.len()));
            ops
        });
        let (mut i, mut j) = (prefix, prefix);
        for op in ops {
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

/// One step of an edit script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Equal,
    Delete,
    Insert,
}

/// The shortest edit script from `a` to `b`, `None` past [`MAX_EDITS`] edits.
fn script(a: &[&str], b: &[&str]) -> Option<Vec<Op>> {
    let n = i64::try_from(a.len()).ok()?;
    let m = i64::try_from(b.len()).ok()?;
    let max = usize::try_from(n.checked_add(m)?).ok()?.min(MAX_EDITS);
    // `trace[d]` holds, for each diagonal k from -d to d, the furthest x reached with d edits,
    // stored at k + d.
    let mut trace: Vec<Vec<i64>> = Vec::new();
    let mut v: Vec<i64> = vec![0];
    for d in 0..=max {
        let d_i = i64::try_from(d).ok()?;
        let mut next = vec![0_i64; d.checked_mul(2)?.checked_add(1)?];
        let mut k = d_i.checked_neg()?;
        while k <= d_i {
            // The previous round's value on diagonal `k`, which it stored at k + d - 1.
            let prev = |k: i64| -> Option<i64> {
                let at = usize::try_from(k.checked_add(d_i)?.checked_sub(1)?).ok()?;
                v.get(at).copied()
            };
            let down = Some(k) == d_i.checked_neg()
                || (k != d_i && prev(k.checked_sub(1)?)? < prev(k.checked_add(1)?)?);
            let mut x = if d == 0 {
                0
            } else if down {
                prev(k.checked_add(1)?)?
            } else {
                prev(k.checked_sub(1)?)?.checked_add(1)?
            };
            let mut y = x.checked_sub(k)?;
            // Follow the diagonal while both lines exist and match; past either end `get` is
            // `None` and the walk stops.
            while let (Some(p), Some(q)) = (
                a.get(usize::try_from(x).ok()?),
                b.get(usize::try_from(y).ok()?),
            ) && p == q
            {
                x = x.checked_add(1)?;
                y = y.checked_add(1)?;
            }
            *next.get_mut(usize::try_from(k.checked_add(d_i)?).ok()?)? = x;
            if x >= n && y >= m {
                trace.push(next);
                return backtrack(&trace, n, m);
            }
            k = k.checked_add(2)?;
        }
        trace.push(next);
        v = trace.last()?.clone();
    }
    None
}

/// The edit script `trace` found, read back from `(n, m)` to the start.
fn backtrack(trace: &[Vec<i64>], n: i64, m: i64) -> Option<Vec<Op>> {
    let mut ops = Vec::new();
    let (mut x, mut y) = (n, m);
    for d in (0..trace.len()).rev() {
        let d_i = i64::try_from(d).ok()?;
        let k = x.checked_sub(y)?;
        if d == 0 {
            ops.extend(std::iter::repeat_n(Op::Equal, usize::try_from(x).ok()?));
            break;
        }
        let previous = trace.get(d.checked_sub(1)?)?;
        let at = |k: i64| -> Option<i64> {
            previous
                .get(usize::try_from(k.checked_add(d_i)?.checked_sub(1)?).ok()?)
                .copied()
        };
        let down = Some(k) == d_i.checked_neg()
            || (k != d_i && at(k.checked_sub(1)?)? < at(k.checked_add(1)?)?);
        let prev_k = if down {
            k.checked_add(1)?
        } else {
            k.checked_sub(1)?
        };
        let prev_x = at(prev_k)?;
        let prev_y = prev_x.checked_sub(prev_k)?;
        while x > prev_x && y > prev_y {
            ops.push(Op::Equal);
            x = x.checked_sub(1)?;
            y = y.checked_sub(1)?;
        }
        ops.push(if down { Op::Insert } else { Op::Delete });
        x = prev_x;
        y = prev_y;
    }
    ops.reverse();
    Some(ops)
}
