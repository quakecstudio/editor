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
use crate::uv::FaceRef;
use crate::{HasMap, MapEditError};

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

/// The id of the command that mirrors a selection along an axis (P7-08).
pub const MAP_FLIP: &str = "map.flip";

/// The id of the command that turns a selection about the vertical (P7-08).
pub const MAP_TURN: &str = "map.turn";

/// The id of the command that puts one texture in another's place (P7-08).
pub const MAP_REPLACE_TEXTURE: &str = "map.replace_texture";

/// The parameters of `map.replace_texture`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplaceTextureParams {
    /// The texture replaced, in any case.
    pub from: String,
    /// The texture put in its place.
    pub to: String,
    /// The brushes it is replaced on; every brush of the map when empty.
    #[serde(default)]
    pub brushes: Vec<(EntityIndex, BrushIndex)>,
}

/// What `map.replace_texture` did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Replaced {
    /// How many faces took the new texture.
    pub faces: usize,
}

/// A selection to flip or turn: whole entities and brushes of entities.
#[derive(Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    /// Whole entities: a point entity's origin and facing, every brush of a brush entity.
    #[serde(default)]
    pub entities: Vec<EntityIndex>,
    /// Brushes, each by its entity.
    #[serde(default)]
    pub brushes: Vec<(EntityIndex, BrushIndex)>,
}

/// The parameters of `map.flip`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlipParams {
    /// What to flip.
    #[serde(flatten)]
    pub selection: Selection,
    /// The axis flipped along: x mirrors left and right.
    pub axis: Axis,
    /// Whether their textures stay fixed in the world; locked when not given.
    #[serde(default)]
    pub lock: Lock,
}

/// The parameters of `map.turn`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TurnParams {
    /// What to turn.
    #[serde(flatten)]
    pub selection: Selection,
    /// Degrees to turn about the vertical through the selection's middle, counter-clockwise
    /// seen from above.
    pub degrees: f64,
    /// Whether their textures stay fixed in the world; locked when not given.
    #[serde(default)]
    pub lock: Lock,
}

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

impl Axis {
    /// The scale factors that mirror along this axis.
    const fn mirror(self) -> [f64; 3] {
        match self {
            Self::X => [-1.0, 1.0, 1.0],
            Self::Y => [1.0, -1.0, 1.0],
            Self::Z => [1.0, 1.0, -1.0],
        }
    }
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
    /// What the move does; an extrusion when not given (P7-08).
    #[serde(default)]
    pub mode: ExtrudeMode,
}

/// What `map.brush_extrude` does with the face, as TrenchBroom's extrude tool's keys choose
/// (P7-08).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExtrudeMode {
    /// The face moves along its normal, the brush's other planes kept.
    #[default]
    Extrude,
    /// Out, a new brush of what the extrusion would add; in, the brush cut in two (Ctrl).
    Split,
    /// A new brush, the face and its copy moved out (Ctrl+Alt).
    Stamp,
    /// The face's corners move along its normal, the other faces tilting (Alt).
    Move,
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

/// Where a split puts its new vertex.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum NewVertex {
    /// At a point, snapped to the grid, 16 units unless given, 0 for none.
    Point {
        /// The point.
        to: [f64; 3],
        /// The grid.
        #[serde(default)]
        grid: Option<f64>,
    },
    /// A distance out from the middle of the edge or face, along its normal (MAP-15.8).
    Out {
        /// The distance, in map units.
        by: f64,
    },
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
    /// The new vertex.
    pub at: NewVertex,
}

