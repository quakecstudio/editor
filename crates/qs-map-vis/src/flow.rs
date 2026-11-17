// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997  Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: vis/vis.cc, ClusterFlow, PortalCompleted, UpdateMightsee
// Ported from ericwa/ericw-tools@36eec1da2a194467e6baac2f444c2dd04b57d266: vis/flow.cc, RecursiveLeafFlow, VisTests, TargetChecks, IterativeTargetChecks

//! What each portal sees: the first-order bound of the leaves it might see (VIS-3), then the
//! portal flow that narrows it (VIS-4), and each leaf's row from its portals' (VIS-4.6).

use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

use crate::portals::{Format, Portal, PortalFile};
use crate::winding::{Clip, Flip, Plane, VIS_ON_EPSILON, Winding};
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
/// How many portals an ericw-tools flow checks for each target check it may make: ericw-tools'
/// `-targetchecks` is 0.5 target checks to a regular check unless given (VIS-7.5).
const VIS_STEPS_PER_TARGET_CHECK: usize = 2;
/// How many places before it in the order an ericw-tools flow's portals are before it sees them
/// as flowed (VIS-7.7).
const VIS_FLOW_WINDOW: usize = 64;
/// The leaves an ericw-tools flow walks into before it is a long one, which the flows after it do
/// not wait for (VIS-7.7).
const VIS_LONG_FLOW_CHAINS: usize = 8192;
/// How many places before it in the order a long flow is before an ericw-tools flow sees it as
/// flowed (VIS-7.7).
const VIS_LONG_FLOW_WINDOW: usize = 1024;
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
    /// ericw-tools' `c_targetcheck`: portals checked by an ericw-tools flow's target checks
    /// (VIS-7.5).
    pub target_checks: usize,
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
            (&mut self.target_checks, other.target_checks),
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

    /// A set of `count` with every one in it.
    fn all(count: usize) -> Self {
        let mut all = Self(vec![u64::MAX; count.div_ceil(WORD_BITS)]);
        let spare = all.0.len().saturating_mul(WORD_BITS).saturating_sub(count);
        if let Some(last) = all.0.last_mut() {
            *last = last
                .checked_shr(u32::try_from(spare).unwrap_or(u32::MAX))
                .unwrap_or(0);
        }
        all
    }

    fn remove(&mut self, leaf: usize) {
        let (word, bit) = Self::place(leaf);
        if let Some(word) = self.0.get_mut(word) {
            *word &= !bit;
        }
    }

    fn and(&mut self, other: &Self) {
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a &= *b;
        }
    }

    fn or(&mut self, other: &Self) {
        for (a, b) in self.0.iter_mut().zip(&other.0) {
            *a |= *b;
        }
    }

    pub(crate) fn count(&self) -> usize {
        self.0
            .iter()
            .map(|w| usize::try_from(w.count_ones()).unwrap_or(usize::MAX))
            .fold(0, usize::saturating_add)
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
    flowed: View<'a>,
    read_earlier: bool,
    seen: Leaves,
    path: Vec<usize>,
    /// The might-see set of each step of the path, the base portal's first: an ericw-tools flow's
    /// target checks narrow those of the steps above the one that makes them (VIS-7.5).
    mights: Vec<Leaves>,
    /// What each step of the path has of target checks.
    targets: Vec<Target>,
    /// Might-see sets of steps already walked, to be reused instead of allocated.
    spare: Vec<Leaves>,
    /// Separating-plane lists of steps already walked, to be reused.
    plane_spare: Vec<Vec<Plane>>,
    counts: VisCounts,
}

/// What one flow sees of the others: each portal's exact set once it is flowed, each portal's
/// place in the order they are flowed in, the place below which a portal counts as flowed, and
/// the flow's own place. A portal from `limit` to before `own` is one a single thread would
/// have flowed first, and the flow sees its might-see set. Of ericw-tools' flows, a long
/// one counts as flowed only below `long_limit`.
#[derive(Clone, Copy)]
struct View<'a> {
    done: &'a [OnceLock<Leaves>],
    rank: &'a [usize],
    limit: usize,
    own: usize,
    long: Option<(&'a Progress, usize)>,
}

impl<'a> View<'a> {
    /// Portal `p`'s place in the order.
    fn rank_of(self, p: usize) -> usize {
        self.rank.get(p).copied().unwrap_or(usize::MAX)
    }

