// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 1996-1997 Id Software, Inc.
// Copyright (C) 2026 QuakeC Studio contributors
//
// Ported from id-Software/Quake@bf4ac424ce754894ac8f1dae6a3981954bc9852d: WinQuake/gl_rsurf.c, BuildSurfaceDisplayList, GL_CreateSurfaceLightmap, R_BuildLightMap

//! A BSP's world surfaces as the GPU draws them (REND-1): each face a fan of triangles with its
//! texture and lightmap coordinates, the textures through the palette, and every face's light
//! packed into one lightmap atlas, made again as the light styles change (REND-5).
//!
//! Coloured light from a `.lit` file (REND-6) follows QuakeSpasm's `R_BuildLightMap`, cited in
//! `docs/spec/render.md` for its behaviour, as `qs-bsp` reads the file; none of its code is
//! ported.

use qs_bsp::{Bsp, Face, Lit, SURF_DRAWSKY, SURF_DRAWTURB, Texinfo};

use crate::alias::rotate_for_entity;
use crate::styles::LightStyles;

/// GLQuake's `vid_gamma` on any card but a 3Dfx, "default to 0.7 on non-3dfx hardware"
/// (REND-3.1).
pub const GAMMA: f32 = 0.7;

/// The bytes of `gfx/palette.lmp`: 256 colours of three bytes (REND-1.8).
const PALETTE_BYTES: usize = 768;
/// `t >>= 7` in `R_BuildLightMap` (REND-1.13).
const LIGHT_SHIFT: u32 = 7;
/// QuakeSpasm's `DIST_EPSILON`, `(0.03125)`, by which `gl_zfix` moves a brush entity
/// (REND-1.12).
const ZFIX: f32 = 0.031_25;
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

    /// The palette with each byte corrected by `gamma` as `Check_Gamma` corrects it (REND-3.1):
    /// `pow ((c+1)/256.0, gamma)*255 + 0.5`, kept within 0 to 255. [`GAMMA`] is GLQuake's on
    /// any card but a 3Dfx.
    #[must_use]
    pub fn with_gamma(&self, gamma: f32) -> Self {
        let correct = |c: u8| {
            let f = ((f32::from(c) + 1.0) / 256.0).powf(gamma);
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "spec REND-3.1: C's (unsigned char) of a value kept within 0 to 255"
            )]
            let byte = (f * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
            byte
        };
        Self(self.0.map(|rgb| rgb.map(correct)))
    }

    /// The colour of index `index`, opaque.
    pub(crate) fn rgba(&self, index: u8) -> [u8; 4] {
        let [r, g, b] = self.0.get(usize::from(index)).copied().unwrap_or_default();
        [r, g, b, 255]
    }

    /// `Mod_FloodFillSkin`'s `filledcolor` (REND-4.1): the first index whose `d_8to24table`
    /// entry is `255 << 0`, red 255 with alpha 0, which only index 255, the one transparent
    /// entry, can be, and only when its colour is pure red; else 0.
    pub(crate) fn filled_colour(&self) -> u8 {
        if self.0.last() == Some(&[255, 0, 0]) {
            255
        } else {
            0
        }
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

/// How a surface is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// Its texture darkened by its lightmap (REND-1).
    Lit,
    /// Turbulent: water, slime, lava and teleporters, rippling (REND-2.2).
    Turbulent,
    /// The sky's two scrolling layers (REND-2.3).
    Sky,
}

/// A texture's frames (REND-2.1): `anim_total`, `anim_min`, `anim_max` and the next frame's
/// texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Animation {
    /// The frames in the cycle.
    pub total: u32,
    /// The first frame this texture is.
    pub min: u32,
    /// The frame after the last this texture is.
    pub max: u32,
    /// The next frame's texture, an index into [`MapMesh::textures`].
    pub next: usize,
}

