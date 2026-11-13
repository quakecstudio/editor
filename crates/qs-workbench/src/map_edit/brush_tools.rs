// SPDX-License-Identifier: GPL-3.0-or-later

//! The brush tools beyond P6-08's (P6-18, MAP-15): `map.brush_scale` and `map.brush_shear`
//! about a point, `map.brush_extrude` of a face, `map.brush_sweep` along a path,
//! `map.brush_hollow`, `map.brush_intersect`, `map.brush_vertices` for several vertices at
//! once, `map.brush_split_edge` and `map.brush_split_face` by a new vertex, and
//! `map.brush_chamfer` of an edge. Each is made through `qs-map`'s brush tools, which give only
//! valid brushes, keeps textures fixed in the world unless lock is free, and has undo; an edit
//! the tools refuse changes nothing.

use qs_commands::{Command, Outcome};
use qs_map::{Affine, Brush, BrushError, BrushFace, WorldAxis};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::brushes::{BrushIndex, Lock, snap};
use super::{EntityIndex, MapDocument, MapEdited, MapStep, edit_map};
use crate::map_uv::FaceRef;
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.brush_scale`.
pub const MAP_BRUSH_SCALE: &str = "map.brush_scale";
/// The id of `map.brush_shear`.
pub const MAP_BRUSH_SHEAR: &str = "map.brush_shear";
/// The id of `map.brush_extrude`.
pub const MAP_BRUSH_EXTRUDE: &str = "map.brush_extrude";
/// The id of `map.brush_sweep`.
pub const MAP_BRUSH_SWEEP: &str = "map.brush_sweep";
/// The id of `map.brush_hollow`.
pub const MAP_BRUSH_HOLLOW: &str = "map.brush_hollow";
/// The id of `map.brush_intersect`.
pub const MAP_BRUSH_INTERSECT: &str = "map.brush_intersect";
/// The id of `map.brush_vertices`.
pub const MAP_BRUSH_VERTICES: &str = "map.brush_vertices";
/// The id of `map.brush_split_edge`.
pub const MAP_BRUSH_SPLIT_EDGE: &str = "map.brush_split_edge";
/// The id of `map.brush_split_face`.
pub const MAP_BRUSH_SPLIT_FACE: &str = "map.brush_split_face";
/// The id of `map.brush_chamfer`.
pub const MAP_BRUSH_CHAMFER: &str = "map.brush_chamfer";

/// An axis of the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// x.
    X,
    /// y.
    Y,
    /// z.
    Z,
}

impl From<Axis> for WorldAxis {
    fn from(axis: Axis) -> Self {
        match axis {
            Axis::X => Self::X,
            Axis::Y => Self::Y,
            Axis::Z => Self::Z,
        }
    }
}

/// The parameters of `map.brush_scale`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushScaleParams {
    /// The entity holding the brushes.
    pub entity: EntityIndex,
    /// The brushes.
    pub brushes: Vec<BrushIndex>,
    /// The factors along x, y and z; a negative one mirrors.
    pub factors: [f64; 3],
    /// The point kept where it is; the middle of the brushes' bounds when not given.
    #[serde(default)]
    pub about: Option<[f64; 3]>,
    /// Whether their textures stay fixed in the world; locked when not given.
    #[serde(default)]
    pub lock: Lock,
}

/// The parameters of `map.brush_shear`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushShearParams {
    /// The entity holding the brushes.
    pub entity: EntityIndex,
    /// The brushes.
    pub brushes: Vec<BrushIndex>,
    /// The axis points move along.
    pub along: Axis,
    /// The axis their distance from `about` is measured along; another than `along`.
    pub by: Axis,
    /// How far a point moves along `along` for each unit of that distance.
    pub factor: f64,
    /// The point kept where it is; the middle of the brushes' bounds when not given.
    #[serde(default)]
    pub about: Option<[f64; 3]>,
    /// Whether their textures stay fixed in the world; locked when not given.
    #[serde(default)]
    pub lock: Lock,
}

/// The parameters of `map.brush_extrude`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushExtrudeParams {
    /// The face.
    pub face: FaceRef,
    /// How far it moves along its outward normal, snapped to the grid; in when negative.
    pub by: f64,
    /// Whether its texture stays fixed in the world; locked when not given.
    #[serde(default)]
    pub lock: Lock,
    /// The grid `by` snaps to; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// The parameters of `map.brush_sweep`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushSweepParams {
    /// The entity holding the brush.
    pub entity: EntityIndex,
    /// The brush; it is replaced by one brush for each step of the path.
    pub brush: BrushIndex,
    /// The path, two or more points, each snapped to the grid; the brush starts at the first.
    pub path: Vec<[f64; 3]>,
    /// Whether the textures stay fixed in the world; locked when not given.
    #[serde(default)]
    pub lock: Lock,
    /// The grid the points snap to; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// The parameters of `map.brush_hollow`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushHollowParams {
    /// The entity holding the brush.
    pub entity: EntityIndex,
    /// The brush; it is replaced by its walls.
    pub brush: BrushIndex,
    /// The walls' thickness, in map units.
    pub wall: f64,
}

/// The parameters of `map.brush_intersect`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushIntersectParams {
    /// The entity holding the brushes.
    pub entity: EntityIndex,
    /// The brushes, two or more; their intersection takes the first's place.
    pub brushes: Vec<BrushIndex>,
}

/// A vertex moved.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VertexMove {
    /// The vertex.
    pub from: [f64; 3],
    /// Where it goes, snapped to the grid.
    pub to: [f64; 3],
}

/// The parameters of `map.brush_vertices`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushVerticesParams {
    /// The entity holding the brush.
    pub entity: EntityIndex,
    /// The brush.
    pub brush: BrushIndex,
    /// The vertices moved, all at once.
    pub moves: Vec<VertexMove>,
    /// The grid each `to` snaps to; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// The parameters of `map.brush_split_edge`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushSplitEdgeParams {
    /// The entity holding the brush.
    pub entity: EntityIndex,
    /// The brush.
    pub brush: BrushIndex,
    /// The edge's two vertices.
    pub edge: [[f64; 3]; 2],
    /// The new vertex, snapped to the grid.
    pub to: [f64; 3],
    /// The grid `to` snaps to; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// The parameters of `map.brush_split_face`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushSplitFaceParams {
    /// The face.
    pub face: FaceRef,
    /// The new vertex, in front of the face, snapped to the grid.
    pub to: [f64; 3],
    /// The grid `to` snaps to; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// The parameters of `map.brush_chamfer`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushChamferParams {
    /// The entity holding the brush.
    pub entity: EntityIndex,
    /// The brush.
    pub brush: BrushIndex,
    /// The edge's two vertices.
    pub edge: [[f64; 3]; 2],
    /// How far along each of the edge's faces the bevel starts, in map units.
    pub distance: f64,
    /// How many bevel faces, 1 to 16; one when not given.
    #[serde(default = "one")]
    pub faces: u32,
}

const fn one() -> u32 {
    1
}

impl MapDocument {
    /// The middle of the bounds of `brushes` of `entity`.
    fn middle(
        &mut self,
        entity: EntityIndex,
        brushes: &[BrushIndex],
    ) -> Result<[f64; 3], WorkbenchError> {
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for &brush in brushes {
            for vertex in self.brush(entity, brush)?.vertices() {
                for ((lo, hi), c) in low.iter_mut().zip(&mut high).zip(vertex) {
                    *lo = lo.min(c);
                    *hi = hi.max(c);
                }
            }
        }
        if low.iter().any(|c| !c.is_finite()) {
            return Err(BrushError::Degenerate.into());
        }
        Ok([0, 1, 2].map(|i| {
            let (lo, hi) = (
                low.get(i).copied().unwrap_or_default(),
                high.get(i).copied().unwrap_or_default(),
            );
            (lo + hi) / 2.0
        }))
    }

    /// Each of `brushes` of `entity` replaced by itself mapped by `map`.
    fn transform(
        &mut self,
        entity: EntityIndex,
        brushes: &[BrushIndex],
        map: &Affine,
        lock: Lock,
    ) -> Result<(), WorkbenchError> {
        for &brush in brushes {
            let moved = self.brush(entity, brush)?.transformed(map, lock.into())?;
            self.replace_brush(entity, brush, vec![moved])?;
        }
        Ok(())
    }

    /// The brush `brush` of `entity` replaced by what `tool` makes of it.
    fn rework(
        &mut self,
        entity: EntityIndex,
        brush: BrushIndex,
        tool: impl FnOnce(&Brush) -> Result<Vec<Brush>, BrushError>,
    ) -> Result<(), WorkbenchError> {
        let made = tool(&self.brush(entity, brush)?)?;
        self.replace_brush(entity, brush, made)
    }
}

impl MapEdited {
    /// The undoable outcome of an edit to `entity`.
    fn undoable<C: HasWorkbench + 'static>(entity: EntityIndex, step: MapStep) -> Outcome<Self, C> {
        Outcome::undoable(
            Self {
                entity: Some(entity),
            },
            step,
        )
    }
}

/// `map.brush_scale`.
pub(super) struct BrushScale;

impl<C: HasWorkbench + 'static> Command<C> for BrushScale {
    const ID: &'static str = MAP_BRUSH_SCALE;
    const SUMMARY: &'static str = "Scales brushes of an entity by factors along x, y and z about \
        a point, the middle of their bounds unless given, their textures fixed in the world \
        unless lock is free (MAP-15.2).";
    type Params = BrushScaleParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushScaleParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            let about = match params.about {
                Some(about) => about,
                None => document.middle(params.entity, &params.brushes)?,
            };
            let map = Affine::scale(about, params.factors);
            document.transform(params.entity, &params.brushes, &map, params.lock)
        })?;
        Ok(MapEdited::undoable(params.entity, step))
    }
}

/// `map.brush_shear`.
pub(super) struct BrushShear;

impl<C: HasWorkbench + 'static> Command<C> for BrushShear {
    const ID: &'static str = MAP_BRUSH_SHEAR;
    const SUMMARY: &'static str = "Shears brushes of an entity: each point moves along one axis \
        by a factor times its distance from a point along another, their textures fixed in the \
        world unless lock is free (MAP-15.2).";
    type Params = BrushShearParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushShearParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            let about = match params.about {
                Some(about) => about,
                None => document.middle(params.entity, &params.brushes)?,
            };
            let map = Affine::shear(about, params.along.into(), params.by.into(), params.factor)
                .ok_or(BrushError::Degenerate)?;
            document.transform(params.entity, &params.brushes, &map, params.lock)
        })?;
        Ok(MapEdited::undoable(params.entity, step))
    }
}

/// `map.brush_extrude`.
pub(super) struct BrushExtrude;

impl<C: HasWorkbench + 'static> Command<C> for BrushExtrude {
    const ID: &'static str = MAP_BRUSH_EXTRUDE;
    const SUMMARY: &'static str = "Moves a face of a brush along its normal by a distance \
        snapped to the grid, out or in, its texture fixed in the world unless lock is free \
        (MAP-15.3).";
    type Params = BrushExtrudeParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushExtrudeParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let FaceRef {
            entity,
            brush,
            face,
        } = params.face;
        let by = snap(params.by, params.grid);
        let ((), step) = edit_map(context, |document| {
            document.rework(entity, brush, |found| {
                Ok(vec![found.extruded(
                    BrushFace::new(face.get()),
                    by,
                    params.lock.into(),
                )?])
            })
        })?;
        Ok(MapEdited::undoable(entity, step))
    }
}

/// `map.brush_sweep`.
pub(super) struct BrushSweep;

impl<C: HasWorkbench + 'static> Command<C> for BrushSweep {
    const ID: &'static str = MAP_BRUSH_SWEEP;
    const SUMMARY: &'static str = "Sweeps a brush along a path of points snapped to the grid: \
        the brush becomes one brush for each step, the hull of it at the step's two ends \
        (MAP-15.4).";
    type Params = BrushSweepParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushSweepParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let path: Vec<[f64; 3]> = params
            .path
            .iter()
            .map(|p| p.map(|c| snap(c, params.grid)))
            .collect();
        let ((), step) = edit_map(context, |document| {
            document.rework(params.entity, params.brush, |found| {
                found.swept(&path, params.lock.into())
            })
        })?;
        Ok(MapEdited::undoable(params.entity, step))
    }
}

/// `map.brush_hollow`.
pub(super) struct BrushHollow;

impl<C: HasWorkbench + 'static> Command<C> for BrushHollow {
    const ID: &'static str = MAP_BRUSH_HOLLOW;
    const SUMMARY: &'static str = "Hollows a brush into walls of a thickness: the brush becomes \
        itself with the brush its walls enclose taken out (MAP-15.5).";
    type Params = BrushHollowParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushHollowParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            document.rework(params.entity, params.brush, |found| {
                found.hollowed(params.wall)
            })
        })?;
        Ok(MapEdited::undoable(params.entity, step))
    }
}

/// `map.brush_intersect`.
pub(super) struct BrushIntersect;

impl<C: HasWorkbench + 'static> Command<C> for BrushIntersect {
    const ID: &'static str = MAP_BRUSH_INTERSECT;
    const SUMMARY: &'static str = "Replaces two or more brushes of an entity with the solid all \
        of them hold, which takes the first's place (MAP-15.6).";
    type Params = BrushIntersectParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushIntersectParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            let mut indices = params.brushes.clone();
            indices.sort();
            indices.dedup();
            let (Some(&first), true) = (indices.first(), indices.len() > 1) else {
                return Err(BrushError::Degenerate.into());
            };
            let mut brushes = Vec::with_capacity(indices.len());
            for &index in &indices {
                brushes.push(document.brush(params.entity, index)?);
            }
            let (head, rest) = brushes.split_first().ok_or(BrushError::Degenerate)?;
            let others: Vec<&Brush> = rest.iter().collect();
            let both = head.intersected(&others)?;
            for &index in indices.iter().skip(1).rev() {
                document.replace_brush(params.entity, index, Vec::new())?;
            }
            document.replace_brush(params.entity, first, vec![both])
        })?;
        Ok(MapEdited::undoable(params.entity, step))
    }
}

/// `map.brush_vertices`.
pub(super) struct BrushVertices;

impl<C: HasWorkbench + 'static> Command<C> for BrushVertices {
    const ID: &'static str = MAP_BRUSH_VERTICES;
    const SUMMARY: &'static str = "Moves several vertices of a brush at once, each to a point \
        snapped to the grid; the brush becomes the hull of its vertices so moved (MAP-15.7).";
    type Params = BrushVerticesParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushVerticesParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let moves: Vec<([f64; 3], [f64; 3])> = params
            .moves
            .iter()
            .map(|m| (m.from, m.to.map(|c| snap(c, params.grid))))
            .collect();
        let ((), step) = edit_map(context, |document| {
            document.rework(params.entity, params.brush, |found| {
                Ok(vec![found.moved_vertices(&moves)?])
            })
        })?;
        Ok(MapEdited::undoable(params.entity, step))
    }
}

/// `map.brush_split_edge`.
pub(super) struct BrushSplitEdge;

impl<C: HasWorkbench + 'static> Command<C> for BrushSplitEdge {
    const ID: &'static str = MAP_BRUSH_SPLIT_EDGE;
    const SUMMARY: &'static str = "Splits an edge of a brush by a new vertex at a point snapped \
        to the grid (MAP-15.8).";
    type Params = BrushSplitEdgeParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushSplitEdgeParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let [a, b] = params.edge;
        let to = params.to.map(|c| snap(c, params.grid));
        let ((), step) = edit_map(context, |document| {
            document.rework(params.entity, params.brush, |found| {
                Ok(vec![found.split_edge(a, b, to)?])
            })
        })?;
        Ok(MapEdited::undoable(params.entity, step))
    }
}

/// `map.brush_split_face`.
pub(super) struct BrushSplitFace;

impl<C: HasWorkbench + 'static> Command<C> for BrushSplitFace {
    const ID: &'static str = MAP_BRUSH_SPLIT_FACE;
    const SUMMARY: &'static str = "Splits a face of a brush by a new vertex in front of it, at a \
        point snapped to the grid (MAP-15.8).";
    type Params = BrushSplitFaceParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushSplitFaceParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let FaceRef {
            entity,
            brush,
            face,
        } = params.face;
        let to = params.to.map(|c| snap(c, params.grid));
        let ((), step) = edit_map(context, |document| {
            document.rework(entity, brush, |found| {
                Ok(vec![found.split_face(BrushFace::new(face.get()), to)?])
            })
        })?;
        Ok(MapEdited::undoable(entity, step))
    }
}

/// `map.brush_chamfer`.
pub(super) struct BrushChamfer;

impl<C: HasWorkbench + 'static> Command<C> for BrushChamfer {
    const ID: &'static str = MAP_BRUSH_CHAMFER;
    const SUMMARY: &'static str = "Chamfers an edge of a brush into one or more bevel faces, from \
        a distance along each of its two faces (MAP-15.9).";
    type Params = BrushChamferParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushChamferParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let [a, b] = params.edge;
        let ((), step) = edit_map(context, |document| {
            document.rework(params.entity, params.brush, |found| {
                Ok(vec![found.chamfered(
                    a,
                    b,
                    params.distance,
                    params.faces,
                )?])
            })
        })?;
        Ok(MapEdited::undoable(params.entity, step))
    }
}

#[cfg(test)]
mod tests;