    /// Whether the flow at `place` counts as flowed.
    fn sees(self, place: usize) -> bool {
        place < self.limit
            && self
                .long
                .is_none_or(|(progress, limit)| place < limit || !progress.is_long(place))
    }

    /// Portal `p`'s exact set, when it counts as flowed.
    fn exact(self, p: usize) -> Option<&'a Leaves> {
        self.sees(self.rank_of(p))
            .then(|| self.done.get(p).and_then(OnceLock::get))
            .flatten()
    }
}

/// How far ericw-tools' flows have come: the places in the order that are flowed and
/// those whose flows are long, the place below which every flow is one or the other, the
/// place below which every one is flowed, and whether a flow failed.
struct Progress {
    state: Mutex<Reached>,
    changed: Condvar,
    long: Vec<AtomicBool>,
    /// For each portal, the leaves found not to see the leaf it leaves, each with the place
    /// of the flow at whose end it was found (VIS-7.8).
    unseen: Vec<Mutex<Vec<(usize, usize)>>>,
}

struct Reached {
    finished: Leaves,
    settled_below: usize,
    finished_below: usize,
    failed: Option<VisError>,
}

impl Progress {
    fn new(portals: usize) -> Self {
        Self {
            state: Mutex::new(Reached {
                finished: Leaves::none(portals),
                settled_below: 0,
                finished_below: 0,
                failed: None,
            }),
            changed: Condvar::new(),
            long: (0..portals).map(|_| AtomicBool::new(false)).collect(),
            unseen: (0..portals).map(|_| Mutex::new(Vec::new())).collect(),
        }
    }

    /// Whether the flow at `place`, which is flowed or long, is long.
    fn is_long(&self, place: usize) -> bool {
        self.long
            .get(place)
            .is_some_and(|long| long.load(Ordering::Acquire))
    }

    /// Waits until every place below `settled` is flowed or long and every place below
    /// `finished` is flowed; the error of a flow that failed.
    fn wait_for(&self, settled: usize, finished: usize) -> Result<(), VisError> {
        let mut state = self.state.lock().map_err(|_| VisError::Thread)?;
        loop {
            if let Some(error) = &state.failed {
                return Err(error.clone());
            }
            if state.settled_below >= settled && state.finished_below >= finished {
                return Ok(());
            }
            state = self.changed.wait(state).map_err(|_| VisError::Thread)?;
        }
    }

    /// The places below which every flow is settled, and flowed, moved up as far as they go.
    fn advance(&self, state: &mut Reached) {
        while state.finished.has(state.finished_below) {
            state.finished_below = state.finished_below.saturating_add(1);
        }
        state.settled_below = state.settled_below.max(state.finished_below);
        while state.finished.has(state.settled_below) || self.is_long(state.settled_below) {
            state.settled_below = state.settled_below.saturating_add(1);
        }
    }

    /// Marks `place` flowed.
    fn finish(&self, place: usize) -> Result<(), VisError> {
        let mut state = self.state.lock().map_err(|_| VisError::Thread)?;
        state.finished.add(place);
        self.advance(&mut state);
        drop(state);
        self.changed.notify_all();
        Ok(())
    }

    /// Marks the flow at `place` a long one.
    fn declare_long(&self, place: usize) {
        if let Some(long) = self.long.get(place) {
            long.store(true, Ordering::Release);
        }
        if let Ok(mut state) = self.state.lock() {
            self.advance(&mut state);
        }
        self.changed.notify_all();
    }

    /// Marks the flows failed with `error`, which each waiting flow then stops with.
    fn fail(&self, error: &VisError) {
        if let Ok(mut state) = self.state.lock() {
            state.failed.get_or_insert_with(|| error.clone());
        }
        self.changed.notify_all();
    }
}

/// The portals and what each might see.
pub(crate) struct Flow<'a> {
    file: &'a PortalFile,
    might: Vec<Leaves>,
    level: Level,
    tool: Tool,
}

/// Whose flow it is: id's vis's (VIS-4), or ericw-tools' vis's (VIS-7.5 to VIS-7.8).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    /// id's for a `PRT1` file, ericw-tools' for a `PRT2` one.
    #[default]
    OfFile,
    /// ericw-tools' for either file (VIS-7.9).
    EricwTools,
}

/// What a step's target checks leave (VIS-7.5): the leaves it might see, the portals that can
/// be seen through it, and how many portals were checked.
struct Narrowed {
    might: Leaves,
    portals: Leaves,
    checks: usize,
}

