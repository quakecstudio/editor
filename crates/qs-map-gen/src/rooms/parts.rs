// SPDX-License-Identifier: GPL-3.0-or-later

//! The parts a room is dressed with on top of its shape, as OBLIGE's shape grammars build
//! rooms from parts the size of furniture rather than of a house (the ObAddon wiki,
//! "Prefabs" and "Shape Grammars"): a grow rule widens a room's footprint with a bay out of a
//! wall that has no doorway; a sprout rule stands a brazier in each bay; decorate rules run a
//! cornice under the ceiling, a skirting along the floor and columns out from the corners.
//! Which parts a room may take is its shape's; which it takes is the theme's and the seed's;
//! and no room takes more brushes than [`ROOM_BUDGET`].

use qs_map::BrushError;

use super::{Built, Room, SPOT_INSET};
use crate::RoomShape;
use crate::layout::Side;
use crate::mass::{Bounds, Coat, Mass};
use crate::theme::Paints;

/// The most brushes a room's parts add.
const ROOM_BUDGET: usize = 48;
/// A bay: how far it reaches out of its wall, half its width at most, and its height.
const BAY_DEPTH: f64 = 80.0;
const BAY_HALF: f64 = 96.0;
/// How far a bay keeps from its wall's pilasters, which stand at the middle of each half of
/// the wall: half a pilaster.
const BAY_MARGIN: f64 = 16.0;
const BAY_HEIGHT: f64 = 128.0;
/// The most bays a room grows.
const MOST_BAYS: usize = 2;
/// A brazier: half its foot, its height, and how bright its flame.
const BRAZIER_HALF: f64 = 12.0;
const BRAZIER_HEIGHT: f64 = 40.0;
const BRAZIER_LIGHT: f64 = 150.0;
/// A cornice's depth out of its wall and height; a skirting's.
const CORNICE_DEPTH: f64 = 8.0;
const CORNICE_HEIGHT: f64 = 16.0;
const SKIRTING_DEPTH: f64 = 8.0;
const SKIRTING_HEIGHT: f64 = 8.0;
/// A column's half-width and how far in from the walls it stands.
const COLUMN_HALF: f64 = 16.0;
const COLUMN_INSET: f64 = 96.0;
/// How near to a column a place to stand is given up.
const COLUMN_CLEAR: f64 = 64.0;

/// A part a room is dressed with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomPart {
    /// Grow: a bay out of each of up to two walls with no doorway.
    Bays,
    /// Sprout: a brazier in each bay, its flame lit.
    Braziers,
    /// Decorate: a cornice under the ceiling along every wall.
    Cornice,
    /// Decorate: a skirting along the floor of every wall with no doorway.
    Skirting,
    /// Decorate: a column out from each corner.
    Columns,
}

impl RoomPart {
    /// How far a bay reaches out of its wall, which a room's rock must hold.
    pub const BAY_DEPTH: f64 = BAY_DEPTH;

    /// Every part.
    pub const ALL: [Self; 5] = [
        Self::Bays,
        Self::Braziers,
        Self::Cornice,
        Self::Skirting,
        Self::Columns,
    ];

    /// Its name, as a summary gives it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Bays => "bays",
            Self::Braziers => "braziers",
            Self::Cornice => "cornice",
            Self::Skirting => "skirting",
            Self::Columns => "columns",
        }
    }

    /// Whether a room of `shape` takes the part: its walls are upright and whole where the
    /// part goes.
    #[must_use]
    pub const fn fits(self, shape: RoomShape) -> bool {
        match self {
            Self::Bays | Self::Braziers => matches!(
                shape,
                RoomShape::Hall
                    | RoomShape::Pit
                    | RoomShape::Court
                    | RoomShape::Vault
                    | RoomShape::Atrium
            ),
            Self::Cornice => matches!(
                shape,
                RoomShape::Hall | RoomShape::Pit | RoomShape::Split | RoomShape::Atrium
            ),
            Self::Skirting => !matches!(shape, RoomShape::Chamber | RoomShape::Cross),
            Self::Columns => matches!(
                shape,
                RoomShape::Court | RoomShape::Vault | RoomShape::Split
            ),
        }
    }
}