/// The triangles drawn with one texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Batch {
    /// The texture, an index into [`MapMesh::textures`].
    pub texture: usize,
    /// How it is drawn.
    pub surface: Surface,
    /// The first index into [`MapMesh::indices`].
    pub first: u32,
    /// How many indices it draws.
    pub count: u32,
    /// The model it draws, an index into [`MapMesh::models`]: 0 for the world.
    pub model: usize,
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
    /// For each sky texture, its two layers, the solid one and the alpha one (REND-2.3).
    pub skies: Vec<Option<[Image; 2]>>,
    /// Each texture's frames, `None` for one that does not animate (REND-2.1).
    pub animations: Vec<Option<Animation>>,
    /// Every face's light for the styles last applied (REND-1.13, REND-5, REND-6).
    pub lightmap: Image,
    /// What [`MapMesh::lightmap`] is made from, to make it again as the styles change.
    pub lightmaps: Lightmaps,
    /// The world model's bounds.
    pub mins: [f32; 3],
    /// The world model's bounds.
    pub maxs: [f32; 3],
    /// Each model's placement when the mesh was built, the world first; a model's vertices
    /// are its own, drawn moved by its placement's matrix (REND-1.9).
    pub models: Vec<Placement>,
}

impl MapMesh {
    /// The world model of `bsp`, its textures read through `palette` (REND-1).
    ///
    /// # Errors
    ///
    /// [`MeshError::NoWorld`] when the BSP has no model, and [`MeshError::AtlasTooLarge`] when
    /// the lightmaps do not fit one texture.
    pub fn build(bsp: &Bsp<'_>, palette: &Palette) -> Result<Self, MeshError> {
        let mut builder = MeshBuilder::new(palette);
        builder.add(bsp, &[Placement::WORLD])?;
        Ok(builder.finish())
    }

    /// Makes [`MapMesh::lightmap`] again for `styles` when a style some face reads changed
    /// since it was made; whether it did (REND-5.5).
    pub fn relight(&mut self, styles: &LightStyles) -> bool {
        if !self.lightmaps.stale(styles) {
            return false;
        }
        self.lightmap = self.lightmaps.image(styles);
        self.lightmaps.applied = *styles;
        true
    }
}

/// One model of a BSP, drawn at its entity's origin and angles (REND-1.9): the world is model 0
/// unmoved, a door or lift a submodel `*n` of the map, a health box model 0 of its own BSP.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// The model's number in its BSP.
    pub model: usize,
    /// Where it is moved to; its texture coordinates stay its own, so its textures move
    /// with it.
    pub origin: [f32; 3],
    /// How it is turned: pitch, yaw and roll in degrees.
    pub angles: [f32; 3],
}

impl Placement {
    /// The world: model 0, not moved.
    pub const WORLD: Self = Self {
        model: 0,
        origin: [0.0; 3],
        angles: [0.0; 3],
    };

    /// The placement QuakeSpasm's `gl_zfix` draws: the origin moved by -1/32 on every axis
    /// (REND-1.12), so a brush entity flush with the world is drawn just behind it.
    #[must_use]
    pub fn zfixed(&self) -> Self {
        Self {
            origin: self.origin.map(|v| v - ZFIX),
            ..*self
        }
    }

    /// The column-major matrix that moves a model's vertex to where [`Self::position`] puts it.
    #[must_use]
    pub fn matrix(&self) -> [[f32; 4]; 4] {
        let o = self.position([0.0; 3]);
        let column = |axis: [f32; 3]| {
            let p = self.position(axis);
            [p[0] - o[0], p[1] - o[1], p[2] - o[2], 0.0]
        };
        [
            column([1.0, 0.0, 0.0]),
            column([0.0, 1.0, 0.0]),
            column([0.0, 0.0, 1.0]),
            [o[0], o[1], o[2], 1.0],
        ]
    }

