// SPDX-License-Identifier: GPL-3.0-or-later

//! What each portal sees: the first-order bound of the leaves it might see (VIS-3), then the
//! portal flow that narrows it (VIS-4), and each leaf's row from its portals' (VIS-4.6).

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::portals::PortalFile;
use crate::winding::{Flip, Plane, VIS_ON_EPSILON, Winding};
use crate::{Level, VisError};

/// Leaves to a word of a row.
const WORD_BITS: usize = 64;
/// Leaves to a byte of a row as vis writes it.
pub(crate) const BYTE_BITS: usize = 8;
/// `WORD_BITS` as a shift.
const WORD_SHIFT: u32 = WORD_BITS.trailing_zeros();
/// How many portals are flowed together, each seeing the others of its batch as not yet
/// flowed, so the result is the same on any number of threads (VIS-4.7).
pub const VIS_FLOW_BATCH: usize = 32;
/// The stack a flow's thread is given: a flow goes as deep as its longest path of leaves.
const FLOW_STACK: usize = 256 << 20;

/// The counts id's vis prints with `-v` and at its end: how often the flow walked into a leaf,
/// tested a portal against a flowed portal's set or a might-see set, and checked, tested and
/// passed a portal (VIS-4.8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VisCounts {
    /// `c_chains`: leaves walked into.
    pub chains: usize,
    /// `c_vistest`: portals tested against a flowed portal's set.
    pub vis_tests: usize,
    /// `c_mighttest`: portals tested against a might-see set.
    pub might_tests: usize,
    /// `c_portalcheck`: portals past the coplanar check.
    pub checks: usize,
    /// `c_portaltest`: portals clipped by the separating planes.
    pub tests: usize,
    /// `c_portalpass`: portals flowed through after them.
    pub passes: usize,
}

impl VisCounts {
    fn add(&mut self, other: Self) {
        for (a, b) in [
            (&mut self.chains, other.chains),
            (&mut self.vis_tests, other.vis_tests),
            (&mut self.might_tests, other.might_tests),
            (&mut self.checks, other.checks),
            (&mut self.tests, other.tests),
            (&mut self.passes, other.passes),
        ] {
            *a = a.saturating_add(b);
        }
    }
}

/// A count raised by one.
fn bump(count: &mut usize) {
    *count = count.saturating_add(1);
}

/// What one portal's flow gives: the leaves it sees, whether it read the might-see set of a
/// portal earlier in its batch, and its counts.
struct Flowed {
    seen: Leaves,
    read_earlier: bool,
    counts: VisCounts,
}

/// A set of leaves, one bit each.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Leaves(Vec<u64>);

impl Leaves {
    pub(crate) fn none(leafs: usize) -> Self {
        Self(vec![0; leafs.div_ceil(WORD_BITS)])
    }

    /// The word holding `leaf`'s bit, and the bit.
    fn place(leaf: usize) -> (usize, u64) {
        (leaf >> WORD_SHIFT, 1 << (leaf & (WORD_BITS - 1)))
    }

    pub(crate) fn has(&self, leaf: usize) -> bool {
        let (word, bit) = Self::place(leaf);
        self.0.get(word).is_some_and(|word| word & bit != 0)
    }

    pub(crate) fn add(&mut self, leaf: usize) -> bool {
        let (word, bit) = Self::place(leaf);
        let Some(word) = self.0.get_mut(word) else {
            return false;
        };
        let new = *word & bit == 0;
        *word |= bit;
        new
    }

    fn or(&mut self, other: &Self) {
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a |= *b;
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.0.iter().map(|w| w.count_ones() as usize).sum()
    }

    /// The row as vis writes it: `(leafs + 7) >> 3` bytes, leaf `i` bit `i & 7` of byte
    /// `i >> 3` (VIS-2.5).
    pub(crate) fn row(&self, leafs: usize) -> Vec<u8> {
        let mut bytes: Vec<u8> = self.0.iter().flat_map(|w| w.to_le_bytes()).collect();
        bytes.truncate(leafs.div_ceil(BYTE_BITS));
        bytes
    }
}

/// One portal's flow under way: the base portal's plane, the portals flowed so far, those of
/// its batch one thread would have flowed first, whether it read one of their might-see sets,
/// the leaves seen and the path of leaves walked.
struct Walk<'a> {
    base: Plane,
    done: &'a [Option<Leaves>],
    earlier: &'a [usize],
    read_earlier: bool,
    seen: Leaves,
    path: Vec<usize>,
    counts: VisCounts,
}

/// The portals and what each might see.
pub(crate) struct Flow<'a> {
    file: &'a PortalFile,
    might: Vec<Leaves>,
    level: Level,
}

/// One step of a flow's path: the portal's plane, the source and the pass windings so far
/// (no pass on the first step) and the leaves that might still be seen.
struct Step {
    plane: Plane,
    source: Winding,
    pass: Option<Winding>,
    might: Leaves,
}

