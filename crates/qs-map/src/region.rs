// SPDX-License-Identifier: GPL-3.0-or-later

//! A region of a map (MAP-16): the brushes and entities in a box, sealed, for a compile of only
//! the rooms being worked on.

use crate::brush_edit::{BrushError, Surface};
use crate::document::{Brush, Entity, Item, MapFile};
use crate::rooms::Room;

/// The thickness of the brushes that seal a cut map (MAP-16.3).
const SEAL: f64 = 16.0;

/// A box of a map, its least corner below its greatest on every axis (MAP-16.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    min: [f64; 3],
    max: [f64; 3],
}

impl Region {
    /// The region from `min` to `max`; `None` when it has no volume.
    #[must_use]
    pub fn new(min: [f64; 3], max: [f64; 3]) -> Option<Self> {
        min.iter()
            .zip(&max)
            .all(|(lo, hi)| lo < hi)
            .then_some(Self { min, max })
    }

    /// The box around `rooms`' bounds (MAP-16.1); `None` when none has bounds.
    #[must_use]
    pub fn of_rooms<'a>(rooms: impl IntoIterator<Item = &'a Room>) -> Option<Self> {
        let mut around: Option<[[f64; 3]; 2]> = None;
        for [min, max] in rooms.into_iter().filter_map(|room| room.bounds) {
            let [mut lo, mut hi] = around.unwrap_or([min, max]);
            for axis in 0..3 {
                if let (Some(l), Some(h), Some(&a), Some(&b)) = (
                    lo.get_mut(axis),
                    hi.get_mut(axis),
                    min.get(axis),
                    max.get(axis),
                ) {
                    *l = l.min(a);
                    *h = h.max(b);
                }
            }
            around = Some([lo, hi]);
        }
        around.and_then(|[min, max]| Self::new(min, max))
    }

    /// Its least corner.
    #[must_use]
    pub const fn min(&self) -> [f64; 3] {
        self.min
    }

    /// Its greatest corner.
    #[must_use]
    pub const fn max(&self) -> [f64; 3] {
        self.max
    }

    /// Whether `point` lies in it, edges included (MAP-16.2).
    #[must_use]
    pub fn holds(&self, point: [f64; 3]) -> bool {
        point
            .iter()
            .zip(self.min.iter().zip(&self.max))
            .all(|(p, (lo, hi))| lo <= p && p <= hi)
    }

    /// Whether the box around `brush`'s corners overlaps it, touching included (MAP-16.2).
    #[must_use]
    pub fn has(&self, brush: &Brush) -> bool {
        let corners = brush.vertices();
        (0..3).all(|axis| {
            let along = corners.iter().filter_map(|c| c.get(axis).copied());
            let (lo, hi) = along.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            });
            let (Some(&min), Some(&max)) = (self.min.get(axis), self.max.get(axis)) else {
                return false;
            };
            lo <= max && hi >= min
        })
    }

    /// Whether `entity`, not the world, is kept in the cut map (MAP-16.2).
    fn keeps(&self, entity: &Entity) -> bool {
        if entity.has_brushes() {
            entity.brushes().any(|brush| self.has(brush))
        } else {
            self.holds(entity.origin())
        }
    }

    /// `file` cut to the region and sealed with brushes of `surface` (MAP-16.2, MAP-16.3).
    ///
    /// # Errors
    ///
    /// [`BrushError`] when a sealing brush cannot be written with `surface`.
    pub fn cut(&self, file: &MapFile, surface: &Surface) -> Result<MapFile, BrushError> {
        let mut cut = file.clone();
        let mut entities = core::mem::take(&mut cut.entities).into_iter();
        let mut kept = Vec::new();
        if let Some(mut world) = entities.next() {
            world.items.retain(|item| match item {
                Item::Brush(brush) => self.has(brush),
                Item::Pair(_) => true,
            });
            for [min, max] in self.seals() {
                world
                    .items
                    .push(Item::Brush(Brush::cuboid(min, max, surface)?));
            }
            kept.push(world);
        }
        kept.extend(entities.filter(|entity| self.keeps(entity)));
        cut.entities = kept;
        Ok(cut)
    }

    /// The six sealing boxes, `[min, max]`, against each face from outside and reaching past
    /// the region on the other axes (MAP-16.3).
    fn seals(&self) -> Vec<[[f64; 3]; 2]> {
        let grown_min = self.min.map(|c| c - SEAL);
        let grown_max = self.max.map(|c| c + SEAL);
        let mut out = Vec::with_capacity(6);
        for axis in 0..3 {
            let (Some(&lo), Some(&hi)) = (self.min.get(axis), self.max.get(axis)) else {
                continue;
            };
            for (from, to) in [(lo - SEAL, lo), (hi, hi + SEAL)] {
                let (mut min, mut max) = (grown_min, grown_max);
                if let (Some(a), Some(b)) = (min.get_mut(axis), max.get_mut(axis)) {
                    *a = from;
                    *b = to;
                }
                out.push([min, max]);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests;
