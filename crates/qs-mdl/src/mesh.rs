// SPDX-License-Identifier: GPL-3.0-or-later

//! A mesh read from a format a model is made in (MDL-9), and the alias model made of it. A
//! reader ([`obj`], [`iqe`]) gives a [`Mesh`]: triangles whose corners name a position and,
//! where the file has them, a texture coordinate, and one or more frames of positions.
//! [`Mesh::model`] makes the `.mdl`: with texture coordinates the mesh's own lie on the skin,
//! and without them the skin is laid out as id's modelgen lays one (MDL-8.3, MDL-8.4).

pub mod iqe;
pub mod obj;

use core::fmt;
use std::collections::BTreeMap;

use crate::mdl::grab::{Base, Corners, GrabError};
use crate::mdl::make::{Pose, PoseEntry, Shape};
use crate::mdl::write::{MdlWriteError, Model, ModelSkin};
use crate::mdl::{
    MDL_DT_FACES_FRONT, MDL_MAX_SKIN_HEIGHT, MDL_MAX_VERTS, MDL_ST_RAND, StVert, Triangle,
};

/// The colour GLQuake fills a skin's background with, `filledcolor` (REND-4.1): the first
/// pixel of a skin of one colour, so that the fill, which starts there, finds nothing to do.
pub const MESH_SKIN_FIRST: u8 = 0;

/// The widest skin a mesh is given. Ours: the format bounds a skin's height (MDL-1.4) and
/// not its width.
pub const MESH_MAX_SKIN_WIDTH: i32 = 1024;
/// What a skin's width is a multiple of (MDL-1.4).
const SKIN_WIDTH_STEP: i32 = 4;

/// A corner of a triangle of a mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Corner {
    /// Which of a frame's positions it is at.
    pub position: usize,
    /// Which of the mesh's texture coordinates it has, where the file gives one.
    pub texcoord: Option<usize>,
}

/// One pose of a mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshFrame {
    /// Its name, which becomes the frame's.
    pub name: String,
    /// Where each position is, in Quake's axes: x forward, y left, z up.
    pub positions: Vec<[f32; 3]>,
}

/// A mesh as a reader gives it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    /// Its triangles, each three corners going around it clockwise as seen from outside, as
    /// an alias model's do (MDL-7.4).
    pub triangles: Vec<[Corner; 3]>,
    /// Its texture coordinates, each `u` across the skin from its left and `v` down it from
    /// its top, both from 0 to 1.
    pub texcoords: Vec<[f32; 2]>,
    /// Its poses, each with a position for every one a corner names. The first is the pose
    /// a skin is laid out from.
    pub frames: Vec<MeshFrame>,
}

/// A skin for a model made of a mesh.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshSkin {
    /// One colour of the palette all over but for the first pixel ([`MESH_SKIN_FIRST`]), on
    /// a skin of the given width and height where the mesh has texture coordinates, and of
    /// the size the layout needs where it has none.
    Flat {
        /// The colour.
        colour: u8,
        /// The skin's width and height for a mesh with texture coordinates.
        size: [i32; 2],
    },
    /// A picture, for a mesh with texture coordinates: its width and height and its pixels,
    /// row by row from the top.
    Picture {
        /// Its width and height.
        size: [i32; 2],
        /// Its pixels.
        pixels: Vec<u8>,
    },
}

/// Why a mesh is no model.
#[derive(Debug, PartialEq)]
pub enum MeshError {
    /// The mesh has no triangle with an area, or no frame.
    Empty,
    /// A corner names a position or a texture coordinate the mesh does not have.
    Corner {
        /// The triangle.
        triangle: usize,
    },
    /// A picture was given for a mesh without texture coordinates, whose skin is laid out
    /// here.
    NoTexcoords,
    /// The skin's size is none a model has: a side under 1, a width that is no multiple of 4
    /// or over [`MESH_MAX_SKIN_WIDTH`], or a height over the format's.
    SkinSize {
        /// The width and height asked for.
        size: [i32; 2],
    },
    /// The mesh needs more vertices than a model has.
    Vertices,
    /// The skin's layout could not be made.
    Layout(GrabError),
    /// The model cannot be written.
    Write(MdlWriteError),
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("the mesh has no triangle with an area, or no frame"),
            Self::Corner { triangle } => write!(
                f,
                "triangle {triangle} has a corner the mesh has no position or texture \
                 coordinate for"
            ),
            Self::NoTexcoords => f.write_str(
                "the mesh has no texture coordinates, so its skin is laid out as it is \
                 imported and a picture cannot be put on it yet: export the skin, paint it and \
                 set it",
            ),
            Self::SkinSize {
                size: [width, height],
            } => write!(
                f,
                "a skin of {width} by {height} is none a model has: its width is a multiple of \
                 4 from 4 to {MESH_MAX_SKIN_WIDTH} and its height from 1 to {MDL_MAX_SKIN_HEIGHT}"
            ),
            Self::Vertices => write!(
                f,
                "the mesh needs more than the {MDL_MAX_VERTS} vertices a model has: make it \
                 simpler in the program it came from"
            ),
            Self::Layout(error) => write!(f, "{error}"),
            Self::Write(error) => write!(f, "{error}"),
        }
    }
}

