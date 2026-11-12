// SPDX-License-Identifier: GPL-3.0-or-later

//! `map.brush_shape` (P6-18, MAP-10): a shape of brushes in a box, added to the world or an
//! entity with undo, made through `qs_map::shapes`, which gives only valid brushes; a shape
//! that would not be is refused and changes nothing.

use qs_commands::{Command, Outcome};
use qs_map::Item;
use qs_map::shapes::{
    CircleMode, CurveSides, Facing, SHAPE_STEP_DEPTH, SHAPE_STEP_HEIGHT, Shape, ShapeError,
    Spandrels, Wall,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::brushes::{DEFAULT_GRID, snap};
use super::{EntityIndex, MapEdited, edit_map};
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.brush_shape`.
pub const MAP_BRUSH_SHAPE: &str = "map.brush_shape";

/// Where a round shape's corners lie (MAP-10.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Circle {
    /// On the box's ellipse.
    #[default]
    Vertices,
    /// Outside it, each edge touching it.
    Edges,
}

impl From<Circle> for CircleMode {
    fn from(circle: Circle) -> Self {
        match circle {
            Circle::Vertices => Self::Vertices,
            Circle::Edges => Self::Edges,
        }
    }
}

/// The way stairs climb (MAP-10.10).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Climb {
    /// Toward +x.
    #[default]
    East,
    /// Toward -x.
    West,
    /// Toward +y.
    North,
    /// Toward -y.
    South,
}

impl From<Climb> for Facing {
    fn from(climb: Climb) -> Self {
        match climb {
            Climb::East => Self::East,
            Climb::West => Self::West,
            Climb::North => Self::North,
            Climb::South => Self::South,
        }
    }
}

/// Whether an arch fills its wall up to the box's top (MAP-10.6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArchWall {
    /// The arch alone.
    #[default]
    Open,
    /// The arch in a wall up to the box's top.
    Filled,
}

impl From<ArchWall> for Spandrels {
    fn from(wall: ArchWall) -> Self {
        match wall {
            ArchWall::Open => Self::Open,
            ArchWall::Filled => Self::Filled,
        }
    }
}

/// The sides a round shape has when none are given.
const DEFAULT_SIDES: u32 = 16;

/// The default sides of a round shape.
const fn default_sides() -> u32 {
    DEFAULT_SIDES
}

/// The bands a UV spheroid has when none are given.
const DEFAULT_RINGS: u32 = 8;

/// The default bands of a UV spheroid.
const fn default_rings() -> u32 {
    DEFAULT_RINGS
}

/// The degrees of an arch when none are given: half a turn.
const DEFAULT_ARC: f64 = 180.0;

/// The default arc of an arch, half a turn.
const fn default_arc() -> f64 {
    DEFAULT_ARC
}

/// An arch's thickness when none is given.
const DEFAULT_ARCH_THICKNESS: f64 = 16.0;

/// The default thickness of an arch.
const fn default_arch_thickness() -> f64 {
    DEFAULT_ARCH_THICKNESS
}

/// A rock's points when none are given.
const DEFAULT_POINTS: u32 = 16;

/// The default points of a rock.
const fn default_points() -> u32 {
    DEFAULT_POINTS
}

/// A scalable curve's sides when none are given.
const DEFAULT_CURVE_SIDES: u32 = 24;

/// The default sides of a scalable curve.
const fn default_curve_sides() -> u32 {
    DEFAULT_CURVE_SIDES
}

/// The default step height, under `STEPSIZE` (MAP-10.10).
const fn default_step_height() -> f64 {
    SHAPE_STEP_HEIGHT
}

/// The default step depth (MAP-10.10).
const fn default_step_depth() -> f64 {
    SHAPE_STEP_DEPTH
}

/// A shape and its settings, as `map.brush_shape` takes them (MAP-10).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShapeSpec {
    /// An upright prism around the box's ellipse; hollow with a wall thickness.
    Cylinder {
        /// Its sides, 3 to 128; 16 when not given.
        #[serde(default = "default_sides")]
        sides: u32,
        /// Where the corners lie.
        #[serde(default)]
        circle: Circle,
        /// The wall's thickness, for a hollow cylinder.
        #[serde(default)]
        hollow: Option<f64>,
    },
    /// A cone to the middle of the box's top.
    Cone {
        /// Its sides, 3 to 128; 16 when not given.
        #[serde(default = "default_sides")]
        sides: u32,
        /// Where the corners lie.
        #[serde(default)]
        circle: Circle,
    },
    /// An arch in the box's x-z face, within half a turn.
    Arch {
        /// Its segments, 3 to 128; 16 when not given.
        #[serde(default = "default_sides")]
        sides: u32,
        /// The degrees of its curve; 180 when not given.
        #[serde(default = "default_arc")]
        arc: f64,
        /// The degrees it starts at; 0 when not given.
        #[serde(default)]
        start: f64,
        /// Its thickness; 16 when not given.
        #[serde(default = "default_arch_thickness")]
        thickness: f64,
        /// Whether its wall is filled to the box's top.
        #[serde(default)]
        wall: ArchWall,
    },
    /// A spheroid of bands and slices.
    UvSpheroid {
        /// Its slices, 3 to 128; 16 when not given.
        #[serde(default = "default_sides")]
        sides: u32,
        /// Its bands, 2 to 64; 8 when not given.
        #[serde(default = "default_rings")]
        rings: u32,
    },
    /// A spheroid of an icosahedron's triangles.
    Icospheroid {
        /// The times each triangle is cut in four, 0 to 3.
        #[serde(default)]
        subdivisions: u32,
    },
    /// A rock, one per seed.
    Rock {
        /// Its points, 4 to 64; 16 when not given.
        #[serde(default = "default_points")]
        points: u32,
        /// Its seed.
        #[serde(default)]
        seed: u64,
    },
    /// A curve on the grid at every size, of 12, 24, 48 or 96 sides.
    Curve {
        /// Its sides; 24 when not given.
        #[serde(default = "default_curve_sides")]
        sides: u32,
        /// The wall's thickness, for a hollow curve.
        #[serde(default)]
        hollow: Option<f64>,
    },
    /// Straight stairs.
    Stairs {
        /// A step's height; 16 when not given.
        #[serde(default = "default_step_height")]
        height: f64,
        /// A step's depth; 32 when not given.
        #[serde(default = "default_step_depth")]
        depth: f64,
        /// The way they climb.
        #[serde(default)]
        climb: Climb,
    },
    /// Spiral stairs.
    SpiralStairs {
        /// Steps a turn, 3 to 128; 16 when not given.
        #[serde(default = "default_sides")]
        sides: u32,
        /// A step's height; 16 when not given.
        #[serde(default = "default_step_height")]
        height: f64,
        /// The radius of the empty middle; 0 when not given.
        #[serde(default)]
        core: f64,
    },
}

impl ShapeSpec {
    /// The shape, its curve on the grid `grid`.
    fn shape(self, grid: f64) -> Result<Shape, WorkbenchError> {
        Ok(match self {
            Self::Cylinder {
                sides,
                circle,
                hollow,
            } => Shape::Cylinder {
                sides,
                mode: circle.into(),
                wall: hollow.map_or(Wall::Solid, |thickness| Wall::Hollow { thickness }),
            },
            Self::Cone { sides, circle } => Shape::Cone {
                sides,
                mode: circle.into(),
            },
            Self::Arch {
                sides,
                arc,
                start,
                thickness,
                wall,
            } => Shape::Arch {
                sides,
                arc,
                start,
                thickness,
                spandrels: wall.into(),
            },
            Self::UvSpheroid { sides, rings } => Shape::UvSpheroid { sides, rings },
            Self::Icospheroid { subdivisions } => Shape::Icospheroid { subdivisions },
            Self::Rock { points, seed } => Shape::Rock { points, seed },
            Self::Curve { sides, hollow } => Shape::Curve {
                sides: match sides {
                    12 => CurveSides::Twelve,
                    24 => CurveSides::TwentyFour,
                    48 => CurveSides::FortyEight,
                    96 => CurveSides::NinetySix,
                    other => {
                        return Err(WorkbenchError::Shape {
                            error: ShapeError::CurveSides(other),
                        });
                    }
                },
                grid,
                wall: hollow.map_or(Wall::Solid, |thickness| Wall::Hollow { thickness }),
            },
            Self::Stairs {
                height,
                depth,
                climb,
            } => Shape::Stairs {
                height,
                depth,
                facing: climb.into(),
            },
            Self::SpiralStairs {
                sides,
                height,
                core,
            } => Shape::SpiralStairs {
                sides,
                height,
                core,
            },
        })
    }
}

/// The parameters of `map.brush_shape`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushShapeParams {
    /// The entity to add it to; the world when not given.
    #[serde(default)]
    pub entity: Option<EntityIndex>,
    /// One corner of the box it fills.
    pub min: [f64; 3],
    /// The opposite corner.
    pub max: [f64; 3],
    /// The shape.
    pub shape: ShapeSpec,
    /// Its texture; the texture of the map's last brush face when not given.
    #[serde(default)]
    pub texture: Option<String>,
    /// The grid the corners snap to, in map units; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

impl From<ShapeError> for WorkbenchError {
    fn from(error: ShapeError) -> Self {
        Self::Shape { error }
    }
}

/// `map.brush_shape`.
pub(super) struct BrushShape;

impl<C: HasWorkbench + 'static> Command<C> for BrushShape {
    const ID: &'static str = MAP_BRUSH_SHAPE;
    const SUMMARY: &'static str = "Adds a shape of brushes filling a box, its corners snapped to \
        the grid: a cylinder (solid or hollow), cone, arch, UV or icosahedron spheroid, rock, \
        scalable curve of 12, 24, 48 or 96 sides, or straight or spiral stairs whose default \
        step a player climbs; to the world or an entity of the open map, with a texture.";
    type Params = BrushShapeParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushShapeParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let entity = params.entity.unwrap_or(EntityIndex::WORLD);
        let grid = params.grid.unwrap_or(DEFAULT_GRID);
        let shape = params.shape.shape(if grid > 0.0 { grid } else { 1.0 })?;
        let ((), step) = edit_map(context, |document| {
            let [min, max] = [params.min, params.max].map(|p| p.map(|c| snap(c, params.grid)));
            let surface = document.new_surface(params.texture.as_deref());
            let brushes = shape.brushes(min, max, &surface)?;
            let target = document.entity_mut(entity)?;
            target.items.extend(brushes.into_iter().map(Item::Brush));
            Ok(())
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(entity),
            },
            step,
        ))
    }
}

#[cfg(test)]
mod tests;