/// The parameters of `map.brush_split_face`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushSplitFaceParams {
    /// The face.
    pub face: FaceRef,
    /// The new vertex, in front of the face.
    pub at: NewVertex,
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
    ) -> Result<[f64; 3], MapEditError> {
        let mut low = [f64::INFINITY, f64::INFINITY, f64::INFINITY];
        let mut high = [f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
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
    ) -> Result<(), MapEditError> {
        let mut brushes = brushes.to_vec();
        brushes.sort();
        brushes.dedup();
        for brush in brushes {
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
    ) -> Result<(), MapEditError> {
        let made = tool(&self.brush(entity, brush)?)?;
        self.replace_brush(entity, brush, made)
    }
}

impl MapEdited {
    /// The undoable outcome of an edit to `entity`.
    pub(super) fn undoable<C: HasMap + 'static>(
        entity: EntityIndex,
        step: MapStep,
    ) -> Outcome<Self, C> {
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

impl<C: HasMap + 'static> Command<C> for BrushScale {
    const ID: &'static str = MAP_BRUSH_SCALE;
    const SUMMARY: &'static str = "Scales brushes of an entity by factors along x, y and z about \
        a point, the middle of their bounds unless given, their textures fixed in the world \
        unless lock is free (MAP-15.2).";
    type Params = BrushScaleParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushScaleParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
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

impl<C: HasMap + 'static> Command<C> for BrushShear {
    const ID: &'static str = MAP_BRUSH_SHEAR;
    const SUMMARY: &'static str = "Shears brushes of an entity: each point moves along one axis \
        by a factor times its distance from a point along another, their textures fixed in the \
        world unless lock is free (MAP-15.2).";
    type Params = BrushShearParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushShearParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
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

impl<C: HasMap + 'static> Command<C> for BrushExtrude {
    const ID: &'static str = MAP_BRUSH_EXTRUDE;
    const SUMMARY: &'static str = "Moves a face of a brush along its normal by a distance \
        snapped to the grid, out or in, its texture fixed in the world unless lock is free \
        (MAP-15.3).";
    type Params = BrushExtrudeParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushExtrudeParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let FaceRef {
            entity,
            brush,
            face,
        } = params.face;
        let by = snap(params.by, params.grid);
        let face = BrushFace::new(face.get());
        let lock = params.lock.into();
        let ((), step) = edit_map(context, |document| {
            let surface = document.new_surface(None);
            document.rework(entity, brush, |found| match params.mode {
                ExtrudeMode::Extrude => Ok(vec![found.extruded(face, by, lock)?]),
                ExtrudeMode::Split => found.split_extruded(face, by, lock, &surface),
                ExtrudeMode::Stamp => Ok(vec![found.clone(), found.stamped(face, by, &surface)?]),
                ExtrudeMode::Move => Ok(vec![found.face_moved(face, by)?]),
            })
        })?;
        Ok(MapEdited::undoable(entity, step))
    }
}

/// `map.brush_sweep`.
pub(super) struct BrushSweep;

impl<C: HasMap + 'static> Command<C> for BrushSweep {
    const ID: &'static str = MAP_BRUSH_SWEEP;
    const SUMMARY: &'static str = "Sweeps a brush along a path of points snapped to the grid: \
        the brush becomes one brush for each step, the hull of it at the step's two ends \
        (MAP-15.4).";
    type Params = BrushSweepParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushSweepParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
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

impl<C: HasMap + 'static> Command<C> for BrushHollow {
    const ID: &'static str = MAP_BRUSH_HOLLOW;
    const SUMMARY: &'static str = "Hollows a brush into walls of a thickness: the brush becomes \
        itself with the brush its walls enclose taken out (MAP-15.5).";
    type Params = BrushHollowParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushHollowParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
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

impl<C: HasMap + 'static> Command<C> for BrushIntersect {
    const ID: &'static str = MAP_BRUSH_INTERSECT;
    const SUMMARY: &'static str = "Replaces two or more brushes of an entity with the solid all \
        of them hold, which takes the first's place (MAP-15.6).";
    type Params = BrushIntersectParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushIntersectParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
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

impl<C: HasMap + 'static> Command<C> for BrushVertices {
    const ID: &'static str = MAP_BRUSH_VERTICES;
    const SUMMARY: &'static str = "Moves several vertices of a brush at once, each to a point \
        snapped to the grid; the brush becomes the hull of its vertices so moved (MAP-15.7).";
    type Params = BrushVerticesParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushVerticesParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
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

impl<C: HasMap + 'static> Command<C> for BrushSplitEdge {
    const ID: &'static str = MAP_BRUSH_SPLIT_EDGE;
    const SUMMARY: &'static str = "Splits an edge of a brush by a new vertex at a point snapped \
        to the grid, or a distance out from the edge's middle (MAP-15.8).";
    type Params = BrushSplitEdgeParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushSplitEdgeParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let [a, b] = params.edge;
        let ((), step) = edit_map(context, |document| {
            document.rework(params.entity, params.brush, |found| {
                let to = match params.at {
                    NewVertex::Point { to, grid } => to.map(|c| snap(c, grid)),
                    NewVertex::Out { by } => {
                        found.edge_out(a, b, by).ok_or(BrushError::NoVertex(b))?
                    }
                };
                Ok(vec![found.split_edge(a, b, to)?])
            })
        })?;
        Ok(MapEdited::undoable(params.entity, step))
    }
}

/// `map.brush_split_face`.
pub(super) struct BrushSplitFace;

impl<C: HasMap + 'static> Command<C> for BrushSplitFace {
    const ID: &'static str = MAP_BRUSH_SPLIT_FACE;
    const SUMMARY: &'static str = "Splits a face of a brush by a new vertex in front of it, at a \
        point snapped to the grid, or a distance out from the face's middle (MAP-15.8).";
    type Params = BrushSplitFaceParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushSplitFaceParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let FaceRef {
            entity,
            brush,
            face,
        } = params.face;
        let face = BrushFace::new(face.get());
        let ((), step) = edit_map(context, |document| {
            document.rework(entity, brush, |found| {
                let to = match params.at {
                    NewVertex::Point { to, grid } => to.map(|c| snap(c, grid)),
                    NewVertex::Out { by } => {
                        found.face_out(face, by).ok_or(BrushError::Degenerate)?
                    }
                };
                Ok(vec![found.split_face(face, to)?])
            })
        })?;
        Ok(MapEdited::undoable(entity, step))
    }
}

