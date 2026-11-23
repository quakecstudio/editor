// SPDX-License-Identifier: GPL-3.0-or-later

//! The score of a level's plan, by which the best of several drawn from one seed is kept, as
//! search-based level makers keep the fittest of what they draw. It counts what Romero's
//! rules and the analyses of id's levels look for: loops back to where the player has been,
//! more than one floor, places seen before they are reached, a secret, locks of more than
//! one kind, and rooms that differ.

use crate::RoomShape;
use crate::layout::Plan;
use crate::mission::Report;

/// What each thing counted is worth.
const LOOP: usize = 4;
const FLOOR: usize = 3;
const TEASER: usize = 3;
const SECRET: usize = 2;
const LOCK_KIND: usize = 4;
const SHAPE: usize = 2;

/// A plan's score: the higher, the better a level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Score(usize);

impl Score {
    /// The score of `plan`, whose mission came to `report`.
    pub(crate) fn of(plan: &Plan, report: &Report) -> Self {
        let mut kinds = report.locks.clone();
        kinds.sort_unstable();
        kinds.dedup();
        let mut shapes: Vec<RoomShape> = Vec::new();
        for room in &plan.rooms {
            if !shapes.contains(&room.shape) {
                shapes.push(room.shape);
            }
        }
        let parts = [
            (report.loops, LOOP),
            (report.floors, FLOOR),
            (report.teasers, TEASER),
            (report.secrets, SECRET),
            (kinds.len(), LOCK_KIND),
            (shapes.len(), SHAPE),
        ];
        Self(
            parts
                .iter()
                .map(|&(count, worth)| count.saturating_mul(worth))
                .fold(0, usize::saturating_add),
        )
    }
}
