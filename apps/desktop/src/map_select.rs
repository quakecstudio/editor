// SPDX-License-Identifier: GPL-3.0-or-later

//! The map editor's selection commands (P7-08, MAP-30), TrenchBroom's Edit menu's: Select All,
//! Select Siblings, Invert Selection, and the selections a brush makes, Select Touching, Select
//! Inside and Select Tall, as its manual's "Selecting Objects" describes them. Each reads the
//! scene the view draws, so what is hidden is never selected.

use std::collections::BTreeSet;

use qs_map_edit::{BrushIndex, EntityIndex};

use crate::map_scene::{BrushShape, Scene};

/// A brush, by its entity and its place among the entity's brushes.
pub(crate) type BrushRef = (EntityIndex, BrushIndex);

/// What a selection is: whole entities and brushes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Picked {
    /// Whole entities, the world never among them.
    pub(crate) entities: Vec<EntityIndex>,
    /// Brushes, each by its entity.
    pub(crate) brushes: Vec<BrushRef>,
}

/// Which objects a selecting brush picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Region {
    /// Those it touches (TrenchBroom's Select Touching).
    Touching,
    /// Those wholly inside it (Select Inside).
    Inside,
    /// Those inside it stretched without end along an axis, as a 2D view sees it (Select
    /// Tall): 0 for x, 1 for y, 2 for z.
    Tall(usize),
}

/// The map's axes, x, y and z.
const AXES: usize = 3;

/// How far a point may be past a plane and still be on it, in map units.
const ON_PLANE: f32 = 0.01;

impl Scene {
    /// Everything not hidden: every entity but the world, and the world's brushes.
    pub(crate) fn all(&self, hidden: &BTreeSet<EntityIndex>) -> Picked {
        Picked {
            entities: self
                .entities
                .iter()
                .map(|shape| shape.index)
                .filter(|entity| !hidden.contains(entity))
                .collect(),
            brushes: self
                .brushes
                .iter()
                .filter(|shape| shape.entity.is_world() && !hidden.contains(&shape.entity))
                .map(|shape| (shape.entity, shape.brush))
                .collect(),
        }
    }

    /// Everything not hidden that `picked` leaves out.
    pub(crate) fn inverted(&self, picked: &Picked, hidden: &BTreeSet<EntityIndex>) -> Picked {
        let all = self.all(hidden);
        Picked {
            entities: all
                .entities
                .into_iter()
                .filter(|entity| !picked.entities.contains(entity))
                .collect(),
            brushes: all
                .brushes
                .into_iter()
                .filter(|brush| !picked.brushes.contains(brush))
                .collect(),
        }
    }

    /// Every brush of the entities holding `brushes`: TrenchBroom's Select Siblings.
    pub(crate) fn siblings(&self, brushes: &[BrushRef]) -> Vec<BrushRef> {
        let owners: BTreeSet<EntityIndex> = brushes.iter().map(|&(entity, _)| entity).collect();
        self.brushes
            .iter()
            .filter(|shape| owners.contains(&shape.entity))
            .map(|shape| (shape.entity, shape.brush))
            .collect()
    }

    /// Every brush the ray from `start` along `direction` enters, nearest first, none hidden:
    /// what selection drilling steps through (P7-08).
    pub(crate) fn brushes_along(
        &self,
        start: [f32; 3],
        direction: [f32; 3],
        hidden: &BTreeSet<EntityIndex>,
    ) -> Vec<BrushRef> {
        let mut entered: Vec<(f32, BrushRef)> = self
            .brushes
            .iter()
            .filter(|shape| !hidden.contains(&shape.entity))
            .filter_map(|shape| {
                let t = shape.entered(start, direction)?;
                Some((t, (shape.entity, shape.brush)))
            })
            .collect();
        entered.sort_by(|a, b| a.0.total_cmp(&b.0));
        entered.into_iter().map(|(_, brush)| brush).collect()
    }

    /// The brush a click in a 2D view picks: of those under the pointer along the view's
    /// `axis`, the one smallest across the view, as TrenchBroom picks the smallest visible
    /// area there (its manual's "Selecting Objects"), here by each brush's box.
    pub(crate) fn pick_across(
        &self,
        start: [f32; 3],
        direction: [f32; 3],
        axis: usize,
        hidden: &BTreeSet<EntityIndex>,
    ) -> Option<BrushRef> {
        let across = |shape: &BrushShape| {
            let corners = shape.corners();
            let extent = |i: usize| {
                let values = corners.iter().filter_map(|c| c.get(i).copied());
                let (low, high) = values.fold((f32::INFINITY, f32::NEG_INFINITY), |(l, h), v| {
                    (l.min(v), h.max(v))
                });
                high - low
            };
            (0..AXES)
                .filter(|&i| i != axis)
                .map(extent)
                .product::<f32>()
        };
        self.brushes_along(start, direction, hidden)
            .into_iter()
            .filter_map(|(entity, brush)| self.brush(entity, brush).map(|s| (across(s), s)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, shape)| (shape.entity, shape.brush))
    }

