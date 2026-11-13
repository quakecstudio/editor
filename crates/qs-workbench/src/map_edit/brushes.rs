// SPDX-License-Identifier: GPL-3.0-or-later

//! Brush editing (P6-08, MAP-3): a box created on the grid, a brush clipped by a plane, a
//! vertex moved, a brush subtracted from the others of its entity, brushes merged into their
//! convex hull, and a brush deleted. Each is a `map.*` command with undo, made through
//! `qs-map`'s brush operations, which give only valid brushes; an edit that would not is
//! refused and changes nothing.

use qs_commands::{Command, Outcome};
use qs_map::geometry::plane as plane_of;
use qs_map::{Alignment, Brush, BrushError, Entity, Item, Surface, TexDef, TextureLock};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{EntityIndex, MapDocument, MapEdited, edit_map};
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.brush_create`.
pub const MAP_BRUSH_CREATE: &str = "map.brush_create";
/// The id of `map.brush_clip`.
pub const MAP_BRUSH_CLIP: &str = "map.brush_clip";
/// The id of `map.brush_vertex`.
pub const MAP_BRUSH_VERTEX: &str = "map.brush_vertex";
/// The id of `map.brush_subtract`.
pub const MAP_BRUSH_SUBTRACT: &str = "map.brush_subtract";
/// The id of `map.brush_merge`.
pub const MAP_BRUSH_MERGE: &str = "map.brush_merge";
/// The id of `map.brush_delete`.
pub const MAP_BRUSH_DELETE: &str = "map.brush_delete";
/// The id of `map.brush_move`.
pub const MAP_BRUSH_MOVE: &str = "map.brush_move";
/// The id of `map.brush_texture`.
pub const MAP_BRUSH_TEXTURE: &str = "map.brush_texture";

/// TrenchBroom's grid, `Grid(4)`: 2^4 units, the step points snap to by default.
pub(super) const DEFAULT_GRID: f64 = 16.0;
/// The texture a new brush takes in a map whose brushes name none.
const DEFAULT_TEXTURE: &[u8] = b"__TB_empty";

/// A brush, by the entity holding it and its place among that entity's brushes, from 0.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct BrushIndex(usize);

impl BrushIndex {
    /// The brush at `index`.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its place among its entity's brushes.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// Which parts of a clipped brush stay.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Keep {
    /// The part behind the plane, the side its normal points away from.
    #[default]
    Back,
    /// The part in front of it.
    Front,
    /// Both, as two brushes.
    Both,
}

/// The parameters of `map.brush_create`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushCreateParams {
    /// The entity to add it to; the world when not given.
    #[serde(default)]
    pub entity: Option<EntityIndex>,
    /// One corner of the box.
    pub min: [f64; 3],
    /// The opposite corner.
    pub max: [f64; 3],
    /// Its texture; the texture of the map's last brush face when not given.
    #[serde(default)]
    pub texture: Option<String>,
    /// The grid the corners snap to, in map units; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// The parameters of `map.brush_clip`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushClipParams {
    /// The entity holding the brush.
    pub entity: EntityIndex,
    /// The brush.
    pub brush: BrushIndex,
    /// Three points on the clipping plane; its normal is `(p0 - p1) x (p2 - p1)`, as a
    /// face's (MAP-3.4).
    pub points: [[f64; 3]; 3],
    /// Which parts stay.
    #[serde(default)]
    pub keep: Keep,
}

/// The parameters of `map.brush_vertex`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushVertexParams {
    /// The entity holding the brush.
    pub entity: EntityIndex,
    /// The brush.
    pub brush: BrushIndex,
    /// The vertex.
    pub from: [f64; 3],
    /// Where it goes.
    pub to: [f64; 3],
    /// The grid `to` snaps to; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// The parameters of `map.brush_subtract` and `map.brush_delete`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushParams {
    /// The entity holding the brush.
    pub entity: EntityIndex,
    /// The brush.
    pub brush: BrushIndex,
}

/// The parameters of `map.brush_merge`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushMergeParams {
    /// The entity holding the brushes.
    pub entity: EntityIndex,
    /// The brushes, two or more.
    pub brushes: Vec<BrushIndex>,
}

/// Whether a moved brush's textures stay fixed in the world (P6-09, MAP-11.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Lock {
    /// Texture lock on: the textures stay where they are in the world.
    #[default]
    Locked,
    /// Texture lock off: the textures move with the brush.
    Free,
}

impl From<Lock> for TextureLock {
    fn from(lock: Lock) -> Self {
        match lock {
            Lock::Locked => Self::Locked,
            Lock::Free => Self::Free,
        }
    }
}

/// The parameters of `map.brush_move`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushMoveParams {
    /// The entity holding the brushes.
    pub entity: EntityIndex,
    /// The brushes.
    pub brushes: Vec<BrushIndex>,
    /// How far to move them, snapped to the grid.
    pub by: [f64; 3],
    /// Whether their textures stay fixed in the world; locked when not given.
    #[serde(default)]
    pub lock: Lock,
    /// The grid the move snaps to, in map units; 16 when not given, 0 for none.
    #[serde(default)]
    pub grid: Option<f64>,
}

/// The parameters of `map.brush_texture`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BrushTextureParams {
    /// The entity holding the brushes.
    pub entity: EntityIndex,
    /// The brushes, every face of each.
    pub brushes: Vec<BrushIndex>,
    /// The texture to put on them; kept when not given.
    #[serde(default)]
    pub texture: Option<String>,
    /// The s and t shift; kept when not given.
    #[serde(default)]
    pub shift: Option<[f64; 2]>,
    /// The rotation in degrees; kept when not given.
    #[serde(default)]
    pub rotate: Option<f64>,
    /// The s and t scale; kept when not given.
    #[serde(default)]
    pub scale: Option<[f64; 2]>,
}

/// `value` on the grid `grid`, or as it is with no grid.
pub(super) fn snap(value: f64, grid: Option<f64>) -> f64 {
    let grid = grid.unwrap_or(DEFAULT_GRID);
    if grid > 0.0 {
        (value / grid).round() * grid + 0.0
    } else {
        value
    }
}

impl MapDocument {
    /// The brush `brush` of `entity`.
    pub(crate) fn brush(
        &mut self,
        entity: EntityIndex,
        brush: BrushIndex,
    ) -> Result<Brush, WorkbenchError> {
        let found = self.entity_mut(entity)?;
        let place = found
            .brush_places()
            .get(brush.get())
            .copied()
            .ok_or(WorkbenchError::NoBrush { entity, brush })?;
        match found.items.get(place) {
            Some(Item::Brush(found)) => Ok(found.clone()),
            Some(Item::Pair(_)) | None => Err(WorkbenchError::NoBrush { entity, brush }),
        }
    }

    /// Replaces the brush `brush` of `entity` with `with`, none to remove it.
    pub(crate) fn replace_brush(
        &mut self,
        entity: EntityIndex,
        brush: BrushIndex,
        with: Vec<Brush>,
    ) -> Result<(), WorkbenchError> {
        let found = self.entity_mut(entity)?;
        let place = found
            .brush_places()
            .get(brush.get())
            .copied()
            .ok_or(WorkbenchError::NoBrush { entity, brush })?;
        let tail = found.items.split_off(place);
        found.items.extend(with.into_iter().map(Item::Brush));
        found.items.extend(tail.into_iter().skip(1));
        Ok(())
    }

    /// The surface a new brush takes: `texture`, else the map's last brush face's.
    pub(super) fn new_surface(&self, texture: Option<&str>) -> Surface {
        let last = self
            .file
            .entities
            .iter()
            .flat_map(Entity::brushes)
            .flat_map(|brush| &brush.faces)
            .last();
        let texdef = last.map_or(
            TexDef::Standard {
                shift: [0.0, 0.0],
                rotate: 0.0,
                scale: [1.0, 1.0],
            },
            |face| *face.texdef(),
        );
        let texture = texture.map_or_else(
            || last.map_or_else(|| DEFAULT_TEXTURE.to_vec(), |f| f.texture().to_vec()),
            |t| t.as_bytes().to_vec(),
        );
        Surface { texture, texdef }
    }
}

impl From<BrushError> for WorkbenchError {
    fn from(error: BrushError) -> Self {
        Self::Brush { error }
    }
}

/// `map.brush_create`.
pub(super) struct BrushCreate;

impl<C: HasWorkbench + 'static> Command<C> for BrushCreate {
    const ID: &'static str = MAP_BRUSH_CREATE;
    const SUMMARY: &'static str = "Adds a box brush from two corners, snapped to the grid (16 \
        units unless given), to the world or an entity of the open map, with a texture.";
    type Params = BrushCreateParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushCreateParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let entity = params.entity.unwrap_or(EntityIndex::WORLD);
        let ((), step) = edit_map(context, |document| {
            let corners = [params.min, params.max].map(|p| p.map(|c| snap(c, params.grid)));
            let [a, b] = corners;
            let min = [0, 1, 2].map(|i| {
                a.get(i)
                    .copied()
                    .unwrap_or_default()
                    .min(b.get(i).copied().unwrap_or_default())
            });
            let max = [0, 1, 2].map(|i| {
                a.get(i)
                    .copied()
                    .unwrap_or_default()
                    .max(b.get(i).copied().unwrap_or_default())
            });
            let surface = document.new_surface(params.texture.as_deref());
            let brush = Brush::cuboid(min, max, &surface)?;
            document.entity_mut(entity)?.items.push(Item::Brush(brush));
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

/// `map.brush_move`.
pub(super) struct BrushMove;

impl<C: HasWorkbench + 'static> Command<C> for BrushMove {
    const ID: &'static str = MAP_BRUSH_MOVE;
    const SUMMARY: &'static str = "Moves brushes of an entity by an offset snapped to the grid \
        (16 units unless given), their textures fixed in the world unless lock is free.";
    type Params = BrushMoveParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushMoveParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let by = params.by.map(|c| snap(c, params.grid));
        let ((), step) = edit_map(context, |document| {
            for &brush in &params.brushes {
                let moved = document
                    .brush(params.entity, brush)?
                    .translated(by, params.lock.into())?;
                document.replace_brush(params.entity, brush, vec![moved])?;
            }
            Ok(())
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.brush_texture`.
pub(super) struct BrushTexture;