impl core::error::Error for MeshError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            Self::Layout(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::Empty
            | Self::Corner { .. }
            | Self::NoTexcoords
            | Self::SkinSize { .. }
            | Self::Vertices => None,
        }
    }
}

/// How a mesh's vertices lie on its skin: the position each vertex takes of a pose, where
/// each is on the skin, the triangles, and the skin's width and height.
struct Layout {
    vertices: Vec<usize>,
    stverts: Vec<StVert>,
    faces: Vec<Triangle>,
    skin_size: [i32; 2],
}

/// C's conversion of a float to an `int` for a value checked to be in range: its whole part.
#[expect(
    clippy::cast_possible_truncation,
    reason = "spec MDL-9.3: the value is clamped to the skin's pixels first"
)]
const fn pixel(value: f32) -> i32 {
    value as i32
}

/// A side of a skin as a float.
#[expect(
    clippy::cast_precision_loss,
    reason = "spec MDL-9.3: a skin's side is at most a few hundred pixels"
)]
const fn side(pixels: i32) -> f32 {
    pixels as f32
}

impl Mesh {
    /// Whether every corner has a texture coordinate.
    #[must_use]
    pub fn has_texcoords(&self) -> bool {
        !self.triangles.is_empty()
            && self
                .triangles
                .iter()
                .flatten()
                .all(|corner| corner.texcoord.is_some())
    }