impl<'a> Flow<'a> {
    /// `BasePortalVis`: each portal's might-see set, a flood from the leaf it looks into
    /// through the portals that face it (VIS-3).
    pub(crate) fn new(file: &'a PortalFile, level: Level) -> Self {
        let portals = &file.portals;
        let mut might = Vec::with_capacity(portals.len());
        for (i, p) in portals.iter().enumerate() {
            let facing: Vec<bool> = portals
                .iter()
                .enumerate()
                .map(|(j, t)| {
                    j != i
                        && t.winding
                            .points()
                            .iter()
                            .any(|&q| dot(q, p.plane) > VIS_ON_EPSILON)
                        && p.winding
                            .points()
                            .iter()
                            .any(|&q| dot(q, t.plane) < -VIS_ON_EPSILON)
                })
                .collect();
            let mut seen = Leaves::none(file.leafs);
            let mut stack = vec![p.leaf];
            while let Some(leaf) = stack.pop() {
                if !seen.add(leaf) {
                    continue;
                }
                for &next in file.leaf_portals.get(leaf).into_iter().flatten() {
                    if facing.get(next).copied().unwrap_or(false)
                        && let Some(portal) = portals.get(next)
                    {
                        stack.push(portal.leaf);
                    }
                }
            }
            might.push(seen);
        }
        Self { file, might, level }
    }