/// `map.brush_chamfer`.
pub(super) struct BrushChamfer;

impl<C: HasMap + 'static> Command<C> for BrushChamfer {
    const ID: &'static str = MAP_BRUSH_CHAMFER;
    const SUMMARY: &'static str = "Chamfers an edge of a brush into one or more bevel faces, from \
        a distance along each of its two faces (MAP-15.9).";
    type Params = BrushChamferParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: BrushChamferParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
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

/// How a point entity's facing changes as its selection is flipped or turned.
#[derive(Clone, Copy, Debug)]
enum Facing {
    /// Mirrored along an axis.
    Flipped(Axis),
    /// Turned by degrees about the vertical.
    Turned(f64),
}

impl Facing {
    /// `angles`, pitch, yaw and roll, as they become: across x a yaw becomes 180 less it,
    /// across y its negative, across z a pitch its negative and straight up and down swap; a
    /// turn adds to the yaw. Straight up and down keep under a flip across x or y or a turn.
    fn of(self, angles: [f64; 3]) -> [f64; 3] {
        let [pitch, yaw, roll] = angles;
        let straight = [super::UP, super::DOWN].contains(&angles);
        let wrap = |degrees: f64| degrees.rem_euclid(super::FULL_TURN);
        match self {
            Self::Flipped(Axis::Z) if [super::UP].contains(&angles) => super::DOWN,
            Self::Flipped(Axis::Z) if [super::DOWN].contains(&angles) => super::UP,
            _ if straight => angles,
            Self::Flipped(Axis::X) => [pitch, wrap(super::FULL_TURN / 2.0 - yaw), roll],
            Self::Flipped(Axis::Y) => [pitch, wrap(-yaw), roll],
            Self::Flipped(Axis::Z) => [-pitch, yaw, roll],
            Self::Turned(degrees) => [pitch, wrap(yaw + degrees), roll],
        }
    }
}