    /// Where `corner` of the model is drawn: turned by roll about x, then pitch about y, then
    /// yaw about z, then moved to the origin, as `R_RotateForEntity` turns it with the pitch
    /// `R_DrawBrushModel` negates twice (REND-1.9).
    #[must_use]
    pub fn position(&self, corner: [f32; 3]) -> [f32; 3] {
        // The pitch, negated twice, turns about y as it is.
        rotate_for_entity(self.origin, self.angles, corner)
    }
}

/// What a BSP's own lumps do not say about drawing it: its map's `.lit` file and the
/// palettes of textures that carry their own.
#[derive(Clone, Copy, Debug, Default)]
pub struct Extras<'a> {
    /// The map's `.lit` file, which lights its faces in colour (REND-6.1).
    pub lit: Option<&'a Lit<'a>>,
    /// For each texture of the BSP, by its slot, a palette of its own, as a Half-Life BSP's or
    /// WAD3's textures have (MAP-24); a texture past the list, or with `None`, is read through
    /// the mesh's palette.
    pub palettes: &'a [Option<Palette>],
}

impl Extras<'_> {
    /// Texture `slot`'s own palette, if it has one.
    fn palette(&self, slot: usize) -> Option<&Palette> {
        self.palettes.get(slot).and_then(Option::as_ref)
    }
}

/// Builds one [`MapMesh`] from models of one or more BSPs, all their faces' light packed into
/// one atlas.
pub struct MeshBuilder<'p> {
    palette: &'p Palette,
    atlas: Lightmaps,
    textures: Vec<Image>,
    skies: Vec<Option<[Image; 2]>>,
    animations: Vec<Option<Animation>>,
    surfaces: Vec<Surface>,
    by_texture: Vec<Vec<Polygon>>,
    bounds: Option<([f32; 3], [f32; 3])>,
    models: Vec<Placement>,
}

impl<'p> MeshBuilder<'p> {
    /// An empty mesh whose textures are read through `palette`.
    #[must_use]
    pub fn new(palette: &'p Palette) -> Self {
        Self {
            palette,
            atlas: Lightmaps::new(),
            textures: Vec::new(),
            skies: Vec::new(),
            animations: Vec::new(),
            surfaces: Vec::new(),
            by_texture: Vec::new(),
            bounds: None,
            models: Vec::new(),
        }
    }

    /// Adds `bsp`'s textures and each of `placements`' models of it (REND-1, REND-2). The
    /// first model added gives the mesh its bounds.
    ///
    /// # Errors
    ///
    /// [`MeshError::NoWorld`] when a placement names a model the BSP does not have, and
    /// [`MeshError::AtlasTooLarge`] when the lightmaps do not fit one texture.
    pub fn add(&mut self, bsp: &Bsp<'_>, placements: &[Placement]) -> Result<(), MeshError> {
        self.add_with(bsp, &Extras::default(), placements)
    }