    /// The triangles two of whose corners are not one position, each with its place among
    /// the mesh's: the rest have no area.
    fn solid(&self) -> Vec<(usize, [Corner; 3])> {
        self.triangles
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, [a, b, c])| {
                a.position != b.position && b.position != c.position && c.position != a.position
            })
            .collect()
    }

    /// The model of the mesh with the skin `skin`: each frame of the mesh a frame of the
    /// model, by its name.
    ///
    /// Implements MDL-9.3 and MDL-9.4.
    ///
    /// # Errors
    ///
    /// [`MeshError`]: a mesh with no triangle or frame, a corner past the mesh's positions
    /// or texture coordinates, a picture for a mesh without texture coordinates, and a model
    /// the writer refuses, as one past the format's limits.
    pub fn model(&self, skin: &MeshSkin) -> Result<Model, MeshError> {
        let solid = self.solid();
        let Some(first) = self.frames.first() else {
            return Err(MeshError::Empty);
        };
        if solid.is_empty() {
            return Err(MeshError::Empty);
        }
        // A size is checked before anything of that size is made.
        let (MeshSkin::Flat { size, .. } | MeshSkin::Picture { size, .. }) = skin;
        let [wide, high] = *size;
        let fits = (SKIN_WIDTH_STEP..=MESH_MAX_SKIN_WIDTH).contains(&wide)
            && wide.checked_rem(SKIN_WIDTH_STEP) == Some(0)
            && (1..=MDL_MAX_SKIN_HEIGHT).contains(&high);
        // A mesh without texture coordinates has the size its layout gives, not this one.
        if self.has_texcoords() && !fits {
            return Err(MeshError::SkinSize { size: *size });
        }
        for (triangle, corners) in &solid {
            let triangle = *triangle;
            let known = corners.iter().all(|corner| {
                self.frames
                    .iter()
                    .all(|frame| corner.position < frame.positions.len())
                    && corner
                        .texcoord
                        .is_none_or(|texcoord| texcoord < self.texcoords.len())
            });
            if !known {
                return Err(MeshError::Corner { triangle });
            }
        }
        // Each vertex of the model: the position it takes of a frame.
        let triangles: Vec<[Corner; 3]> = solid.into_iter().map(|(_, corners)| corners).collect();
        let Layout {
            vertices,
            stverts,
            faces,
            skin_size,
        } = if self.has_texcoords() {
            self.own_layout(&triangles, *size)?
        } else {
            if matches!(skin, MeshSkin::Picture { .. }) {
                return Err(MeshError::NoTexcoords);
            }
            Self::laid_out(&triangles, first)?
        };
        let [width, height] = skin_size;
        let pixels = match skin {
            MeshSkin::Flat { colour, .. } => {
                let count = usize::try_from(width)
                    .unwrap_or(0)
                    .saturating_mul(usize::try_from(height).unwrap_or(0));
                // An engine made from GLQuake fills what is joined to a skin's first pixel
                // and of its colour with black (REND-4.1), which is all of a skin of one
                // colour: the first pixel is that black already, and nothing is filled.
                let mut pixels = vec![*colour; count];
                if let Some(first) = pixels.first_mut() {
                    *first = MESH_SKIN_FIRST;
                }
                pixels
            }
            MeshSkin::Picture { pixels, .. } => pixels.clone(),
        };
        let poses = self
            .frames
            .iter()
            .map(|frame| {
                PoseEntry::Single(Pose {
                    name: frame.name.as_bytes().to_vec(),
                    positions: vertices
                        .iter()
                        .map(|position| frame.positions.get(*position).copied().unwrap_or_default())
                        .collect(),
                })
            })
            .collect();
        Shape {
            skin_width: width,
            skin_height: height,
            skins: vec![ModelSkin::Single(pixels)],
            stverts,
            triangles: faces,
            poses,
            eye_position: [0.0, 0.0, 0.0],
            flags: 0,
            synctype: MDL_ST_RAND,
        }
        .build()
        .map_err(MeshError::Write)
    }

    /// The vertices of a mesh with texture coordinates on a skin of `size`: one for each
    /// pair of a position and a texture coordinate a corner has, in the order they come,
    /// with the pixel its texture coordinate is at; every triangle is on the skin's front.
    fn own_layout(
        &self,
        triangles: &[[Corner; 3]],
        [width, height]: [i32; 2],
    ) -> Result<Layout, MeshError> {
        let most = usize::try_from(MDL_MAX_VERTS).unwrap_or(0);
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        let mut places: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        let mut faces = Vec::with_capacity(triangles.len());
        for corners in triangles {
            let vertindex = corners.map(|corner| {
                let pair = (corner.position, corner.texcoord.unwrap_or(0));
                *places.entry(pair).or_insert_with(|| {
                    pairs.push(pair);
                    pairs.len().saturating_sub(1)
                })
            });
            // A mesh of more vertices than a model has is refused as soon as it is known.
            if pairs.len() > most {
                return Err(MeshError::Vertices);
            }
            faces.push(Triangle {
                facesfront: MDL_DT_FACES_FRONT,
                vertindex,
            });
        }
        let at = |part: f32, pixels: i32| {
            let last = side(pixels.saturating_sub(1)).max(0.0);
            pixel((part * side(pixels)).floor().clamp(0.0, last))
        };
        let stverts = pairs
            .iter()
            .map(|(_, texcoord)| {
                let [u, v] = self.texcoords.get(*texcoord).copied().unwrap_or_default();
                StVert {
                    onseam: 0,
                    s: at(u, width),
                    t: at(v, height),
                }
            })
            .collect();
        Ok(Layout {
            vertices: pairs.into_iter().map(|(position, _)| position).collect(),
            stverts,
            faces,
            skin_size: [width, height],
        })
    }

    /// The vertices of a mesh without texture coordinates, laid out as modelgen lays its
    /// base (MDL-8.1, MDL-8.3, MDL-8.4) from the pose `first`: the model's front, what faces
    /// +x, on the skin's left half and its back on the right, with the skin's size.
    fn laid_out(triangles: &[[Corner; 3]], first: &MeshFrame) -> Result<Layout, MeshError> {
        // modelgen turns what it reads to face down +x, (x, y, z) to (-y, x, z); this is the
        // pose turned the other way, so that its layout is of the mesh as it stands.
        let soup: Vec<Corners> = triangles
            .iter()
            .map(|corners| {
                corners.map(|corner| {
                    let [x, y, z] = first
                        .positions
                        .get(corner.position)
                        .copied()
                        .unwrap_or_default();
                    [y, -x, z]
                })
            })
            .collect();
        let base = Base::of(&soup).map_err(|error| match error {
            GrabError::Vertices => MeshError::Vertices,
            other => MeshError::Layout(other),
        })?;
        // The position each of the base's vertices is: that of the first corner at it.
        let mut vertices = vec![None; base.vertices()];
        for (corners, at) in triangles.iter().zip(base.corners()) {
            for (corner, vertex) in corners.iter().zip(at) {
                if let Some(slot @ None) = vertices.get_mut(*vertex) {
                    *slot = Some(corner.position);
                }
            }
        }
        Ok(Layout {
            vertices: vertices
                .into_iter()
                .map(Option::unwrap_or_default)
                .collect(),
            stverts: base.stverts().to_vec(),
            faces: base.triangles(),
            skin_size: base.skin_size(),
        })
    }
}

#[cfg(test)]
mod tests;
