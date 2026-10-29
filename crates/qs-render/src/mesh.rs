// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_rsurf.c, BuildSurfaceDisplayList, GL_CreateSurfaceLightmap, R_BuildLightMap

//! A BSP's world surfaces as the GPU draws them (REND-1): each face a fan of triangles with its
//! texture and lightmap coordinates, the textures through the palette, and every face's light
//! packed into one lightmap atlas.

use qs_bsp::{Bsp, Face, SURF_DRAWSKY, SURF_DRAWTURB, Texinfo};

/// The bytes of `gfx/palette.lmp`: 256 colours of three bytes (REND-1.8).
const PALETTE_BYTES: usize = 768;
/// `t >>= 7` in `R_BuildLightMap` (REND-1.4).
const LIGHT_SHIFT: u32 = 7;
/// A style's value while no light string is set for it (REND-1.4).
const UNSET_STYLE: u32 = 256;
/// The world units of one lightmap texel, `light_s*16` (REND-1.3).
const LIGHTMAP_TEXEL: f32 = 16.0;
/// The atlas's width in texels; its height grows with the faces it holds.
const ATLAS_WIDTH: usize = 1024;

/// Why a map's surfaces could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MeshError {
    /// The palette is not 768 bytes (REND-1.8).
    Palette(usize),
    /// The BSP has no world model.
    NoWorld,
    /// The lightmaps do not fit one texture of the largest size a GPU is sure to have.
    AtlasTooLarge,
}

impl core::fmt::Display for MeshError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Palette(size) => write!(f, "a palette is 768 bytes, not {size}"),
            Self::NoWorld => f.write_str("the BSP has no world model"),
            Self::AtlasTooLarge => f.write_str("the lightmaps do not fit one 1024 by 8192 texture"),
        }
    }
}

impl core::error::Error for MeshError {}

/// The 256 colours texture pixels index (REND-1.8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette([[u8; 3]; 256]);

impl Palette {
    /// The palette `gfx/palette.lmp` holds: 256 red, green and blue triples.
    ///
    /// # Errors
    ///
    /// [`MeshError::Palette`] when `bytes` is not 768 bytes long.
    pub fn read(bytes: &[u8]) -> Result<Self, MeshError> {
        if bytes.len() != PALETTE_BYTES {
            return Err(MeshError::Palette(bytes.len()));
        }
        let mut colours = [[0_u8; 3]; 256];
        let (triples, _) = bytes.as_chunks::<3>();
        for (colour, rgb) in colours.iter_mut().zip(triples) {
            *colour = *rgb;
        }
        Ok(Self(colours))
    }

    /// The colour of index `index`, opaque.
    fn rgba(&self, index: u8) -> [u8; 4] {
        let [r, g, b] = self.0.get(usize::from(index)).copied().unwrap_or_default();
        [r, g, b, 255]
    }
}

/// An RGBA image, rows top to bottom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
    /// Four bytes a pixel.
    pub rgba: Vec<u8>,
}

/// One corner of a surface's triangles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    /// Its position in the map.
    pub position: [f32; 3],
    /// Its texture coordinates (REND-1.2).
    pub texture: [f32; 2],
    /// Its lightmap atlas coordinates (REND-1.3).
    pub light: [f32; 2],
}

/// The triangles drawn with one texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    /// The texture, an index into [`MapMesh::textures`].
    pub texture: usize,
    /// The first index into [`MapMesh::indices`].
    pub first: u32,
    /// How many indices it draws.
    pub count: u32,
}

/// A BSP's world model ready for the GPU (REND-1).
#[derive(Clone, Debug, PartialEq)]
pub struct MapMesh {
    /// Every vertex.
    pub vertices: Vec<Vertex>,
    /// Three indices a triangle.
    pub indices: Vec<u32>,
    /// The triangles of each texture, in texture order.
    pub batches: Vec<Batch>,
    /// The BSP's textures through the palette, a missing one as a 1 by 1 black image.
    pub textures: Vec<Image>,
    /// Every face's light, one byte repeated in red, green and blue (REND-1.4).
    pub lightmap: Image,
    /// The world model's bounds.
    pub mins: [f32; 3],
    /// The world model's bounds.
    pub maxs: [f32; 3],
}