impl MapDocument {
    /// The middle of the box around `selection`: its brushes' corners, every brush of its
    /// brush entities and its point entities' origins.
    fn selection_middle(&mut self, selection: &Selection) -> Result<[f64; 3], MapEditError> {
        let mut points = Vec::new();
        for &(entity, brush) in &selection.brushes {
            points.extend(self.brush(entity, brush)?.vertices());
        }
        for &entity in &selection.entities {
            let found = self.entity_mut(entity)?;
            if found.has_brushes() {
                points.extend(found.brushes().flat_map(Brush::vertices));
            } else {
                points.push(found.origin());
            }
        }
        let mut low = [f64::INFINITY, f64::INFINITY, f64::INFINITY];
        let mut high = [f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
        for point in points {
            for ((lo, hi), c) in low.iter_mut().zip(&mut high).zip(point) {
                *lo = lo.min(c);
                *hi = hi.max(c);
            }
        }
        if low.iter().any(|c| !c.is_finite()) {
            return Err(MapEditError::NoSelection);
        }
        let [lx, ly, lz] = low;
        let [hx, hy, hz] = high;
        Ok([(lx + hx) / 2.0, (ly + hy) / 2.0, (lz + hz) / 2.0])
    }

    /// `selection` mapped by `map`: its brushes, every brush of its brush entities, and its
    /// point entities' origins, their facing changed as `facing` says. The world is not a
    /// whole entity to move.
    fn arrange(
        &mut self,
        selection: &Selection,
        map: &Affine,
        facing: Facing,
        lock: Lock,
    ) -> Result<(), MapEditError> {
        let mut by_entity: std::collections::BTreeMap<EntityIndex, Vec<BrushIndex>> =
            std::collections::BTreeMap::new();
        for &(entity, brush) in &selection.brushes {
            by_entity.entry(entity).or_default().push(brush);
        }
        for &entity in &selection.entities {
            if entity.is_world() {
                return Err(MapEditError::NotPointEntity { entity });
            }
            let found = self.entity_mut(entity)?;
            if found.has_brushes() {
                let count = found.brushes().count();
                by_entity
                    .entry(entity)
                    .or_default()
                    .extend((0..count).map(BrushIndex::new));
                continue;
            }
            let origin = super::origin_text(map.apply(found.origin()));
            let angles = found.angles().map(|angles| facing.of(angles));
            let mut result = found.set(b"origin", origin.as_bytes());
            if let Some([pitch, yaw, roll]) = angles {
                result = result.and_then(|()| {
                    if found.value(b"angles").is_some() {
                        let text = [pitch, yaw, roll].map(super::number).join(" ");
                        found.set(b"angles", text.as_bytes())
                    } else {
                        found.set(b"angle", super::number(yaw).as_bytes())
                    }
                });
            }
            result.map_err(|error| self.map_error(error))?;
        }
        for (entity, brushes) in by_entity {
            self.transform(entity, &brushes, map, lock)?;
        }
        Ok(())
    }
}

/// `map.replace_texture`.
pub(super) struct ReplaceTexture;

impl<C: HasMap + 'static> Command<C> for ReplaceTexture {
    const ID: &'static str = MAP_REPLACE_TEXTURE;
    const SUMMARY: &'static str = "Puts one texture in another's place on the faces of brushes, \
        or of every brush of the map, keeping their alignment (TrenchBroom's Replace Material).";
    type Params = ReplaceTextureParams;
    type Output = Replaced;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: ReplaceTextureParams,
    ) -> Result<Outcome<Replaced, C>, MapEditError> {
        let (faces, step) = edit_map(context, |document| {
            let from = params.from.as_bytes();
            let mut replaced = 0_usize;
            // Over the whole map, a locked entity's faces are left as they are, as TrenchBroom
            // leaves what cannot be selected; a brush chosen in one is refused by the lock.
            let locked = document.locked_in(&document.file);
            let chosen = |entity: usize, brush: usize| {
                if params.brushes.is_empty() {
                    return !locked.contains(&EntityIndex::new(entity));
                }
                params
                    .brushes
                    .contains(&(EntityIndex::new(entity), BrushIndex::new(brush)))
            };
            for (place, entity) in document.file.entities.iter_mut().enumerate() {
                let brushes = entity.items.iter_mut().filter_map(|item| match item {
                    qs_map::Item::Brush(brush) => Some(brush),
                    qs_map::Item::Pair(_) => None,
                });
                for (index, brush) in brushes.enumerate() {
                    if !chosen(place, index) {
                        continue;
                    }
                    for face in &mut brush.faces {
                        if face.texture().eq_ignore_ascii_case(from) {
                            let (points, texdef) = (*face.points(), *face.texdef());
                            face.set(points, params.to.as_bytes(), texdef)
                                .map_err(|error| MapEditError::Brush {
                                    error: BrushError::Map(error),
                                })?;
                            replaced = replaced.saturating_add(1);
                        }
                    }
                }
            }
            if replaced == 0 {
                return Err(MapEditError::NoTexture {
                    texture: params.from.clone(),
                });
            }
            Ok(replaced)
        })?;
        Ok(Outcome::undoable(Replaced { faces }, step))
    }
}

/// `map.flip`.
pub(super) struct Flip;

impl<C: HasMap + 'static> Command<C> for Flip {
    const ID: &'static str = MAP_FLIP;
    const SUMMARY: &'static str = "Mirrors entities and brushes along an axis through the middle \
        of their box: brushes and point entities' origins, and their facing, as TrenchBroom's \
        Flip.";
    type Params = FlipParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: FlipParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let ((), step) = edit_map(context, |document| {
            let about = document.selection_middle(&params.selection)?;
            let map = Affine::scale(about, params.axis.mirror());
            document.arrange(
                &params.selection,
                &map,
                Facing::Flipped(params.axis),
                params.lock,
            )
        })?;
        Ok(MapEdited::undoable(params.selection.first(), step))
    }
}

/// `map.turn`.
pub(super) struct Turn;

impl<C: HasMap + 'static> Command<C> for Turn {
    const ID: &'static str = MAP_TURN;
    const SUMMARY: &'static str = "Turns entities and brushes by degrees about the vertical \
        through the middle of their box, counter-clockwise seen from above: brushes and point \
        entities' origins and facing, as TrenchBroom's Rotate.";
    type Params = TurnParams;
    type Output = MapEdited;
    type Error = MapEditError;

    fn run(
        &self,
        context: &mut C,
        params: TurnParams,
    ) -> Result<Outcome<MapEdited, C>, MapEditError> {
        let ((), step) = edit_map(context, |document| {
            let about = document.selection_middle(&params.selection)?;
            let map = Affine::turn(about, params.degrees);
            document.arrange(
                &params.selection,
                &map,
                Facing::Turned(params.degrees),
                params.lock,
            )
        })?;
        Ok(MapEdited::undoable(params.selection.first(), step))
    }
}

impl Selection {
    /// Its first entity, which the edit reports.
    fn first(&self) -> EntityIndex {
        self.entities
            .first()
            .copied()
            .or_else(|| self.brushes.first().map(|&(entity, _)| entity))
            .unwrap_or(EntityIndex::new(0))
    }
}

#[cfg(test)]
mod tests;