    /// [`MeshBuilder::add`] with what the BSP's own lumps do not say: the map's `.lit` file
    /// and each texture's own palette (`extras`).
    ///
    /// # Errors
    ///
    /// As [`MeshBuilder::add`].
    pub fn add_with(
        &mut self,
        bsp: &Bsp<'_>,
        extras: &Extras<'_>,
        placements: &[Placement],
    ) -> Result<(), MeshError> {
        let lit = extras.lit;
        let base = self.textures.len();
        let mut surfaces = vec![Surface::Lit; bsp.textures.len()];
        let mut by_texture: Vec<Vec<Polygon>> = vec![Vec::new(); bsp.textures.len()];
        // A surface has one lightmap, shared by every entity drawing its model.
        let mut placed = vec![None; bsp.faces.len()];
        for placement in placements {
            let model = bsp.models.get(placement.model).ok_or(MeshError::NoWorld)?;
            // The first model added is the world, drawn before every entity (REND-1.11).
            if self.bounds.is_none() {
                self.bounds = Some((model.mins, model.maxs));
            }
            let index = self.models.len();
            self.models.push(*placement);
            let numbers = model.firstface..model.firstface.saturating_add(model.numfaces);
            for (number, face) in numbers
                .clone()
                .zip(bsp.faces.get(numbers).unwrap_or_default())
            {
                let Some(texinfo) = bsp.texinfo.get(face.texinfo) else {
                    continue;
                };
                let Some(slot) = texinfo.texture else {
                    continue;
                };
                let Some(Some(texture)) = bsp.textures.get(slot) else {
                    continue;
                };
                let surface = if face.flags & SURF_DRAWSKY != 0 {
                    Surface::Sky
                } else if face.flags & SURF_DRAWTURB != 0 {
                    Surface::Turbulent
                } else {
                    Surface::Lit
                };
                let place = match placed.get(number).copied().flatten() {
                    Some(place) => place,
                    None if surface == Surface::Lit => self.atlas.place(face, bsp, lit)?,
                    None => self.atlas.full_bright(),
                };
                if let Some(slot) = placed.get_mut(number) {
                    *slot = Some(place);
                }
                if let Some(kind) = surfaces.get_mut(slot) {
                    *kind = surface;
                }
                if let Some(list) = by_texture.get_mut(slot) {
                    list.push(Polygon {
                        corners: corners(face, bsp),
                        model: index,
                        texinfo: *texinfo,
                        size: [f32_of(texture.width), f32_of(texture.height)],
                        texturemins: face.texturemins,
                        place,
                        surface,
                    });
                }
            }
        }
        // A texture with a palette of its own is read through it; the others through the
        // mesh's (MAP-24).
        let palette_of = |slot: usize| extras.palette(slot).unwrap_or(self.palette);
        self.textures
            .extend(bsp.textures.iter().enumerate().map(|(slot, texture)| {
                texture
                    .as_ref()
                    .map_or_else(black, |t| image(t, palette_of(slot)))
            }));
        self.skies
            .extend(bsp.textures.iter().zip(&surfaces).enumerate().map(
                |(slot, (texture, &surface))| {
                    (surface == Surface::Sky)
                        .then(|| sky_layers(texture.as_ref()?, palette_of(slot)))
                        .flatten()
                },
            ));
        self.animations.extend(bsp.textures.iter().map(|texture| {
            let animation = texture.as_ref()?.animation?;
            Some(Animation {
                total: u32::try_from(animation.total).ok().filter(|&t| t > 0)?,
                min: u32::try_from(animation.min).ok()?,
                max: u32::try_from(animation.max).ok()?,
                next: animation.next.saturating_add(base),
            })
        }));
        self.surfaces.extend(surfaces);
        self.by_texture.extend(by_texture);
        Ok(())
    }

    /// The mesh of everything added.
    #[must_use]
    pub fn finish(self) -> MapMesh {
        let (width, height) = self.atlas.size();
        let (mins, maxs) = self.bounds.unwrap_or_default();
        let mut mesh = MapMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            batches: Vec::new(),
            textures: self.textures,
            skies: self.skies,
            animations: self.animations,
            lightmap: self.atlas.image(&LightStyles::unset()),
            lightmaps: self.atlas.clone(),
            mins,
            maxs,
            models: self.models.clone(),
        };
        // REND-1.11: the world's batches, then each entity's.
        for model in 0..self.models.len() {
            for (slot, polygons) in self.by_texture.iter().enumerate() {
                let first = u32_of(mesh.indices.len());
                for polygon in polygons.iter().filter(|p| p.model == model) {
                    polygon.emit(&mut mesh, (width, height));
                }
                let count = u32_of(mesh.indices.len()).saturating_sub(first);
                if count > 0 {
                    mesh.batches.push(Batch {
                        texture: slot,
                        surface: self.surfaces.get(slot).copied().unwrap_or(Surface::Lit),
                        first,
                        count,
                        model,
                    });
                }
            }
        }
        mesh
    }
}