    /// What the brushes `by` pick in `region`, themselves left out, nothing hidden picked.
    pub(crate) fn picked_by(
        &self,
        by: &[BrushRef],
        region: Region,
        hidden: &BTreeSet<EntityIndex>,
    ) -> Picked {
        let selectors: Vec<&BrushShape> = by
            .iter()
            .filter_map(|&(entity, brush)| self.brush(entity, brush))
            .collect();
        let planes_of = |shape: &BrushShape| -> Vec<([f32; 3], f32)> {
            match region {
                Region::Tall(axis) => shape
                    .planes
                    .iter()
                    .filter(|(normal, _)| normal.get(axis).is_none_or(|n| n.abs() < f32::EPSILON))
                    .copied()
                    .collect(),
                Region::Touching | Region::Inside => shape.planes.clone(),
            }
        };
        // Two convex solids touch unless a face plane of one has all of the other past it.
        let takes = |points: &[[f32; 3]], own: &[([f32; 3], f32)]| {
            selectors.iter().any(|selector| {
                let planes = planes_of(selector);
                match region {
                    Region::Touching => {
                        let theirs = selector.corners();
                        !planes.iter().any(|&plane| all_outside(points, plane))
                            && !own.iter().any(|&plane| all_outside(&theirs, plane))
                    }
                    Region::Inside | Region::Tall(_) => planes
                        .iter()
                        .all(|&plane| points.iter().all(|&p| !outside(p, plane))),
                }
            })
        };
        let brushes = self
            .brushes
            .iter()
            .filter(|shape| !hidden.contains(&shape.entity))
            .filter(|shape| !by.contains(&(shape.entity, shape.brush)))
            .filter(|shape| takes(&shape.corners(), &shape.planes))
            .map(|shape| (shape.entity, shape.brush))
            .collect();
        let entities = self
            .entities
            .iter()
            .filter(|shape| !hidden.contains(&shape.index))
            .filter(|shape| takes(&box_corners(shape.bounds), &box_planes(shape.bounds)))
            .map(|shape| shape.index)
            .collect();
        Picked { entities, brushes }
    }
}

impl BrushShape {
    /// Its corners as drawn.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a brush corner of a map is well inside f32's range"
    )]
    fn corners(&self) -> Vec<[f32; 3]> {
        self.vertices
            .iter()
            .map(|corner| corner.map(|c| c as f32))
            .collect()
    }
}

/// The eight corners of the box `[min, max]`.
fn box_corners([min, max]: [[f32; 3]; 2]) -> Vec<[f32; 3]> {
    let mut corners = Vec::new();
    for x in [min[0], max[0]] {
        for y in [min[1], max[1]] {
            for z in [min[2], max[2]] {
                corners.push([x, y, z]);
            }
        }
    }
    corners
}

/// The six face planes of the box `[min, max]`, each a normal pointing out and a distance.
fn box_planes([min, max]: [[f32; 3]; 2]) -> Vec<([f32; 3], f32)> {
    vec![
        ([1.0, 0.0, 0.0], max[0]),
        ([-1.0, 0.0, 0.0], -min[0]),
        ([0.0, 1.0, 0.0], max[1]),
        ([0.0, -1.0, 0.0], -min[1]),
        ([0.0, 0.0, 1.0], max[2]),
        ([0.0, 0.0, -1.0], -min[2]),
    ]
}

/// Whether `point` is past the plane `(normal, distance)`, on the side its normal points to.
fn outside(point: [f32; 3], (normal, distance): ([f32; 3], f32)) -> bool {
    let [nx, ny, nz] = normal;
    let [x, y, z] = point;
    nx * x + ny * y + nz * z - distance > ON_PLANE
}

/// Whether every one of `points` is past the plane: the plane parts them from the brush.
fn all_outside(points: &[[f32; 3]], plane: ([f32; 3], f32)) -> bool {
    !points.is_empty() && points.iter().all(|&p| outside(p, plane))
}

#[cfg(test)]
mod tests;
