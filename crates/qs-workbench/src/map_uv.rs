// SPDX-License-Identifier: GPL-3.0-or-later

//! The UV editor's commands (P6-22, MAP-13): `map.face_uv` fits, justifies, flips, resets or
//! snaps the texture of chosen faces, each at the size its WAD gives it, and `map.face_align`
//! carries one face's alignment onto others so the texture continues across them. Each is one
//! step with undo.

use std::path::{Path, PathBuf};

use qs_commands::{Command, Outcome};
use qs_map::{BrushError, Face, Justify, TexAxis, TextureSize, UvEdit};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::map_edit::{BrushIndex, EntityIndex, MapDocument, MapEdited, edit_map};
use crate::map_textures::TextureSources;
use crate::preview::HasPreview;
use crate::{HasWorkbench, WorkbenchError};

/// The id of `map.face_uv`.
pub const MAP_FACE_UV: &str = "map.face_uv";
/// The id of `map.face_align`.
pub const MAP_FACE_ALIGN: &str = "map.face_align";

/// A face's place among its brush's faces, from 0, in file order.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct FaceIndex(usize);

impl FaceIndex {
    /// The face at `index`.
    #[must_use]
    pub const fn new(index: usize) -> Self {
        Self(index)
    }

    /// Its place among its brush's faces.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0
    }
}

/// A face of the open map: the entity, the brush and the face.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct FaceRef {
    /// The entity holding the brush.
    pub entity: EntityIndex,
    /// The brush.
    pub brush: BrushIndex,
    /// The face.
    pub face: FaceIndex,
}

/// Where `justify` puts the texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JustifyTo {
    /// Its left edge on the face's.
    Left,
    /// Its right edge on the face's.
    Right,
    /// Its top edge on the face's.
    Top,
    /// Its bottom edge on the face's.
    Bottom,
    /// Its centre on the face's.
    Centre,
}

/// A texture axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Across the texture.
    S,
    /// Down the texture.
    T,
}

/// What `map.face_uv` does to each face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum UvChoice {
    /// Stretch or shrink the texture so one copy spans the face (MAP-13.2).
    Fit,
    /// Shift it to an edge or the centre of the face (MAP-13.3).
    Justify {
        /// Where.
        to: JustifyTo,
    },
    /// Mirror it along an axis about the face's centre (MAP-13.4).
    Flip {
        /// The axis.
        axis: Axis,
    },
    /// No shift, rotation or scaling (MAP-13.5).
    Reset,
    /// Shift it so its nearest edge lands on the nearest corner (MAP-13.6).
    Snap,
}

impl UvChoice {
    /// The edit in `qs-map`'s terms.
    const fn edit(self) -> UvEdit {
        match self {
            Self::Fit => UvEdit::Fit,
            Self::Justify { to } => UvEdit::Justify(match to {
                JustifyTo::Left => Justify::Left,
                JustifyTo::Right => Justify::Right,
                JustifyTo::Top => Justify::Top,
                JustifyTo::Bottom => Justify::Bottom,
                JustifyTo::Centre => Justify::Centre,
            }),
            Self::Flip { axis } => UvEdit::Flip(match axis {
                Axis::S => TexAxis::S,
                Axis::T => TexAxis::T,
            }),
            Self::Reset => UvEdit::Reset,
            Self::Snap => UvEdit::Snap,
        }
    }

    /// Whether the edit reads the texture's size.
    const fn sized(self) -> bool {
        match self {
            Self::Fit | Self::Justify { .. } | Self::Snap => true,
            Self::Flip { .. } | Self::Reset => false,
        }
    }
}

/// The parameters of `map.face_uv`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaceUvParams {
    /// The faces.
    pub faces: Vec<FaceRef>,
    /// What to do to each.
    pub edit: UvChoice,
}

/// The parameters of `map.face_align`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FaceAlignParams {
    /// The face whose alignment is carried.
    pub from: FaceRef,
    /// The faces it is carried onto.
    pub to: Vec<FaceRef>,
}

impl MapDocument {
    /// The face `at` of the open map.
    fn face(&mut self, at: FaceRef) -> Result<Face, WorkbenchError> {
        let brush = self.brush(at.entity, at.brush)?;
        brush
            .faces
            .get(at.face.get())
            .cloned()
            .ok_or(WorkbenchError::NoFace {
                entity: at.entity,
                brush: at.brush,
                face: at.face,
            })
    }