/// A face on its way into the mesh.
#[derive(Clone, Debug)]
struct Polygon {
    corners: Vec<[f32; 3]>,
    /// Its model, an index into [`MapMesh::models`].
    model: usize,
    texinfo: Texinfo,
    size: [f32; 2],
    texturemins: [i16; 2],
    /// The face's texel position in the atlas, `light_s` and `light_t`.
    place: [usize; 2],
    /// How it is drawn; only a lit face's light coordinates follow its surface (REND-1.6).
    surface: Surface,
}

impl Polygon {
    /// REND-1.1 to REND-1.3: the face's vertices and a fan of triangles over them.
    fn emit(&self, mesh: &mut MapMesh, (width, height): (f32, f32)) {
        let base = u32_of(mesh.vertices.len());
        for corner in &self.corners {
            let s = dot(corner, self.texinfo.vecs[0]);
            let t = dot(corner, self.texinfo.vecs[1]);
            let texture = match self.surface {
                Surface::Lit => [s / self.size[0], t / self.size[1]],
                // REND-2.2: the raw dot products, without the offset.
                Surface::Turbulent => [s - self.texinfo.vecs[0][3], t - self.texinfo.vecs[1][3]],
                // REND-2.3: the sky's coordinates come from the eye, in the shader.
                Surface::Sky => [0.0, 0.0],
            };
            let light = if self.surface == Surface::Lit {
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
                texture,
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

/// Where one face's light comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FaceLight {
    /// A map with no lighting lump: full bright, whatever the styles (REND-1.13).
    Full,
    /// No samples (`lightofs` of -1): black (REND-1.5).
    Dark,
    /// The face's styles and, for each, its samples: grey from the lighting lump, summed as
    /// GLQuake does (REND-5.4), or coloured from a `.lit` file, summed as QuakeSpasm does
    /// (REND-6.2).
    Samples {
        styles: Vec<u8>,
        maps: Vec<Vec<[u8; 3]>>,
        colour: Colour,
    },
}

/// Whether a face's samples are the lighting lump's grey or a `.lit` file's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Colour {
    Grey,
    Lit,
}

/// One face's light and where it is in the atlas.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PlacedLight {
    at: [usize; 2],
    size: [usize; 2],
    light: FaceLight,
}

/// Every face's light, packed into rows of one texture with a full-bright texel at its corner
/// for the faces that have no lightmap, and made into an image for the light styles' values
/// at a time (REND-1.3 to REND-1.6, REND-5, REND-6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lightmaps {
    faces: Vec<PlacedLight>,
    /// The next free column of the current row, the row's top and its height.
    cursor: [usize; 3],
    height: usize,
    /// The styles the image was last made for.
    applied: LightStyles,
}

/// The tallest atlas, the height every GPU wgpu supports.
const ATLAS_MAX_HEIGHT: usize = 8192;

impl Lightmaps {
    fn new() -> Self {
        Self {
            faces: Vec::new(),
            cursor: [1, 0, 1],
            height: 1,
            applied: LightStyles::unset(),
        }
    }

    /// The texel that is always 255 (REND-1.6).
    const fn full_bright(&self) -> [usize; 2] {
        [0, 0]
    }

    /// Places `face`'s lightmap, its samples from `bsp` or, coloured, from `lit`; returns its
    /// position (REND-1.3).
    fn place(
        &mut self,
        face: &Face,
        bsp: &Bsp<'_>,
        lit: Option<&Lit<'_>>,
    ) -> Result<[usize; 2], MeshError> {
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
        self.height = self.height.max(bottom);
        self.cursor = [column.saturating_add(smax), top, row_height];
        self.faces.push(PlacedLight {
            at: [column, top],
            size: [smax, tmax],
            light: face_light(face, bsp, lit),
        });
        Ok([column, top])
    }

    fn size(&self) -> (f32, f32) {
        (f32_of_usize(ATLAS_WIDTH), f32_of_usize(self.height))
    }

