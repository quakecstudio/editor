// SPDX-License-Identifier: GPL-3.0-or-later

//! Line diffs. [`script`] is the shortest edit script from one list of lines to another,
//! Myers' (E. W. Myers, "An O(ND) Difference Algorithm and Its Variations", Algorithmica 1,
//! 1986), the one git and most editors use, run on the lines between the common first and last
//! lines. Past [`MAX_EDITS`] edits it gives up on a minimal script and gives the whole middle
//! as deleted and inserted, so a caller never waits on a huge diff. [`hunks`] groups a script
//! into the runs of changed lines a reviewer accepts or rejects one by one.

/// The most line insertions and deletions the minimal search tries before giving the middle
/// as replaced.
pub const MAX_EDITS: usize = 2_000;

/// One step of an edit script: the next line of both lists is the same, or the next line of
/// the first is deleted, or the next line of the second is inserted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// The next line of both is the same.
    Equal,
    /// The next line of the first is deleted.
    Delete,
    /// The next line of the second is inserted.
    Insert,
}

/// A script that turns `a` into `b`, as short as one can be within [`MAX_EDITS`] edits: as
/// many [`Op::Equal`] and [`Op::Delete`] as `a` has lines, and as many [`Op::Equal`] and
/// [`Op::Insert`] as `b` has.
#[must_use]
pub fn script<T: PartialEq>(a: &[T], b: &[T]) -> Vec<Op> {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
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
    let middle = shortest(mid_a, mid_b).unwrap_or_else(|| {
        let mut ops = vec![Op::Delete; mid_a.len()];
        ops.extend(std::iter::repeat_n(Op::Insert, mid_b.len()));
        ops
    });
    let mut ops = vec![Op::Equal; prefix];
    ops.extend(middle);
    ops.extend(std::iter::repeat_n(Op::Equal, suffix));
    ops
}

/// One run of changed lines: the lines `old` of the first list replaced by the lines `new` of
/// the second, each a range of line indices; either may be empty, not both.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// The lines of the first list it replaces.
    pub old: core::ops::Range<usize>,
    /// The lines of the second list it puts in their place.
    pub new: core::ops::Range<usize>,
}

/// The hunks of `ops`, a script from [`script`]: each maximal run of deletions and insertions,
/// in order.
#[must_use]
pub fn hunks(ops: &[Op]) -> Vec<Hunk> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0_usize, 0_usize);
    let mut open: Option<Hunk> = None;
    for op in ops {
        match op {
            Op::Equal => {
                out.extend(open.take());
                i = i.saturating_add(1);
                j = j.saturating_add(1);
            }
            Op::Delete => {
                let hunk = open.get_or_insert(Hunk {
                    old: i..i,
                    new: j..j,
                });
                i = i.saturating_add(1);
                hunk.old.end = i;
            }
            Op::Insert => {
                let hunk = open.get_or_insert(Hunk {
                    old: i..i,
                    new: j..j,
                });
                j = j.saturating_add(1);
                hunk.new.end = j;
            }
        }
    }
    out.extend(open);
    out
}

/// The shortest edit script from `a` to `b`, `None` past [`MAX_EDITS`] edits.
fn shortest<T: PartialEq>(a: &[T], b: &[T]) -> Option<Vec<Op>> {
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

#[cfg(test)]
mod tests;