    /// Gives each of `faces` the texture definition `edit` makes of it and its polygon, in
    /// one brush replacement per face.
    fn edit_faces(
        &mut self,
        faces: &[FaceRef],
        mut edit: impl FnMut(&Face, &[[f64; 3]]) -> Result<Option<qs_map::TexDef>, WorkbenchError>,
    ) -> Result<(), WorkbenchError> {
        for &at in faces {
            let mut brush = self.brush(at.entity, at.brush)?;
            let polygon = brush
                .face_polygons()
                .into_iter()
                .nth(at.face.get())
                .flatten()
                .unwrap_or_default();
            let face = brush
                .faces
                .get_mut(at.face.get())
                .ok_or(WorkbenchError::NoFace {
                    entity: at.entity,
                    brush: at.brush,
                    face: at.face,
                })?;
            if let Some(texdef) = edit(face, &polygon)? {
                let (points, texture) = (*face.points(), face.texture().to_vec());
                face.set(points, &texture, texdef)
                    .map_err(|error| WorkbenchError::Brush {
                        error: BrushError::Map(error),
                    })?;
            }
            self.replace_brush(at.entity, at.brush, vec![brush])?;
        }
        Ok(())
    }
}

/// `map.face_uv`.
pub(crate) struct FaceUv;

impl<C: HasPreview + 'static> Command<C> for FaceUv {
    const ID: &'static str = MAP_FACE_UV;
    const SUMMARY: &'static str = "Fits, justifies, flips, resets or snaps the texture of faces \
        of the open map, at the size its WAD gives it (MAP-13).";
    type Params = FaceUvParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: FaceUvParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let sources = params
            .edit
            .sized()
            .then(|| texture_sources(context))
            .transpose()?;
        let edit = params.edit.edit();
        let entity = params.faces.first().map(|at| at.entity);
        let ((), step) = edit_map(context, |document| {
            document.edit_faces(&params.faces, |face, polygon| {
                let name = String::from_utf8_lossy(face.texture()).into_owned();
                let size = match &sources {
                    Some(sources) => sources
                        .size(&name)
                        .ok_or(WorkbenchError::NoTextureSize { texture: name })?,
                    None => [0, 0],
                };
                let [width, height] = size;
                Ok(face.uv_edited(polygon, edit, TextureSize::new(width, height)))
            })
        })?;
        Ok(Outcome::undoable(MapEdited { entity }, step))
    }
}

/// The texture sources of the open map, found as the texture browser finds them.
fn texture_sources<C: HasPreview>(context: &mut C) -> Result<TextureSources, WorkbenchError> {
    let (workbench, preview) = context.workbench_and_preview();
    let game = preview.game_base_path();
    let chosen = preview.game_folder().map(Path::to_path_buf);
    let root: Option<PathBuf> = workbench.root().map(Path::to_path_buf);
    let document = workbench.map().ok_or(WorkbenchError::NoMap)?;
    Ok(TextureSources::load(
        document,
        root.as_deref(),
        game.as_deref(),
        chosen.as_deref(),
    ))
}

/// `map.face_align`.
pub(crate) struct FaceAlign;

impl<C: HasWorkbench + 'static> Command<C> for FaceAlign {
    const ID: &'static str = MAP_FACE_ALIGN;
    const SUMMARY: &'static str = "Carries one face's texture alignment onto other faces of the \
        open map, so the texture continues across the line their planes share (MAP-13.8).";
    type Params = FaceAlignParams;
    type Output = MapEdited;
    type Error = WorkbenchError;

    fn run(
        &self,
        context: &mut C,
        params: FaceAlignParams,
    ) -> Result<Outcome<MapEdited, C>, WorkbenchError> {
        let entity = params.to.first().map(|at| at.entity);
        let ((), step) = edit_map(context, |document| {
            let source = document.face(params.from)?;
            document.edit_faces(&params.to, |face, _| Ok(face.aligned_from(&source)))
        })?;
        Ok(Outcome::undoable(MapEdited { entity }, step))
    }
}

#[cfg(test)]
mod tests;