    /// The atlas for `styles`: each texel's light in red, green and blue.
    #[must_use]
    pub fn image(&self, styles: &LightStyles) -> Image {
        let mut rgba = vec![0_u8; ATLAS_WIDTH.saturating_mul(self.height).saturating_mul(4)];
        if let Some(corner) = rgba.get_mut(..4) {
            corner.copy_from_slice(&[255; 4]);
        }
        for face in &self.faces {
            let [smax, tmax] = face.size;
            let texels = face.light.texels(smax.saturating_mul(tmax), styles);
            for t in 0..tmax {
                for s in 0..smax {
                    let [r, g, b] = texels
                        .get(t.saturating_mul(smax).saturating_add(s))
                        .copied()
                        .unwrap_or_default();
                    let at = face.at[1]
                        .saturating_add(t)
                        .saturating_mul(ATLAS_WIDTH)
                        .saturating_add(face.at[0])
                        .saturating_add(s)
                        .saturating_mul(4);
                    if let Some(texel) = rgba.get_mut(at..at.saturating_add(4)) {
                        texel.copy_from_slice(&[r, g, b, 255]);
                    }
                }
            }
        }
        Image {
            width: u32_of(ATLAS_WIDTH),
            height: u32_of(self.height),
            rgba,
        }
    }

    /// Whether `styles` differ from the styles the image was made for in a style some face
    /// reads, as `R_RenderDynamicLightmaps` compares `cached_light` (REND-5.5).
    fn stale(&self, styles: &LightStyles) -> bool {
        self.faces.iter().any(|face| match &face.light {
            FaceLight::Samples { styles: used, .. } => used
                .iter()
                .any(|&style| styles.value(style) != self.applied.value(style)),
            FaceLight::Full | FaceLight::Dark => false,
        })
    }
}

impl FaceLight {
    /// The face's `size` texels for `styles`.
    fn texels(&self, size: usize, styles: &LightStyles) -> Vec<[u8; 3]> {
        match self {
            Self::Full => vec![[255; 3]; size],
            Self::Dark => vec![[0; 3]; size],
            Self::Samples {
                styles: used,
                maps,
                colour,
            } => {
                let mut sums = vec![[0_u32; 3]; size];
                for (&style, map) in used.iter().zip(maps) {
                    let scale = styles.scale(style);
                    for (sum, sample) in sums.iter_mut().zip(map) {
                        for (channel, &value) in sum.iter_mut().zip(sample) {
                            *channel = channel.wrapping_add(u32::from(value).wrapping_mul(scale));
                        }
                    }
                }
                sums.iter()
                    .map(|sum| sum.map(|channel| colour.texel(channel)))
                    .collect()
            }
        }
    }
}

impl Colour {
    /// One channel's texel from its sum.
    fn texel(self, sum: u32) -> u8 {
        match self {
            // REND-5.4: GLQuake reads the sum into a signed `t`, shifts it and caps it at
            // 255; the GL texture keeps the low byte of what is left.
            Self::Grey => {
                let t = sum.cast_signed() >> LIGHT_SHIFT;
                u8::try_from(t.min(255).rem_euclid(256)).unwrap_or(u8::MAX)
            }
            // REND-6.2: QuakeSpasm shifts the unsigned sum and caps it at 255.
            Self::Lit => u8::try_from(sum >> LIGHT_SHIFT).unwrap_or(u8::MAX),
        }
    }
}