impl MapMesh {
    /// The world model of `bsp`, its textures read through `palette` (REND-1).
    ///
    /// # Errors
    ///
    /// [`MeshError::NoWorld`] when the BSP has no model, and [`MeshError::AtlasTooLarge`] when
    /// the lightmaps do not fit one texture.
    pub fn build(bsp: &Bsp<'_>, palette: &Palette) -> Result<Self, MeshError> {
        let world = bsp.models.first().ok_or(MeshError::NoWorld)?;
        let faces = bsp
            .faces
            .get(world.firstface..world.firstface.saturating_add(world.numfaces))
            .unwrap_or_default();
        let mut atlas = Atlas::new();
        let mut by_texture: Vec<Vec<Polygon>> = vec![Vec::new(); bsp.textures.len()];
        for face in faces {
            let Some(texinfo) = bsp.texinfo.get(face.texinfo) else {
                continue;
            };
            let Some(slot) = texinfo.texture else {
                continue;
            };
            let Some(Some(texture)) = bsp.textures.get(slot) else {
                continue;
            };
            let lit = face.flags & (SURF_DRAWSKY | SURF_DRAWTURB) == 0;
            let place = if lit {
                atlas.place(face, bsp)?
            } else {
                atlas.full_bright()
            };
            let polygon = Polygon {
                corners: corners(face, bsp),
                texinfo: *texinfo,
                size: [f32_of(texture.width), f32_of(texture.height)],
                texturemins: face.texturemins,
                place,
                lit,
            };
            if let Some(list) = by_texture.get_mut(slot) {
                list.push(polygon);
            }
        }
        let (width, height) = atlas.size();
        let mut mesh = Self {
            vertices: Vec::new(),
            indices: Vec::new(),
            batches: Vec::new(),
            textures: bsp
                .textures
                .iter()
                .map(|texture| texture.as_ref().map_or_else(black, |t| image(t, palette)))
                .collect(),
            lightmap: atlas.image(),
            mins: world.mins,
            maxs: world.maxs,
        };
        for (slot, polygons) in by_texture.iter().enumerate() {
            let first = u32_of(mesh.indices.len());
            for polygon in polygons {
                polygon.emit(&mut mesh, (width, height));
            }
            let count = u32_of(mesh.indices.len()).saturating_sub(first);
            if count > 0 {
                mesh.batches.push(Batch {
                    texture: slot,
                    first,
                    count,
                });
            }
        }
        Ok(mesh)
    }
}

/// A face on its way into the mesh.
#[derive(Clone, Debug)]
struct Polygon {
    corners: Vec<[f32; 3]>,
    texinfo: Texinfo,
    size: [f32; 2],
    texturemins: [i16; 2],
    /// The face's texel position in the atlas, `light_s` and `light_t`.
    place: [usize; 2],
    /// Whether it has a lightmap, so its light coordinates follow its surface (REND-1.6).
    lit: bool,
}

impl Polygon {
    /// REND-1.1 to REND-1.3: the face's vertices and a fan of triangles over them.
    fn emit(&self, mesh: &mut MapMesh, (width, height): (f32, f32)) {
        let base = u32_of(mesh.vertices.len());
        for corner in &self.corners {
            let s = dot(corner, self.texinfo.vecs[0]);
            let t = dot(corner, self.texinfo.vecs[1]);
            let light = if self.lit {
                [
                    (s - f32::from(self.texturemins[0])
                        + f32_of_usize(self.place[0]) * LIGHTMAP_TEXEL
                        + LIGHTMAP_TEXEL / 2.0)
                        / (width * LIGHTMAP_TEXEL),
                    (t - f32::from(self.texturemins[1])
                        + f32_of_usize(self.place[1]) * LIGHTMAP_TEXEL
                        + LIGHTMAP_TEXEL / 2.0)
                        / (height * LIGHTMAP_TEXEL),
                ]
            } else {
                // The full-bright texel's centre.
                [0.5 / width, 0.5 / height]
            };
            mesh.vertices.push(Vertex {
                position: *corner,
                texture: [s / self.size[0], t / self.size[1]],
                light,
            });
        }
        let count = u32_of(self.corners.len());
        for i in 1..count.saturating_sub(1) {
            mesh.indices.extend([
                base,
                base.saturating_add(i),
                base.saturating_add(i).saturating_add(1),
            ]);
        }
    }
}

/// REND-1.1: a face's vertices in order: for a surfedge `lindex > 0` edge `lindex`'s first
/// vertex, otherwise edge `-lindex`'s second, as `BuildSurfaceDisplayList` reads them. A
/// reference outside the lumps ends the polygon there.
fn corners(face: &Face, bsp: &Bsp<'_>) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity(face.numedges);
    for i in 0..face.numedges {
        let Some(&lindex) = bsp.surfedges.get(face.firstedge.saturating_add(i)) else {
            break;
        };
        let (edge, end) = if lindex > 0 {
            (lindex, 0)
        } else {
            (lindex.saturating_neg(), 1)
        };
        let vertex = usize::try_from(edge)
            .ok()
            .and_then(|e| bsp.edges.get(e))
            .and_then(|e| e.get(end))
            .and_then(|&v| usize::try_from(v).ok())
            .and_then(|v| bsp.vertexes.get(v));
        let Some(vertex) = vertex else {
            break;
        };
        out.push(*vertex);
    }
    out
}

/// `DotProduct (vec, vecs[n]) + vecs[n][3]` (REND-1.2).
fn dot(v: &[f32; 3], axis: [f32; 4]) -> f32 {
    v[0] * axis[0] + v[1] * axis[1] + v[2] * axis[2] + axis[3]
}

