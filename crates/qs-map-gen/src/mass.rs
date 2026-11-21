// SPDX-License-Identifier: GPL-3.0-or-later

//! The solid a level is taken out of. Each room's cell starts as one block of rock; a room or
//! a passage is a convex space subtracted from the blocks it reaches
//! ([`qs_map::Brush::subtract`]), which paints what is left with the space's own faces. So a
//! level leaks nowhere: whatever was not taken out is still rock.
//!
//! Every space has its corners on whole units and its faces level, upright or at half a
//! right angle, so no corner the subtraction makes is off the grid (MAP-3.3).

use qs_map::{Brush, BrushError, Surface};

use crate::layout::{Cell, HALF_CELL};

/// The lowest and the highest a block reaches: below the deepest pit and above the highest
/// dome, with rock to spare.
const BLOCK_BOTTOM: f64 = -384.0;
const BLOCK_TOP: f64 = 704.0;

/// How far up or down a face's normal points before the face is a top or a bottom, which a
/// face at half a right angle is not, and before it is a slope and no wall.
const LEVEL: f64 = 0.8;
const SLOPING: f64 = 0.3;

/// A point in map units.
pub(crate) type Point = [f64; 3];

/// A box: its lowest and its highest corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Bounds {
    /// The lowest corner.
    pub(crate) min: Point,
    /// The highest corner.
    pub(crate) max: Point,
}

impl Bounds {
    /// The box around `points`; `None` with none.
    fn around(points: &[Point]) -> Option<Self> {
        let (&first, rest) = points.split_first()?;
        let mut bounds = Self {
            min: first,
            max: first,
        };
        for point in rest {
            for ((low, high), &at) in bounds.min.iter_mut().zip(&mut bounds.max).zip(point) {
                *low = low.min(at);
                *high = high.max(at);
            }
        }
        Some(bounds)
    }

    /// Whether the two boxes share any volume.
    fn overlaps(&self, other: &Self) -> bool {
        self.min
            .iter()
            .zip(&other.max)
            .chain(other.min.iter().zip(&self.max))
            .all(|(low, high)| low < high)
    }
}

/// What the faces of a brush are painted with, by the way each faces.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Coat<'a> {
    /// The faces that look up.
    pub(crate) top: &'a Surface,
    /// The faces that look down.
    pub(crate) bottom: &'a Surface,
    /// The upright faces.
    pub(crate) sides: &'a Surface,
    /// The faces that slope.
    pub(crate) slopes: &'a Surface,
}

impl<'a> Coat<'a> {
    /// One surface on every face.
    pub(crate) const fn all(surface: &'a Surface) -> Self {
        Self {
            top: surface,
            bottom: surface,
            sides: surface,
            slopes: surface,
        }
    }

    /// The coat of something walked on: `floor` on top, `trim` on the rest.
    pub(crate) const fn deck(floor: &'a Surface, trim: &'a Surface) -> Self {
        Self {
            top: floor,
            bottom: trim,
            sides: trim,
            slopes: trim,
        }
    }

    /// The coat of a space in the rock, whose faces paint what is left around it: its bottom
    /// is the floor, its top the ceiling.
    pub(crate) const fn space(floor: &'a Surface, ceiling: &'a Surface, wall: &'a Surface) -> Self {
        Self {
            top: ceiling,
            bottom: floor,
            sides: wall,
            slopes: wall,
        }
    }

    /// The brush around `corners`, painted with the coat.
    ///
    /// # Errors
    ///
    /// The [`BrushError`] of corners that span no volume.
    pub(crate) fn hull(&self, corners: &[Point]) -> Result<Brush, BrushError> {
        let mut brush = Brush::convex_hull(corners, self.sides)?;
        for face in &mut brush.faces {
            let Some(plane) = face.plane() else { continue };
            let [_, _, up] = plane.normal;
            let surface = if up > LEVEL {
                self.top
            } else if up < -LEVEL {
                self.bottom
            } else if up.abs() > SLOPING {
                self.slopes
            } else {
                self.sides
            };
            let points = *face.points();
            face.set(points, &surface.texture, surface.texdef)?;
        }
        Ok(brush)
    }

    /// The box from `min` to `max`, painted with the coat.
    ///
    /// # Errors
    ///
    /// The [`BrushError`] of a box with no volume.
    pub(crate) fn boxed(&self, bounds: Bounds) -> Result<Brush, BrushError> {
        let Bounds {
            min: [x0, y0, z0],
            max: [x1, y1, z1],
        } = bounds;
        self.hull(&[
            [x0, y0, z0],
            [x1, y0, z0],
            [x1, y1, z0],
            [x0, y1, z0],
            [x0, y0, z1],
            [x1, y0, z1],
            [x1, y1, z1],
            [x0, y1, z1],
        ])
    }

    /// The upright prism over `outline` from `low` to `high`, painted with the coat.
    ///
    /// # Errors
    ///
    /// The [`BrushError`] of an outline with no area.
    pub(crate) fn prism(
        &self,
        outline: &[[f64; 2]],
        [low, high]: [f64; 2],
    ) -> Result<Brush, BrushError> {
        let corners: Vec<Point> = outline
            .iter()
            .flat_map(|&[x, y]| [[x, y, low], [x, y, high]])
            .collect();
        self.hull(&corners)
    }
}

/// One cell's rock: what is left of its block.
#[derive(Clone, Debug)]
struct Block {
    bounds: Bounds,
    pieces: Vec<Brush>,
}

/// The rock of a level.
#[derive(Clone, Debug, Default)]
pub(crate) struct Mass {
    blocks: Vec<Block>,
}

impl Mass {
    /// A block of rock for each of `cells`, painted with `rock`.
    ///
    /// # Errors
    ///
    /// The [`BrushError`] of a block that could not be made.
    pub(crate) fn of(cells: &[Cell], rock: &Surface) -> Result<Self, BrushError> {
        let blocks: Vec<Bounds> = cells
            .iter()
            .map(|cell| {
                let [x, y] = cell.middle();
                Bounds {
                    min: [x - HALF_CELL, y - HALF_CELL, BLOCK_BOTTOM],
                    max: [x + HALF_CELL, y + HALF_CELL, BLOCK_TOP],
                }
            })
            .collect();
        Self::of_blocks(&blocks, rock)
    }

    /// Rock filling each of `bounds`, painted with `rock`.
    ///
    /// # Errors
    ///
    /// The [`BrushError`] of a block with no volume.
    pub(crate) fn of_blocks(bounds: &[Bounds], rock: &Surface) -> Result<Self, BrushError> {
        let mut blocks = Vec::new();
        for &bounds in bounds {
            blocks.push(Block {
                bounds,
                pieces: vec![Coat::all(rock).boxed(bounds)?],
            });
        }
        Ok(Self { blocks })
    }

    /// Takes `space` out of the rock.
    ///
    /// # Errors
    ///
    /// The [`BrushError`] of a piece too thin to be a brush.
    pub(crate) fn carve(&mut self, space: &Brush) -> Result<(), BrushError> {
        let Some(reach) = Bounds::around(&space.vertices()) else {
            return Err(BrushError::Degenerate);
        };
        for block in &mut self.blocks {
            if !block.bounds.overlaps(&reach) {
                continue;
            }
            let mut left = Vec::new();
            for piece in &block.pieces {
                left.extend(piece.subtract(space)?);
            }
            block.pieces = left;
        }
        Ok(())
    }

    /// The rock's brushes.
    pub(crate) fn into_brushes(self) -> Vec<Brush> {
        self.blocks
            .into_iter()
            .flat_map(|block| block.pieces)
            .collect()
    }
}