/// Where `face`'s light comes from (REND-1.13, REND-1.5, REND-6.1).
fn face_light(face: &Face, bsp: &Bsp<'_>, lit: Option<&Lit<'_>>) -> FaceLight {
    // "set to full bright if no light data"
    if bsp.lighting.is_empty() {
        return FaceLight::Full;
    }
    let Some(samples) = bsp.face_samples(face) else {
        return FaceLight::Dark;
    };
    let [smax, tmax] = face.lightmap_size;
    let size = smax.saturating_mul(tmax).max(1);
    let styles: Vec<u8> = face
        .styles
        .iter()
        .copied()
        .take(face.style_count())
        .collect();
    let start = usize::try_from(face.lightofs).unwrap_or(0);
    let (maps, colour) = match lit {
        Some(lit) => (
            (0..styles.len())
                .map(|map| {
                    (0..size)
                        .map(|i| {
                            let index = start
                                .saturating_add(map.saturating_mul(size))
                                .saturating_add(i);
                            lit.sample(index).unwrap_or_default()
                        })
                        .collect()
                })
                .collect(),
            Colour::Lit,
        ),
        None => (
            samples
                .chunks_exact(size)
                .map(|map| map.iter().map(|&v| [v; 3]).collect())
                .collect(),
            Colour::Grey,
        ),
    };
    FaceLight::Samples {
        styles,
        maps,
        colour,
    }
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

/// The side of a sky layer, `R_InitSky`'s 128 by 128.
const SKY_LAYER: usize = 128;

/// REND-2.3: the sky's solid layer, its texture's right half, and its alpha layer, the left
/// half with colour 0 transparent and coloured as the solid layer's average, so filtering
/// draws no dark fringe at a cloud's edge (`R_InitSky`); `None` when it is not 256 by 128.
fn sky_layers(texture: &qs_bsp::Texture<'_>, palette: &Palette) -> Option<[Image; 2]> {
    let pixels = texture.mip(0)?;
    if (texture.width, texture.height) != (256, 128) {
        return None;
    }
    let row = |i: usize| {
        pixels
            .get(i.checked_mul(2 * SKY_LAYER)?..)
            .map(|r| r.iter().take(2 * SKY_LAYER))
    };
    let mut solid = Vec::with_capacity(SKY_LAYER * SKY_LAYER * 4);
    let mut sums = [0_u32; 3];
    for i in 0..SKY_LAYER {
        for &p in row(i)?.skip(SKY_LAYER) {
            let rgba = palette.rgba(p);
            for (sum, &c) in sums.iter_mut().zip(&rgba) {
                *sum = sum.saturating_add(u32::from(c));
            }
            solid.extend(rgba);
        }
    }
    let count = u32::try_from(SKY_LAYER * SKY_LAYER).ok()?;
    let [r, g, b] =
        sums.map(|s| u8::try_from(s.checked_div(count).unwrap_or(0)).unwrap_or(u8::MAX));
    let mut alpha = Vec::with_capacity(SKY_LAYER * SKY_LAYER * 4);
    for i in 0..SKY_LAYER {
        for &p in row(i)?.take(SKY_LAYER) {
            if p == 0 {
                alpha.extend([r, g, b, 0]);
            } else {
                alpha.extend(palette.rgba(p));
            }
        }
    }
    let side = u32::try_from(SKY_LAYER).ok()?;
    let layer = |rgba| Image {
        width: side,
        height: side,
        rgba,
    };
    Some([layer(solid), layer(alpha)])
}

/// The texture `base` is drawn as at `time` seconds (REND-2.1): its frame for
/// `(int)(time*10) % anim_total`, following `next` from the base through `animations`.
#[must_use]
pub fn frame(animations: &[Option<Animation>], base: usize, time: f32) -> usize {
    let Some(Some(first)) = animations.get(base) else {
        return base;
    };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "spec REND-2.1: GLQuake's (int)(cl.time*10), never negative here"
    )]
    let tenths = (time.max(0.0) * 10.0) as u32;
    let relative = tenths.checked_rem(first.total).unwrap_or(0);
    let mut texture = base;
    // GLQuake stops on a cycle longer than 100 with "infinite cycle".
    for _ in 0..100 {
        match animations.get(texture) {
            Some(Some(a)) if a.min > relative || a.max <= relative => texture = a.next,
            _ => return texture,
        }
    }
    base
}