    /// The order the portals are flowed in: fewest leaves might be seen first, the earlier of
    /// equal ones (VIS-4.1).
    pub(crate) fn order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.might.len()).collect();
        order.sort_by_key(|&p| self.might.get(p).map_or(0, Leaves::count));
        order
    }

    /// The fast mode: each portal sees what it might (VIS-1.3).
    pub(crate) fn fast(self) -> Vec<Leaves> {
        self.might
    }

    /// The full mode: each portal flowed in order, as vis flows them on one thread, the
    /// portals of a batch at once on `threads` threads (VIS-4, VIS-4.7).
    pub(crate) fn full(self, threads: usize) -> Result<(Vec<Leaves>, VisCounts), VisError> {
        let order = self.order();
        let mut done: Vec<Option<Leaves>> = vec![None; self.might.len()];
        let mut counts = VisCounts::default();
        for batch in order.chunks(VIS_FLOW_BATCH) {
            let results = self.batch(batch, &done, threads)?;
            // A flow that read the might-see set of a portal of its batch that one thread would
            // have flowed first is flowed again, in order, with that portal's exact set.
            for (&portal, flowed) in batch.iter().zip(results) {
                let flowed = if flowed.read_earlier {
                    self.portal_flow(portal, &done, &[])?
                } else {
                    flowed
                };
                counts.add(flowed.counts);
                if let Some(slot) = done.get_mut(portal) {
                    *slot = Some(flowed.seen);
                }
            }
        }
        let seen = done
            .into_iter()
            .zip(self.might)
            .map(|(seen, might)| seen.unwrap_or(might))
            .collect();
        Ok((seen, counts))
    }

    /// Flows the portals of `batch` on up to `threads` threads, each seeing `done` as the
    /// portals flowed so far; each result with whether it read the might-see set of a portal
    /// earlier in the batch.
    fn batch(
        &self,
        batch: &[usize],
        done: &[Option<Leaves>],
        threads: usize,
    ) -> Result<Vec<Flowed>, VisError> {
        let next = AtomicUsize::new(0);
        let mut results: Vec<Option<Result<Flowed, VisError>>> =
            std::iter::repeat_with(|| None).take(batch.len()).collect();
        let workers = threads.clamp(1, batch.len().max(1));
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .filter_map(|_| {
                    std::thread::Builder::new()
                        .stack_size(FLOW_STACK)
                        .spawn_scoped(scope, || {
                            let mut mine = Vec::new();
                            loop {
                                let at = next.fetch_add(1, Ordering::Relaxed);
                                let Some(&portal) = batch.get(at) else {
                                    break;
                                };
                                let earlier = batch.get(..at).unwrap_or_default();
                                mine.push((at, self.portal_flow(portal, done, earlier)));
                            }
                            mine
                        })
                        .ok()
                })
                .collect();
            for handle in handles {
                if let Ok(mine) = handle.join() {
                    for (at, result) in mine {
                        if let Some(slot) = results.get_mut(at) {
                            *slot = Some(result);
                        }
                    }
                }
            }
        });
        results
            .into_iter()
            .map(|result| result.unwrap_or(Err(VisError::Thread)))
            .collect()
    }

    /// `PortalFlow`: the leaves portal `p` sees, walking leaves from the one it looks into
    /// (VIS-4.2 to VIS-4.5), with whether it read the might-see set of one of `earlier`, which
    /// one thread would have flowed first.
    fn portal_flow(
        &self,
        p: usize,
        done: &[Option<Leaves>],
        earlier: &[usize],
    ) -> Result<Flowed, VisError> {
        let portal = self.file.portals.get(p).ok_or(VisError::Thread)?;
        let might = self.might.get(p).cloned().ok_or(VisError::Thread)?;
        let head = Step {
            plane: portal.plane,
            source: portal.winding.clone(),
            pass: None,
            might,
        };
        let mut walk = Walk {
            base: portal.plane,
            done,
            earlier,
            read_earlier: false,
            seen: Leaves::none(self.file.leafs),
            path: Vec::new(),
            counts: VisCounts::default(),
        };
        self.leaf_flow(portal.leaf, &head, &mut walk)?;
        Ok(Flowed {
            seen: walk.seen,
            read_earlier: walk.read_earlier,
            counts: walk.counts,
        })
    }

    /// `RecursiveLeafFlow`: marks `leaf` seen and follows each of its portals that can still
    /// show a leaf not yet seen (VIS-4.2 to VIS-4.5).
    fn leaf_flow(&self, leaf: usize, previous: &Step, walk: &mut Walk<'_>) -> Result<(), VisError> {
        bump(&mut walk.counts.chains);
        if walk.path.contains(&leaf) {
            return Err(VisError::LeafRecursion);
        }
        walk.seen.add(leaf);
        walk.path.push(leaf);
        for &p in self.file.leaf_portals.get(leaf).into_iter().flatten() {
            let Some(portal) = self.file.portals.get(p) else {
                continue;
            };
            if !previous.might.has(portal.leaf) {
                continue;
            }
            let test = if let Some(exact) = walk.done.get(p).and_then(Option::as_ref) {
                bump(&mut walk.counts.vis_tests);
                exact
            } else {
                bump(&mut walk.counts.might_tests);
                if walk.earlier.contains(&p) {
                    walk.read_earlier = true;
                }
                let Some(might) = self.might.get(p) else {
                    continue;
                };
                might
            };
            let mut might = previous.might.clone();
            for (m, t) in might.0.iter_mut().zip(&test.0) {
                *m &= *t;
            }
            let more = might.0.iter().zip(&walk.seen.0).any(|(m, s)| m & !s != 0);
            if !more {
                continue;
            }
            let back = portal.plane.flipped();
            if previous.plane.same_normal(back) {
                continue;
            }
            bump(&mut walk.counts.checks);
            let Some(target) = portal.winding.clipped(walk.base) else {
                continue;
            };
            let Some(pass) = &previous.pass else {
                let step = Step {
                    plane: portal.plane,
                    source: previous.source.clone(),
                    pass: Some(target),
                    might,
                };
                self.leaf_flow(portal.leaf, &step, walk)?;
                continue;
            };
            let Some(target) = target.clipped(previous.plane) else {
                continue;
            };
            let Some(mut source) = previous.source.clipped(back) else {
                continue;
            };
            bump(&mut walk.counts.tests);
            let mut target = target;
            if self.level >= Level::One {
                let Some(t) = source.separated(pass, target, Flip::Front) else {
                    continue;
                };
                target = t;
            }
            if self.level >= Level::Two {
                let Some(t) = pass.separated(&source, target, Flip::Back) else {
                    continue;
                };
                target = t;
            }
            if self.level >= Level::Three {
                let Some(s) = target.separated(pass, source, Flip::Front) else {
                    continue;
                };
                source = s;
            }
            if self.level >= Level::Four {
                let Some(s) = pass.separated(&target, source, Flip::Back) else {
                    continue;
                };
                source = s;
            }
            bump(&mut walk.counts.passes);
            let step = Step {
                plane: portal.plane,
                source,
                pass: Some(target),
                might,
            };
            self.leaf_flow(portal.leaf, &step, walk)?;
        }
        walk.path.pop();
        Ok(())
    }

    /// `LeafFlow`: each leaf's row, the leaves its portals see and itself (VIS-4.6).
    pub(crate) fn rows(file: &PortalFile, seen: &[Leaves]) -> Result<Vec<Leaves>, VisError> {
        let mut rows = Vec::with_capacity(file.leafs);
        for (leaf, portals) in file.leaf_portals.iter().enumerate() {
            let mut row = Leaves::none(file.leafs);
            for &p in portals {
                if let Some(s) = seen.get(p) {
                    row.or(s);
                }
            }
            if !row.add(leaf) {
                return Err(VisError::SawIntoLeaf { leaf });
            }
            rows.push(row);
        }
        Ok(rows)
    }
}

/// How far `point` is in front of `plane`.
fn dot(point: [f32; 3], plane: Plane) -> f32 {
    point[0] * plane.normal[0] + point[1] * plane.normal[1] + point[2] * plane.normal[2]
        - plane.dist
}