impl Room<'_> {
    /// The walls with no doorway, and not an atrium's balcony side.
    fn bare_walls(&self) -> Vec<Side> {
        Side::ALL
            .into_iter()
            .filter(|side| !self.doors.contains(side))
            .filter(|&side| !(self.plan.shape == RoomShape::Atrium && side == self.plan.turned))
            .collect()
    }

    /// The box `out` beyond the room's middle toward `side` and `across` along its wall,
    /// between the heights `z`, as from the room's middle.
    fn wall_box(&self, side: Side, out: [f64; 2], across: [f64; 2], z: [f64; 2]) -> Bounds {
        let [fx, fy] = side.facing();
        let [o0, o1] = out;
        let [a0, a1] = across;
        // Along the wall is a quarter turn from the way it faces.
        let corner = |o: f64, a: f64| [fx * o - fy * a, fy * o + fx * a];
        let [x0, y0] = corner(o0, a0);
        let [x1, y1] = corner(o1, a1);
        self.bounds([x0.min(x1), x0.max(x1)], [y0.min(y1), y0.max(y1)], z)
    }

    /// How far the wall on `side` runs each way from its middle.
    fn wall_half(&self, side: Side) -> f64 {
        let [hx, hy] = self.plan.half;
        let [fx, _] = side.facing();
        if fx.abs() > 0.0 { hy } else { hx }
    }

    /// Dresses the room with its parts, up to [`ROOM_BUDGET`] brushes: bays first, then what
    /// stands in them, then what decorates the walls.
    ///
    /// # Errors
    ///
    /// The [`BrushError`] of a brush that could not be made.
    pub(super) fn dress(
        &self,
        mass: &mut Mass,
        paints: &Paints,
        built: &mut Built,
    ) -> Result<(), BrushError> {
        let start = built.solids.len();
        let room_left = |built: &Built| built.solids.len().saturating_sub(start) < ROOM_BUDGET;
        let floor = self.plan.floor;
        let top = floor + self.plan.height;
        let trim = Coat::all(&paints.trim);
        let support = Coat::all(&paints.support);
        let mut bays = Vec::new();
        if self.takes(RoomPart::Bays) {
            for side in self.bare_walls().into_iter().take(MOST_BAYS) {
                let reach = self.plan.reach(side);
                let half = (self.wall_half(side) / 2.0 - BAY_MARGIN).min(BAY_HALF);
                let space = Coat::space(&paints.floor, &paints.ceiling, &paints.wall);
                mass.carve(&space.boxed(self.wall_box(
                    side,
                    [reach, reach + BAY_DEPTH],
                    [-half, half],
                    [floor, floor + BAY_HEIGHT],
                ))?)?;
                bays.push(side);
            }
        }
        if self.takes(RoomPart::Braziers) {
            for &side in &bays {
                if !room_left(built) {
                    break;
                }
                let out = self.plan.reach(side) + BAY_DEPTH / 2.0;
                built.solids.push(support.boxed(self.wall_box(
                    side,
                    [out - BRAZIER_HALF, out + BRAZIER_HALF],
                    [-BRAZIER_HALF, BRAZIER_HALF],
                    [floor, floor + BRAZIER_HEIGHT],
                ))?);
                let [fx, fy] = side.facing();
                built.lights.push((
                    self.at(
                        [fx * out, fy * out],
                        floor + BRAZIER_HEIGHT + CORNICE_HEIGHT,
                    ),
                    BRAZIER_LIGHT,
                ));
            }
        }
        if self.takes(RoomPart::Cornice) {
            for side in Side::ALL {
                if !room_left(built) {
                    break;
                }
                let reach = self.plan.reach(side);
                let half = self.wall_half(side);
                built.solids.push(trim.boxed(self.wall_box(
                    side,
                    [reach - CORNICE_DEPTH, reach],
                    [-half, half],
                    [top - CORNICE_HEIGHT, top],
                ))?);
            }
        }
        if self.takes(RoomPart::Skirting) {
            for side in self.bare_walls() {
                if !room_left(built) || bays.contains(&side) {
                    continue;
                }
                let reach = self.plan.reach(side);
                let half = self.wall_half(side) - SKIRTING_DEPTH;
                built.solids.push(trim.boxed(self.wall_box(
                    side,
                    [reach - SKIRTING_DEPTH, reach],
                    [-half, half],
                    [floor, floor + SKIRTING_HEIGHT],
                ))?);
            }
        }
        if self.takes(RoomPart::Columns) {
            let (corners, _) = self.around(COLUMN_INSET);
            let [px, py, _] = self.stand();
            for [x, y] in corners {
                if !room_left(built) {
                    break;
                }
                // Not where the player, the key or the exit stands.
                let [cx, cy, _] = self.at([x, y], floor);
                if (px - cx).abs() <= COLUMN_CLEAR && (py - cy).abs() <= COLUMN_CLEAR {
                    continue;
                }
                built.solids.push(support.boxed(self.bounds(
                    [x - COLUMN_HALF, x + COLUMN_HALF],
                    [y - COLUMN_HALF, y + COLUMN_HALF],
                    [floor, top],
                ))?);
                built.spots.retain(|&[sx, sy, _]| {
                    (sx - cx).abs() > COLUMN_CLEAR || (sy - cy).abs() > COLUMN_CLEAR
                });
            }
        }
        for &side in &bays {
            let out = self.plan.reach(side) + BAY_DEPTH / 2.0;
            let [fx, fy] = side.facing();
            // In the bay beside its brazier, or in its middle when it has none.
            let over = if self.takes(RoomPart::Braziers) {
                SPOT_INSET
            } else {
                0.0
            };
            built
                .spots
                .push(self.standing([fx * out - fy * over, fy * out + fx * over], floor));
        }
        Ok(())
    }

    /// Whether the room is dressed with `part`.
    fn takes(&self, part: RoomPart) -> bool {
        self.dress.contains(&part) && part.fits(self.plan.shape)
    }
}