/// REND-3.2: the mip levels GLQuake uploads for a world texture: `image` point-sampled up to
/// powers of two (`GL_ResampleTexture`), then each level the 2 by 2 average of the one above
/// down to 1 by 1 (`GL_MipMap`), largest first.
#[must_use]
pub fn mip_levels(image: &Image) -> Vec<Image> {
    let (mut width, mut height) = (power_of_two(image.width), power_of_two(image.height));
    let mut scaled = resample(image, width, height);
    let mut levels = vec![Image {
        width,
        height,
        rgba: scaled.clone(),
    }];
    while width > 1 || height > 1 {
        mip_map(&mut scaled, width, height);
        width = (width >> 1).max(1);
        height = (height >> 1).max(1);
        let bytes = usize::try_from(width.saturating_mul(height).saturating_mul(4)).unwrap_or(0);
        levels.push(Image {
            width,
            height,
            rgba: scaled.get(..bytes).map(<[u8]>::to_vec).unwrap_or_default(),
        });
    }
    levels
}

/// The first power of two that is `n` or more, as `GL_Upload32`'s `scaled_width <<= 1` loop
/// finds it.
fn power_of_two(n: u32) -> u32 {
    n.max(1).checked_next_power_of_two().unwrap_or(n)
}

/// `GL_ResampleTexture`: each row from `in + inwidth*(i*inheight/outheight)`, and each pixel
/// from it at `frac>>16`, `frac` starting at half of `fracstep = inwidth*0x10000/outwidth`.
fn resample(image: &Image, width: u32, height: u32) -> Vec<u8> {
    if (width, height) == (image.width, image.height) {
        return image.rgba.clone();
    }
    let (in_width, in_height) = (u64::from(image.width), u64::from(image.height));
    let fracstep = (in_width << 16).checked_div(u64::from(width)).unwrap_or(0);
    let mut out = Vec::with_capacity(
        usize::try_from(width.saturating_mul(height))
            .unwrap_or(0)
            .saturating_mul(4),
    );
    for i in 0..u64::from(height) {
        let row = in_width.saturating_mul(
            i.saturating_mul(in_height)
                .checked_div(u64::from(height))
                .unwrap_or(0),
        );
        let mut frac = fracstep >> 1;
        for _ in 0..width {
            let at = usize::try_from(row.saturating_add(frac >> 16).saturating_mul(4))
                .unwrap_or(usize::MAX);
            out.extend(
                image
                    .rgba
                    .get(at..at.saturating_add(4))
                    .unwrap_or(&[0, 0, 0, 255]),
            );
            frac = frac.saturating_add(fracstep);
        }
    }
    out
}

/// `GL_MipMap` on `scaled`, a level `width` by `height`: each output pixel the `>>2` average
/// of a 2 by 2 block, written from the start; with `height` 1 no row is averaged.
fn mip_map(scaled: &mut [u8], width: u32, height: u32) {
    let row = usize::try_from(width).unwrap_or(0).saturating_mul(4);
    let rows = usize::try_from(height >> 1).unwrap_or(0);
    let mut out = 0_usize;
    let mut input = 0_usize;
    let byte = |buffer: &[u8], at: usize| u16::from(buffer.get(at).copied().unwrap_or(0));
    for _ in 0..rows {
        let mut j = 0;
        while j < row {
            for c in 0..4 {
                let at = input.saturating_add(c);
                // Four bytes sum to at most 1020, which a u16 holds.
                let sum = [
                    at,
                    at.saturating_add(4),
                    at.saturating_add(row),
                    at.saturating_add(row).saturating_add(4),
                ]
                .into_iter()
                .fold(0_u16, |sum, i| sum.saturating_add(byte(scaled, i)));
                if let Some(slot) = scaled.get_mut(out.saturating_add(c)) {
                    *slot = u8::try_from(sum >> 2).unwrap_or(u8::MAX);
                }
            }
            out = out.saturating_add(4);
            input = input.saturating_add(8);
            j = j.saturating_add(8);
        }
        input = input.saturating_add(row);
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
pub(crate) mod tests;