impl<C: HasWorkbench + 'static> Command<C> for BrushTexture {
    const ID: &'static str = MAP_BRUSH_TEXTURE;
    const SUMMARY: &'static str = "Sets the texture, shift, rotation or scale of every face of \
        brushes of an entity, keeping what is not given (MAP-12).";
    type Params = BrushTextureParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushTextureParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let alignment = Alignment {
            shift: params.shift,
            rotate: params.rotate,
            scale: params.scale,
        };
        let ((), step) = edit_map(context, |document| {
            for &brush in &params.brushes {
                let mut found = document.brush(params.entity, brush)?;
                for face in &mut found.faces {
                    let texture = params
                        .texture
                        .as_ref()
                        .map_or_else(|| face.texture().to_vec(), |t| t.as_bytes().to_vec());
                    let texdef = face.texdef().aligned(alignment);
                    let points = *face.points();
                    face.set(points, &texture, texdef)
                        .map_err(BrushError::Map)?;
                }
                document.replace_brush(params.entity, brush, vec![found])?;
            }
            Ok(())
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.brush_clip`.
pub(super) struct BrushClip;

impl<C: HasWorkbench + 'static> Command<C> for BrushClip {
    const ID: &'static str = MAP_BRUSH_CLIP;
    const SUMMARY: &'static str = "Clips a brush of the open map by the plane through three \
        points, keeping the part behind it, in front of it, or both as two brushes.";
    type Params = BrushClipParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushClipParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            let brush = document.brush(params.entity, params.brush)?;
            let plane = plane_of(params.points).ok_or(BrushError::Degenerate)?;
            let surface = brush
                .faces
                .first()
                .map(|face| Surface {
                    texture: face.texture().to_vec(),
                    texdef: *face.texdef(),
                })
                .ok_or(BrushError::Degenerate)?;
            let (back, front) = brush.split(plane, &surface)?;
            let kept: Vec<Brush> = match params.keep {
                Keep::Back => back.into_iter().collect(),
                Keep::Front => front.into_iter().collect(),
                Keep::Both => back.into_iter().chain(front).collect(),
            };
            if kept.is_empty() {
                return Err(BrushError::Degenerate.into());
            }
            if kept == [brush] {
                return Err(BrushError::Missed.into());
            }
            document.replace_brush(params.entity, params.brush, kept)
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.brush_vertex`.
pub(super) struct BrushVertex;

impl<C: HasWorkbench + 'static> Command<C> for BrushVertex {
    const ID: &'static str = MAP_BRUSH_VERTEX;
    const SUMMARY: &'static str = "Moves a vertex of a brush of the open map to a point snapped \
        to the grid; the brush becomes the convex hull of its vertices so moved.";
    type Params = BrushVertexParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushVertexParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            let brush = document.brush(params.entity, params.brush)?;
            let to = params.to.map(|c| snap(c, params.grid));
            let moved = brush.move_vertex(params.from, to)?;
            document.replace_brush(params.entity, params.brush, vec![moved])
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.brush_subtract`.
pub(super) struct BrushSubtract;

impl<C: HasWorkbench + 'static> Command<C> for BrushSubtract {
    const ID: &'static str = MAP_BRUSH_SUBTRACT;
    const SUMMARY: &'static str = "Subtracts a brush of the open map from every other brush of \
        its entity, then deletes it: the brushes it overlapped become the parts outside it.";
    type Params = BrushParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            let cutter = document.brush(params.entity, params.brush)?;
            let found = document.entity_mut(params.entity)?;
            let place = found
                .brush_places()
                .get(params.brush.get())
                .copied()
                .ok_or(WorkbenchError::NoBrush {
                    entity: params.entity,
                    brush: params.brush,
                })?;
            // Each brush is replaced where it stands by its pieces, so the map's text changes
            // only around the brushes the cutter overlaps.
            let mut items = Vec::with_capacity(found.items.len());
            for (index, item) in core::mem::take(&mut found.items).into_iter().enumerate() {
                match item {
                    Item::Brush(_) if index == place => {}
                    Item::Brush(brush) => {
                        items.extend(brush.subtract(&cutter)?.into_iter().map(Item::Brush));
                    }
                    Item::Pair(pair) => items.push(Item::Pair(pair)),
                }
            }
            document.entity_mut(params.entity)?.items = items;
            Ok(())
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.brush_merge`.
pub(super) struct BrushMerge;

impl<C: HasWorkbench + 'static> Command<C> for BrushMerge {
    const ID: &'static str = MAP_BRUSH_MERGE;
    const SUMMARY: &'static str = "Merges two or more brushes of an entity of the open map into \
        their convex hull, which takes the first's place.";
    type Params = BrushMergeParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushMergeParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            let mut indices = params.brushes.clone();
            indices.sort();
            indices.dedup();
            let (Some(&first), true) = (indices.first(), indices.len() > 1) else {
                return Err(BrushError::Degenerate.into());
            };
            let mut brushes = Vec::new();
            for &index in &indices {
                brushes.push(document.brush(params.entity, index)?);
            }
            let (head, rest) = brushes.split_first().ok_or(BrushError::Degenerate)?;
            let others: Vec<&Brush> = rest.iter().collect();
            let merged = head.merge(&others)?;
            for &index in indices.iter().skip(1).rev() {
                document.replace_brush(params.entity, index, Vec::new())?;
            }
            document.replace_brush(params.entity, first, vec![merged])
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

/// `map.brush_delete`.
pub(super) struct BrushDelete;

impl<C: HasWorkbench + 'static> Command<C> for BrushDelete {
    const ID: &'static str = MAP_BRUSH_DELETE;
    const SUMMARY: &'static str = "Deletes a brush of an entity of the open map.";
    type Params = BrushParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: BrushParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let ((), step) = edit_map(context, |document| {
            document.replace_brush(params.entity, params.brush, Vec::new())
        })?;
        Ok(Outcome::undoable(
            MapEdited {
                entity: Some(params.entity),
            },
            step,
        ))
    }
}

#[cfg(test)]
mod tests;