/// One step of a flow's path: the portal's plane, the source and the pass windings so far
/// (no pass on the first step) and the step before it. The leaves that might still be seen
/// from it are the walk's ([`Walk::mights`]).
struct Step<'w> {
    plane: Plane,
    source: Cow<'w, Winding>,
    pass: Option<Cow<'w, Winding>>,
    above: Option<&'w Step<'w>>,
}

/// A step's target checks (VIS-7.5): whether they are made, and how many it and the steps
/// above it are expected to take while they are not.
#[derive(Clone, Copy, Default)]
struct Target {
    made: Made,
    expected: usize,
}

/// Whether a step's target checks are made.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Made {
    #[default]
    No,
    Yes,
}

impl<'a> Flow<'a> {
    /// `BasePortalVis`: each portal's might-see set, a flood from the leaf it looks into
    /// through the portals that face it (VIS-3).
    pub(crate) fn new(file: &'a PortalFile, level: Level) -> Self {
        let portals = &file.portals;
        let might_of = |i: usize, p: &Portal| {
            let facing: Vec<bool> = portals
                .iter()
                .enumerate()
                .map(|(j, t)| {
                    j != i
                        && t.winding
                            .points()
                            .iter()
                            .any(|&q| f64::from(dot(q, p.plane)) > VIS_ON_EPSILON)
                        && p.winding
                            .points()
                            .iter()
                            .any(|&q| f64::from(dot(q, t.plane)) < -VIS_ON_EPSILON)
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
            seen
        };
        // Each portal's set is its own: the portals are shared out over the threads in runs,
        // and the sets put back in order.
        let threads = std::thread::available_parallelism().map_or(1, usize::from);
        let run = portals.len().div_ceil(threads).max(1);
        let might = std::thread::scope(|scope| {
            let handles: Vec<_> = portals
                .chunks(run)
                .enumerate()
                .map(|(number, chunk)| {
                    let first = number.saturating_mul(run);
                    let might_of = &might_of;
                    scope.spawn(move || {
                        chunk
                            .iter()
                            .zip(first..)
                            .map(|(p, i)| might_of(i, p))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            let mut might = Vec::with_capacity(portals.len());
            for (handle, chunk) in handles.into_iter().zip(portals.chunks(run)) {
                match handle.join() {
                    Ok(sets) => might.extend(sets),
                    // A thread that could not finish: its portals' sets are found here.
                    Err(_) => {
                        let first = might.len();
                        might.extend(chunk.iter().zip(first..).map(|(p, i)| might_of(i, p)));
                    }
                }
            }
            might
        });
        let tool = match file.format {
            Format::Leaves => Tool::OfFile,
            Format::Clusters => Tool::EricwTools,
        };
        Self {
            file,
            might,
            level,
            tool,
        }
    }

    /// The flow as `tool`'s: a `PRT2` file's is ericw-tools' whichever is asked for (VIS-7.9).
    pub(crate) fn of(mut self, tool: Tool) -> Self {
        if tool == Tool::EricwTools {
            self.tool = tool;
        }
        self
    }

    /// Whether the flow is ericw-tools'.
    fn is_ericw(&self) -> bool {
        self.tool == Tool::EricwTools
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
    /// portals of a batch at once on `threads` threads (VIS-4, VIS-4.7); a `PRT2` file's flowed
    /// as its threads come free (VIS-7.7).
    pub(crate) fn full(self, threads: usize) -> Result<(Vec<Leaves>, VisCounts), VisError> {
        let order = self.order();
        let mut rank = vec![0; order.len()];
        for (place, &portal) in order.iter().enumerate() {
            if let Some(slot) = rank.get_mut(portal) {
                *slot = place;
            }
        }
        let done: Vec<OnceLock<Leaves>> = order.iter().map(|_| OnceLock::new()).collect();
        let counts = if self.is_ericw() {
            self.as_threads_come_free(&order, (&done, &rank), threads)?
        } else {
            self.in_batches(&order, (&done, &rank), threads)?
        };
        let seen = done
            .into_iter()
            .zip(self.might)
            .map(|(seen, might)| seen.into_inner().unwrap_or(might))
            .collect();
        Ok((seen, counts))
    }

    /// id's flows (VIS-4.7): the portals of a batch at once, then, in order, a flow
    /// that read the might-see set of a portal of its batch that one thread would have flowed
    /// first is flowed again with that portal's exact set; so is one that failed, which may
    /// have failed only on such a set.
    fn in_batches(
        &self,
        order: &[usize],
        (done, rank): (&[OnceLock<Leaves>], &[usize]),
        threads: usize,
    ) -> Result<VisCounts, VisError> {
        let mut counts = VisCounts::default();
        for (number, batch) in order.chunks(VIS_FLOW_BATCH).enumerate() {
            let limit = number.saturating_mul(VIS_FLOW_BATCH);
            let view = |own: usize| View {
                done,
                rank,
                limit,
                own,
                long: None,
            };
            let results = self.batch(batch, view(limit), threads);
            for ((&portal, flowed), own) in batch.iter().zip(results).zip(limit..) {
                let flowed = match flowed {
                    Ok(flowed) if !flowed.read_earlier => flowed,
                    _ => self.alone(
                        portal,
                        View {
                            limit: own,
                            ..view(own)
                        },
                    )?,
                };
                counts.add(flowed.counts);
                if let Some(slot) = done.get(portal) {
                    slot.get_or_init(|| flowed.seen);
                }
            }
        }
        Ok(counts)
    }

    /// ericw-tools' flows (VIS-7.7): each thread takes the next portal of the order as it
    /// comes free, and its flow sees as flowed the portals [`VIS_FLOW_WINDOW`] places and more
    /// before it, which it waits for, a long flow only [`VIS_LONG_FLOW_WINDOW`] places before; so the result is the same on any number of threads. A
    /// flow that fails is made again with every portal before it flowed.
    fn as_threads_come_free(
        &self,
        order: &[usize],
        (done, rank): (&[OnceLock<Leaves>], &[usize]),
        threads: usize,
    ) -> Result<VisCounts, VisError> {
        let next = AtomicUsize::new(0);
        let progress = Progress::new(order.len());
        let flows = || -> Result<VisCounts, VisError> {
            let mut counts = VisCounts::default();
            loop {
                let own = next.fetch_add(1, Ordering::Relaxed);
                let Some(&portal) = order.get(own) else {
                    return Ok(counts);
                };
                let view = |limit: usize, long_limit: usize| View {
                    done,
                    rank,
                    limit,
                    own,
                    long: Some((&progress, long_limit)),
                };
                let limit = own.saturating_sub(VIS_FLOW_WINDOW);
                let long_limit = own.saturating_sub(VIS_LONG_FLOW_WINDOW);
                progress.wait_for(limit, long_limit)?;
                let flowed = match self.portal_flow(portal, view(limit, long_limit)) {
                    Ok(flowed) => flowed,
                    Err(_) => {
                        progress.wait_for(own, own)?;
                        self.portal_flow(portal, view(own, own))?
                    }
                };
                counts.add(flowed.counts);
                if let Some(slot) = done.get(portal) {
                    slot.get_or_init(|| flowed.seen);
                }
                self.portal_completed(portal, view(limit, long_limit), &progress)?;
                progress.finish(own)?;
            }
        };
        let workers = threads.clamp(1, order.len().max(1));
        let mut counts = VisCounts::default();
        let mut failed = None;
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    std::thread::Builder::new()
                        .stack_size(FLOW_STACK)
                        .spawn_scoped(scope, || flows().inspect_err(|error| progress.fail(error)))
                })
                .collect();
            for handle in handles {
                match handle.map(|handle| handle.join()) {
                    Ok(Ok(Ok(mine))) => counts.add(mine),
                    Ok(Ok(Err(error))) => {
                        failed.get_or_insert(error);
                    }
                    Ok(Err(_)) | Err(_) => {
                        progress.fail(&VisError::Thread);
                        failed.get_or_insert(VisError::Thread);
                    }
                }
            }
        });
        failed.map_or(Ok(counts), Err)
    }

    /// `PortalCompleted`, `UpdateMightsee`: when portal `completed`'s flow ends, each leaf
    /// that a flowed portal of the leaf it looks into might have seen and did not, and that
    /// no other portal of that leaf sees or might see, cannot see that leaf either; so the
    /// portals leaving such a leaf are noted not to see it (VIS-7.8). A portal counts as
    /// flowed as the completed flow saw it.
    fn portal_completed(
        &self,
        completed: usize,
        view: View<'_>,
        progress: &Progress,
    ) -> Result<(), VisError> {
        let Some(leaf) = self.file.portals.get(completed).map(|portal| portal.leaf) else {
            return Ok(());
        };
        let around = self
            .file
            .leaf_portals
            .get(leaf)
            .map_or(&[][..], Vec::as_slice);
        let seen_by = |p: usize| view.exact(p).or_else(|| self.might.get(p));
        for &p in around {
            let (Some(exact), Some(might)) = (view.exact(p), self.might.get(p)) else {
                continue;
            };
            for (word, (m, e)) in might.0.iter().zip(&exact.0).enumerate() {
                let mut changed = m & !e;
                for &other in around {
                    if changed == 0 {
                        break;
                    }
                    if other != p
                        && let Some(sees) = seen_by(other).and_then(|set| set.0.get(word))
                    {
                        changed &= !sees;
                    }
                }
                while changed != 0 {
                    let bit = usize::try_from(changed.trailing_zeros()).unwrap_or(0);
                    changed &= changed.wrapping_sub(1);
                    let unseeing = word.saturating_mul(WORD_BITS).saturating_add(bit);
                    for &from in self.file.leaf_portals.get(unseeing).into_iter().flatten() {
                        if let Some(list) = progress.unseen.get(from) {
                            list.lock()
                                .map_err(|_| VisError::Thread)?
                                .push((view.own, leaf));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Portal `p` flowed as one thread flows it, seeing what `view` shows as flowed, on a
    /// thread with a flow's stack.
    fn alone(&self, p: usize, view: View<'_>) -> Result<Flowed, VisError> {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(FLOW_STACK)
                .spawn_scoped(scope, || self.portal_flow(p, view))
                .map_err(|_| VisError::Thread)?
                .join()
                .unwrap_or(Err(VisError::Thread))
        })
    }

    /// Flows the portals of `batch` on up to `threads` threads, each seeing what `view` shows
    /// as flowed, its own place its place in the batch after `view`'s; each result with whether
    /// it read the might-see set of a portal earlier in the batch.
    fn batch(
        &self,
        batch: &[usize],
        view: View<'_>,
        threads: usize,
    ) -> Vec<Result<Flowed, VisError>> {
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
                                // The portals that might see most are last in the batch, and take
                                // longest: start them first, so the batch ends with short flows.
                                let taken = next.fetch_add(1, Ordering::Relaxed);
                                let Some(at) = batch.len().checked_sub(taken.saturating_add(1))
                                else {
                                    break;
                                };
                                let Some(&portal) = batch.get(at) else {
                                    break;
                                };
                                let view = View {
                                    own: view.limit.saturating_add(at),
                                    ..view
                                };
                                mine.push((at, self.portal_flow(portal, view)));
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
    /// (VIS-4.2 to VIS-4.5), with whether it read the might-see set of a portal one thread
    /// would have flowed first.
    fn portal_flow(&self, p: usize, view: View<'_>) -> Result<Flowed, VisError> {
        let portal = self.file.portals.get(p).ok_or(VisError::Thread)?;
        let mut might = self.might.get(p).cloned().ok_or(VisError::Thread)?;
        // VIS-7.8: the leaves found, by the end of a flow this one sees as flowed, not to see
        // the leaf this portal leaves.
        if let Some((progress, _)) = view.long
            && let Some(list) = progress.unseen.get(p)
        {
            for &(place, leaf) in list.lock().map_err(|_| VisError::Thread)?.iter() {
                if view.sees(place) {
                    might.remove(leaf);
                }
            }
        }
        let head = Step {
            plane: portal.plane,
            source: Cow::Borrowed(&portal.winding),
            pass: None,
            above: None,
        };
        let mut walk = Walk {
            base: portal.plane,
            flowed: view,
            read_earlier: false,
            seen: Leaves::none(self.file.leafs),
            path: Vec::new(),
            mights: vec![might],
            targets: vec![Target::default()],
            spare: Vec::new(),
            plane_spare: Vec::new(),
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
    fn leaf_flow(
        &self,
        leaf: usize,
        previous: &Step<'_>,
        walk: &mut Walk<'_>,
    ) -> Result<(), VisError> {
        bump(&mut walk.counts.chains);
        if walk.counts.chains == VIS_LONG_FLOW_CHAINS
            && let Some((progress, _)) = walk.flowed.long
        {
            progress.declare_long(walk.flowed.own);
        }
        if walk.path.contains(&leaf) {
            return Err(VisError::LeafRecursion);
        }
        walk.seen.add(leaf);
        walk.path.push(leaf);
        self.target_checks_when_due(previous, walk);
        let flowed = walk.flowed;
        // The separating planes of this step's source and pass, for the portals whose clip leaves
        // the source whole: then they are the same for each of them.
        let mut planes: [Option<Vec<Plane>>; 2] = [None, None];
        for &p in self.file.leaf_portals.get(leaf).into_iter().flatten() {
            let Some(portal) = self.file.portals.get(p) else {
                continue;
            };
            // The step's might-see set is read again for each portal: a target check below the
            // last one followed may have narrowed it.
            let Some(might) = walk.mights.last() else {
                continue;
            };
            if !might.has(portal.leaf) {
                continue;
            }
            let test = if let Some(exact) = flowed.exact(p) {
                bump(&mut walk.counts.vis_tests);
                exact
            } else {
                bump(&mut walk.counts.might_tests);
                if flowed.rank_of(p) < flowed.own {
                    walk.read_earlier = true;
                }
                let Some(might) = self.might.get(p) else {
                    continue;
                };
                might
            };
            let Some(might) = walk.mights.last() else {
                continue;
            };
            let more = might
                .0
                .iter()
                .zip(&test.0)
                .zip(&walk.seen.0)
                .any(|((m, t), s)| m & t & !s != 0);
            if !more {
                continue;
            }
            let back = portal.plane.flipped();
            if previous.plane.same_normal(back) {
                continue;
            }
            bump(&mut walk.counts.checks);
            let target = match portal.winding.clip(walk.base) {
                None => continue,
                Some(Clip::Whole) => Cow::Borrowed(&portal.winding),
                Some(Clip::Cut(cut)) => Cow::Owned(cut),
            };
            let Some(pass) = &previous.pass else {
                let step = Step {
                    plane: portal.plane,
                    source: Cow::Borrowed(&*previous.source),
                    pass: Some(target),
                    above: Some(previous),
                };
                self.step_into(portal.leaf, &step, test, walk)?;
                continue;
            };
            let target = match target.clip(previous.plane) {
                None => continue,
                Some(Clip::Whole) => target,
                Some(Clip::Cut(cut)) => Cow::Owned(cut),
            };
            let (mut source, whole) = match previous.source.clip(back) {
                None => continue,
                Some(Clip::Whole) => (Cow::Borrowed(&*previous.source), true),
                Some(Clip::Cut(cut)) => (Cow::Owned(cut), false),
            };
            bump(&mut walk.counts.tests);
            // VIS-7.6: an ericw-tools flow finds the separating planes of the first two tests from
            // the step's source before this portal cuts it, so they are the same for every
            // portal of the leaf.
            let shared = whole || self.is_ericw();
            let mut target = target;
            if self.level >= Level::One {
                let clipped = if shared {
                    let source = (&*previous.source, walk.base);
                    self.cached(
                        &mut planes[0],
                        &mut walk.plane_spare,
                        source,
                        pass,
                        Flip::Front,
                    )
                    .iter()
                    .try_fold(target, |t, &p| Winding::clip_cow(t, p))
                } else {
                    source.separated(pass, target, Flip::Front)
                };
                let Some(t) = clipped else {
                    continue;
                };
                target = t;
            }
            if self.level >= Level::Two {
                let clipped = if shared {
                    let source = (&**pass, previous.plane);
                    self.cached(
                        &mut planes[1],
                        &mut walk.plane_spare,
                        source,
                        &previous.source,
                        Flip::Back,
                    )
                    .iter()
                    .try_fold(target, |t, &p| Winding::clip_cow(t, p))
                } else {
                    pass.separated(&source, target, Flip::Back)
                };
                let Some(t) = clipped else {
                    continue;
                };
                target = t;
            }
            if self.level >= Level::Three {
                let Some(s) = self.separated((&target, portal.plane), pass, source, Flip::Front)
                else {
                    continue;
                };
                source = s;
            }
            if self.level >= Level::Four {
                let Some(s) = self.separated((pass, previous.plane), &target, source, Flip::Back)
                else {
                    continue;
                };
                source = s;
            }
            bump(&mut walk.counts.passes);
            let step = Step {
                plane: portal.plane,
                source,
                pass: Some(target),
                above: Some(previous),
            };
            self.step_into(portal.leaf, &step, test, walk)?;
        }
        walk.path.pop();
        walk.plane_spare.extend(planes.into_iter().flatten());
        Ok(())
    }

    /// The flow through `step` into `leaf`: the step's might-see set, the one before it
    /// narrowed by `test`, is the walk's last while the leaf is flowed.
    fn step_into(
        &self,
        leaf: usize,
        step: &Step<'_>,
        test: &Leaves,
        walk: &mut Walk<'_>,
    ) -> Result<(), VisError> {
        let Some(previous) = walk.mights.last() else {
            return Ok(());
        };
        let might = Self::intersection(&mut walk.spare, previous, test);
        let expected = walk.targets.last().map_or(0, |target| target.expected);
        walk.targets.push(Target {
            made: Made::No,
            expected: expected.saturating_add(might.count()),
        });
        walk.mights.push(might);
        let flowed = self.leaf_flow(leaf, step, walk);
        walk.targets.pop();
        walk.spare.extend(walk.mights.pop());
        flowed
    }

    /// The target checks of an ericw-tools flow's path down to `last`, made once the flow has
    /// checked [`VIS_STEPS_PER_TARGET_CHECK`] portals for each check made so far and each the path
    /// is expected to take (VIS-7.5).
    fn target_checks_when_due(&self, last: &Step<'_>, walk: &mut Walk<'_>) {
        let expected = walk.targets.last().map_or(0, |target| target.expected);
        let due = walk
            .counts
            .target_checks
            .saturating_add(expected)
            .saturating_mul(VIS_STEPS_PER_TARGET_CHECK);
        if self.is_ericw() && expected > 0 && walk.counts.checks >= due {
            let made = self.iterative_target_checks(last, walk);
            walk.counts.target_checks = walk.counts.target_checks.saturating_add(made);
        }
    }

    /// `IterativeTargetChecks`: the might-see set of each step of the path down to `last`
    /// whose target checks are not made is narrowed to the leaves a portal the step can see
    /// looks into ([`Self::target_checks`]), the base portal's step first, and each narrows the
    /// step after it; how many portals were checked (VIS-7.5).
    fn iterative_target_checks(&self, last: &Step<'_>, walk: &mut Walk<'_>) -> usize {
        let mut path = Vec::with_capacity(walk.mights.len());
        let mut at = Some(last);
        while let Some(step) = at {
            path.push(step);
            at = step.above;
        }
        path.reverse();
        let mut checks = 0_usize;
        let mut portals = Leaves::all(self.file.portals.len());
        for (depth, step) in path.into_iter().enumerate() {
            if walk
                .targets
                .get(depth)
                .is_none_or(|target| target.made == Made::Yes)
            {
                continue;
            }
            if let Some(narrowed) =
                self.target_checks(step, walk.mights.get(depth), &portals, walk.base)
            {
                checks = checks.saturating_add(narrowed.checks);
                portals = narrowed.portals;
                if let Some(might) = walk.mights.get_mut(depth) {
                    *might = narrowed.might;
                }
            }
            if let Some((might, [next, ..])) = walk
                .mights
                .get_mut(depth..)
                .and_then(<[Leaves]>::split_first_mut)
            {
                next.and(might);
            }
            if let Some(target) = walk.targets.get_mut(depth) {
                *target = Target {
                    made: Made::Yes,
                    expected: 0,
                };
            }
        }
        checks
    }

    /// `TargetChecks`: of the portals in `portals` that look into a leaf `might` holds, those
    /// the flow's base portal can see through `step`'s pass, each clipped as a flow through it
    /// would clip it, and the leaves they look into; none for the base portal's own step,
    /// which has no pass and narrows nothing (VIS-7.5).
    fn target_checks(
        &self,
        step: &Step<'_>,
        might: Option<&Leaves>,
        portals: &Leaves,
        base: Plane,
    ) -> Option<Narrowed> {
        let (pass, might) = (step.pass.as_ref()?, might?);
        // The separating planes of the step's source and pass are the same for every portal.
        let mut planes: [Vec<Plane>; 2] = [Vec::new(), Vec::new()];
        let [front, back] = &mut planes;
        if self.level >= Level::One {
            self.separators((&step.source, base), pass, Flip::Front, |plane| {
                front.push(plane);
                true
            });
        }
        if self.level >= Level::Two {
            self.separators((pass, step.plane), &step.source, Flip::Back, |plane| {
                back.push(plane);
                true
            });
        }
        let mut narrowed = Narrowed {
            might: Leaves::none(self.file.leafs),
            portals: Leaves::none(self.file.portals.len()),
            checks: 0,
        };
        for (i, portal) in self.file.portals.iter().enumerate() {
            if narrowed.might.has(portal.leaf) || !might.has(portal.leaf) || !portals.has(i) {
                continue;
            }
            let back = portal.plane.flipped();
            if step.plane.same_normal(back) {
                continue;
            }
            bump(&mut narrowed.checks);
            let seen = Winding::clip_cow(Cow::Borrowed(&portal.winding), base)
                .and_then(|target| Winding::clip_cow(target, step.plane))
                .zip(Winding::clip_cow(Cow::Borrowed(&*step.source), back))
                .and_then(|(target, source)| {
                    self.vis_tests((pass, step.plane), (portal.plane, &planes), target, source)
                });
            if seen.is_none() {
                continue;
            }
            narrowed.might.add(portal.leaf);
            narrowed.portals.add(i);
            // The other portals into the leaf, of those still to come, stay as they were.
            for &out in self
                .file
                .leaf_portals
                .get(portal.leaf)
                .into_iter()
                .flatten()
            {
                let into = out ^ 1;
                if i < into && portals.has(into) {
                    narrowed.portals.add(into);
                }
            }
        }
        Some(narrowed)
    }

    /// `VisTests` for a target check: `target` clipped by `planes`, the separating planes of
    /// the step's source and `pass` for the first two tests, and `source` by those of the
    /// target and the pass, as far as the level goes; none when either is clipped away
    /// (VIS-4.4, VIS-7.5).
    fn vis_tests(
        &self,
        (pass, pass_plane): (&Winding, Plane),
        (target_plane, planes): (Plane, &[Vec<Plane>; 2]),
        target: Cow<'_, Winding>,
        mut source: Cow<'_, Winding>,
    ) -> Option<()> {
        let target = planes
            .iter()
            .flatten()
            .try_fold(target, |target, &plane| Winding::clip_cow(target, plane))?;
        if self.level >= Level::Three {
            source = self.separated((&target, target_plane), pass, source, Flip::Front)?;
        }
        if self.level >= Level::Four {
            self.separated((pass, pass_plane), &target, source, Flip::Back)?;
        }
        Some(())
    }

    /// The separating planes of `source`, which lies in `plane`, and `pass`: id's for a `PRT1`
    /// file (VIS-4.4), ericw-tools' for a `PRT2` one (VIS-7.6).
    fn separators(
        &self,
        (source, plane): (&Winding, Plane),
        pass: &Winding,
        flip: Flip,
        visit: impl FnMut(Plane) -> bool,
    ) {
        if self.is_ericw() {
            source.separators_from(plane, pass, flip, visit);
        } else {
            source.separators(pass, flip, visit);
        }
    }

    /// [`Self::separators`]' planes, found once for the portals of a leaf and kept in `slot`,
    /// in a list from `spare` when there is one.
    fn cached<'p>(
        &self,
        slot: &'p mut Option<Vec<Plane>>,
        spare: &mut Vec<Vec<Plane>>,
        source: (&Winding, Plane),
        pass: &Winding,
        flip: Flip,
    ) -> &'p [Plane] {
        slot.get_or_insert_with(|| {
            let mut all = spare.pop().unwrap_or_default();
            all.clear();
            self.separators(source, pass, flip, |plane| {
                all.push(plane);
                true
            });
            all
        })
    }

    /// `target` clipped by [`Self::separators`]' planes; none when it is clipped away.
    fn separated<'t>(
        &self,
        source: (&Winding, Plane),
        pass: &Winding,
        target: Cow<'t, Winding>,
        flip: Flip,
    ) -> Option<Cow<'t, Winding>> {
        let mut target = Some(target);
        self.separators(source, pass, flip, |plane| {
            target = target.take().and_then(|t| Winding::clip_cow(t, plane));
            target.is_some()
        });
        target
    }

    /// `previous & test`, in a set taken from `spare` when there is one.
    fn intersection(spare: &mut Vec<Leaves>, previous: &Leaves, test: &Leaves) -> Leaves {
        let mut might = spare.pop().unwrap_or_else(|| previous.clone());
        for ((m, p), t) in might.0.iter_mut().zip(&previous.0).zip(&test.0) {
            *m = *p & *t;
        }
        might
    }

    /// `LeafFlow`: each leaf's row, the leaves its portals see and itself (VIS-4.6); for a
    /// cluster, ericw-tools' `ClusterFlow` only warns when its portals see it (VIS-7.3).
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
                match file.format {
                    Format::Leaves => return Err(VisError::SawIntoLeaf { leaf }),
                    Format::Clusters => {
                        tracing::warn!("Leaf portals saw into cluster ({leaf})");
                    }
                }
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

#[cfg(test)]
mod tests;