/// Every face's lightmap packed into rows of one texture, with a full-bright texel at its
/// corner for the faces that have no lightmap.
struct Atlas {
    texels: Vec<u8>,
    /// The next free column of the current row, the row's top and its height.
    cursor: [usize; 3],
    height: usize,
}

/// The tallest atlas, the height every GPU wgpu supports.
const ATLAS_MAX_HEIGHT: usize = 8192;

impl Atlas {
    fn new() -> Self {
        let mut atlas = Self {
            texels: Vec::new(),
            cursor: [1, 0, 1],
            height: 1,
        };
        atlas.grow(1);
        if let Some(texel) = atlas.texels.first_mut() {
            *texel = 255;
        }
        atlas
    }

    /// The texel that is always 255 (REND-1.6).
    const fn full_bright(&self) -> [usize; 2] {
        [0, 0]
    }

    /// Makes the atlas `height` rows tall.
    fn grow(&mut self, height: usize) {
        self.height = self.height.max(height);
        self.texels
            .resize(ATLAS_WIDTH.saturating_mul(self.height), 0);
    }

    /// Places `face`'s lightmap and writes its texels (REND-1.3 to REND-1.5); returns its
    /// position.
    fn place(&mut self, face: &Face, bsp: &Bsp<'_>) -> Result<[usize; 2], MeshError> {
        let [smax, tmax] = face.lightmap_size;
        let [mut column, mut top, mut row_height] = self.cursor;
        if column.saturating_add(smax) > ATLAS_WIDTH {
            column = 0;
            top = top.saturating_add(row_height);
            row_height = 0;
        }
        row_height = row_height.max(tmax);
        let bottom = top.saturating_add(row_height);
        if bottom > ATLAS_MAX_HEIGHT || smax > ATLAS_WIDTH {
            return Err(MeshError::AtlasTooLarge);
        }
        self.grow(bottom);
        self.cursor = [column.saturating_add(smax), top, row_height];
        let texels = light_texels(face, bsp);
        for t in 0..tmax {
            for s in 0..smax {
                let value = texels
                    .get(t.saturating_mul(smax).saturating_add(s))
                    .copied()
                    .unwrap_or(0);
                let at = top
                    .saturating_add(t)
                    .saturating_mul(ATLAS_WIDTH)
                    .saturating_add(column)
                    .saturating_add(s);
                if let Some(texel) = self.texels.get_mut(at) {
                    *texel = value;
                }
            }
        }
        Ok([column, top])
    }

    fn size(&self) -> (f32, f32) {
        (f32_of_usize(ATLAS_WIDTH), f32_of_usize(self.height))
    }

    /// The atlas as an image, each texel's light in red, green and blue.
    fn image(&self) -> Image {
        Image {
            width: u32_of(ATLAS_WIDTH),
            height: u32_of(self.height),
            rgba: self.texels.iter().flat_map(|&l| [l, l, l, 255]).collect(),
        }
    }
}

/// REND-1.4, REND-1.5: `face`'s light texels, `smax * tmax` of them.
fn light_texels(face: &Face, bsp: &Bsp<'_>) -> Vec<u8> {
    let [smax, tmax] = face.lightmap_size;
    let size = smax.saturating_mul(tmax);
    // "set to full bright if no light data"
    if bsp.lighting.is_empty() {
        return vec![255; size];
    }
    let mut sums = vec![0_u32; size];
    if let Some(samples) = bsp.face_samples(face) {
        // Every style is the unset value until lightstyles animate (REND-1.4).
        for map in samples.chunks_exact(size.max(1)).take(face.style_count()) {
            for (sum, &sample) in sums.iter_mut().zip(map) {
                *sum = sum.saturating_add(u32::from(sample).saturating_mul(UNSET_STYLE));
            }
        }
    }
    sums.iter()
        .map(|&sum| u8::try_from(sum >> LIGHT_SHIFT).unwrap_or(u8::MAX))
        .collect()
}

/// The texture `texture`'s first mip level through `palette` (REND-1.8).
fn image(texture: &qs_bsp::Texture<'_>, palette: &Palette) -> Image {
    let pixels = texture.mip(0).unwrap_or_default();
    Image {
        width: texture.width.max(1),
        height: texture.height.max(1),
        rgba: if pixels.is_empty() {
            vec![0, 0, 0, 255]
        } else {
            pixels.iter().flat_map(|&p| palette.rgba(p)).collect()
        },
    }
}

/// A missing texture's stand-in.
fn black() -> Image {
    Image {
        width: 1,
        height: 1,
        rgba: vec![0, 0, 0, 255],
    }
}

/// A count as a float; every count here is far below 2^24, where floats stop being exact.
fn f32_of_usize(n: usize) -> f32 {
    u16::try_from(n).map_or(f32::MAX, f32::from)
}

fn f32_of(n: u32) -> f32 {
    u16::try_from(n).map_or(f32::MAX, f32::from)
}

/// A count as an index; a mesh this large does not fit a GPU index buffer anyway.
fn u32_of(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests;
