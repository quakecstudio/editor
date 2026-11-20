// SPDX-License-Identifier: GPL-3.0-or-later

//! A map drawn from its brushes rather than from a compiled BSP (P7-13, MAP-17): each face a
//! polygon with its texture, shaded by the way it faces, as TrenchBroom draws an editor's 3D
//! view. The faces become a [`MapMesh`] the same renderer draws, with a lightmap of one texel
//! per shade in place of a BSP's lightmaps, so the editor's view and the game share one
//! renderer.

use crate::look::Look;
use crate::mesh::{Batch, Image, Lightmaps, MapMesh, Placement, Surface, Vertex, u32_of};
use crate::{CHANNEL_MAX, RGBA_BYTES};

/// How brightly a face is drawn, as a share of full light out of [`Shade::FULL`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Shade(u8);

impl Shade {
    /// Full light: the texture as it is.
    pub const FULL: Self = Self(CHANNEL_MAX);

    /// `level` out of 255.
    #[must_use]
    pub const fn new(level: u8) -> Self {
        Self(level)
    }

    /// The level, out of 255.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// A face drawn from its brush: its corners going around it, each with its texture coordinates
/// already divided by the texture's size, the texture it is drawn with, an index into the
/// mesh's textures, and its shade.
#[derive(Clone, Debug, PartialEq)]
pub struct FlatFace {
    /// Each corner and its texture coordinates.
    pub corners: Vec<([f32; 3], [f32; 2])>,
    /// The texture, an index into the textures the mesh is made with.
    pub texture: usize,
    /// How brightly it is drawn.
    pub shade: Shade,
}

impl MapMesh {
    /// The mesh of `faces` drawn with `textures`, in `look`: one batch per texture, in texture
    /// order, and a lightmap of one texel per shade the faces use, so each face is its texture
    /// darkened to its shade. A face naming no texture, or with fewer than three corners, is
    /// not drawn.
    #[must_use]
    pub fn flat(faces: &[FlatFace], textures: Vec<Image>, look: Look) -> Self {
        let mut shades: Vec<Shade> = faces.iter().map(|face| face.shade).collect();
        shades.sort_unstable();
        shades.dedup();
        let width = shades.len().max(1);
        let scale = look.light_scale();
        let mut lightmap = Vec::with_capacity(width.saturating_mul(RGBA_BYTES));
        for shade in &shades {
            // The shader multiplies a texel by the look's scale, so the texel is the shade
            // divided by it.
            let level = level_of(f32::from(shade.get()) / scale);
            lightmap.extend([level, level, level, CHANNEL_MAX]);
        }
        if shades.is_empty() {
            lightmap.extend([CHANNEL_MAX; RGBA_BYTES]);
        }
        let mut mesh = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            batches: Vec::new(),
            glows: vec![None; textures.len()],
            skies: vec![None; textures.len()],
            animations: vec![None; textures.len()],
            lightmap: Image {
                width: u32_of(width),
                height: 1,
                rgba: lightmap,
            },
            lightmaps: Lightmaps::new(look),
            mins: [0.0, 0.0, 0.0],
            maxs: [0.0, 0.0, 0.0],
            models: vec![Placement::WORLD],
            look,
            textures,
        };
        let mut bounds: Option<([f32; 3], [f32; 3])> = None;
        let span = f32_of(width);
        for texture in 0..mesh.textures.len() {
            let first = u32_of(mesh.indices.len());
            for face in faces
                .iter()
                .filter(|face| face.texture == texture && face.corners.len() >= TRIANGLE)
            {
                let column = shades.binary_search(&face.shade).unwrap_or_default();
                let light = [(f32_of(column) + TEXEL_CENTRE) / span, TEXEL_CENTRE];
                let base = u32_of(mesh.vertices.len());
                for &(position, texture) in &face.corners {
                    bounds = Some(match bounds {
                        None => (position, position),
                        Some((low, high)) => (
                            widened(low, position, f32::min),
                            widened(high, position, f32::max),
                        ),
                    });
                    mesh.vertices.push(Vertex {
                        position,
                        texture,
                        light,
                    });
                }
                for i in 1..u32_of(face.corners.len()).saturating_sub(1) {
                    mesh.indices.extend([
                        base,
                        base.saturating_add(i),
                        base.saturating_add(i).saturating_add(1),
                    ]);
                }
            }
            let count = u32_of(mesh.indices.len()).saturating_sub(first);
            if count > 0 {
                mesh.batches.push(Batch {
                    texture,
                    surface: Surface::Lit,
                    first,
                    count,
                    model: 0,
                });
            }
        }
        (mesh.mins, mesh.maxs) = bounds.unwrap_or_default();
        mesh
    }
}

/// `bound` taken each axis with `point` by `pick`, the smaller or the larger.
fn widened(bound: [f32; 3], point: [f32; 3], pick: fn(f32, f32) -> f32) -> [f32; 3] {
    let ([bx, by, bz], [px, py, pz]) = (bound, point);
    [pick(bx, px), pick(by, py), pick(bz, pz)]
}

/// The corners of the smallest face drawn, a triangle.
const TRIANGLE: usize = 3;
/// A texel's centre, half a texel in.
const TEXEL_CENTRE: f32 = 0.5;

/// A light level as a texel's channel, rounded and held to 0 to 255.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..=255 and rounded first"
)]
fn level_of(value: f32) -> u8 {
    value.round().clamp(0.0, f32::from(CHANNEL_MAX)) as u8
}

/// A small count as a float.
fn f32_of(n: usize) -> f32 {
    u16::try_from(n).map_or(f32::MAX, f32::from)
}

#[cfg(test)]
mod tests;
